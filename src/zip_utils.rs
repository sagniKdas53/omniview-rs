use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::scanner::SUPPORTED_EXTENSIONS;

#[derive(Clone, Debug)]
pub struct ZipImageEntry {
    pub inner_name: String,
    pub uncompressed_size: u64,
    #[allow(dead_code)]
    pub mtime: f64,
}

pub fn parse_zip_path<P: AsRef<Path>>(path: P) -> Option<(PathBuf, String)> {
    let p_str = path.as_ref().to_string_lossy();
    let hash_idx = p_str.find('#')?;
    let zip_str = &p_str[..hash_idx];
    if !zip_str.to_lowercase().ends_with(".zip") {
        return None;
    }
    let inner = p_str[hash_idx + 1..].to_string();
    Some((PathBuf::from(zip_str), inner))
}

pub fn make_zip_path<P: AsRef<Path>>(zip_path: P, inner_name: &str) -> PathBuf {
    PathBuf::from(format!("{}#{}", zip_path.as_ref().display(), inner_name))
}

pub fn is_raw_duplicate(zip_dir: &Path, zip_stem: &str, inner_name: &str) -> bool {
    let stem_slash = format!("{}/", zip_stem);
    let inner_clean = if inner_name.to_lowercase().starts_with(&stem_slash.to_lowercase()) {
        &inner_name[stem_slash.len()..]
    } else {
        inner_name
    };

    let cand1 = zip_dir.join(zip_stem).join(inner_clean);
    let cand2 = zip_dir.join(inner_name);

    cand1.exists() || cand2.exists()
}

pub fn list_zip_images(zip_path: &Path) -> Vec<ZipImageEntry> {
    let mut results = Vec::new();
    let Ok(file) = File::open(zip_path) else {
        return results;
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return results;
    };

    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.starts_with("__MACOSX/") || name.contains("/.") {
            continue;
        }

        let entry_path = Path::new(&name);
        let filename = entry_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default();
        if filename.starts_with('.') {
            continue;
        }

        let ext = entry_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let uncompressed_size = entry.size();
        let mtime = 0.0; // Archive file mtime can be used as fallback

        results.push(ZipImageEntry {
            inner_name: name,
            uncompressed_size,
            mtime,
        });
    }

    results
}

pub fn read_zip_entry_bytes(zip_path: &Path, inner_name: &str) -> Option<Vec<u8>> {
    let file = File::open(zip_path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;

    let mut buf = Vec::new();
    if let Ok(mut entry) = archive.by_name(inner_name) {
        buf.reserve_exact(entry.size() as usize);
        entry.read_to_end(&mut buf).ok()?;
        return Some(buf);
    }

    let alt = inner_name.replace('\\', "/");
    if let Ok(mut entry) = archive.by_name(&alt) {
        buf.reserve_exact(entry.size() as usize);
        entry.read_to_end(&mut buf).ok()?;
        return Some(buf);
    }

    None
}

pub fn get_extracted_cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".cache")
        .join("omniview")
        .join("extracted")
}

pub fn extract_zip_entry_to_cache(zip_path: &Path, inner_name: &str) -> Option<PathBuf> {
    let cache_dir = get_extracted_cache_dir();
    let _ = fs::create_dir_all(&cache_dir);

    let composite = format!("{}#{}", zip_path.display(), inner_name);
    let mut hasher = Sha256::new();
    hasher.update(composite.as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    let prefix = if hex.len() >= 16 { &hex[..16] } else { &hex };

    let ext = Path::new(inner_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();

    let target = cache_dir.join(format!("{}{}", prefix, ext));
    if target.exists() && fs::metadata(&target).map(|m| m.len() > 0).unwrap_or(false) {
        return Some(target);
    }

    let bytes = read_zip_entry_bytes(zip_path, inner_name)?;
    fs::write(&target, bytes).ok()?;
    Some(target)
}

pub fn ensure_accessible_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let p = path.as_ref();
    if let Some((zip_path, inner_name)) = parse_zip_path(p) {
        if let Some(extracted) = extract_zip_entry_to_cache(&zip_path, &inner_name) {
            return extracted;
        }
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_make_zip_path() {
        let zip = Path::new("/path/to/archive.zip");
        let composite = make_zip_path(zip, "0.png");
        assert_eq!(composite.to_string_lossy(), "/path/to/archive.zip#0.png");

        let parsed = parse_zip_path(&composite);
        assert!(parsed.is_some());
        let (z, inner) = parsed.unwrap();
        assert_eq!(z, zip);
        assert_eq!(inner, "0.png");
    }

    #[test]
    fn test_is_raw_duplicate() {
        let temp_dir = std::env::temp_dir().join("omniview_rs_test_zip_dupe");
        let _ = fs::remove_dir_all(&temp_dir);
        let extracted_dir = temp_dir.join("folder1");
        fs::create_dir_all(&extracted_dir).unwrap();

        fs::write(extracted_dir.join("0.png"), b"fake0").unwrap();

        // 0.png exists in folder1/0.png -> true
        assert!(is_raw_duplicate(&temp_dir, "folder1", "0.png"));
        // folder1/0.png where inner name already has folder1 prefix
        assert!(is_raw_duplicate(&temp_dir, "folder1", "folder1/0.png"));
        // 1.png does not exist -> false
        assert!(!is_raw_duplicate(&temp_dir, "folder1", "1.png"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
