use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, Color32, ColorImage, Key, Pos2, Rect, CornerRadius, Stroke, TextureHandle, TextureOptions, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;


use crate::cache::ThumbnailCache;
use crate::clipboard::{
    copy_file_uri, copy_files_uris, copy_image_bitmap, copy_multiple_paths,
    copy_text, open_in_default_app, reveal_in_file_manager, start_drag_files,
};
use crate::scanner::{scan_directory, ImageEntry};
use crate::watcher::FolderWatcher;

#[derive(PartialEq, Clone, Copy)]
pub enum AspectFilter {
    All,
    Portrait,
    Landscape,
    Square,
}

#[derive(PartialEq, Clone, Copy)]
pub enum SortMode {
    Newest,
    Oldest,
    SizeDesc,
    SizeAsc,
    NameAsc,
}

pub struct OmniViewApp {
    pub current_dir: PathBuf,
    pub all_entries: Vec<ImageEntry>,
    pub subfolders: Vec<(String, usize)>,
    pub selected_subfolder: Option<String>,
    pub folder_search: String,
    pub search_term: String,
    pub aspect_filter: AspectFilter,
    pub sort_mode: SortMode,
    pub card_size: f32,
    pub dark_mode: bool,

    // Batch Selection Mode
    pub select_mode: bool,
    pub selected_paths: HashSet<PathBuf>,

    // Caching & Async
    pub cache: Arc<ThumbnailCache>,
    pub textures: HashMap<PathBuf, TextureHandle>,
    pub pending_requests: HashSet<PathBuf>,
    pub thumb_tx: Sender<(PathBuf, u32, u32, ColorImage)>,
    pub thumb_rx: Receiver<(PathBuf, u32, u32, ColorImage)>,

    // Live watching
    pub watcher: Option<FolderWatcher>,
    pub watcher_rx: Receiver<()>,
    pub watcher_tx: Sender<()>,

    // Lightbox / Fullscreen Viewer
    pub viewing_index: Option<usize>,
    pub viewer_texture: Option<(PathBuf, TextureHandle)>,
    pub viewer_zoom: f32,
    pub viewer_pan: Vec2,
    pub viewer_is_fit: bool,

    // Status
    pub total_bytes: u64,
}

impl OmniViewApp {
    pub fn new(initial_dir: PathBuf) -> Self {
        let (thumb_tx, thumb_rx) = unbounded();
        let (watcher_tx, watcher_rx) = unbounded();
        let cache = Arc::new(ThumbnailCache::new());

        let mut app = Self {
            current_dir: initial_dir.clone(),
            all_entries: Vec::new(),
            subfolders: Vec::new(),
            selected_subfolder: None,
            folder_search: String::new(),
            search_term: String::new(),
            aspect_filter: AspectFilter::All,
            sort_mode: SortMode::Newest,
            card_size: 220.0,
            dark_mode: true,

            select_mode: false,
            selected_paths: HashSet::new(),

            cache,
            textures: HashMap::new(),
            pending_requests: HashSet::new(),
            thumb_tx,
            thumb_rx,

            watcher: None,
            watcher_rx,
            watcher_tx,

            viewing_index: None,
            viewer_texture: None,
            viewer_zoom: 1.0,
            viewer_pan: Vec2::ZERO,
            viewer_is_fit: true,

            total_bytes: 0,
        };

        app.load_directory(initial_dir);
        app
    }

    pub fn load_directory(&mut self, dir: PathBuf) {
        self.current_dir = dir.clone();
        self.textures.clear();
        self.pending_requests.clear();
        self.selected_paths.clear();
        self.viewing_index = None;
        self.viewer_texture = None;

        // Perform scan
        let (entries, folders) = scan_directory(&dir);
        self.total_bytes = entries.iter().map(|e| e.file_size).sum();
        self.all_entries = entries;
        self.subfolders = folders;
        self.selected_subfolder = None;

        // Start filesystem watcher
        let tx = self.watcher_tx.clone();
        self.watcher = FolderWatcher::new(&dir, tx).ok();
    }

    pub fn rescan(&mut self) {
        let (entries, folders) = scan_directory(&self.current_dir);
        self.total_bytes = entries.iter().map(|e| e.file_size).sum();
        self.all_entries = entries;
        self.subfolders = folders;
    }

