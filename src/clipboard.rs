use arboard::{Clipboard, ImageData};
use std::borrow::Cow;
use std::path::Path;
use std::process::Command;

pub fn copy_text(text: &str) {
    if let Ok(mut clip) = Clipboard::new() {
        let _ = clip.set_text(text);
    }
}

pub fn copy_file_uri<P: AsRef<Path>>(path: P) {
    let p = path.as_ref();
    let abs_path = match std::fs::canonicalize(p) {
        Ok(c) => c.to_string_lossy().to_string(),
        Err(_) => p.to_string_lossy().to_string(),
    };
    let uri = format!("file://{}", abs_path);

    // Try setting clipboard via arboard text
    if let Ok(mut clip) = Clipboard::new() {
        let _ = clip.set_text(&uri);
    }

    // On Linux X11/Wayland, also try setting text/uri-list via xclip or wl-copy if available
    let uri_with_crlf = format!("{}\r\n", uri);
    let _ = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/uri-list"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(uri_with_crlf.as_bytes());
            }
            child.wait()
        });

    let _ = Command::new("wl-copy")
        .args(["-t", "text/uri-list"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(uri_with_crlf.as_bytes());
            }
            child.wait()
        });
}

pub fn copy_image_bitmap<P: AsRef<Path>>(path: P) {
    if let Ok(dyn_img) = image::open(path.as_ref()) {
        let rgba = dyn_img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let img_data = ImageData {
            width: w as usize,
            height: h as usize,
            bytes: Cow::Owned(rgba.into_raw()),
        };
        if let Ok(mut clip) = Clipboard::new() {
            let _ = clip.set_image(img_data);
        }
    }
}

pub fn open_in_default_app<P: AsRef<Path>>(path: P) {
    let _ = Command::new("xdg-open").arg(path.as_ref()).spawn();
}

pub fn reveal_in_file_manager<P: AsRef<Path>>(path: P) {
    let p = path.as_ref();
    let res = Command::new("nautilus").arg("--select").arg(p).spawn();
    if res.is_err() {
        if let Some(parent) = p.parent() {
            let _ = Command::new("xdg-open").arg(parent).spawn();
        }
    }
}
