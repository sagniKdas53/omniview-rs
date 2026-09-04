use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Clone, Debug)]
pub struct ImageEntry {
    pub path: PathBuf,
    pub filename: String,
    pub subfolder: String,
    pub file_size: u64,
    pub mtime: SystemTime,
    pub width: u32,
    pub height: u32,
}

pub fn is_image_ext(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif"
    )
}

pub fn scan_directory<P: AsRef<Path>>(root: P) -> (Vec<ImageEntry>, Vec<(String, usize)>) {
    let root = root.as_ref();
    let mut entries = Vec::new();
    let mut folder_counts: HashMap<String, usize> = HashMap::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.')
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if is_image_ext(ext) {
                    let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let rel_parent = path.parent().unwrap_or(root).strip_prefix(root).unwrap_or(Path::new(""));
                    let subfolder = if rel_parent.as_os_str().is_empty() {
                        "__root__".to_string()
                    } else {
                        rel_parent.to_string_lossy().to_string()
                    };

                    let metadata = entry.metadata().ok();
                    let file_size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                    let mtime = metadata
                        .as_ref()
                        .and_then(|m| m.modified().ok())
                        .unwrap_or(SystemTime::UNIX_EPOCH);

                    *folder_counts.entry(subfolder.clone()).or_insert(0) += 1;

                    entries.push(ImageEntry {
                        path: path.to_path_buf(),
                        filename,
                        subfolder,
                        file_size,
                        mtime,
                        width: 0,
                        height: 0,
                    });
                }
            }
        }
    }

    let mut folders: Vec<(String, usize)> = folder_counts.into_iter().collect();
    folders.sort_by(|a, b| b.1.cmp(&a.1));

    (entries, folders)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_is_image_ext() {
        assert!(is_image_ext("jpg"));
        assert!(is_image_ext("JPG"));
        assert!(is_image_ext("jpeg"));
        assert!(is_image_ext("Jpeg"));
        assert!(is_image_ext("png"));
        assert!(is_image_ext("PNG"));
        assert!(is_image_ext("webp"));
        assert!(is_image_ext("WebP"));
        assert!(is_image_ext("bmp"));
        assert!(is_image_ext("gif"));

        assert!(!is_image_ext("txt"));
        assert!(!is_image_ext("pdf"));
        assert!(!is_image_ext("exe"));
        assert!(!is_image_ext("mp4"));
        assert!(!is_image_ext("json"));
        assert!(!is_image_ext(""));
    }

    #[test]
    fn test_scan_directory_structure() {
        let temp_dir = std::env::temp_dir().join(format!("omniview_scan_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        // Root images
        let mut f1 = File::create(temp_dir.join("root1.png")).unwrap();
        f1.write_all(b"fake png data").unwrap();
        let mut f2 = File::create(temp_dir.join("root2.jpg")).unwrap();
        f2.write_all(b"fake jpg data").unwrap();
        let mut non_img = File::create(temp_dir.join("notes.txt")).unwrap();
        non_img.write_all(b"text file").unwrap();

        // Subdirectory 1: chapter_1 (3 images)
        let ch1 = temp_dir.join("chapter_1");
        fs::create_dir_all(&ch1).unwrap();
        File::create(ch1.join("01.png")).unwrap().write_all(b"img1").unwrap();
        File::create(ch1.join("02.webp")).unwrap().write_all(b"img2").unwrap();
        File::create(ch1.join("03.jpg")).unwrap().write_all(b"img3").unwrap();

        // Subdirectory 2: chapter_2 (1 image)
        let ch2 = temp_dir.join("chapter_2");
        fs::create_dir_all(&ch2).unwrap();
        File::create(ch2.join("cover.gif")).unwrap().write_all(b"cover").unwrap();

        // Hidden folder: .git (should be ignored)
        let hidden = temp_dir.join(".git");
        fs::create_dir_all(&hidden).unwrap();
        File::create(hidden.join("ignored.png")).unwrap().write_all(b"ignored").unwrap();

        let (entries, folders) = scan_directory(&temp_dir);

        // Total scanned images should be 2 (root) + 3 (ch1) + 1 (ch2) = 6
        assert_eq!(entries.len(), 6, "Must find exactly 6 image files");

        // Folders list must have chapter_1 (3), __root__ (2), chapter_2 (1)
        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0], ("chapter_1".to_string(), 3));
        assert_eq!(folders[1], ("__root__".to_string(), 2));
        assert_eq!(folders[2], ("chapter_2".to_string(), 1));

        // Verify root entry subfolder name
        let root_entries: Vec<_> = entries.iter().filter(|e| e.subfolder == "__root__").collect();
        assert_eq!(root_entries.len(), 2);

        // Verify file sizes are correct
        for e in &entries {
            assert!(e.file_size > 0, "File size must be positive");
            assert!(!e.filename.is_empty(), "Filename must not be empty");
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
