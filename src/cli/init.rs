use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::{debug, info, instrument};

use crate::db::Database;
use crate::scanner::Scanner;

/// Database filename stored in the library root
pub const DB_FILENAME: &str = ".picman.db";

/// Run the init command: scan directory tree and populate database
pub fn run_init(library_path: &Path) -> Result<InitStats> {
    let library_path = library_path
        .canonicalize()
        .with_context(|| format!("Library path does not exist: {}", library_path.display()))?;

    let db_path = library_path.join(DB_FILENAME);
    let db = Database::open(&db_path)
        .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

    let scanner = Scanner::new(library_path);
    let stats = populate_database(&db, &scanner)?;

    Ok(stats)
}

/// Statistics from the init operation
#[derive(Debug, Default)]
pub struct InitStats {
    pub directories: usize,
    pub files: usize,
    pub images: usize,
    pub videos: usize,
}

/// Populate the database from a scanner
#[instrument(skip_all)]
fn populate_database(db: &Database, scanner: &Scanner) -> Result<InitStats> {
    let mut stats = InitStats::default();

    // Use a single transaction for all inserts (massive performance improvement)
    db.begin_transaction()?;

    // Map from relative path to directory ID
    let mut dir_ids: HashMap<String, i64> = HashMap::new();

    // First pass: insert all directories
    info!("scanning directories");
    for dir in scanner.scan_directories() {
        let parent_id = dir
            .parent_relative_path
            .as_ref()
            .and_then(|p| dir_ids.get(p).copied());

        let id = db.insert_directory(&dir.relative_path, parent_id, Some(dir.mtime))?;
        dir_ids.insert(dir.relative_path, id);
        stats.directories += 1;
    }
    info!(count = stats.directories, "directories scanned");

    // Second pass: insert all files
    info!("scanning files");
    for file in scanner.scan_files() {
        // Get or create the directory for this file
        let dir_id = if file.directory.is_empty() {
            // File is in root - need to insert root directory
            if let Some(id) = dir_ids.get("") {
                *id
            } else {
                let id = db.insert_directory("", None, None)?;
                dir_ids.insert(String::new(), id);
                id
            }
        } else if let Some(id) = dir_ids.get(&file.directory) {
            *id
        } else {
            // Directory wasn't in our scan (shouldn't happen normally)
            let id = db.insert_directory(&file.directory, None, None)?;
            dir_ids.insert(file.directory.clone(), id);
            id
        };

        db.insert_file(
            dir_id,
            &file.filename,
            file.size as i64,
            file.mtime,
            Some(file.media_type.as_str()),
        )?;

        stats.files += 1;
        match file.media_type {
            crate::scanner::MediaType::Image => stats.images += 1,
            crate::scanner::MediaType::Video => stats.videos += 1,
            crate::scanner::MediaType::Other => {}
        }
    }
    info!(
        files = stats.files,
        images = stats.images,
        videos = stats.videos,
        "files scanned"
    );

    debug!("committing to database");
    db.commit()?;
    info!("init complete");

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_init_empty_directory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let stats = run_init(root).unwrap();
        assert_eq!(stats.directories, 0);
        assert_eq!(stats.files, 0);

        // Database should exist
        assert!(root.join(DB_FILENAME).exists());
    }

    #[test]
    fn test_init_with_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create test structure
        fs::create_dir_all(root.join("subject1/shoot1")).unwrap();
        fs::create_dir_all(root.join("subject1/shoot2")).unwrap();
        fs::create_dir_all(root.join("subject2")).unwrap();

        fs::write(root.join("subject1/shoot1/photo1.jpg"), "image data").unwrap();
        fs::write(root.join("subject1/shoot1/photo2.jpg"), "image data").unwrap();
        fs::write(root.join("subject1/shoot2/video.mp4"), "video data").unwrap();
        fs::write(root.join("subject2/portrait.png"), "image data").unwrap();

        let stats = run_init(root).unwrap();

        assert_eq!(stats.directories, 4); // subject1, subject1/shoot1, subject1/shoot2, subject2
        assert_eq!(stats.files, 4);
        assert_eq!(stats.images, 3);
        assert_eq!(stats.videos, 1);

        // Verify database contents
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();

        // Check directories
        let root_dirs = db.get_child_directories(None).unwrap();
        assert_eq!(root_dirs.len(), 2); // subject1, subject2

        // Check files
        let subject1 = db.get_directory_by_path("subject1").unwrap().unwrap();
        let shoot1 = db.get_directory_by_path("subject1/shoot1").unwrap().unwrap();
        assert_eq!(shoot1.parent_id, Some(subject1.id));

        let files = db.get_files_in_directory(shoot1.id).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_init_skips_hidden() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create visible and hidden
        fs::create_dir_all(root.join("visible")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("visible/photo.jpg"), "data").unwrap();
        fs::write(root.join(".hidden/secret.jpg"), "data").unwrap();
        fs::write(root.join("visible/.hidden_file.jpg"), "data").unwrap();

        let stats = run_init(root).unwrap();

        // Should only find visible directory and visible file
        assert_eq!(stats.directories, 1);
        assert_eq!(stats.files, 1);
    }

    #[test]
    fn test_init_nonexistent_path() {
        let result = run_init(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }
}
