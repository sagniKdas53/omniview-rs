mod app;
mod cache;
mod clipboard;
mod scanner;
mod watcher;

use app::OmniViewApp;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let initial_dir = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        let pixiv = PathBuf::from("/home/sagnik/Downloads/pixiv_downloads");
        if pixiv.exists() {
            pixiv
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
    };

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("OmniView RS - Standalone Rust Image Gallery")
            .with_app_id("omniview-rs"),
        ..Default::default()
    };

    eframe::run_native(
        "OmniView RS",
        native_options,
        Box::new(|_cc| Ok(Box::new(OmniViewApp::new(initial_dir)))),
    )
}
