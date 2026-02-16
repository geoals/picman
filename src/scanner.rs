use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

/// Media type classification based on file extension
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaType {
    Image,
    Video,
    Other,
}

impl MediaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaType::Image => "image",
            MediaType::Video => "video",
            MediaType::Other => "other",
        }
    }
}

/// Classify a file by its extension
pub fn classify_media(path: &Path) -> MediaType {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        // Images
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "tif" | "webp" | "heic" | "heif"
        | "raw" | "cr2" | "cr3" | "nef" | "arw" | "orf" | "rw2" | "dng" | "raf" => MediaType::Image,
        // Videos
        "mp4" | "mov" | "avi" | "mkv" | "wmv" | "flv" | "webm" | "m4v" | "3gp" | "mts" | "m2ts" => {
            MediaType::Video
        }
        _ => MediaType::Other,
    }
}

/// Check if a file is a media file (image or video)
pub fn is_media_file(path: &Path) -> bool {
    matches!(classify_media(path), MediaType::Image | MediaType::Video)
}

/// Information about a scanned file
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub filename: String,
    pub directory: String,
    pub size: u64,
    pub mtime: i64,
    pub media_type: MediaType,
}

/// Information about a scanned directory
#[derive(Debug, Clone)]
pub struct ScannedDirectory {
    pub path: PathBuf,
    pub relative_path: String,
    pub parent_relative_path: Option<String>,
    pub mtime: i64,
}

/// Scan a directory tree and yield files and directories
pub struct Scanner {
    root: PathBuf,
}

impl Scanner {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Scan all directories (excluding the root itself)
    pub fn scan_directories(&self) -> impl Iterator<Item = ScannedDirectory> + '_ {
        WalkDir::new(&self.root)
            .min_depth(1) // Skip root directory
            .into_iter()
            .filter_entry(|e| !is_hidden(e))
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_dir())
            .filter_map(|entry| self.make_scanned_directory(&entry))
    }

    /// Scan all media files
    pub fn scan_files(&self) -> Vec<ScannedFile> {
        let mut files = Vec::new();
        for entry in WalkDir::new(&self.root).into_iter().filter_entry(|e| !is_hidden(e)) {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_file() {
                continue;
            }
            if !is_media_file(entry.path()) {
                continue;
            }
            if let Some(scanned) = self.make_scanned_file(&entry) {
                files.push(scanned);
            }
        }
        files
    }

    fn make_scanned_directory(&self, entry: &DirEntry) -> Option<ScannedDirectory> {
        let path = entry.path().to_path_buf();
        let relative_path = path.strip_prefix(&self.root).ok()?.to_string_lossy().to_string();

        let parent_relative_path = path
            .parent()
            .and_then(|p| p.strip_prefix(&self.root).ok())
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| !s.is_empty());

        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Some(ScannedDirectory {
            path,
            relative_path,
            parent_relative_path,
            mtime,
        })
    }

    fn make_scanned_file(&self, entry: &DirEntry) -> Option<ScannedFile> {
        let path = entry.path().to_path_buf();
        let relative_path = path.strip_prefix(&self.root).ok()?.to_string_lossy().to_string();
        let filename = entry.file_name().to_string_lossy().to_string();

        let directory = path
            .parent()
            .and_then(|p| p.strip_prefix(&self.root).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let metadata = entry.metadata().ok()?;
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let media_type = classify_media(&path);

        Some(ScannedFile {
            path,
            relative_path,
            filename,
            directory,
            size,
            mtime,
            media_type,
        })
    }
}

/// Read EXIF orientation value from an image file.
/// Returns orientation 1-8, or None if unavailable.
fn get_exif_orientation(path: &Path) -> Option<u16> {
    let file = std::fs::File::open(path).ok()?;
    let mut bufreader = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut bufreader).ok()?;
    let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;

    match &field.value {
        exif::Value::Short(vals) => vals.first().copied(),
        _ => None,
    }
}

/// Detect image orientation from dimensions, accounting for EXIF rotation.
/// Returns "landscape" if width > height, "portrait" if height > width, None if square or error.
pub fn detect_orientation(path: &Path) -> Option<&'static str> {
    let size = imagesize::size(path).ok()?;
    let (mut width, mut height) = (size.width, size.height);

    // EXIF orientations 5-8 involve 90° rotation, swapping dimensions
    if let Some(orientation) = get_exif_orientation(path) {
        if (5..=8).contains(&orientation) {
            std::mem::swap(&mut width, &mut height);
        }
    }

    match width.cmp(&height) {
        std::cmp::Ordering::Greater => Some("landscape"),
        std::cmp::Ordering::Less => Some("portrait"),
        std::cmp::Ordering::Equal => None,
    }
}

