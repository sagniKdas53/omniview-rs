use crate::models::{AspectFilter, ImageRecord};
use crate::statx::get_file_ctime;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif"];
pub const IGNORED_DIR_NAMES: &[&str] = &[
    "target", "node_modules", ".git", "__pycache__", ".cargo", "dist", "build", ".venv", "venv",
    ".cache", ".local",
];

pub fn scan_directory<P: AsRef<Path>>(root_dir: P) -> (Vec<ImageRecord>, Vec<(String, usize)>, u64) {
    let root = root_dir.as_ref();
    let mut entries = Vec::new();
    let mut subfolder_counts = std::collections::HashMap::new();
    let mut total_bytes = 0u64;

    for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        if name.starts_with('.') {
            return false;
        }
        if e.file_type().is_dir() && IGNORED_DIR_NAMES.contains(&name.as_ref()) {
            return false;
        }
        true
    }) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let Ok(meta) = fs::metadata(path) else { continue };

        if ext == "zip" {
            let mtime = meta.mtime() as f64 + (meta.mtime_nsec() as f64 * 1e-9);
            let ctime = get_file_ctime(path, &meta);
            let zip_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            let zip_dir = path.parent().unwrap_or(root);
            let rel_zip = path
                .strip_prefix(root)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());

            let zip_images = crate::zip_utils::list_zip_images(path);
            for img in zip_images {
                if crate::zip_utils::is_raw_duplicate(zip_dir, zip_stem, &img.inner_name) {
                    continue; // Skip duplicate, prioritize raw file
                }

                total_bytes += img.uncompressed_size;
                *subfolder_counts.entry(rel_zip.clone()).or_insert(0) += 1;

                let inner_filename = Path::new(&img.inner_name)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&img.inner_name)
                    .to_string();

                let composite_path = crate::zip_utils::make_zip_path(path, &img.inner_name);

                entries.push(ImageRecord {
                    path: composite_path,
                    filename: inner_filename,
                    subfolder: rel_zip.clone(),
                    file_size: img.uncompressed_size,
                    mtime,
                    ctime,
                    width: 0,
                    height: 0,
                    thumb_path: PathBuf::new(),
                    is_favorite: false,
                    aspect: AspectFilter::Square,
                    color_name: String::new(),
                    dhash: String::new(),
                });
            }
            continue;
        }

        if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let file_size = meta.len();
        total_bytes += file_size;

        let mtime = meta.mtime() as f64 + (meta.mtime_nsec() as f64 * 1e-9);
        let ctime = get_file_ctime(path, &meta);

        let filename = entry.file_name().to_string_lossy().to_string();
        let rel_dir = path
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        *subfolder_counts.entry(rel_dir.clone()).or_insert(0) += 1;

        entries.push(ImageRecord {
            path: path.to_path_buf(),
            filename,
            subfolder: rel_dir,
            file_size,
            mtime,
            ctime,
            width: 0,
            height: 0,
            thumb_path: PathBuf::new(),
            is_favorite: false,
            aspect: AspectFilter::Square,
            color_name: String::new(),
            dhash: String::new(),
        });
    }

    let mut subfolders_vec: Vec<(String, usize)> = subfolder_counts.into_iter().collect();
    subfolders_vec.sort_by(|a, b| a.0.cmp(&b.0));

    (entries, subfolders_vec, total_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_directory() {
        let temp_dir = std::env::temp_dir().join("omniview_test_scanner");
        let _ = fs::remove_dir_all(&temp_dir);
        let sub = temp_dir.join("sub1");
        fs::create_dir_all(&sub).unwrap();

        fs::write(temp_dir.join("test1.png"), b"fakepng").unwrap();
        fs::write(sub.join("test2.jpg"), b"fakejpg").unwrap();
        fs::write(temp_dir.join("ignored.txt"), b"text").unwrap();

        let (entries, subfolders, total_bytes) = scan_directory(&temp_dir);
        assert_eq!(entries.len(), 2);
        assert_eq!(subfolders.len(), 2); // "" and "sub1"
        assert_eq!(total_bytes, 7 + 7);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scan_zip_and_deduplication() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir().join("omniview_test_zip_scanner");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // 1. Create a zip archive with two entries: "duplicate.png" and "unique_inside.png"
        let zip_path = temp_dir.join("archive1.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("duplicate.png", options).unwrap();
        zip.write_all(b"zip_dup_content").unwrap();

        zip.start_file("unique_inside.png", options).unwrap();
        zip.write_all(b"unique_content").unwrap();

        zip.finish().unwrap();

        // 2. Create raw extracted file for "duplicate.png"
        fs::write(temp_dir.join("duplicate.png"), b"raw_content").unwrap();

        // 3. Scan directory
        let (entries, _subfolders, _) = scan_directory(&temp_dir);

        // Expected:
        // - "duplicate.png" raw file (path: temp_dir/duplicate.png)
        // - "archive1.zip#unique_inside.png" zip file entry
        // - "archive1.zip#duplicate.png" MUST BE FILTERED OUT (deduplicated)
        assert_eq!(entries.len(), 2, "Expected 2 entries (1 raw + 1 zip)");

        let filenames: Vec<String> = entries.iter().map(|e| e.filename.clone()).collect();
        assert!(filenames.contains(&"duplicate.png".to_string()));
        assert!(filenames.contains(&"unique_inside.png".to_string()));

        let raw_entry = entries.iter().find(|e| e.filename == "duplicate.png").unwrap();
        assert!(!raw_entry.path.to_string_lossy().contains('#'));

        let zip_entry = entries.iter().find(|e| e.filename == "unique_inside.png").unwrap();
        assert!(zip_entry.path.to_string_lossy().contains('#'));

        // 4. Test extraction to cache and reading bytes
        let (zip_p, inner_name) = crate::zip_utils::parse_zip_path(&zip_entry.path).unwrap();
        let bytes = crate::zip_utils::read_zip_entry_bytes(&zip_p, &inner_name).unwrap();
        assert_eq!(bytes, b"unique_content");

        let cached = crate::zip_utils::ensure_accessible_path(&zip_entry.path);
        assert!(cached.exists());
        assert_eq!(fs::read(cached).unwrap(), b"unique_content");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_pixiv_zips_dataset_if_present() {
        let pixiv_path = Path::new("/home/sagnik/Documents/pixiv_zips");
        if !pixiv_path.exists() {
            return;
        }

        let (entries, _, _) = scan_directory(pixiv_path);
        let mut raw_count = 0;
        let mut zip_count = 0;
        for e in &entries {
            if e.path.to_string_lossy().contains('#') {
                zip_count += 1;
            } else {
                raw_count += 1;
            }
        }
        assert_eq!(entries.len(), 1155, "Total items should be 1155");
        assert_eq!(raw_count, 109, "Raw items should be 109");
        assert_eq!(zip_count, 1046, "Zipped unique items should be 1046");
    }
}
