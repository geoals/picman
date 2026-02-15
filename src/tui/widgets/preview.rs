use image::DynamicImage;
use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{picker::Picker, FilterType, Resize, StatefulImage};
use std::collections::HashMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::tui::state::{AppState, PreviewCache};

// Global picker (created once, thread-safe)
static PICKER: OnceLock<Mutex<Option<Picker>>> = OnceLock::new();

// In-memory cache mapping original path -> thumbnail path
static THUMBNAIL_CACHE: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();

// Thumbnail settings
const THUMBNAIL_MAX_HEIGHT: u32 = 1440;
const THUMBNAIL_QUALITY: u8 = 80;

fn get_thumbnail_cache() -> &'static Mutex<HashMap<PathBuf, PathBuf>> {
    THUMBNAIL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get the thumbnail cache directory (~/.cache/picman/thumbnails)
fn get_thumbnail_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let cache_dir = PathBuf::from(home).join(".cache/picman/thumbnails");
    std::fs::create_dir_all(&cache_dir).ok()?;
    Some(cache_dir)
}

/// Generate a thumbnail path for an image based on its path hash
fn get_thumbnail_path(original_path: &Path) -> Option<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let cache_dir = get_thumbnail_dir()?;
    let mut hasher = DefaultHasher::new();
    original_path.hash(&mut hasher);

    // Include mtime in hash so thumbnails regenerate when file changes
    let mtime = std::fs::metadata(original_path).ok()?.modified().ok()?;
    mtime.hash(&mut hasher);

    Some(cache_dir.join(format!("{:016x}.jpg", hasher.finish())))
}

/// Get cached thumbnail for an image file (does NOT generate)
fn get_cached_image_thumbnail(image_path: &Path) -> Option<PathBuf> {
    let mut cache = get_thumbnail_cache().lock().ok()?;

    // Check in-memory cache first
    if let Some(thumb_path) = cache.get(image_path) {
        if thumb_path.exists() {
            return Some(thumb_path.clone());
        }
    }

    let thumb_path = get_thumbnail_path(image_path)?;

    // If thumbnail exists on disk, use it
    if thumb_path.exists() {
        cache.insert(image_path.to_path_buf(), thumb_path.clone());
        return Some(thumb_path);
    }

    None
}

/// Generate thumbnail for an image file
pub fn generate_image_thumbnail(image_path: &Path) -> Option<PathBuf> {
    let thumb_path = get_thumbnail_path(image_path)?;

    // Generate thumbnail: load, apply EXIF, resize, save
    let img = image::open(image_path).ok()?;
    let img = apply_exif_orientation(image_path, img);

    // Resize to max height, preserving aspect ratio
    let img = if img.height() > THUMBNAIL_MAX_HEIGHT {
        img.resize(
            u32::MAX,
            THUMBNAIL_MAX_HEIGHT,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    // Save as JPEG with specified quality
    let mut output = std::fs::File::create(&thumb_path).ok()?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, THUMBNAIL_QUALITY);
    img.to_rgb8().write_with_encoder(encoder).ok()?;

    // Update in-memory cache
    if let Ok(mut cache) = get_thumbnail_cache().lock() {
        cache.insert(image_path.to_path_buf(), thumb_path.clone());
    }

    Some(thumb_path)
}

fn get_picker_mutex() -> &'static Mutex<Option<Picker>> {
    PICKER.get_or_init(|| {
        Mutex::new(Picker::from_termios().ok().map(|mut picker| {
            picker.guess_protocol();
            picker
        }))
    })
}

/// Read EXIF orientation and apply rotation/flip to image
fn apply_exif_orientation(path: &Path, img: DynamicImage) -> DynamicImage {
    let orientation = (|| {
        let file = std::fs::File::open(path).ok()?;
        let mut bufreader = BufReader::new(file);
        let exif = exif::Reader::new().read_from_container(&mut bufreader).ok()?;
        let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
        if let exif::Value::Short(ref vals) = field.value {
            vals.first().copied()
        } else {
            None
        }
    })();

    match orientation {
        Some(2) => img.fliph(),
        Some(3) => img.rotate180(),
        Some(4) => img.flipv(),
        Some(5) => img.rotate90().fliph(),
        Some(6) => img.rotate90(),
        Some(7) => img.rotate270().fliph(),
        Some(8) => img.rotate270(),
        _ => img, // 1 or unknown = no transformation
    }
}

pub fn is_image_file(path: &Path) -> bool {
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

pub fn is_video_file(path: &Path) -> bool {
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

/// Get video thumbnail path with "vid_" prefix
fn get_video_thumbnail_path(video_path: &Path) -> Option<PathBuf> {
    get_thumbnail_path(video_path)
        .map(|p| p.with_file_name(format!("vid_{}", p.file_name().unwrap().to_string_lossy())))
}

/// Get cached thumbnail for a video file (does NOT generate)
fn get_cached_video_thumbnail(video_path: &Path) -> Option<PathBuf> {
    let mut cache = get_thumbnail_cache().lock().ok()?;

    // Check in-memory cache first
    if let Some(thumb_path) = cache.get(video_path) {
        if thumb_path.exists() {
            return Some(thumb_path.clone());
        }
    }

    let thumb_path = get_video_thumbnail_path(video_path)?;

    // If thumbnail exists on disk, use it
    if thumb_path.exists() {
        cache.insert(video_path.to_path_buf(), thumb_path.clone());
        return Some(thumb_path);
    }

    None
}

/// Generate thumbnail for a video file using ffmpeg
pub fn generate_video_thumbnail(video_path: &Path) -> Option<PathBuf> {
    let thumb_path = get_video_thumbnail_path(video_path)?;

    // Extract thumbnail using ffmpeg (grab frame at 1 second)
    // Scale to max height while preserving aspect ratio
    let scale_filter = format!("scale=-1:'min({},ih)'", THUMBNAIL_MAX_HEIGHT);
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", video_path.to_str()?,
            "-ss", "00:00:01",
            "-vframes", "1",
            "-vf", &scale_filter,
            "-q:v", "5",
            thumb_path.to_str()?,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if status.success() && thumb_path.exists() {
        if let Ok(mut cache) = get_thumbnail_cache().lock() {
            cache.insert(video_path.to_path_buf(), thumb_path.clone());
        }
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

    // Determine what to preview: use cached thumbnail (no auto-generation)
    let preview_path = if is_image_file(&file_path) {
        match get_cached_image_thumbnail(&file_path) {
            Some(thumb) => thumb,
            None => {
                let info = format!("{}\n\nNo thumbnail.\nPress Shift+T to generate.", file_path.file_name().unwrap_or_default().to_string_lossy());
                let placeholder = Paragraph::new(info)
                    .block(block)
                    .alignment(Alignment::Center);
                frame.render_widget(placeholder, area);
                return;
            }
        }
    } else if is_video_file(&file_path) {
        match get_cached_video_thumbnail(&file_path) {
            Some(thumb) => thumb,
            None => {
                let info = format!("{}\n\nNo thumbnail.\nPress Shift+T to generate.", file_path.file_name().unwrap_or_default().to_string_lossy());
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

        // Load the thumbnail (EXIF orientation already applied during thumbnail generation)
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
