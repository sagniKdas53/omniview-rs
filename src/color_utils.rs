use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};

pub const COLOR_PALETTE: &[(&str, &str, &str)] = &[
    ("all", "All", "#888888"),
    ("red", "Red", "#ef4444"),
    ("orange", "Orange", "#f97316"),
    ("yellow", "Yellow", "#eab308"),
    ("green", "Green", "#22c55e"),
    ("cyan", "Cyan", "#06b6d4"),
    ("blue", "Blue", "#3b82f6"),
    ("purple", "Purple", "#a855f7"),
    ("pink", "Pink", "#ec4899"),
    ("white", "Light", "#f1f5f9"),
    ("black", "Dark", "#1e293b"),
];

pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let v = max;
    let s = if max > 0.0 { delta / max } else { 0.0 };

    let h = if delta > 0.0 {
        let raw = if max == r {
            (g - b) / delta + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        raw / 6.0
    } else {
        0.0
    };

    (h, s, v)
}

pub fn classify_hsv(h: f32, s: f32, v: f32) -> &'static str {
    if v < 0.18 {
        return "black";
    }
    if s < 0.16 {
        if v > 0.80 {
            return "white";
        }
        return "gray";
    }

    let deg = (h * 360.0) % 360.0;
    if deg < 15.0 || deg >= 345.0 {
        "red"
    } else if deg < 45.0 {
        "orange"
    } else if deg < 70.0 {
        "yellow"
    } else if deg < 160.0 {
        "green"
    } else if deg < 200.0 {
        "cyan"
    } else if deg < 260.0 {
        "blue"
    } else if deg < 310.0 {
        "purple"
    } else {
        "pink"
    }
}

pub fn get_aspect_type(width: u32, height: u32) -> &'static str {
    if width == 0 || height == 0 {
        return "square";
    }
    let ratio = width as f32 / height as f32;
    if ratio < 0.85 {
        "portrait"
    } else if ratio > 1.18 {
        "landscape"
    } else {
        "square"
    }
}

pub fn compute_dhash(img: &DynamicImage) -> String {
    let gray = img.to_luma8();
    let resized = image::imageops::resize(&gray, 9, 8, FilterType::Triangle);

    let mut hash: u64 = 0;
    for y in 0..8 {
        for x in 0..8 {
            let p_left = resized.get_pixel(x, y)[0];
            let p_right = resized.get_pixel(x + 1, y)[0];
            let bit = if p_right > p_left { 1 } else { 0 };
            hash = (hash << 1) | bit;
        }
    }

    format!("{:016x}", hash)
}

pub fn hamming_distance(h1: &str, h2: &str) -> u32 {
    let v1 = u64::from_str_radix(h1, 16).unwrap_or(0);
    let v2 = u64::from_str_radix(h2, 16).unwrap_or(0);
    (v1 ^ v2).count_ones()
}

pub fn extract_image_features(
    img: &DynamicImage,
) -> (u32, u32, &'static str, &'static str, u8, u8, u8, String) {
    let (orig_w, orig_h) = img.dimensions();
    let aspect = get_aspect_type(orig_w, orig_h);

    let tiny = img.resize_exact(16, 16, FilterType::Triangle).to_rgba8();
    let mut total_r = 0u64;
    let mut total_g = 0u64;
    let mut total_b = 0u64;
    let pixel_count = 16 * 16;

    for pixel in tiny.pixels() {
        total_r += pixel[0] as u64;
        total_g += pixel[1] as u64;
        total_b += pixel[2] as u64;
    }

    let avg_r = (total_r / pixel_count) as u8;
    let avg_g = (total_g / pixel_count) as u8;
    let avg_b = (total_b / pixel_count) as u8;

    let (h, s, v) = rgb_to_hsv(
        avg_r as f32 / 255.0,
        avg_g as f32 / 255.0,
        avg_b as f32 / 255.0,
    );
    let color_name = classify_hsv(h, s, v);

    let dhash = compute_dhash(img);

    (orig_w, orig_h, aspect, color_name, avg_r, avg_g, avg_b, dhash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hamming_distance() {
        let h1 = "0000000000000000";
        let h2 = "0000000000000001";
        assert_eq!(hamming_distance(h1, h2), 1);

        let h3 = "ffffffffffffffff";
        assert_eq!(hamming_distance(h1, h3), 64);
    }

    #[test]
    fn test_aspect_type() {
        assert_eq!(get_aspect_type(1000, 2000), "portrait");
        assert_eq!(get_aspect_type(2000, 1000), "landscape");
        assert_eq!(get_aspect_type(1000, 1000), "square");
    }

    #[test]
    fn test_classify_hsv() {
        assert_eq!(classify_hsv(0.0, 0.9, 0.9), "red");
        assert_eq!(classify_hsv(0.33, 0.9, 0.9), "green");
        assert_eq!(classify_hsv(0.66, 0.9, 0.9), "blue");
    }

    #[test]
    fn test_extract_features_and_dhash() {
        use image::DynamicImage;
        let img = DynamicImage::new_rgb8(100, 200);
        let (w, h, aspect, color, _r, _g, _b, dhash) = extract_image_features(&img);
        assert_eq!(w, 100);
        assert_eq!(h, 200);
        assert_eq!(aspect, "portrait");
        assert_eq!(color, "black");
        assert_eq!(dhash.len(), 16);
    }
}
