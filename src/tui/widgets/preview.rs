use image::DynamicImage;
use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol, FilterType, Resize, StatefulImage};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::db::Directory;
use crate::thumbnails::{
    self, generate_dir_preview_from_paths, get_cached_dir_preview, is_image_file,
};
use crate::tui::colors::UNFOCUS_COLOR;
use crate::tui::state::{AppState, Focus};

// Global picker (created once, thread-safe)
static PICKER: OnceLock<Mutex<Option<Picker>>> = OnceLock::new();

fn get_picker_mutex() -> &'static Mutex<Option<Picker>> {
    PICKER.get_or_init(|| {
        Mutex::new(Picker::from_termios().ok().map(|mut picker| {
            picker.guess_protocol();
            picker
        }))
    })
}

/// Create a stateful protocol from a DynamicImage (for use with background loader)
pub fn create_protocol(image: DynamicImage) -> Option<Box<dyn StatefulProtocol>> {
    let mut picker_guard = get_picker_mutex().lock().ok()?;
    let picker = picker_guard.as_mut()?;
    Some(picker.new_resize_protocol(image))
}

// ==================== AppState-dependent preview image collection ====================

const DIR_PREVIEW_MAX_IMAGES: usize = 12;

/// Collect up to 12 image paths for directory preview
/// If dir has subdirs: 1 image from each of up to 12 subdirs (searching recursively)
/// If no subdirs: up to 12 images from current directory
fn collect_preview_images(state: &AppState, dir: &Directory) -> Vec<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut images = Vec::new();

    // Get child directories
    let child_dirs: Vec<&Directory> = state
        .tree
        .directories
        .iter()
        .filter(|d| d.parent_id == Some(dir.id))
        .collect();

    if !child_dirs.is_empty() {
        // Has subdirs: pick up to 12 subdirs and get 1 image from each
        let mut hasher = DefaultHasher::new();
        dir.id.hash(&mut hasher);
        let seed = hasher.finish();

        // Deterministically select up to 12 subdirs
        let mut selected_dirs: Vec<_> = child_dirs.iter().enumerate().collect();
        selected_dirs.sort_by_key(|(i, _)| {
            let mut h = DefaultHasher::new();
            seed.hash(&mut h);
            i.hash(&mut h);
            h.finish()
        });

        for (_, subdir) in selected_dirs.into_iter().take(DIR_PREVIEW_MAX_IMAGES) {
            if let Some(img) = find_image_in_dir_recursive(state, subdir.id) {
                images.push(img);
            }
        }
    } else {
        // No subdirs: pick up to 12 images from current directory
        if let Ok(files) = state.db.get_files_in_directory(dir.id) {
            let image_files: Vec<_> = files
                .into_iter()
                .filter(|f| {
                    let path = get_file_path(state, dir, &f.filename);
                    is_image_file(&path)
                })
                .collect();

            // Deterministic selection using dir_id as seed
            let mut hasher = DefaultHasher::new();
            dir.id.hash(&mut hasher);
            let seed = hasher.finish();

            let mut indexed: Vec<_> = image_files.iter().enumerate().collect();
            indexed.sort_by_key(|(i, _)| {
                let mut h = DefaultHasher::new();
                seed.hash(&mut h);
                i.hash(&mut h);
                h.finish()
            });

            for (_, file) in indexed.into_iter().take(DIR_PREVIEW_MAX_IMAGES) {
                images.push(get_file_path(state, dir, &file.filename));
            }
        }
    }

    images
}

/// Find first image in a directory tree (recursive search)
fn find_image_in_dir_recursive(state: &AppState, dir_id: i64) -> Option<PathBuf> {
    // First try to find an image directly in this directory
    let dir = state.tree.directories.iter().find(|d| d.id == dir_id)?;

    if let Ok(files) = state.db.get_files_in_directory(dir_id) {
        for file in &files {
            let path = get_file_path(state, dir, &file.filename);
            if is_image_file(&path) {
                return Some(path);
            }
        }
    }

    // No image found, search child directories
    let child_dirs: Vec<i64> = state
        .tree
        .directories
        .iter()
        .filter(|d| d.parent_id == Some(dir_id))
        .map(|d| d.id)
        .collect();

    for child_id in child_dirs {
        if let Some(img) = find_image_in_dir_recursive(state, child_id) {
            return Some(img);
        }
    }

    None
}

/// Get full path to a file
fn get_file_path(state: &AppState, dir: &Directory, filename: &str) -> PathBuf {
    dir.file_path(&state.library_path, filename)
}

/// Generate dynamic grid composite preview for a directory
pub fn generate_dir_preview(state: &AppState, dir: &Directory) -> Option<PathBuf> {
    let images = collect_preview_images(state, dir);
    generate_dir_preview_from_paths(dir.id, &images)
}

// ==================== TUI Rendering ====================

/// Build a synthetic cache key for directory previews.
/// Uses a prefix that can never collide with real file paths.
fn dir_cache_key(dir_id: i64) -> PathBuf {
    PathBuf::from(format!("\0dir/{}", dir_id))
}

