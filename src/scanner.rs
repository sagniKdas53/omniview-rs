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

        if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let Ok(meta) = fs::metadata(path) else { continue };
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
}
