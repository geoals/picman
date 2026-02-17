use image::{DynamicImage, GenericImageView, RgbImage};
use std::collections::HashMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::db::{Database, Directory};

// ==================== Media Type Detection ====================

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

// ==================== EXIF Orientation ====================

/// Read EXIF orientation and apply rotation/flip to image
pub fn apply_exif_orientation(path: &Path, img: DynamicImage) -> DynamicImage {
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

// ==================== Thumbnail Cache ====================

// In-memory cache mapping original path -> thumbnail path
static THUMBNAIL_CACHE: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();

// Thumbnail settings
const THUMBNAIL_MAX_HEIGHT: u32 = 1440;
const THUMBNAIL_QUALITY: u8 = 80;

// Web thumbnail settings (small grid thumbnails for web UI)
const WEB_THUMBNAIL_MAX_WIDTH: u32 = 400;
const WEB_THUMBNAIL_QUALITY: u8 = 75;

fn get_thumbnail_cache() -> &'static Mutex<HashMap<PathBuf, PathBuf>> {
    THUMBNAIL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get the thumbnail cache directory (~/.cache/picman/thumbnails)
pub fn get_thumbnail_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let cache_dir = PathBuf::from(home).join(".cache/picman/thumbnails");
    std::fs::create_dir_all(&cache_dir).ok()?;
    Some(cache_dir)
}

/// Get the web thumbnail cache directory (~/.cache/picman/web_thumbnails)
pub fn get_web_thumbnail_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let cache_dir = PathBuf::from(home).join(".cache/picman/web_thumbnails");
    std::fs::create_dir_all(&cache_dir).ok()?;
    Some(cache_dir)
}

// ==================== Thumbnail Path Computation ====================

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

/// Compute thumbnail path for a given path and mtime (for moving thumbnails)
/// This allows computing the path without the file existing at that location
pub fn compute_thumbnail_path(original_path: &Path, mtime: std::time::SystemTime) -> Option<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let cache_dir = get_thumbnail_dir()?;
    let mut hasher = DefaultHasher::new();
    original_path.hash(&mut hasher);
    mtime.hash(&mut hasher);

    Some(cache_dir.join(format!("{:016x}.jpg", hasher.finish())))
}

/// Compute video thumbnail path for a given path and mtime
pub fn compute_video_thumbnail_path(original_path: &Path, mtime: std::time::SystemTime) -> Option<PathBuf> {
    compute_thumbnail_path(original_path, mtime)
        .map(|p| p.with_file_name(format!("vid_{}", p.file_name().unwrap().to_string_lossy())))
}

/// Get video thumbnail path with "vid_" prefix
fn get_video_thumbnail_path(video_path: &Path) -> Option<PathBuf> {
    get_thumbnail_path(video_path)
        .map(|p| p.with_file_name(format!("vid_{}", p.file_name().unwrap().to_string_lossy())))
}

// ==================== Thumbnail Existence Checks ====================

/// Check if a thumbnail exists for a file (image or video)
pub fn has_thumbnail(path: &Path) -> bool {
    if is_image_file(path) {
        get_thumbnail_path(path).map(|p| p.exists()).unwrap_or(false)
    } else if is_video_file(path) {
        get_video_thumbnail_path(path).map(|p| p.exists()).unwrap_or(false)
    } else {
        false
    }
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

/// Get the preview path for a file (thumbnail or original) and whether it's a thumbnail.
/// For images: returns cached thumbnail if exists, otherwise original
/// For videos: returns cached thumbnail if exists, otherwise None
pub fn get_preview_path_for_file(file_path: &Path) -> Option<(PathBuf, bool)> {
    if is_image_file(file_path) {
        match get_cached_image_thumbnail(file_path) {
            Some(thumb) => Some((thumb, true)),
            None => Some((file_path.to_path_buf(), false)), // Fall back to original image
        }
    } else if is_video_file(file_path) {
        // For videos, only use cached thumbnail (don't generate during preload)
        get_cached_video_thumbnail(file_path).map(|thumb| (thumb, true))
    } else {
        None
    }
}

// ==================== Thumbnail Generation ====================

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

// ==================== Web Thumbnail (Grid) Generation ====================

/// Generate a web thumbnail path for an image based on its path hash.
/// Canonicalizes the path before hashing so that relative and absolute paths
/// produce the same hash (e.g. `./photo.jpg` and `/home/user/lib/photo.jpg`).
pub fn get_web_thumbnail_path(original_path: &Path) -> Option<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let cache_dir = get_web_thumbnail_dir()?;
    let canonical = original_path.canonicalize().ok()?;
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);

    let mtime = std::fs::metadata(original_path).ok()?.modified().ok()?;
    mtime.hash(&mut hasher);

    Some(cache_dir.join(format!("{:016x}.jpg", hasher.finish())))
}

