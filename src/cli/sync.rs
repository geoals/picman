use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::db::Database;
use crate::hash::compute_file_hash;
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
    pub files_hashed: usize,
    pub hash_errors: usize,
    pub orientation_tagged: usize,
}

/// Run the sync command: update database to match filesystem
pub fn run_sync(library_path: &Path, compute_hashes: bool, tag_orientation_flag: bool) -> Result<SyncStats> {
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

    let scanner = Scanner::new(library_path.clone());
    let mut stats = sync_database(&db, &scanner)?;

    // Tag orientation for image files (only if requested)
    if tag_orientation_flag {
        stats.orientation_tagged = tag_orientation(&db, &library_path)?;
    }

    if compute_hashes {
        let (hashed, errors) = hash_files(&db, &library_path)?;
        stats.files_hashed = hashed;
        stats.hash_errors = errors;
    }

    Ok(stats)
}

const HASH_BATCH_SIZE: usize = 1000;

const ORIENTATION_BATCH_SIZE: usize = 5000;

/// Detect image orientation and add landscape/portrait tags
fn tag_orientation(db: &Database, library_path: &Path) -> Result<usize> {
    print!("Finding images needing orientation...");
    let _ = io::stdout().flush();
    let files = db.get_files_needing_orientation()?;
    let total = files.len();
    println!(" {}", total);

    if total == 0 {
        return Ok(0);
    }

    let mut total_tagged = 0usize;
    let mut processed = 0usize;

    // Process in batches for progress updates and DB writes
    for batch in files.chunks(ORIENTATION_BATCH_SIZE) {
        // Detect orientations in parallel
        let results: Vec<_> = batch
            .par_iter()
            .map(|file| {
                let full_path = library_path.join(&file.path);
                let orientation = detect_orientation(&full_path);
                (file.id, orientation)
            })
            .collect();

        // Write batch to database in a transaction
        db.begin_transaction()?;
        for (id, orientation) in results {
            if let Some(tag) = orientation {
                db.add_file_tag(id, tag)?;
                total_tagged += 1;
            }
        }
        db.commit()?;

        processed += batch.len();
        print!("\rOrientation: {}/{}", processed, total);
        let _ = io::stdout().flush();
    }

    println!(); // Newline after progress

    Ok(total_tagged)
}

/// Detect orientation from image dimensions
/// Returns "landscape" if width > height, "portrait" if height > width, None if square or error
fn detect_orientation(path: &Path) -> Option<&'static str> {
    match imagesize::size(path) {
        Ok(size) => {
            if size.width > size.height {
                Some("landscape")
            } else if size.height > size.width {
                Some("portrait")
            } else {
                None // Square
            }
        }
        Err(_) => None,
    }
}