    pub fn get_filtered_indices(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.all_entries.len())
            .filter(|&i| {
                let entry = &self.all_entries[i];

                // Subfolder filter
                if let Some(ref sub) = self.selected_subfolder {
                    if &entry.subfolder != sub {
                        return false;
                    }
                }

                // Search term filter
                if !self.search_term.is_empty() {
                    let term = self.search_term.to_lowercase();
                    if !entry.filename.to_lowercase().contains(&term)
                        && !entry.subfolder.to_lowercase().contains(&term)
                    {
                        return false;
                    }
                }

                // Aspect ratio filter (if dimensions are known)
                if entry.width > 0 && entry.height > 0 {
                    let ratio = entry.width as f32 / entry.height as f32;
                    match self.aspect_filter {
                        AspectFilter::All => true,
                        AspectFilter::Portrait => ratio < 0.85,
                        AspectFilter::Landscape => ratio > 1.18,
                        AspectFilter::Square => ratio >= 0.85 && ratio <= 1.18,
                    }
                } else {
                    true
                }
            })
            .collect();

        // Sorting
        match self.sort_mode {
            SortMode::Newest => indices.sort_by(|&a, &b| self.all_entries[b].mtime.cmp(&self.all_entries[a].mtime)),
            SortMode::Oldest => indices.sort_by(|&a, &b| self.all_entries[a].mtime.cmp(&self.all_entries[b].mtime)),
            SortMode::SizeDesc => indices.sort_by(|&a, &b| self.all_entries[b].file_size.cmp(&self.all_entries[a].file_size)),
            SortMode::SizeAsc => indices.sort_by(|&a, &b| self.all_entries[a].file_size.cmp(&self.all_entries[b].file_size)),
            SortMode::NameAsc => indices.sort_by(|&a, &b| self.all_entries[a].filename.cmp(&self.all_entries[b].filename)),
        }