/// Check if a web thumbnail exists for a file
pub fn has_web_thumbnail(path: &Path) -> bool {
    if is_image_file(path) {
        get_web_thumbnail_path(path).map(|p| p.exists()).unwrap_or(false)
    } else if is_video_file(path) {
        // Video web thumbnails use same path (extracted frame is already small)
        get_web_thumbnail_path(path).map(|p| p.exists()).unwrap_or(false)
    } else {
        false
    }
}

/// Generate a small (400px wide) web thumbnail for grid display
pub fn generate_web_thumbnail(image_path: &Path) -> Option<PathBuf> {
    let thumb_path = get_web_thumbnail_path(image_path)?;

    let img = image::open(image_path).ok()?;
    let img = apply_exif_orientation(image_path, img);

    // Resize to max width, preserving aspect ratio
    let img = if img.width() > WEB_THUMBNAIL_MAX_WIDTH {
        img.resize(
            WEB_THUMBNAIL_MAX_WIDTH,
            u32::MAX,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    let mut output = std::fs::File::create(&thumb_path).ok()?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, WEB_THUMBNAIL_QUALITY);
    img.to_rgb8().write_with_encoder(encoder).ok()?;

    Some(thumb_path)
}

/// Generate a small web thumbnail from a video file using ffmpeg
pub fn generate_web_video_thumbnail(video_path: &Path) -> Option<PathBuf> {
    let thumb_path = get_web_thumbnail_path(video_path)?;

    let scale_filter = format!("scale={}:-1", WEB_THUMBNAIL_MAX_WIDTH);
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
        Some(thumb_path)
    } else {
        None
    }
}

// ==================== Directory Preview Generation ====================

const DIR_PREVIEW_HEIGHT: u32 = 1440;
const DIR_PREVIEW_MAX_COLS: usize = 4;
const DIR_PREVIEW_MAX_ROWS: usize = 3;
const DIR_PREVIEW_MAX_IMAGES: usize = DIR_PREVIEW_MAX_COLS * DIR_PREVIEW_MAX_ROWS; // 12

/// Calculate grid layout (cols, rows) for n images
fn calc_grid_layout(n: usize) -> (usize, usize) {
    match n {
        0 => (0, 0),
        1 => (1, 1),
        2 => (2, 1),
        3 => (3, 1),
        4 => (2, 2),
        5 | 6 => (3, 2),
        7 | 8 => (4, 2),
        9 => (3, 3),
        _ => (4, 3), // 10-12
    }
}

/// Get the directory preview cache directory (~/.cache/picman/dir_previews)
fn get_dir_preview_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let cache_dir = PathBuf::from(home).join(".cache/picman/dir_previews");
    std::fs::create_dir_all(&cache_dir).ok()?;
    Some(cache_dir)
}

/// Get cache path for a directory preview
fn get_dir_preview_path(dir_id: i64) -> Option<PathBuf> {
    let cache_dir = get_dir_preview_dir()?;
    Some(cache_dir.join(format!("{}.jpg", dir_id)))
}

