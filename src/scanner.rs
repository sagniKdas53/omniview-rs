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
