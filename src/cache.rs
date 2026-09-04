use egui::ColorImage;
use image::imageops::FilterType;
use image::GenericImageView;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct ThumbnailCache {
    pub cache_dir: PathBuf,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let cache_dir = PathBuf::from(home)
            .join(".cache")
            .join("omniview")
            .join("thumbnails");
        let _ = fs::create_dir_all(&cache_dir);
        Self { cache_dir }
    }

    pub fn get_cache_path<P: AsRef<Path>>(&self, path: P, mtime: SystemTime, file_size: u64) -> PathBuf {
        let mut hasher = DefaultHasher::new();
        path.as_ref().hash(&mut hasher);
        mtime.hash(&mut hasher);
        file_size.hash(&mut hasher);
        let hash_val = hasher.finish();
        self.cache_dir.join(format!("{:016x}.webp", hash_val))
    }

    pub fn load_or_generate_thumbnail<P: AsRef<Path>>(
        &self,
        source_path: P,
        mtime: SystemTime,
        file_size: u64,
        max_size: u32,
    ) -> Option<(u32, u32, ColorImage)> {
        let source_path = source_path.as_ref();
        let cache_path = self.get_cache_path(source_path, mtime, file_size);

        // 1. If WebP thumbnail exists in cache, load directly!
        let meta_path = cache_path.with_extension("meta");
        if cache_path.exists() {
            if let Ok(cached_bytes) = fs::read(&cache_path) {
                if !cached_bytes.is_empty() {
                    if let Ok(dyn_img) = image::load_from_memory_with_format(&cached_bytes, image::ImageFormat::WebP) {
                        let rgba = dyn_img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        let color_image = ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            rgba.as_flat_samples().as_slice(),
                        );

                        // Read true original dimensions from .meta or fast header probe
                        let (orig_w, orig_h) = if let Ok(meta_str) = fs::read_to_string(&meta_path) {
                            let parts: Vec<&str> = meta_str.trim().split(',').collect();
                            if parts.len() == 2 {
                                let ow = parts[0].parse::<u32>().unwrap_or(w);
                                let oh = parts[1].parse::<u32>().unwrap_or(h);
                                (ow, oh)
                            } else {
                                (w, h)
                            }
                        } else if let Ok(dims) = image::image_dimensions(source_path) {
                            let _ = fs::write(&meta_path, format!("{},{}", dims.0, dims.1));
                            dims
                        } else {
                            (w, h)
                        };

                        return Some((orig_w, orig_h, color_image));
                    }
                }
            }
        }

        // 2. Otherwise, generate high-quality WebP thumbnail
        let dyn_img = image::open(source_path).ok()?;
        let (orig_w, orig_h) = dyn_img.dimensions();

        let (thumb_w, thumb_h) = if orig_w > orig_h {
            let h = ((orig_h as f32 / orig_w as f32) * max_size as f32).round() as u32;
            (max_size, h.max(1))
        } else {
            let w = ((orig_w as f32 / orig_h as f32) * max_size as f32).round() as u32;
            (w.max(1), max_size)
        };

        let resized = dyn_img.resize(thumb_w, thumb_h, FilterType::Triangle);
        let (actual_w, actual_h) = resized.dimensions();
        let rgb = resized.to_rgb8();

        // Encode as WebP and save to cache
        let encoder = webp::Encoder::from_rgb(&rgb, actual_w, actual_h);
        let webp_memory = encoder.encode(85.0);
        let _ = fs::write(&cache_path, &*webp_memory);
        let _ = fs::write(&meta_path, format!("{},{}", orig_w, orig_h));

        // Return egui ColorImage
        let rgba = resized.to_rgba8();
        let color_image = ColorImage::from_rgba_unmultiplied(
            [actual_w as usize, actual_h as usize],
            rgba.as_flat_samples().as_slice(),
        );

        Some((orig_w, orig_h, color_image))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use std::time::Duration;

    #[test]
    fn test_cache_path_determinism() {
        let cache = ThumbnailCache {
            cache_dir: PathBuf::from("/tmp/test_cache"),
        };
        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(123456);
        let path1 = PathBuf::from("/path/to/image.png");
        let path2 = PathBuf::from("/path/to/other.png");

        let p1_a = cache.get_cache_path(&path1, mtime, 5000);
        let p1_b = cache.get_cache_path(&path1, mtime, 5000);
        let p2 = cache.get_cache_path(&path2, mtime, 5000);

        assert_eq!(p1_a, p1_b, "Cache path must be deterministic for identical inputs");
        assert_ne!(p1_a, p2, "Cache paths must differ for different files");
        assert!(p1_a.to_string_lossy().ends_with(".webp"));
    }

    #[test]
    fn test_load_or_generate_thumbnail_and_preserve_dimensions() {
        let temp_dir = std::env::temp_dir().join(format!("omniview_cache_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let cache = ThumbnailCache {
            cache_dir: temp_dir.join("cache"),
        };
        let _ = fs::create_dir_all(&cache.cache_dir);

        // Create a test 300x150 image
        let img_path = temp_dir.join("test_img.png");
        let mut img = RgbImage::new(300, 150);
        for pixel in img.pixels_mut() {
            *pixel = Rgb([50, 120, 200]);
        }
        img.save(&img_path).expect("Failed to save test image");

        let mtime = fs::metadata(&img_path).unwrap().modified().unwrap();
        let size = fs::metadata(&img_path).unwrap().len();

        // 1st run: Generates thumbnail and writes .webp + .meta
        let res1 = cache.load_or_generate_thumbnail(&img_path, mtime, size, 100);
        assert!(res1.is_some());
        let (ow1, oh1, color_img1) = res1.unwrap();
        assert_eq!(ow1, 300, "Original width must be 300");
        assert_eq!(oh1, 150, "Original height must be 150");
        assert!(color_img1.width() <= 100, "Thumbnail width should be clamped to max_size");

        // 2nd run: Loads from cache, must STILL return original dimensions
        let res2 = cache.load_or_generate_thumbnail(&img_path, mtime, size, 100);
        assert!(res2.is_some());
        let (ow2, oh2, color_img2) = res2.unwrap();
        assert_eq!(ow2, 300, "Cached load must report original width 300");
        assert_eq!(oh2, 150, "Cached load must report original height 150");
        assert_eq!(color_img1.width(), color_img2.width());
        assert_eq!(color_img1.height(), color_img2.height());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