/// Check if a cached directory preview exists, returning its path
pub fn get_cached_dir_preview(dir_id: i64) -> Option<PathBuf> {
    let path = get_dir_preview_path(dir_id)?;
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Check if a directory preview exists
pub fn has_dir_preview(dir_id: i64) -> bool {
    get_cached_dir_preview(dir_id).is_some()
}

/// Load an image, applying EXIF and using cached thumbnail if available
fn load_image_for_composite(path: &Path) -> Option<DynamicImage> {
    // Prefer cached thumbnail
    if let Some(thumb_path) = get_thumbnail_path(path).filter(|p| p.exists()) {
        return image::open(&thumb_path).ok();
    }

    // Fall back to original with EXIF applied
    let img = image::open(path).ok()?;
    Some(apply_exif_orientation(path, img))
}

/// Center-crop an image to square
fn center_crop_square(img: &DynamicImage) -> DynamicImage {
    let (width, height) = img.dimensions();
    let size = width.min(height);
    let x = (width - size) / 2;
    let y = (height - size) / 2;
    img.crop_imm(x, y, size, size)
}

/// Generate preview from pre-collected image paths (no DB access - thread-safe)
pub fn generate_dir_preview_from_paths(dir_id: i64, images: &[PathBuf]) -> Option<PathBuf> {
    if images.is_empty() {
        return None;
    }

    let cache_path = get_dir_preview_path(dir_id)?;

    // Special case: 1 image - preserve original aspect ratio
    if images.len() == 1 {
        if let Some(img) = load_image_for_composite(&images[0]) {
            let (w, h) = img.dimensions();
            let scale = DIR_PREVIEW_HEIGHT as f64 / h as f64;
            let new_width = (w as f64 * scale) as u32;
            let resized = img.resize_exact(
                new_width,
                DIR_PREVIEW_HEIGHT,
                image::imageops::FilterType::Lanczos3,
            );

            let mut output = std::fs::File::create(&cache_path).ok()?;
            let encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, THUMBNAIL_QUALITY);
            resized.to_rgb8().write_with_encoder(encoder).ok()?;

            return Some(cache_path);
        }
        return None;
    }

    // Multiple images: create grid layout
    let (cols, rows) = calc_grid_layout(images.len());
    let cell_size = DIR_PREVIEW_HEIGHT / rows as u32;
    let canvas_width = cell_size * cols as u32;
    let canvas_height = DIR_PREVIEW_HEIGHT;

    let mut canvas = RgbImage::new(canvas_width, canvas_height);

    for pixel in canvas.pixels_mut() {
        *pixel = image::Rgb([30, 30, 30]);
    }

    for (i, img_path) in images.iter().enumerate() {
        if let Some(img) = load_image_for_composite(img_path) {
            let cropped = center_crop_square(&img);
            let resized = cropped.resize_exact(
                cell_size,
                cell_size,
                image::imageops::FilterType::Lanczos3,
            );

            let col = i % cols;
            let row = i / cols;
            let x = col as u32 * cell_size;
            let y = row as u32 * cell_size;

            let rgb = resized.to_rgb8();
            for (px, py, pixel) in rgb.enumerate_pixels() {
                if x + px < canvas_width && y + py < canvas_height {
                    canvas.put_pixel(x + px, y + py, *pixel);
                }
            }
        }
    }

    let mut output = std::fs::File::create(&cache_path).ok()?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, THUMBNAIL_QUALITY);
    canvas.write_with_encoder(encoder).ok()?;

    Some(cache_path)
}

// ==================== Standalone Preview Image Collection ====================

/// Minimal state needed for background directory preview generation
pub struct TempPreviewState {
    pub library_path: PathBuf,
    pub db: Database,
    pub directories: Vec<Directory>,
}

/// Collect preview images using TempPreviewState (requires DB access)
pub fn collect_preview_images_standalone(state: &TempPreviewState, dir: &Directory) -> Vec<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut images = Vec::new();

    let child_dirs: Vec<&Directory> = state
        .directories
        .iter()
        .filter(|d| d.parent_id == Some(dir.id))
        .collect();

    if !child_dirs.is_empty() {
        let mut hasher = DefaultHasher::new();
        dir.id.hash(&mut hasher);
        let seed = hasher.finish();

        let mut selected_dirs: Vec<_> = child_dirs.iter().enumerate().collect();
        selected_dirs.sort_by_key(|(i, _)| {
            let mut h = DefaultHasher::new();
            seed.hash(&mut h);
            i.hash(&mut h);
            h.finish()
        });

        for (_, subdir) in selected_dirs.into_iter().take(DIR_PREVIEW_MAX_IMAGES) {
            if let Some(img) = find_image_in_dir_recursive_standalone(state, subdir.id) {
                images.push(img);
            }
        }
    } else {
        if let Ok(files) = state.db.get_files_in_directory(dir.id) {
            let image_files: Vec<_> = files
                .into_iter()
                .filter(|f| {
                    let path = get_file_path_standalone(state, dir, &f.filename);
                    is_image_file(&path)
                })
                .collect();

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
                images.push(get_file_path_standalone(state, dir, &file.filename));
            }
        }
    }

    images
}

fn find_image_in_dir_recursive_standalone(state: &TempPreviewState, dir_id: i64) -> Option<PathBuf> {
    let dir = state.directories.iter().find(|d| d.id == dir_id)?;

    if let Ok(files) = state.db.get_files_in_directory(dir_id) {
        for file in &files {
            let path = get_file_path_standalone(state, dir, &file.filename);
            if is_image_file(&path) {
                return Some(path);
            }
        }
    }

    let child_dirs: Vec<i64> = state
        .directories
        .iter()
        .filter(|d| d.parent_id == Some(dir_id))
        .map(|d| d.id)
        .collect();

    for child_id in child_dirs {
        if let Some(img) = find_image_in_dir_recursive_standalone(state, child_id) {
            return Some(img);
        }
    }

    None
}

fn get_file_path_standalone(state: &TempPreviewState, dir: &Directory, filename: &str) -> PathBuf {
    if dir.path.is_empty() {
        state.library_path.join(filename)
    } else {
        state.library_path.join(&dir.path).join(filename)
    }
}
