use crate::color_utils::extract_image_features;
use egui::ColorImage;
use image::imageops::FilterType;
use image::GenericImageView;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ThumbnailCache {
    pub cache_dir: PathBuf,
    pub legacy_cache_dir: PathBuf,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let cache_dir = PathBuf::from(&home)
            .join(".cache")
            .join("omniview")
            .join("thumbnails");
        let legacy_cache_dir = PathBuf::from(&home)
            .join(".cache")
            .join("pixiv_gallery")
            .join("thumbnails");
        let _ = fs::create_dir_all(&cache_dir);
        Self {
            cache_dir,
            legacy_cache_dir,
        }
    }

    pub fn get_canonical_thumb_path<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let path_str = path.as_ref().to_string_lossy();
        let mut hasher = Sha256::new();
        hasher.update(path_str.as_bytes());
        let hex = format!("{:x}", hasher.finalize());
        let prefix = if hex.len() >= 24 { &hex[..24] } else { &hex };
        self.cache_dir.join(format!("{}.webp", prefix))
    }

    pub fn load_thumbnail<P: AsRef<Path>>(
        &self,
        source_path: P,
        existing_thumb: Option<&Path>,
        max_size: u32,
    ) -> Option<(u32, u32, ColorImage)> {
        let source_path = source_path.as_ref();

        // 1. Try existing thumb path from DB
        if let Some(thumb) = existing_thumb {
            if !thumb.as_os_str().is_empty() && thumb.exists() {
                if let Some(res) = Self::load_image_to_color_image(thumb) {
                    return Some(res);
                }
            }
        }

        // 2. Try canonical sha256 WebP thumbnail
        let cache_path = self.get_canonical_thumb_path(source_path);
        if cache_path.exists() {
            if let Some(res) = Self::load_image_to_color_image(&cache_path) {
                return Some(res);
            }
        }

        // 3. Try legacy ~/.cache/pixiv_gallery/thumbnails
        if let Some(name) = cache_path.file_name() {
            let legacy_path = self.legacy_cache_dir.join(name);
            if legacy_path.exists() {
                if let Some(res) = Self::load_image_to_color_image(&legacy_path) {
                    return Some(res);
                }
            }
        }

    // 4. Generate WebP thumbnail on the fly
    let dyn_img = open_image_any(source_path)?;
    let (orig_w, orig_h) = dyn_img.dimensions();
    if orig_w == 0 || orig_h == 0 {
        return None;
    }

    let resized = dyn_img.resize(max_size, max_size, FilterType::Triangle);
    let rgba = resized.to_rgba8();
    let (w, h) = rgba.dimensions();

    let encoder = webp::Encoder::new(&rgba, webp::PixelLayout::Rgba, w, h);
    let webp_data = encoder.encode(82.0);
    let _ = fs::write(&cache_path, &*webp_data);

    let color_image = ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_flat_samples().as_slice(),
    );

    Some((orig_w, orig_h, color_image))
}

fn load_image_to_color_image<P: AsRef<Path>>(thumb_path: P) -> Option<(u32, u32, ColorImage)> {
    let bytes = fs::read(thumb_path).ok()?;
    let dyn_img = if let Ok(img) = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP) {
        img
    } else {
        image::load_from_memory(&bytes).ok()?
    };
    let rgba = dyn_img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let color_image = ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        rgba.as_flat_samples().as_slice(),
    );
    Some((w, h, color_image))
}

pub fn process_image_features<P: AsRef<Path>>(
    &self,
    source_path: P,
    max_size: u32,
) -> Option<(PathBuf, u32, u32, &'static str, &'static str, u8, u8, u8, String)> {
    let source_path = source_path.as_ref();
    let cache_path = self.get_canonical_thumb_path(source_path);

    if let Some(dyn_img) = open_image_any(source_path) {
        let (w, h, aspect, color_name, r, g, b, dhash) = extract_image_features(&dyn_img);

        if !cache_path.exists() {
            let resized = dyn_img.resize(max_size, max_size, FilterType::Triangle);
            let rgba = resized.to_rgba8();
            let (rw, rh) = rgba.dimensions();
            let encoder = webp::Encoder::new(&rgba, webp::PixelLayout::Rgba, rw, rh);
            let webp_data = encoder.encode(82.0);
            let _ = fs::write(&cache_path, &*webp_data);
        }

        return Some((cache_path, w, h, aspect, color_name, r, g, b, dhash));
    }

    None
}
}

pub fn open_image_any<P: AsRef<Path>>(path: P) -> Option<image::DynamicImage> {
    let p = path.as_ref();
    if let Some((zip_path, inner_name)) = crate::zip_utils::parse_zip_path(p) {
        let bytes = crate::zip_utils::read_zip_entry_bytes(&zip_path, &inner_name)?;
        image::load_from_memory(&bytes).ok()
    } else {
        image::open(p).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn test_canonical_thumb_path_prefix() {
        let cache = ThumbnailCache::new();
        let path = PathBuf::from("/home/sagnik/Downloads/pixiv_downloads/sample.jpg");
        let thumb = cache.get_canonical_thumb_path(&path);
        let fname = thumb.file_name().unwrap().to_str().unwrap();
        assert!(fname.ends_with(".webp"));
        assert_eq!(fname.len(), 24 + 5); // 24 hex chars + ".webp"
    }

    #[test]
    fn test_load_and_cache_thumbnail() {
        let temp_dir = std::env::temp_dir().join(format!("omniview_rs_cache_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let cache = ThumbnailCache {
            cache_dir: temp_dir.join("cache"),
            legacy_cache_dir: temp_dir.join("legacy"),
        };
        let _ = fs::create_dir_all(&cache.cache_dir);

        let img_path = temp_dir.join("test_img.png");
        let mut img = RgbImage::new(100, 50);
        for pixel in img.pixels_mut() {
            *pixel = Rgb([200, 100, 50]);
        }
        img.save(&img_path).unwrap();

        let res = cache.load_thumbnail(&img_path, None, 80);
        assert!(res.is_some());
        let (w, h, color_img) = res.unwrap();
        assert_eq!(w, 100);
        assert_eq!(h, 50);
        assert!(color_img.width() <= 80);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