/// Check if a directory entry is hidden (starts with .)
/// Never considers the root entry (depth 0) as hidden.
fn is_hidden(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false; // Never filter the root
    }
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_classify_media() {
        assert_eq!(classify_media(Path::new("photo.jpg")), MediaType::Image);
        assert_eq!(classify_media(Path::new("photo.JPEG")), MediaType::Image);
        assert_eq!(classify_media(Path::new("photo.png")), MediaType::Image);
        assert_eq!(classify_media(Path::new("photo.cr2")), MediaType::Image);
        assert_eq!(classify_media(Path::new("video.mp4")), MediaType::Video);
        assert_eq!(classify_media(Path::new("video.MOV")), MediaType::Video);
        assert_eq!(classify_media(Path::new("doc.pdf")), MediaType::Other);
        assert_eq!(classify_media(Path::new("noext")), MediaType::Other);
    }

    #[test]
    fn test_is_media_file() {
        assert!(is_media_file(Path::new("photo.jpg")));
        assert!(is_media_file(Path::new("video.mp4")));
        assert!(!is_media_file(Path::new("doc.txt")));
    }

    #[test]
    fn test_scanner_directories() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create directory structure
        fs::create_dir_all(root.join("subject1/shoot1")).unwrap();
        fs::create_dir_all(root.join("subject1/shoot2")).unwrap();
        fs::create_dir_all(root.join("subject2")).unwrap();

        let scanner = Scanner::new(root.to_path_buf());
        let dirs: Vec<_> = scanner.scan_directories().collect();

        assert_eq!(dirs.len(), 4);

        // Verify relative paths
        let paths: Vec<_> = dirs.iter().map(|d| d.relative_path.as_str()).collect();
        assert!(paths.contains(&"subject1"));
        assert!(paths.contains(&"subject1/shoot1"));
        assert!(paths.contains(&"subject1/shoot2"));
        assert!(paths.contains(&"subject2"));

        // Verify parent relationships
        let shoot1 = dirs.iter().find(|d| d.relative_path == "subject1/shoot1").unwrap();
        assert_eq!(shoot1.parent_relative_path, Some("subject1".to_string()));

        let subject1 = dirs.iter().find(|d| d.relative_path == "subject1").unwrap();
        assert_eq!(subject1.parent_relative_path, None);
    }

    #[test]
    fn test_walkdir_basic() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create test structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "data").unwrap();

        // Check files exist
        assert!(root.join("photos").exists(), "photos dir should exist");
        assert!(root.join("photos/image.jpg").exists(), "image.jpg should exist");

        // Check walkdir sees them
        let entries: Vec<_> = WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .collect();

        // Debug: show what was found
        let paths: Vec<_> = entries.iter().map(|e| e.path().display().to_string()).collect();
        assert!(
            entries.len() >= 3,
            "Should find root, photos, and image.jpg. Found: {:?}",
            paths
        );

        // Check file types
        let files: Vec<_> = entries.iter().filter(|e| e.file_type().is_file()).collect();
        assert!(
            !files.is_empty(),
            "Should find at least one file. All entries: {:?}",
            paths
        );
    }

    #[test]
    fn test_is_hidden_respects_depth() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create a hidden directory
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join(".hidden/file.jpg"), "data").unwrap();

        // Root should never be hidden regardless of name
        let entries: Vec<_> = WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .collect();

        let root_entry = entries.iter().find(|e| e.depth() == 0).unwrap();
        assert!(!is_hidden(root_entry), "Root at depth 0 should never be hidden");

        // But child hidden dirs should be hidden
        let hidden_dir = entries
            .iter()
            .find(|e| e.file_name().to_string_lossy() == ".hidden");
        if let Some(hidden) = hidden_dir {
            assert!(is_hidden(hidden), ".hidden dir should be detected as hidden");
        }
    }

    #[test]
    fn test_scanner_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create directory and files
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "fake image data").unwrap();
        fs::write(root.join("photos/video.mp4"), "fake video data").unwrap();
        fs::write(root.join("photos/doc.txt"), "not a media file").unwrap();

        let scanner = Scanner::new(root.to_path_buf());
        let files = scanner.scan_files();

        // Should only find media files
        assert_eq!(files.len(), 2);

        let filenames: Vec<_> = files.iter().map(|f| f.filename.as_str()).collect();
        assert!(filenames.contains(&"image.jpg"));
        assert!(filenames.contains(&"video.mp4"));
        assert!(!filenames.contains(&"doc.txt"));

        // Verify file info
        let image = files.iter().find(|f| f.filename == "image.jpg").unwrap();
        assert_eq!(image.directory, "photos");
        assert_eq!(image.media_type, MediaType::Image);
        assert!(image.size > 0);
    }

    #[test]
    fn test_scanner_skips_hidden() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create visible and hidden directories/files
        fs::create_dir_all(root.join("visible")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("visible/photo.jpg"), "data").unwrap();
        fs::write(root.join(".hidden/secret.jpg"), "hidden").unwrap();
        fs::write(root.join("visible/.hidden.jpg"), "hidden file").unwrap();

        let scanner = Scanner::new(root.to_path_buf());

        let dirs: Vec<_> = scanner.scan_directories().collect();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].relative_path, "visible");

        let files = scanner.scan_files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "photo.jpg");
    }

    #[test]
    fn test_detect_orientation_landscape() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("landscape.jpg");
        let img = image::RgbImage::new(100, 50);
        img.save(&path).unwrap();

        assert_eq!(detect_orientation(&path), Some("landscape"));
    }

    #[test]
    fn test_detect_orientation_portrait() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("portrait.jpg");
        let img = image::RgbImage::new(50, 100);
        img.save(&path).unwrap();

        assert_eq!(detect_orientation(&path), Some("portrait"));
    }

    #[test]
    fn test_detect_orientation_square() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("square.jpg");
        let img = image::RgbImage::new(100, 100);
        img.save(&path).unwrap();

        assert_eq!(detect_orientation(&path), None);
    }

    #[test]
    fn test_detect_orientation_nonexistent() {
        let path = Path::new("/nonexistent/file.jpg");
        assert_eq!(detect_orientation(path), None);
    }
}
