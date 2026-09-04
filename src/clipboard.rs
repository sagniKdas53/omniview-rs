use arboard::{Clipboard, ImageData};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn format_file_uri<P: AsRef<Path>>(path: P) -> String {
    let p = path.as_ref();
    let abs_path = match std::fs::canonicalize(p) {
        Ok(c) => c.to_string_lossy().to_string(),
        Err(_) => p.to_string_lossy().to_string(),
    };
    format!("file://{}", abs_path)
}

pub fn format_uri_list<P: AsRef<Path>>(paths: &[P]) -> String {
    let mut list = String::new();
    for p in paths {
        list.push_str(&format_file_uri(p));
        list.push_str("\r\n");
    }
    list
}

pub fn copy_text(text: &str) {
    if let Ok(mut clip) = Clipboard::new() {
        let _ = clip.set_text(text);
    }
}

pub fn copy_file_uri<P: AsRef<Path>>(path: P) {
    copy_files_uris(&[path]);
}

pub fn copy_files_uris<P: AsRef<Path>>(paths: &[P]) {
    if paths.is_empty() {
        return;
    }

    let uri_list = format_uri_list(paths);

    // Try setting clipboard via arboard text
    if let Ok(mut clip) = Clipboard::new() {
        let _ = clip.set_text(uri_list.trim());
    }

    // On Linux X11/Wayland, also try setting text/uri-list via xclip or wl-copy if available
    let _ = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/uri-list"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(uri_list.as_bytes());
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
                let _ = stdin.write_all(uri_list.as_bytes());
            }
            child.wait()
        });
}

pub fn copy_multiple_paths<P: AsRef<Path>>(paths: &[P]) {
    if paths.is_empty() {
        return;
    }
    let combined = paths
        .iter()
        .map(|p| p.as_ref().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    copy_text(&combined);
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

/// Find the dragon binary on the system
fn find_dragon_binary() -> Option<PathBuf> {
    // 1. ~/.local/bin/dragon
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".local/bin/dragon");
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. Relative to current exe / assets
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate1 = parent.join("dragon");
            if candidate1.is_file() {
                return Some(candidate1);
            }
            let candidate2 = parent.join("../assets/dragon");
            if candidate2.is_file() {
                return Some(candidate2);
            }
        }
    }

    // 3. System PATH
    if let Ok(output) = Command::new("which").arg("dragon").output() {
        if output.status.success() {
            let p_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !p_str.is_empty() {
                return Some(PathBuf::from(p_str));
            }
        }
    }

    None
}

/// Starts an interactive Linux drag-and-drop session to drag files into Discord, Slack, etc.
pub fn start_drag_files<P: AsRef<Path>>(paths: &[P]) {
    if paths.is_empty() {
        return;
    }

    let file_args: Vec<String> = paths
        .iter()
        .map(|p| p.as_ref().to_string_lossy().to_string())
        .collect();

    // Try dragon utility first
    if let Some(dragon_path) = find_dragon_binary() {
        let mut cmd = Command::new(dragon_path);
        // -x: exit after completed drop
        // -a: drag all files at once
        // -T: always on top
        // -s 120: preview size
        cmd.args(["-x", "-a", "-T", "-s", "120"]);
        cmd.args(&file_args);
        if cmd.spawn().is_ok() {
            return;
        }
    }

    // Fallback: Python PyQt5 drag helper
    let py_script = r#"
import sys
from PyQt5.QtWidgets import QApplication, QWidget
from PyQt5.QtCore import Qt, QUrl, QMimeData, QPoint
from PyQt5.QtGui import QDrag, QPixmap, QColor, QPainter

app = QApplication(sys.argv)
files = sys.argv[1:]

w = QWidget()
w.setWindowFlags(Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.Tool)
w.setAttribute(Qt.WA_TranslucentBackground)
pos = app.desktop().cursor().pos()
w.setGeometry(pos.x() - 50, pos.y() - 30, 100, 60)
w.show()

def do_drag():
    drag = QDrag(w)
    mime = QMimeData()
    mime.setUrls([QUrl.fromLocalFile(f) for f in files])
    mime.setText("\n".join(files))
    drag.setMimeData(mime)

    badge = QPixmap(120, 36)
    badge.fill(QColor(15, 23, 42, 240))
    p = QPainter(badge)
    p.setPen(QColor(56, 189, 248))
    p.drawText(badge.rect(), Qt.AlignCenter, f"📤 {len(files)} file(s)")
    p.end()
    drag.setPixmap(badge)
    drag.setHotSpot(QPoint(60, 18))
    drag.exec_(Qt.CopyAction)
    app.quit()

from PyQt5.QtCore import QTimer
QTimer.singleShot(50, do_drag)
app.exec_()
"#;

    let _ = Command::new("python3")
        .arg("-c")
        .arg(py_script)
        .args(&file_args)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_uri() {
        let path = Path::new("/tmp/test_image.png");
        let uri = format_file_uri(path);
        assert!(uri.starts_with("file://"));
        assert!(uri.ends_with("test_image.png"));
    }

    #[test]
    fn test_format_uri_list() {
        let paths = vec![
            PathBuf::from("/tmp/img1.png"),
            PathBuf::from("/tmp/img2.jpg"),
        ];
        let uri_list = format_uri_list(&paths);
        assert!(uri_list.contains("file://"));
        assert!(uri_list.contains("img1.png"));
        assert!(uri_list.contains("img2.jpg"));
        assert!(uri_list.ends_with("\r\n"));
    }
}