        indices
    }

    fn request_thumbnail(&mut self, entry: &ImageEntry) {
        if self.pending_requests.contains(&entry.path) {
            return;
        }
        self.pending_requests.insert(entry.path.clone());

        let path = entry.path.clone();
        let mtime = entry.mtime;
        let file_size = entry.file_size;
        let cache = Arc::clone(&self.cache);
        let tx = self.thumb_tx.clone();

        rayon::spawn(move || {
            if let Some((w, h, color_img)) = cache.load_or_generate_thumbnail(&path, mtime, file_size, 340) {
                let _ = tx.send((path, w, h, color_img));
            }
        });
    }

    fn update_pending_thumbnails(&mut self, ctx: &egui::Context) {
        while let Ok((path, w, h, color_img)) = self.thumb_rx.try_recv() {
            let tex = ctx.load_texture(
                path.to_string_lossy(),
                color_img,
                TextureOptions::LINEAR,
            );
            self.textures.insert(path.clone(), tex);
            self.pending_requests.remove(&path);

            // Update dimensions on entry
            if let Some(entry) = self.all_entries.iter_mut().find(|e| e.path == path) {
                entry.width = w;
                entry.height = h;
            }
        }
    }

    fn render_lightbox(&mut self, ctx: &egui::Context) {
        let filtered_indices = self.get_filtered_indices();
        let total_in_view = filtered_indices.len();
        let Some(curr_idx) = self.viewing_index else { return };
        if curr_idx >= total_in_view {
            self.viewing_index = None;
            self.viewer_texture = None;
            return;
        }

        let global_idx = filtered_indices[curr_idx];
        let entry = self.all_entries[global_idx].clone();

        // Ensure full high-res texture is loaded
        if self.viewer_texture.as_ref().map(|(p, _)| p != &entry.path).unwrap_or(true) {
            if let Ok(dyn_img) = image::open(&entry.path) {
                let rgba = dyn_img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let color_image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_flat_samples().as_slice());
                let tex = ctx.load_texture("lightbox_full", color_image, TextureOptions::LINEAR);
                self.viewer_texture = Some((entry.path.clone(), tex));
                self.viewer_zoom = 1.0;
                self.viewer_pan = Vec2::ZERO;
                self.viewer_is_fit = true;
            }
        }

        let mut close_requested = false;
        let mut nav_next = false;
        let mut nav_prev = false;
        let mut toggle_fit = false;

        let title = format!("{} — OmniView Viewer ({} / {})", entry.filename, curr_idx + 1, total_in_view);

        let active_tex_clone = self.viewer_texture.as_ref()
            .map(|(_, tex)| tex.clone())
            .or_else(|| self.textures.get(&entry.path).cloned());

        let mut viewer_pan = self.viewer_pan;
        let mut viewer_zoom = self.viewer_zoom;
        let mut viewer_is_fit = self.viewer_is_fit;
        let dark_mode = self.dark_mode;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("omniview_viewer_subwindow"),
            egui::ViewportBuilder::default()
                .with_title(&title)
                .with_inner_size([1150.0, 860.0])
                .with_min_inner_size([640.0, 480.0])
                .with_app_id("omniview-viewer"),
            |sub_ctx, _class| {
                if dark_mode {
                    sub_ctx.set_visuals(egui::Visuals::dark());
                } else {
                    sub_ctx.set_visuals(egui::Visuals::light());
                }

                if sub_ctx.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }
                if sub_ctx.input(|i| i.key_pressed(Key::Escape)) {
                    close_requested = true;
                }
                if sub_ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
                    nav_prev = true;
                }
                if sub_ctx.input(|i| i.key_pressed(Key::ArrowRight)) {
                    nav_next = true;
                }
                if sub_ctx.input(|i| i.key_pressed(Key::Space)) {
                    toggle_fit = true;
                }

                egui::TopBottomPanel::top("viewer_top_panel").show(sub_ctx, |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                        let size_mb = entry.file_size as f64 / (1024.0 * 1024.0);
                        let dim_str = if entry.width > 0 {
                            format!("{} × {} px", entry.width, entry.height)
                        } else {
                            "Loading...".to_string()
                        };

                        ui.label(
                            egui::RichText::new(format!(
                                "{}  •  {}  •  {:.2} MB  •  📁 {}",
                                entry.filename, dim_str, size_mb, entry.subfolder
                            ))
                            .strong()
                            .size(13.0),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✕ Close (Esc)").clicked() {
                                close_requested = true;
                            }
                            if ui.button("↗️ Open Default").clicked() {
                                open_in_default_app(&entry.path);
                            }
                            if ui.button("🖼️ Copy Image").clicked() {
                                copy_image_bitmap(&entry.path);
                            }
                            if ui.button("📋 Copy Path").clicked() {
                                copy_text(&entry.path.to_string_lossy());
                            }
                            if ui.button(egui::RichText::new("📤 Drag to Attach").strong()).clicked() {
                                start_drag_files(&[entry.path.clone()]);
                            }
                            if ui.button(egui::RichText::new("📁 Copy File (Attach)").strong()).clicked() {
                                copy_files_uris(&[entry.path.clone()]);
                            }
                        });
                    });
                    ui.add_space(4.0);
                });

                egui::TopBottomPanel::bottom("viewer_bottom_panel").show(sub_ctx, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("◀ Previous (Left)").clicked() && curr_idx > 0 {
                            nav_prev = true;
                        }
                        if ui.button("Next (Right) ▶").clicked() && curr_idx + 1 < total_in_view {
                            nav_next = true;
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Fit to Window (Space)").clicked() {
                                viewer_is_fit = true;
                                viewer_pan = Vec2::ZERO;
                            }
                            if ui.button("1:1 (100%)").clicked() {
                                viewer_is_fit = false;
                                viewer_zoom = 1.0;
                                viewer_pan = Vec2::ZERO;
                            }
                            if ui.button("🔍 +").clicked() {
                                viewer_is_fit = false;
                                viewer_zoom *= 1.25;
                            }
                            if ui.button("🔍 -").clicked() {
                                viewer_is_fit = false;
                                viewer_zoom *= 0.8;
                            }
                        });
                    });
                    ui.add_space(4.0);
                });

                egui::CentralPanel::default().show(sub_ctx, |ui| {
                    let avail_rect = ui.available_rect_before_wrap();

                    if let Some(tex) = &active_tex_clone {
                        let img_size = tex.size_vec2();

                        let scale_x = avail_rect.width() / img_size.x;
                        let scale_y = avail_rect.height() / img_size.y;
                        let fit_scale = scale_x.min(scale_y) * 0.98;

                        let final_scale = if viewer_is_fit {
                            fit_scale
                        } else {
                            viewer_zoom
                        };

                        let display_size = img_size * final_scale;
                        let center_pos = avail_rect.center() + viewer_pan;
                        let img_rect = Rect::from_center_size(center_pos, display_size);

                        ui.painter().image(
                            tex.id(),
                            img_rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );

                        let response = ui.allocate_rect(avail_rect, egui::Sense::click_and_drag());
                        if response.dragged() {
                            viewer_pan += response.drag_delta();
                        }

                        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
                        if scroll_delta != 0.0 {
                            viewer_is_fit = false;
                            let factor = if scroll_delta > 0.0 { 1.15 } else { 1.0 / 1.15 };
                            viewer_zoom = (viewer_zoom * factor).clamp(0.05, 50.0);
                        }

                        if response.double_clicked() {
                            viewer_is_fit = !viewer_is_fit;
                            viewer_pan = Vec2::ZERO;
                            if !viewer_is_fit {
                                viewer_zoom = 1.0;
                            }
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.spinner();
                        });
                    }
                });
            },
        );

        if toggle_fit {
            viewer_is_fit = !viewer_is_fit;
            viewer_pan = Vec2::ZERO;
            if !viewer_is_fit {
                viewer_zoom = 1.0;
            }
        }

        self.viewer_pan = viewer_pan;
        self.viewer_zoom = viewer_zoom;
        self.viewer_is_fit = viewer_is_fit;

        if close_requested {
            self.viewing_index = None;
            self.viewer_texture = None;
        } else if nav_next && curr_idx + 1 < total_in_view {
            self.viewing_index = Some(curr_idx + 1);
            self.viewer_texture = None;
        } else if nav_prev && curr_idx > 0 {
            self.viewing_index = Some(curr_idx - 1);
            self.viewer_texture = None;
        }
    }
}