/// Hash files that have NULL hash values
fn hash_files(db: &Database, library_path: &Path) -> Result<(usize, usize)> {
    let files_to_hash = db.get_files_needing_hash()?;
    let total = files_to_hash.len();

    if total == 0 {
        return Ok((0, 0));
    }

    let mut total_hashed = 0usize;
    let mut total_errors = 0usize;

    // Process in batches for resumability
    for (batch_idx, batch) in files_to_hash.chunks(HASH_BATCH_SIZE).enumerate() {
        let batch_start = batch_idx * HASH_BATCH_SIZE;
        let hashed_in_batch = AtomicUsize::new(0);

        // Compute hashes in parallel for this batch
        let results: Vec<_> = batch
            .par_iter()
            .map(|file| {
                let full_path = library_path.join(&file.path);
                let result = compute_file_hash(&full_path);

                let current = batch_start + hashed_in_batch.fetch_add(1, Ordering::Relaxed) + 1;
                print!("\rHashing: {}/{}", current, total);
                let _ = io::stdout().flush();

                (file.id, result)
            })
            .collect();

        // Write batch to database
        for (id, result) in results {
            match result {
                Ok(hash) => {
                    db.set_file_hash(id, &hash)?;
                    total_hashed += 1;
                }
                Err(e) => {
                    eprintln!("\nWarning: Failed to hash file: {}", e);
                    total_errors += 1;
                }
            }
        }
    }

    println!(); // Newline after progress

    Ok((total_hashed, total_errors))
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

    // Delete removed directories (deepest first to avoid FK constraint on parent_id)
    let mut dirs_to_delete: Vec<_> = db_dirs
        .iter()
        .filter(|(path, _)| !fs_dirs.contains_key(*path))
        .collect();
    dirs_to_delete.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()));
    for (_, id) in dirs_to_delete {
        db.delete_directory(*id)?;
        stats.directories_removed += 1;
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
        let stats = run_sync(root, false, false).unwrap();
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
        let stats = run_sync(root, false, false).unwrap();
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
        let stats = run_sync(root, false, false).unwrap();
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
        let stats = run_sync(root, false, false).unwrap();
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
        let stats = run_sync(root, false, false).unwrap();
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
        let stats = run_sync(root, false, false).unwrap();
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
        let result = run_sync(temp.path(), false, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No database found"));
    }

    #[test]
    fn test_sync_with_hash_computes_hashes() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image1.jpg"), "content A").unwrap();
        fs::write(root.join("photos/image2.jpg"), "content B").unwrap();

        // Init (creates files without hashes)
        run_init(root).unwrap();

        // Sync with hashing
        let stats = run_sync(root, true, false).unwrap();

        // Should have hashed both files
        assert_eq!(stats.files_hashed, 2);
        assert_eq!(stats.hash_errors, 0);

        // Verify hashes are in DB
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("photos").unwrap().unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();

        for file in &files {
            assert!(file.hash.is_some(), "File {} should have hash", file.filename);
            assert_eq!(file.hash.as_ref().unwrap().len(), 16);
        }

        // Different content should produce different hashes
        let hashes: Vec<_> = files.iter().map(|f| f.hash.as_ref().unwrap()).collect();
        assert_ne!(hashes[0], hashes[1]);
    }

    #[test]
    fn test_sync_hash_is_resumable() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "data").unwrap();

        // Init and sync with hash
        run_init(root).unwrap();
        let stats1 = run_sync(root, true, false).unwrap();
        assert_eq!(stats1.files_hashed, 1);

        // Sync again - should hash 0 files (already hashed)
        let stats2 = run_sync(root, true, false).unwrap();
        assert_eq!(stats2.files_hashed, 0);
    }

    #[test]
    fn test_sync_modified_file_gets_rehashed() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create and init
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "original").unwrap();
        run_init(root).unwrap();

        // Hash
        let stats1 = run_sync(root, true, false).unwrap();
        assert_eq!(stats1.files_hashed, 1);

        // Get original hash
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("photos").unwrap().unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();
        let original_hash = files[0].hash.clone().unwrap();
        drop(db);

        // Modify file
        sleep(Duration::from_millis(100));
        fs::write(root.join("photos/image.jpg"), "modified content").unwrap();

        // Sync (detects modification, clears hash)
        let stats2 = run_sync(root, true, false).unwrap();
        assert_eq!(stats2.files_modified, 1);
        assert_eq!(stats2.files_hashed, 1); // Should rehash modified file

        // Verify new hash is different
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();
        let new_hash = files[0].hash.clone().unwrap();

        assert_ne!(original_hash, new_hash);
    }

    #[test]
    fn test_sync_tags_orientation() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("photos")).unwrap();

        // Create a landscape image (wider than tall) - 100x50
        let landscape_img = image::RgbImage::new(100, 50);
        landscape_img.save(root.join("photos/landscape.jpg")).unwrap();

        // Create a portrait image (taller than wide) - 50x100
        let portrait_img = image::RgbImage::new(50, 100);
        portrait_img.save(root.join("photos/portrait.jpg")).unwrap();

        // Create a square image - should not be tagged
        let square_img = image::RgbImage::new(100, 100);
        square_img.save(root.join("photos/square.jpg")).unwrap();

        // Init and sync with orientation tagging
        run_init(root).unwrap();
        let stats = run_sync(root, false, true).unwrap();

        // Should have tagged 2 files (landscape and portrait, not square)
        assert_eq!(stats.orientation_tagged, 2);

        // Verify tags in database
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("photos").unwrap().unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();

        for file in &files {
            let tags = db.get_file_tags(file.id).unwrap();
            match file.filename.as_str() {
                "landscape.jpg" => assert!(tags.contains(&"landscape".to_string())),
                "portrait.jpg" => assert!(tags.contains(&"portrait".to_string())),
                "square.jpg" => assert!(!tags.contains(&"landscape".to_string()) && !tags.contains(&"portrait".to_string())),
                _ => panic!("Unexpected file"),
            }
        }

        // Sync again - should not re-tag already tagged files
        let stats2 = run_sync(root, false, true).unwrap();
        assert_eq!(stats2.orientation_tagged, 0);
    }
}
