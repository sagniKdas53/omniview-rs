# OmniView

A blazingly fast, multi-threaded generic image gallery and viewer written in **Rust** with **WebP thumbnail caching**, **live filesystem watching**, and **dark/light mode**.

Designed to effortlessly handle massive repositories (10,000+ files, 15+ GB) across nested subfolders with zero UI stuttering or freezes.

---

## ⚡ Highlights

- **Pure Rust Performance**: Written in 100% Rust using `eframe` (egui) and `rayon` multi-threading.
- **WebP Thumbnail Caching**: Thumbnails are generated asynchronously and cached to disk (`~/.cache/omniview/thumbnails/*.webp`). If a thumbnail already exists, it is loaded directly with zero re-encoding!
- **Dynamic Folder Loading**: Works with **ANY** directory (Pixiv downloads, manga TL corpus, wallpapers, screenshots) with zero hardcoded path restrictions.
- **Live Folder Watching**: Uses the `notify` crate to watch your open directory. Whenever new images are added, renamed, or deleted, OmniView updates in real-time.
- **Sensible Lightbox Fit Mode**: When opening images, OmniView calculates available screen dimensions and fits the image cleanly to 95% of your screen (never a tiny speck). Includes 1:1 original pixel toggle and mouse-wheel zoom.
- **Dark / Light Mode**: Instant one-click toggle between sleek dark mode and bright light mode.
- **Native Clipboard & Drag Integration**:
  - `Copy File (Attach)`: Copies file URI (`file://`) to clipboard for immediate pasting in Discord, Slack, and Nautilus.
  - `Copy Path`: Copies absolute path.
  - `Copy Image`: Copies bitmap pixels.
  - `Open in Default App` (`xdg-open`) and `Reveal in File Manager` (`nautilus --select`).

---

## 🚀 How to Run

### 1. From Desktop Application Menu
Press the **Super** (Windows) key, search for **`OmniView`**, and press Enter.

### 2. From Terminal
```bash
omniview
```
Or open any specific directory:
```bash
omniview /home/sagnik/Downloads/pixiv_downloads
omniview /path/to/manga_corpus
```

### 3. Binary Location
```bash
/home/sagnik/Projects/omniview/target/release/omniview
```

---

## ⌨️ Controls & Shortcuts

| Action | Shortcut / Control |
|---|---|
| Open Lightbox Viewer | Double-click any card |
| Close Viewer | `Esc` |
| Next / Previous Image | `Right` / `Left` Arrow keys |
| Toggle Fit / 1:1 Actual Size | `Space` or Double-click |
| Zoom In / Out | Mouse Wheel |
| Pan Image | Click & Drag |
| Switch Folder | Click `📁 [Folder Name]` button |