impl eframe::App for OmniViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply Dark / Light Mode
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Live file change watcher event
        if self.watcher_rx.try_recv().is_ok() {
            self.rescan();
        }

        // Receive loaded thumbnails
        self.update_pending_thumbnails(ctx);

        // 1. Top Panel
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);

                // Open Folder Button
                let folder_name = self.current_dir.file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Root".to_string());

                if ui.button(egui::RichText::new(format!("📁 {}", folder_name)).strong()).clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.load_directory(folder);
                    }
                }

                // Search Box
                ui.add(egui::TextEdit::singleline(&mut self.search_term).hint_text("🔍 Search images or folders..."));

                // Card Size Zoom Slider
                ui.label("Card Size:");
                ui.add(egui::Slider::new(&mut self.card_size, 110.0..=450.0).show_value(false));

                // Sort Mode
                egui::ComboBox::from_id_salt("sort_mode")
                    .selected_text(match self.sort_mode {
                        SortMode::Newest => "Newest First",
                        SortMode::Oldest => "Oldest First",
                        SortMode::SizeDesc => "Size (Large)",
                        SortMode::SizeAsc => "Size (Small)",
                        SortMode::NameAsc => "Name (A-Z)",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.sort_mode, SortMode::Newest, "Newest First");
                        ui.selectable_value(&mut self.sort_mode, SortMode::Oldest, "Oldest First");
                        ui.selectable_value(&mut self.sort_mode, SortMode::SizeDesc, "Size (Large)");
                        ui.selectable_value(&mut self.sort_mode, SortMode::SizeAsc, "Size (Small)");
                        ui.selectable_value(&mut self.sort_mode, SortMode::NameAsc, "Name (A-Z)");
                    });

                // Rescan Button
                if ui.button("🔄 Rescan").clicked() {
                    self.rescan();
                }

                // Batch Selection Toggle Button
                let select_btn_text = if self.select_mode {
                    format!("☑️ Selecting ({})", self.selected_paths.len())
                } else {
                    "☑️ Batch Select".to_string()
                };
                let select_color = if self.select_mode {
                    Color32::from_rgb(0, 210, 255)
                } else if self.dark_mode {
                    Color32::from_rgb(220, 225, 235)
                } else {
                    Color32::from_rgb(30, 40, 55)
                };
                if ui.button(egui::RichText::new(select_btn_text).color(select_color).strong()).clicked() {
                    self.select_mode = !self.select_mode;
                    if !self.select_mode {
                        self.selected_paths.clear();
                    }
                }

                // Dark / Light Mode Toggle
                let theme_icon = if self.dark_mode { "☀️ Light" } else { "🌙 Dark" };
                if ui.button(theme_icon).clicked() {
                    self.dark_mode = !self.dark_mode;
                }
            });

            ui.add_space(4.0);
            ui.separator();

            // Aspect Filter & Status Row
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Aspect:").strong());
                ui.selectable_value(&mut self.aspect_filter, AspectFilter::All, "All");
                ui.selectable_value(&mut self.aspect_filter, AspectFilter::Portrait, "📱 Portrait");
                ui.selectable_value(&mut self.aspect_filter, AspectFilter::Landscape, "🖥️ Landscape");
                ui.selectable_value(&mut self.aspect_filter, AspectFilter::Square, "🔲 Square");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let gb = self.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                    ui.label(format!("Total: {} images ({:.1} GB)", self.all_entries.len(), gb));
                });
            });
            ui.add_space(4.0);
        });

        // 2. Batch Selection Bottom Action Bar (when active)
        if self.select_mode || !self.selected_paths.is_empty() {
            egui::TopBottomPanel::bottom("batch_panel")
                .frame(
                    egui::Frame::side_top_panel(ctx.style().as_ref())
                        .fill(if self.dark_mode { Color32::from_rgb(15, 23, 42) } else { Color32::from_rgb(240, 245, 255) })
                        .stroke(Stroke::new(1.0_f32, if self.dark_mode { Color32::from_rgb(30, 58, 138) } else { Color32::from_rgb(180, 200, 240) }))
                )
                .show(ctx, |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(12.0, 0.0);

                        let count = self.selected_paths.len();
                        ui.label(
                            egui::RichText::new(format!("📦 {} item(s) selected", count))
                                .strong()
                                .size(13.0)
                                .color(if self.dark_mode { Color32::from_rgb(56, 189, 248) } else { Color32::from_rgb(2, 132, 199) }),
                        );

                        let selected_vec: Vec<PathBuf> = self.selected_paths.iter().cloned().collect();

                        // Drag & Drop
                        let drag_btn = ui.add_enabled(count > 0, egui::Button::new(format!("📤 Drag & Drop ({})", count)));
                        if drag_btn.clicked() {
                            start_drag_files(&selected_vec);
                        }

                        // Copy Files (Attach)
                        let copy_files_btn = ui.add_enabled(count > 0, egui::Button::new("📁 Copy Files (Attach)"));
                        if copy_files_btn.clicked() {
                            copy_files_uris(&selected_vec);
                        }

                        // Copy Paths
                        let copy_paths_btn = ui.add_enabled(count > 0, egui::Button::new("📋 Copy Paths"));
                        if copy_paths_btn.clicked() {
                            copy_multiple_paths(&selected_vec);
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✕ Exit Select Mode").clicked() {
                                self.select_mode = false;
                                self.selected_paths.clear();
                            }

                            if count > 0 && ui.button("Deselect All").clicked() {
                                self.selected_paths.clear();
                            }

                            let filtered = self.get_filtered_indices();
                            let total_filtered = filtered.len();
                            if ui.button(format!("✓ Select All ({})", total_filtered)).clicked() {
                                for &idx in &filtered {
                                    self.selected_paths.insert(self.all_entries[idx].path.clone());
                                }
                            }
                        });
                    });
                    ui.add_space(6.0);
                });
        }

        // 3. Left Sidebar (Subfolder Browser)
        egui::SidePanel::left("subfolder_sidebar")
            .resizable(true)
            .default_width(220.0)
            .width_range(160.0..=380.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(&mut self.folder_search).hint_text("Filter folders..."));
                ui.add_space(4.0);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    // All Images Button
                    let is_all_selected = self.selected_subfolder.is_none();
                    let all_text = format!("🖼️ All Images ({})", self.all_entries.len());
                    if ui.selectable_label(is_all_selected, all_text).clicked() {
                        self.selected_subfolder = None;
                    }

                    ui.separator();

                    let filter_term = self.folder_search.to_lowercase();
                    for (folder, count) in &self.subfolders {
                        let label_name = if folder == "__root__" {
                            format!("📁 Root ({})", count)
                        } else {
                            format!("📁 {} ({})", folder, count)
                        };

                        if !filter_term.is_empty() && !label_name.to_lowercase().contains(&filter_term) {
                            continue;
                        }

                        let is_selected = self.selected_subfolder.as_ref() == Some(folder);
                        if ui.selectable_label(is_selected, label_name).clicked() {
                            self.selected_subfolder = Some(folder.clone());
                        }
                    }
                });
            });

        // 3. Central Gallery Grid
        egui::CentralPanel::default().show(ctx, |ui| {
            let filtered_indices = self.get_filtered_indices();
            let total_items = filtered_indices.len();

            let avail_width = ui.available_width();
            let spacing = 8.0;
            let card_w = self.card_size;
            let card_h = card_w + 38.0;

            let cols = ((avail_width + spacing) / (card_w + spacing)).floor().max(1.0) as usize;
            let num_rows = (total_items + cols - 1) / cols;

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show_rows(ui, card_h + spacing, num_rows, |ui, row_range| {
                    for row in row_range {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(spacing, spacing);
                            for col in 0..cols {
                                let item_idx_in_filtered = row * cols + col;
                                if item_idx_in_filtered < total_items {
                                    let global_idx = filtered_indices[item_idx_in_filtered];
                                    let entry = self.all_entries[global_idx].clone();

                                    // Request thumbnail if not loaded
                                    if !self.textures.contains_key(&entry.path) {
                                        self.request_thumbnail(&entry);
                                    }

                                    let is_selected = self.selected_paths.contains(&entry.path);

                                    // Allocate interactive card rect with click & drag sensing
                                    let (card_rect, card_resp) = ui.allocate_exact_size(
                                        Vec2::new(card_w, card_h),
                                        egui::Sense::click_and_drag(),
                                    );

                                    // Drag-and-drop initiation!
                                    if card_resp.drag_started() {
                                        if is_selected && self.selected_paths.len() > 1 {
                                            let files: Vec<PathBuf> = self.selected_paths.iter().cloned().collect();
                                            start_drag_files(&files);
                                        } else {
                                            start_drag_files(&[entry.path.clone()]);
                                        }
                                    }

                                    // Click handling: toggle select if select mode or Ctrl, else open viewer
                                    if card_resp.clicked() {
                                        if self.select_mode || ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
                                            if !self.select_mode {
                                                self.select_mode = true;
                                            }
                                            if is_selected {
                                                self.selected_paths.remove(&entry.path);
                                            } else {
                                                self.selected_paths.insert(entry.path.clone());
                                            }
                                        } else {
                                            self.viewing_index = Some(item_idx_in_filtered);
                                        }
                                    }

                                    // Double click opens Lightbox even in select mode
                                    if card_resp.double_clicked() {
                                        self.viewing_index = Some(item_idx_in_filtered);
                                    }

                                    // Hover styling & pointer cursor
                                    let is_hovered = card_resp.hovered();
                                    if is_hovered {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }

                                    let bg_color = if is_selected {
                                        if self.dark_mode { Color32::from_rgb(18, 38, 62) } else { Color32::from_rgb(224, 242, 254) }
                                    } else if is_hovered {
                                        if self.dark_mode { Color32::from_rgb(34, 40, 56) } else { Color32::from_rgb(235, 242, 252) }
                                    } else {
                                        if self.dark_mode { Color32::from_rgb(22, 25, 34) } else { Color32::from_rgb(248, 250, 252) }
                                    };

                                    let stroke = if is_selected {
                                        Stroke::new(2.0_f32, Color32::from_rgb(0, 210, 255))
                                    } else if is_hovered {
                                        Stroke::new(1.5_f32, if self.dark_mode { Color32::from_rgb(90, 160, 255) } else { Color32::from_rgb(40, 120, 230) })
                                    } else {
                                        Stroke::new(1.0_f32, if self.dark_mode { Color32::from_rgb(45, 50, 65) } else { Color32::from_rgb(220, 225, 235) })
                                    };

                                    ui.painter().rect(card_rect, CornerRadius::same(6), bg_color, stroke, egui::StrokeKind::Inside);

                                    // Thumbnail bounds inside card
                                    let pad = 4.0;
                                    let thumb_h = card_w - (pad * 2.0);
                                    let thumb_area = Rect::from_min_size(
                                        card_rect.min + Vec2::new(pad, pad),
                                        Vec2::new(card_w - (pad * 2.0), thumb_h),
                                    );

                                    if let Some(tex) = self.textures.get(&entry.path) {
                                        let t_size = tex.size_vec2();
                                        let scale = (thumb_area.width() / t_size.x).min(thumb_area.height() / t_size.y);
                                        let draw_size = t_size * scale;
                                        let draw_rect = Rect::from_center_size(thumb_area.center(), draw_size);

                                        ui.painter().image(
                                            tex.id(),
                                            draw_rect,
                                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                            Color32::WHITE,
                                        );
                                    } else {
                                        ui.painter().rect_filled(
                                            thumb_area,
                                            CornerRadius::same(4),
                                            if self.dark_mode { Color32::from_rgb(28, 32, 44) } else { Color32::from_rgb(230, 235, 245) },
                                        );
                                    }

                                    // Batch Selection Checkbox Indicator
                                    if self.select_mode || is_selected {
                                        let check_box_rect = Rect::from_min_size(card_rect.min + Vec2::new(8.0, 8.0), Vec2::new(20.0, 20.0));
                                        if is_selected {
                                            ui.painter().rect_filled(check_box_rect, CornerRadius::same(4), Color32::from_rgb(0, 180, 255));
                                            ui.painter().text(
                                                check_box_rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "✓",
                                                egui::FontId::proportional(13.0),
                                                Color32::WHITE,
                                            );
                                        } else {
                                            ui.painter().rect_filled(check_box_rect, CornerRadius::same(4), Color32::from_black_alpha(150));
                                            ui.painter().rect_stroke(
                                                check_box_rect,
                                                CornerRadius::same(4),
                                                Stroke::new(1.5_f32, Color32::from_white_alpha(200)),
                                                egui::StrokeKind::Inside,
                                            );
                                        }
                                    }

                                    // Filename label
                                    let text_color = if self.dark_mode { Color32::from_rgb(220, 225, 235) } else { Color32::from_rgb(30, 40, 55) };
                                    let text_pos = Pos2::new(card_rect.min.x + pad + 2.0, thumb_area.max.y + 4.0);
                                    let max_chars = ((card_w - pad * 2.0) / 7.0).max(8.0) as usize;
                                    let display_name = if entry.filename.chars().count() > max_chars {
                                        let s: String = entry.filename.chars().take(max_chars.saturating_sub(3)).collect();
                                        format!("{}...", s)
                                    } else {
                                        entry.filename.clone()
                                    };

                                    ui.painter().text(
                                        text_pos,
                                        egui::Align2::LEFT_TOP,
                                        display_name,
                                        egui::FontId::proportional(11.0),
                                        text_color,
                                    );

                                    // Context Menu on right click
                                    card_resp.context_menu(|ui| {
                                        if ui.button("🔍 Open in Viewer").clicked() {
                                            self.viewing_index = Some(item_idx_in_filtered);
                                            ui.close_menu();
                                        }
                                        if ui.button("📤 Drag to Share / Attach").clicked() {
                                            if is_selected && self.selected_paths.len() > 1 {
                                                let files: Vec<PathBuf> = self.selected_paths.iter().cloned().collect();
                                                start_drag_files(&files);
                                            } else {
                                                start_drag_files(&[entry.path.clone()]);
                                            }
                                            ui.close_menu();
                                        }
                                        if ui.button(if is_selected { "☑️ Deselect" } else { "☑️ Select" }).clicked() {
                                            if !self.select_mode {
                                                self.select_mode = true;
                                            }
                                            if is_selected {
                                                self.selected_paths.remove(&entry.path);
                                            } else {
                                                self.selected_paths.insert(entry.path.clone());
                                            }
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        if ui.button("📁 Copy File (Attach)").clicked() {
                                            copy_file_uri(&entry.path);
                                            ui.close_menu();
                                        }
                                        if ui.button("📋 Copy Path").clicked() {
                                            copy_text(&entry.path.to_string_lossy());
                                            ui.close_menu();
                                        }
                                        if ui.button("🖼️ Copy Image").clicked() {
                                            copy_image_bitmap(&entry.path);
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        let sub = entry.subfolder.clone();
                                        if ui.button(format!("📂 Show Folder: {}", sub)).clicked() {
                                            self.selected_subfolder = Some(sub);
                                            ui.close_menu();
                                        }
                                        if ui.button("↗️ Open in Default Viewer").clicked() {
                                            open_in_default_app(&entry.path);
                                            ui.close_menu();
                                        }
                                        if ui.button("🗂️ Reveal in File Manager").clicked() {
                                            reveal_in_file_manager(&entry.path);
                                            ui.close_menu();
                                        }
                                    });
                                }
                            }
                        });
                    }
                });
        });

        // 4. Lightbox Viewer Overlay (if active)
        self.render_lightbox(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn make_test_entry(name: &str, sub: &str, size: u64, w: u32, h: u32, secs_ago: u64) -> ImageEntry {
        ImageEntry {
            path: PathBuf::from(format!("/fake/{}/{}", sub, name)),
            filename: name.to_string(),
            subfolder: sub.to_string(),
            file_size: size,
            mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - secs_ago),
            width: w,
            height: h,
        }
    }

    fn create_test_app() -> OmniViewApp {
        let (thumb_tx, thumb_rx) = unbounded();
        let (watcher_tx, watcher_rx) = unbounded();
        let cache = Arc::new(ThumbnailCache {
            cache_dir: PathBuf::from("/tmp/test_cache_app"),
        });

        let entries = vec![
            make_test_entry("portrait.png", "folder_a", 1000, 800, 1200, 10),  // ratio ~0.66 (Portrait)
            make_test_entry("landscape.jpg", "folder_a", 5000, 1920, 1080, 50), // ratio ~1.77 (Landscape)
            make_test_entry("square.webp", "folder_b", 2000, 1000, 1000, 20),  // ratio 1.0 (Square)
            make_test_entry("zeta.png", "folder_b", 8000, 1600, 900, 100),     // Landscape, largest
            make_test_entry("alpha.png", "folder_c", 500, 600, 900, 5),        // Portrait, smallest, newest
        ];

        OmniViewApp {
            current_dir: PathBuf::from("/fake"),
            all_entries: entries,
            subfolders: vec![("folder_a".into(), 2), ("folder_b".into(), 2), ("folder_c".into(), 1)],
            selected_subfolder: None,
            folder_search: String::new(),
            search_term: String::new(),
            aspect_filter: AspectFilter::All,
            sort_mode: SortMode::Newest,
            card_size: 220.0,
            dark_mode: true,
            select_mode: false,
            selected_paths: HashSet::new(),
            cache,
            textures: HashMap::new(),
            pending_requests: HashSet::new(),
            thumb_tx,
            thumb_rx,
            watcher: None,
            watcher_rx,
            watcher_tx,
            viewing_index: None,
            viewer_texture: None,
            viewer_zoom: 1.0,
            viewer_pan: Vec2::ZERO,
            viewer_is_fit: true,
            total_bytes: 16500,
        }
    }

    #[test]
    fn test_filter_by_subfolder() {
        let mut app = create_test_app();
        assert_eq!(app.get_filtered_indices().len(), 5);

        app.selected_subfolder = Some("folder_a".to_string());
        let indices = app.get_filtered_indices();
        assert_eq!(indices.len(), 2);
        for &idx in &indices {
            assert_eq!(app.all_entries[idx].subfolder, "folder_a");
        }

        app.selected_subfolder = Some("folder_c".to_string());
        let indices = app.get_filtered_indices();
        assert_eq!(indices.len(), 1);
        assert_eq!(app.all_entries[indices[0]].filename, "alpha.png");
    }

    #[test]
    fn test_filter_by_search_term() {
        let mut app = create_test_app();

        // Search by partial filename
        app.search_term = "LANDSCAPE".to_string(); // case-insensitive
        let indices = app.get_filtered_indices();
        assert_eq!(indices.len(), 1);
        assert_eq!(app.all_entries[indices[0]].filename, "landscape.jpg");

        // Search matching subfolder name
        app.search_term = "folder_b".to_string();
        let indices = app.get_filtered_indices();
        assert_eq!(indices.len(), 2);
    }

    #[test]
    fn test_filter_by_aspect_ratio() {
        let mut app = create_test_app();

        // Portrait (< 0.85)
        app.aspect_filter = AspectFilter::Portrait;
        let p_indices = app.get_filtered_indices();
        assert_eq!(p_indices.len(), 2); // portrait.png (800x1200) and alpha.png (600x900)

        // Landscape (> 1.18)
        app.aspect_filter = AspectFilter::Landscape;
        let l_indices = app.get_filtered_indices();
        assert_eq!(l_indices.len(), 2); // landscape.jpg (1920x1080) and zeta.png (1600x900)

        // Square (0.85 .. 1.18)
        app.aspect_filter = AspectFilter::Square;
        let s_indices = app.get_filtered_indices();
        assert_eq!(s_indices.len(), 1);
        assert_eq!(app.all_entries[s_indices[0]].filename, "square.webp");
    }

    #[test]
    fn test_sort_modes() {
        let mut app = create_test_app();

        // Name Ascending
        app.sort_mode = SortMode::NameAsc;
        let indices = app.get_filtered_indices();
        let names: Vec<&str> = indices.iter().map(|&i| app.all_entries[i].filename.as_str()).collect();
        assert_eq!(names, vec!["alpha.png", "landscape.jpg", "portrait.png", "square.webp", "zeta.png"]);

        // Size Descending
        app.sort_mode = SortMode::SizeDesc;
        let indices = app.get_filtered_indices();
        let sizes: Vec<u64> = indices.iter().map(|&i| app.all_entries[i].file_size).collect();
        assert_eq!(sizes, vec![8000, 5000, 2000, 1000, 500]);

        // Size Ascending
        app.sort_mode = SortMode::SizeAsc;
        let indices = app.get_filtered_indices();
        let sizes: Vec<u64> = indices.iter().map(|&i| app.all_entries[i].file_size).collect();
        assert_eq!(sizes, vec![500, 1000, 2000, 5000, 8000]);

        // Newest First (smallest secs_ago)
        app.sort_mode = SortMode::Newest;
        let indices = app.get_filtered_indices();
        assert_eq!(app.all_entries[indices[0]].filename, "alpha.png"); // 5 secs ago

        // Oldest First (largest secs_ago)
        app.sort_mode = SortMode::Oldest;
        let indices = app.get_filtered_indices();
        assert_eq!(app.all_entries[indices[0]].filename, "zeta.png"); // 100 secs ago
    }

    #[test]
    fn test_batch_selection_logic() {
        let mut app = create_test_app();
        assert!(!app.select_mode);
        assert_eq!(app.selected_paths.len(), 0);

        // Turn on select mode
        app.select_mode = true;

        // Toggle select an item
        let path1 = app.all_entries[0].path.clone();
        app.selected_paths.insert(path1.clone());
        assert_eq!(app.selected_paths.len(), 1);
        assert!(app.selected_paths.contains(&path1));

        // Select All filtered items
        let filtered = app.get_filtered_indices();
        for &idx in &filtered {
            app.selected_paths.insert(app.all_entries[idx].path.clone());
        }
        assert_eq!(app.selected_paths.len(), 5);

        // Deselect one item
        app.selected_paths.remove(&path1);
        assert_eq!(app.selected_paths.len(), 4);
        assert!(!app.selected_paths.contains(&path1));

        // Clear all
        app.selected_paths.clear();
        assert_eq!(app.selected_paths.len(), 0);
    }
}
