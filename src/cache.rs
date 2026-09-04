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
                        return Some((w, h, color_image));
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

        // Return egui ColorImage
        let rgba = resized.to_rgba8();
        let color_image = ColorImage::from_rgba_unmultiplied(
            [actual_w as usize, actual_h as usize],
            rgba.as_flat_samples().as_slice(),
        );

        Some((orig_w, orig_h, color_image))
    }
}
