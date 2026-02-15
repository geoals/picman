use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{picker::Picker, FilterType, Resize, StatefulImage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::tui::state::{AppState, PreviewCache};

// Global picker (created once, thread-safe)
static PICKER: OnceLock<Mutex<Option<Picker>>> = OnceLock::new();

// Cache for video thumbnails (video path -> thumbnail path)
static THUMBNAIL_CACHE: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();

fn get_thumbnail_cache() -> &'static Mutex<HashMap<PathBuf, PathBuf>> {
    THUMBNAIL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_picker_mutex() -> &'static Mutex<Option<Picker>> {
    PICKER.get_or_init(|| {
        Mutex::new(Picker::from_termios().ok().map(|mut picker| {
            picker.guess_protocol();
            picker
        }))
    })
}

fn is_image_file(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    matches!(
        extension.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" | "tif"
    )
}

fn is_video_file(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    matches!(
        extension.as_str(),
        "mp4" | "mov" | "avi" | "mkv" | "wmv" | "flv" | "webm" | "m4v" | "3gp" | "mts" | "m2ts"
    )
}

/// Extract a thumbnail from a video file using ffmpeg
fn get_video_thumbnail(video_path: &Path) -> Option<PathBuf> {
    let mut cache = get_thumbnail_cache().lock().ok()?;

    // Check cache first
    if let Some(thumb_path) = cache.get(video_path) {
        if thumb_path.exists() {
            return Some(thumb_path.clone());
        }
    }

    // Generate thumbnail path in temp directory
    let hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        video_path.hash(&mut hasher);
        hasher.finish()
    };
    let thumb_path = std::env::temp_dir().join(format!("picman_thumb_{}.jpg", hash));

    // If thumbnail already exists, use it
    if thumb_path.exists() {
        cache.insert(video_path.to_path_buf(), thumb_path.clone());
        return Some(thumb_path);
    }

    // Extract thumbnail using ffmpeg (grab frame at 1 second or first frame)
    let status = Command::new("ffmpeg")
        .args([
            "-y",                    // Overwrite output
            "-i",
            video_path.to_str()?,
            "-ss", "00:00:01",       // Seek to 1 second
            "-vframes", "1",         // Extract 1 frame
            "-q:v", "2",             // Quality
            thumb_path.to_str()?,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if status.success() && thumb_path.exists() {
        cache.insert(video_path.to_path_buf(), thumb_path.clone());
        Some(thumb_path)
    } else {
        None
    }
}

pub fn render_preview(frame: &mut Frame, area: Rect, state: &AppState) {
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

    // Determine what to preview: image directly, video thumbnail, or nothing
    let preview_path = if is_image_file(&file_path) {
        file_path.clone()
    } else if is_video_file(&file_path) {
        match get_video_thumbnail(&file_path) {
            Some(thumb) => thumb,
            None => {
                let info = format!("Video: {}\n\n(generating thumbnail...)", file_path.file_name().unwrap_or_default().to_string_lossy());
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

    // Check if we need to load a new image (cache miss)
    // For videos, we cache by the original file path, not the thumbnail path
    let mut cache = state.preview_cache.borrow_mut();
    let needs_load = match cache.as_ref() {
        Some(c) => c.path != file_path,
        None => true,
    };

    if needs_load {
        // Get mutable access to picker
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

        // Load the image (or video thumbnail)
        let image = match image::open(&preview_path) {
            Ok(img) => img,
            Err(_) => {
                *cache = None;
                let error = Paragraph::new("Failed to load preview");
                frame.render_widget(error, inner);
                return;
            }
        };

        // Create protocol for the image
        let protocol = picker.new_resize_protocol(image);
        // Cache by original file path so video thumbnails work correctly
        *cache = Some(PreviewCache::new(file_path, protocol));
    }

    // Render the cached image
    if let Some(ref mut preview) = *cache {
        let image_widget =
            StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
        frame.render_stateful_widget(image_widget, inner, &mut preview.protocol);
    }
}
