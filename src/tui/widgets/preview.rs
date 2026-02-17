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
    self, generate_dir_preview_from_paths, generate_video_thumbnail, get_cached_dir_preview,
    get_preview_path_for_file, is_image_file,
};
use crate::tui::state::{AppState, DirectoryPreviewCache, Focus};

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
    if dir.path.is_empty() {
        state.library_path.join(filename)
    } else {
        state.library_path.join(&dir.path).join(filename)
    }
}

/// Generate dynamic grid composite preview for a directory
pub fn generate_dir_preview(state: &AppState, dir: &Directory) -> Option<PathBuf> {
    let images = collect_preview_images(state, dir);
    generate_dir_preview_from_paths(dir.id, &images)
}

// ==================== TUI Rendering ====================

/// Render directory preview (composite image from directory samples)
fn render_directory_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
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

    // Check in-memory cache
    let mut cache = state.directory_preview_cache.borrow_mut();
    let needs_load = match cache.as_ref() {
        Some(c) => c.dir_id != dir.id,
        None => true,
    };

    if needs_load {
        // Only show cached previews (no auto-generation)
        let preview_path = match get_cached_dir_preview(dir.id) {
            Some(p) => p,
            None => {
                let placeholder = Paragraph::new("Press 'o' → Dir preview to generate");
                frame.render_widget(placeholder, inner);
                *cache = None;
                return;
            }
        };

        // Get picker
        let mut picker_guard = match get_picker_mutex().lock() {
            Ok(g) => g,
            Err(_) => {
                let error = Paragraph::new("Preview unavailable");
                frame.render_widget(error, inner);
                return;
            }
        };

        let picker = match picker_guard.as_mut() {
            Some(p) => p,
            None => {
                let placeholder = Paragraph::new(
                    "Image preview not available\n(terminal doesn't support graphics)",
                );
                frame.render_widget(placeholder, inner);
                return;
            }
        };

        // Load the composite image
        let image = match image::open(&preview_path) {
            Ok(img) => img,
            Err(_) => {
                *cache = None;
                let error = Paragraph::new("Failed to load preview");
                frame.render_widget(error, inner);
                return;
            }
        };

        // Create protocol and cache
        let protocol = picker.new_resize_protocol(image);
        *cache = Some(DirectoryPreviewCache::new(dir.id, protocol));
    }

    // Render the cached image
    if let Some(ref mut preview) = *cache {
        let image_widget =
            StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
        frame.render_stateful_widget(image_widget, inner, &mut preview.protocol);
    }
}

/// Render file preview (single image/video thumbnail)
fn render_file_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
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

    // Determine what to preview: use cached thumbnail or fall back to original
    let (preview_path, is_thumbnail) = if thumbnails::is_image_file(&file_path) {
        match get_preview_path_for_file(&file_path) {
            Some((path, is_thumb)) => (path, is_thumb),
            None => (file_path.clone(), false),
        }
    } else if thumbnails::is_video_file(&file_path) {
        // Try cached thumbnail first, auto-generate if missing
        match get_preview_path_for_file(&file_path)
            .or_else(|| generate_video_thumbnail(&file_path).map(|thumb| (thumb, true)))
        {
            Some((path, is_thumb)) => (path, is_thumb),
            None => {
                let info = format!("{}\n\nFailed to generate thumbnail", file_path.file_name().unwrap_or_default().to_string_lossy());
                let placeholder = Paragraph::new(info)
                    .block(block)
                    .alignment(Alignment::Center);
                frame.render_widget(placeholder, area);
                return;
            }
        }
    } else {
        let info = format!("File: {}", file_path.display());
        let placeholder = Paragraph::new(info)
            .block(block)
            .alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    };

    // Calculate inner area for image
    let inner = block.inner(area);
    frame.render_widget(block.clone(), area);

    // Update the preview area hint so the background worker can pre-encode
    // images at the right size (cheap atomic store, runs every render cycle)
    state.preview_loader.borrow().set_preview_area(inner.width, inner.height);

    // Check if we need to load a new image (cache miss)
    // For videos, we cache by the original file path, not the thumbnail path
    let mut cache = state.preview_cache.borrow_mut();

    // Check if already in cache
    let in_cache = cache.contains(&file_path);

    // During rapid navigation (skip_preview), don't block to load new images
    // Show cached version or last displayed image instead
    if state.skip_preview && !in_cache {
        // Show the most recently accessed cached image (stale but no stutter)
        if let Some(preview) = cache.get_last_accessed_mut() {
            let image_widget =
                StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
            frame.render_stateful_widget(image_widget, inner, &mut preview.protocol);
        }
        return;
    }

    if in_cache {
        // Cache hit - render the cached image
        if let Some(preview) = cache.get_mut(&file_path) {
            let image_widget =
                StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
            frame.render_stateful_widget(image_widget, inner, &mut preview.protocol);
        }
        return;
    }

    // Cache miss - queue background load (never block on image::open)
    let mut loader = state.preview_loader.borrow_mut();
    let is_pending = loader.is_pending(&file_path);

    if !is_pending {
        // Queue background load
        if let Some(dir_id) = state.current_dir_id {
            loader.queue_load(
                file_path.clone(),
                preview_path.clone(),
                is_thumbnail,
                dir_id,
            );
        }
    }

    // Drop the loader borrow before accessing cache again
    drop(loader);

    // While waiting for background load, show the last cached image (stale but no stutter)
    if let Some(preview) = cache.get_last_accessed_mut() {
        let image_widget =
            StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
        frame.render_stateful_widget(image_widget, inner, &mut preview.protocol);
    }
}

/// Main preview render function - dispatches based on current focus
pub fn render_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    match state.focus {
        Focus::DirectoryTree => render_directory_preview(frame, area, state),
        Focus::FileList => render_file_preview(frame, area, state),
    }
}
