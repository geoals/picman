use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};

use crate::db::Database;
use crate::scanner::Scanner;

use super::init::DB_FILENAME;

/// Statistics from the sync operation
#[derive(Debug, Default)]
pub struct SyncStats {
    pub directories_added: usize,
    pub directories_removed: usize,
    pub files_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
}

/// Run the sync command: update database to match filesystem
pub fn run_sync(library_path: &Path) -> Result<SyncStats> {
    let library_path = library_path
        .canonicalize()
        .with_context(|| format!("Library path does not exist: {}", library_path.display()))?;

    let db_path = library_path.join(DB_FILENAME);
    if !db_path.exists() {
        anyhow::bail!(
            "No database found at {}. Run 'picman init' first.",
            db_path.display()
        );
    }

    let db = Database::open(&db_path)
        .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

    let scanner = Scanner::new(library_path);
    let stats = sync_database(&db, &scanner)?;

    Ok(stats)
}

/// Sync database with filesystem
fn sync_database(db: &Database, scanner: &Scanner) -> Result<SyncStats> {
    let mut stats = SyncStats::default();

    db.begin_transaction()?;

    // === Phase 1: Gather current state ===

    // Get all directories from filesystem
    let fs_dirs: HashMap<String, i64> = scanner
        .scan_directories()
        .map(|d| (d.relative_path, d.mtime))
        .collect();

    // Get all directories from database
    let db_dirs: HashMap<String, i64> = db.get_all_directories()?
        .into_iter()
        .map(|d| (d.path, d.id))
        .collect();

    // Get all files from filesystem
    let fs_files: Vec<_> = scanner.scan_files();
    let fs_file_set: HashSet<(String, String)> = fs_files
        .iter()
        .map(|f| (f.directory.clone(), f.filename.clone()))
        .collect();

    // Get all files from database
    let db_files = db.get_all_files()?;

    // Create lookup from directory_id to path
    let dir_id_to_path: HashMap<i64, String> = db.get_all_directories()?
        .into_iter()
        .map(|d| (d.id, d.path))
        .collect();

    // === Phase 2: Delete removed items (files first due to FK constraint) ===

    // Delete removed files
    for file in &db_files {
        let dir_path = dir_id_to_path.get(&file.directory_id).cloned().unwrap_or_default();
        if !fs_file_set.contains(&(dir_path, file.filename.clone())) {
            db.delete_file(file.id)?;
            stats.files_removed += 1;
        }
    }

    // Delete removed directories
    for (path, id) in &db_dirs {
        if !fs_dirs.contains_key(path) {
            db.delete_directory(*id)?;
            stats.directories_removed += 1;
        }
    }

    // === Phase 3: Add new directories ===

    let mut dir_path_to_id: HashMap<String, i64> = HashMap::new();
    for (path, mtime) in &fs_dirs {
        if !db_dirs.contains_key(path) {
            let parent_path = Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|s| !s.is_empty());

            let parent_id = parent_path
                .as_ref()
                .and_then(|p| dir_path_to_id.get(p).copied().or_else(|| {
                    db.get_directory_by_path(p).ok().flatten().map(|d| d.id)
                }));

            let id = db.insert_directory(path, parent_id, Some(*mtime))?;
            dir_path_to_id.insert(path.clone(), id);
            stats.directories_added += 1;
        } else {
            dir_path_to_id.insert(path.clone(), db_dirs[path]);
        }
    }

    // === Phase 4: Add/update files ===

    for file in &fs_files {
        let dir_id = if file.directory.is_empty() {
            match db.get_directory_by_path("")? {
                Some(d) => d.id,
                None => {
                    let id = db.insert_directory("", None, None)?;
                    dir_path_to_id.insert(String::new(), id);
                    id
                }
            }
        } else {
            *dir_path_to_id.get(&file.directory).unwrap_or(&0)
        };

        if dir_id == 0 {
            continue; // Skip if directory not found (shouldn't happen)
        }

        // Check if file exists in DB
        match db.get_file_by_name(dir_id, &file.filename)? {
            Some(db_file) => {
                // File exists - check if modified
                if db_file.mtime != file.mtime || db_file.size != file.size as i64 {
                    db.update_file_metadata(db_file.id, file.size as i64, file.mtime)?;
                    stats.files_modified += 1;
                }
            }
            None => {
                // New file
                db.insert_file(
                    dir_id,
                    &file.filename,
                    file.size as i64,
                    file.mtime,
                    Some(file.media_type.as_str()),
                )?;
                stats.files_added += 1;
            }
        }
    }

    db.commit()?;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::TempDir;
    use crate::cli::run_init;

    #[test]
    fn test_sync_no_changes() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "data").unwrap();

        // Init
        run_init(root).unwrap();

        // Sync with no changes
        let stats = run_sync(root).unwrap();
        assert_eq!(stats.directories_added, 0);
        assert_eq!(stats.directories_removed, 0);
        assert_eq!(stats.files_added, 0);
        assert_eq!(stats.files_removed, 0);
        assert_eq!(stats.files_modified, 0);
    }

    #[test]
    fn test_sync_added_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image1.jpg"), "data").unwrap();

        // Init
        run_init(root).unwrap();

        // Add new file
        fs::write(root.join("photos/image2.jpg"), "more data").unwrap();

        // Sync
        let stats = run_sync(root).unwrap();
        assert_eq!(stats.files_added, 1);
        assert_eq!(stats.files_removed, 0);

        // Verify file is in DB
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("photos").unwrap().unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_sync_removed_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image1.jpg"), "data").unwrap();
        fs::write(root.join("photos/image2.jpg"), "data").unwrap();

        // Init
        run_init(root).unwrap();

        // Remove a file
        fs::remove_file(root.join("photos/image2.jpg")).unwrap();

        // Sync
        let stats = run_sync(root).unwrap();
        assert_eq!(stats.files_added, 0);
        assert_eq!(stats.files_removed, 1);

        // Verify file is removed from DB
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("photos").unwrap().unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "image1.jpg");
    }

    #[test]
    fn test_sync_added_directory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "data").unwrap();

        // Init
        run_init(root).unwrap();

        // Add new directory with file
        fs::create_dir_all(root.join("videos")).unwrap();
        fs::write(root.join("videos/clip.mp4"), "video").unwrap();

        // Sync
        let stats = run_sync(root).unwrap();
        assert_eq!(stats.directories_added, 1);
        assert_eq!(stats.files_added, 1);

        // Verify in DB
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("videos").unwrap();
        assert!(dir.is_some());
    }

    #[test]
    fn test_sync_removed_directory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::create_dir_all(root.join("videos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "data").unwrap();
        fs::write(root.join("videos/clip.mp4"), "video").unwrap();

        // Init
        run_init(root).unwrap();

        // Remove directory
        fs::remove_dir_all(root.join("videos")).unwrap();

        // Sync
        let stats = run_sync(root).unwrap();
        assert_eq!(stats.directories_removed, 1);
        assert_eq!(stats.files_removed, 1);

        // Verify removed from DB
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("videos").unwrap();
        assert!(dir.is_none());
    }

    #[test]
    fn test_sync_modified_file() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "data").unwrap();

        // Init
        run_init(root).unwrap();

        // Modify file (need to wait for mtime to change)
        sleep(Duration::from_millis(100));
        fs::write(root.join("photos/image.jpg"), "modified data with more content").unwrap();

        // Sync
        let stats = run_sync(root).unwrap();
        assert_eq!(stats.files_modified, 1);

        // Verify size updated in DB
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("photos").unwrap().unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();
        assert_eq!(files[0].size, "modified data with more content".len() as i64);
    }

    #[test]
    fn test_sync_without_init() {
        let temp = TempDir::new().unwrap();
        let result = run_sync(temp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No database found"));
    }
}