/// Render directory preview (composite image from directory samples)
fn render_directory_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(UNFOCUS_COLOR))
        .title(" Directory Preview ");

    // Get selected directory
    let dir = match state.get_selected_directory() {
        Some(d) => d.clone(),
        None => {
            let placeholder = Paragraph::new("Select a directory")
                .block(block)
                .alignment(Alignment::Center);
            frame.render_widget(placeholder, area);
            return;
        }
    };

    // Calculate inner area for image
    let inner = block.inner(area);
    frame.render_widget(block.clone(), area);

    let key = dir_cache_key(dir.id);

    // Cache hit — render directly from LRU
    {
        let mut cache = state.dir_preview_cache.borrow_mut();
        if let Some(entry) = cache.get_mut(&key) {
            if let Some(ref mut protocol) = entry.protocol {
                let image_widget =
                    StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
                frame.render_stateful_widget(image_widget, inner, protocol);
                return;
            }
        }
    }

    // Cache miss — check if already loading
    {
        let loader = state.preview_loader.borrow();
        if loader.is_pending(&key) {
            let placeholder = Paragraph::new("Loading preview...");
            frame.render_widget(placeholder, inner);
            return;
        }
    }

    // Try to find the cached composite on disk (1 stat — acceptable, once per uncached dir)
    let preview_path = match get_cached_dir_preview(dir.id) {
        Some(p) => p,
        None => {
            let placeholder = Paragraph::new("Press 'o' → Dir preview to generate");
            frame.render_widget(placeholder, inner);
            return;
        }
    };

    // Queue for background decoding — no image::open() on the UI thread
    {
        let mut loader = state.preview_loader.borrow_mut();
        if let Some(dir_id) = state.current_dir_id {
            loader.queue_dir_load(key, preview_path, dir_id);
        }
    }

    let placeholder = Paragraph::new("Loading preview...");
    frame.render_widget(placeholder, inner);
}

/// Render file preview (single image/video thumbnail)
fn render_file_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(UNFOCUS_COLOR))
        .title(" Preview ");

    // Get selected file path
    let file_path = match state.selected_file_path() {
        Some(path) => path,
        None => {
            let placeholder = Paragraph::new("Select a file to preview")
                .block(block)
                .alignment(Alignment::Center);
            frame.render_widget(placeholder, area);
            return;
        }
    };

    // Early return for non-media files (extension check only — no disk I/O)
    if !thumbnails::is_image_file(&file_path) && !thumbnails::is_video_file(&file_path) {
        let info = format!("File: {}", file_path.display());
        let placeholder = Paragraph::new(info)
            .block(block)
            .alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    // Calculate inner area for image
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Update the preview area hint so the background worker can pre-encode
    // images at the right size (cheap atomic store, runs every render cycle)
    state.preview_loader.borrow().set_preview_area(inner.width, inner.height);

    // During rapid navigation (skip_preview), render from cache if available
    // but don't queue new loads — the user is still scrolling.
    if state.skip_preview {
        let mut cache = state.preview_cache.borrow_mut();
        if let Some(entry) = cache.get_mut(&file_path) {
            if let Some(ref mut protocol) = entry.protocol {
                let image_widget =
                    StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
                frame.render_stateful_widget(image_widget, inner, protocol);
                *state.render_protocol.borrow_mut() = Some(file_path);
                return;
            }
        }
        drop(cache);
        // Cache miss — show last rendered image, don't queue load
        render_fallback_protocol(frame, inner, state);
        return;
    }

    // Try to render directly from cache — instant path for preloaded files
    {
        let mut cache = state.preview_cache.borrow_mut();
        if let Some(entry) = cache.get_mut(&file_path) {
            if let Some(ref mut protocol) = entry.protocol {
                let image_widget =
                    StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
                frame.render_stateful_widget(image_widget, inner, protocol);
                *state.render_protocol.borrow_mut() = Some(file_path);
                return;
            }
        }
    }

    // Not in cache — queue for background loading (all disk I/O on worker thread)
    {
        let mut loader = state.preview_loader.borrow_mut();
        if !loader.is_pending(&file_path) {
            if let Some(dir_id) = state.current_dir_id {
                loader.queue_file_load(file_path, dir_id);
            }
        }
    }

    // While waiting, keep showing the previous image (no flash)
    render_fallback_protocol(frame, inner, state);
}

/// Render the last successfully rendered protocol from cache (keeps previous image visible).
/// Used during rapid navigation or while waiting for a new image to load.
fn render_fallback_protocol(frame: &mut Frame, area: Rect, state: &AppState) {
    let render_path = state.render_protocol.borrow().clone();
    if let Some(ref path) = render_path {
        let mut cache = state.preview_cache.borrow_mut();
        if let Some(entry) = cache.get_mut(path) {
            if let Some(ref mut protocol) = entry.protocol {
                let image_widget =
                    StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
                frame.render_stateful_widget(image_widget, area, protocol);
            }
        }
    }
}

/// Main preview render function - dispatches based on current focus
pub fn render_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    match state.focus {
        Focus::DirectoryTree => render_directory_preview(frame, area, state),
        Focus::FileList => render_file_preview(frame, area, state),
    }
}
