use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, Color32, ColorImage, Frame, Key, Pos2, Rect, CornerRadius, Stroke, TextureHandle, TextureOptions, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;


use crate::cache::ThumbnailCache;
use crate::clipboard::{copy_file_uri, copy_image_bitmap, copy_text, open_in_default_app, reveal_in_file_manager};
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
        let Some(curr_idx) = self.viewing_index else { return };
        if curr_idx >= self.all_entries.len() {
            self.viewing_index = None;
            return;
        }

        let entry = self.all_entries[curr_idx].clone();

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

        // Fullscreen Overlay
        egui::Area::new(egui::Id::new("lightbox_overlay"))
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                let screen_rect = ctx.screen_rect();
                ui.painter().rect_filled(screen_rect, CornerRadius::ZERO, Color32::from_black_alpha(240));

                let mut top_rect = screen_rect;
                top_rect.max.y = top_rect.min.y + 48.0;

                // Top Toolbar
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(top_rect), |ui| {
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
                                "{}  •  {}  •  {:.2} MB  •  📁 {}  ({} / {})",
                                entry.filename, dim_str, size_mb, entry.subfolder, curr_idx + 1, self.all_entries.len()
                            ))
                            .color(Color32::WHITE)
                            .strong()
                            .size(13.0),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✕ Close (Esc)").clicked() {
                                self.viewing_index = None;
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
                            // Direct file copy / attachment button
                            if ui.button("📁 Copy File (Attach)").clicked() {
                                copy_file_uri(&entry.path);
                            }
                        });
                    });
                });

                // Central Image Viewport
                let mut view_rect = screen_rect;
                view_rect.min.y += 48.0;
                view_rect.max.y -= 48.0;

                if let Some((_, ref tex)) = self.viewer_texture {
                    let img_size = tex.size_vec2();

                    // Calculate sensible Fit Scale (fills ~95% of available space)
                    let scale_x = view_rect.width() / img_size.x;
                    let scale_y = view_rect.height() / img_size.y;
                    let fit_scale = scale_x.min(scale_y) * 0.95;

                    let final_scale = if self.viewer_is_fit {
                        fit_scale
                    } else {
                        self.viewer_zoom
                    };

                    let display_size = img_size * final_scale;
                    let center_pos = view_rect.center() + self.viewer_pan;
                    let img_rect = Rect::from_center_size(center_pos, display_size);

                    // Render the image
                    ui.painter().image(
                        tex.id(),
                        img_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );

                    // Pan and zoom interaction
                    let response = ui.allocate_rect(view_rect, egui::Sense::click_and_drag());
                    if response.dragged() {
                        self.viewer_pan += response.drag_delta();
                    }

                    // Mouse wheel zoom
                    let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
                    if scroll_delta != 0.0 {
                        self.viewer_is_fit = false;
                        let factor = if scroll_delta > 0.0 { 1.15 } else { 1.0 / 1.15 };
                        self.viewer_zoom *= factor;
                    }

                    // Double click toggle fit / 100%
                    if response.double_clicked() {
                        self.viewer_is_fit = !self.viewer_is_fit;
                        self.viewer_pan = Vec2::ZERO;
                        if !self.viewer_is_fit {
                            self.viewer_zoom = 1.0;
                        }
                    }
                }

                // Bottom Navigation Bar
                let mut bot_rect = screen_rect;
                bot_rect.min.y = bot_rect.max.y - 48.0;

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bot_rect), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("◀ Previous (Left)").clicked() && curr_idx > 0 {
                            self.viewing_index = Some(curr_idx - 1);
                        }
                        if ui.button("Next (Right) ▶").clicked() && curr_idx + 1 < self.all_entries.len() {
                            self.viewing_index = Some(curr_idx + 1);
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Fit to Window").clicked() {
                                self.viewer_is_fit = true;
                                self.viewer_pan = Vec2::ZERO;
                            }
                            if ui.button("1:1 (100%)").clicked() {
                                self.viewer_is_fit = false;
                                self.viewer_zoom = 1.0;
                                self.viewer_pan = Vec2::ZERO;
                            }
                            if ui.button("🔍 +").clicked() {
                                self.viewer_is_fit = false;
                                self.viewer_zoom *= 1.25;
                            }
                            if ui.button("🔍 -").clicked() {
                                self.viewer_is_fit = false;
                                self.viewer_zoom *= 0.8;
                            }
                        });
                    });
                });
            });

        // Shortcuts
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.viewing_index = None;
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) && curr_idx > 0 {
            self.viewing_index = Some(curr_idx - 1);
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowRight)) && curr_idx + 1 < self.all_entries.len() {
            self.viewing_index = Some(curr_idx + 1);
        }
        if ctx.input(|i| i.key_pressed(Key::Space)) {
            self.viewer_is_fit = !self.viewer_is_fit;
            self.viewer_pan = Vec2::ZERO;
            if !self.viewer_is_fit {
                self.viewer_zoom = 1.0;
            }
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

        // 2. Left Sidebar (Subfolder Browser)
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

                                    // Card Frame
                                    let card_rect = ui.available_rect_before_wrap();
                                    let _card_bounds = Rect::from_min_size(card_rect.min, Vec2::new(card_w, card_h));

                                    let card_frame = Frame::canvas(ui.style())
                                        .corner_radius(CornerRadius::same(6))
                                        .stroke(Stroke::new(1.0_f32, if self.dark_mode { Color32::from_rgb(45, 50, 65) } else { Color32::from_rgb(220, 225, 235) }))
                                        .fill(if self.dark_mode { Color32::from_rgb(22, 25, 34) } else { Color32::from_rgb(248, 250, 252) });

                                    let response = card_frame.show(ui, |ui| {
                                        ui.set_width(card_w);
                                        ui.set_height(card_h);
                                        ui.vertical(|ui| {
                                            let thumb_height = card_w - 6.0;
                                            let thumb_area = Rect::from_min_size(
                                                ui.cursor().min + Vec2::new(3.0, 3.0),
                                                Vec2::new(card_w - 6.0, thumb_height),
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

                                            ui.add_space(thumb_height + 2.0);

                                            // Filename & Details
                                            ui.horizontal(|ui| {
                                                ui.add_space(4.0);
                                                ui.label(
                                                    egui::RichText::new(&entry.filename)
                                                        .size(11.0)
                                                        .color(if self.dark_mode { Color32::from_rgb(220, 225, 235) } else { Color32::from_rgb(30, 40, 55) }),
                                                );
                                            });
                                        });
                                    });

                                    // Double click opens Lightbox
                                    if response.response.double_clicked() {
                                        self.viewing_index = Some(global_idx);
                                    }

                                    // Context Menu on right click
                                    response.response.context_menu(|ui| {
                                        if ui.button("🔍 Open in Viewer").clicked() {
                                            self.viewing_index = Some(global_idx);
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
