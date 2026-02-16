use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::db::Database;
use crate::hash::compute_file_hash;
use crate::scanner::{detect_orientation, Scanner};
use crate::tui::widgets::{compute_thumbnail_path, compute_video_thumbnail_path, is_image_file, is_video_file};

use super::init::DB_FILENAME;

/// Statistics from the sync operation
#[derive(Debug, Default)]
pub struct SyncStats {
    pub directories_added: usize,
    pub directories_removed: usize,
    pub directories_moved: usize,
    pub files_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
    pub files_hashed: usize,
    pub hash_errors: usize,
    pub orientation_tagged: usize,
}

/// Metadata to preserve when a directory is moved
struct DirectoryMetadata {
    rating: Option<i32>,
    tags: Vec<String>,
    /// Files in this directory (filename only) for thumbnail migration
    files: Vec<String>,
    /// The old directory path (for computing old thumbnail paths)
    old_path: String,
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
    let mut stats = sync_database(&db, &scanner, &library_path)?;

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
fn sync_database(db: &Database, scanner: &Scanner, library_path: &Path) -> Result<SyncStats> {
    let mut stats = SyncStats::default();

    db.begin_transaction()?;

    // === Phase 1: Gather current state ===

    // Get all directories from filesystem
    eprint!("[sync] Scanning directories...");
    let fs_dirs: HashMap<String, i64> = scanner
        .scan_directories()
        .map(|d| (d.relative_path, d.mtime))
        .collect();
    eprintln!(" {} found", fs_dirs.len());

    // Get all directories from database
    eprint!("[sync] Loading directories from DB...");
    let db_dirs: HashMap<String, i64> = db.get_all_directories()?
        .into_iter()
        .map(|d| (d.path, d.id))
        .collect();
    eprintln!(" {} loaded", db_dirs.len());

    // Get all files from filesystem
    eprint!("[sync] Scanning files...");
    let fs_files: Vec<_> = scanner.scan_files();
    eprintln!(" {} found", fs_files.len());
    let fs_file_set: HashSet<(String, String)> = fs_files
        .iter()
        .map(|f| (f.directory.clone(), f.filename.clone()))
        .collect();

    // Get all files from database
    eprint!("[sync] Loading files from DB...");
    let db_files = db.get_all_files()?;
    eprintln!(" {} loaded", db_files.len());

    // Create lookup from directory_id to path
    let dir_id_to_path: HashMap<i64, String> = db.get_all_directories()?
        .into_iter()
        .map(|d| (d.id, d.path))
        .collect();

    // === Phase 2: Detect moved directories and collect metadata ===
    eprint!("[sync] Detecting changes...");

    // Find directories that will be deleted
    // Note: The "" directory represents root-level files and is never in the filesystem scan,
    // so we must skip it to avoid FK constraint failures
    let dirs_to_delete: Vec<_> = db_dirs
        .iter()
        .filter(|(path, _)| !path.is_empty() && !fs_dirs.contains_key(*path))
        .collect();

    // Find new directories
    let new_dir_paths: Vec<_> = fs_dirs
        .keys()
        .filter(|path| !db_dirs.contains_key(*path))
        .collect();

    // Collect metadata from directories being deleted, keyed by basename
    // Also collect file names for thumbnail migration
    let mut deleted_metadata: HashMap<String, DirectoryMetadata> = HashMap::new();
    for (path, &id) in &dirs_to_delete {
        let dir = db.get_directory(id)?;
        let tags = db.get_directory_tags(id)?;
        let files_in_dir = db.get_files_in_directory(id)?;

        let has_metadata = dir.as_ref().map(|d| d.rating.is_some()).unwrap_or(false)
            || !tags.is_empty();
        let has_files = !files_in_dir.is_empty();

        // Preserve if has metadata OR has files (for thumbnail migration)
        if has_metadata || has_files {
            let basename = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Check if this basename exists in new directories (potential move)
            let matches_in_new: Vec<_> = new_dir_paths
                .iter()
                .filter(|p| {
                    Path::new(*p)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .as_ref() == Some(&basename)
                })
                .collect();

            // Only preserve if exactly one match (unambiguous move)
            if matches_in_new.len() == 1 {
                deleted_metadata.insert(
                    basename,
                    DirectoryMetadata {
                        rating: dir.and_then(|d| d.rating),
                        tags,
                        files: files_in_dir.into_iter().map(|f| f.filename).collect(),
                        old_path: path.to_string(),
                    },
                );
            }
        }
    }
    eprintln!(" {} dirs to delete, {} new dirs", dirs_to_delete.len(), new_dir_paths.len());

    // === Phase 3: Delete removed items (files first due to FK constraint) ===
    eprint!("[sync] Applying changes...");

    // Delete removed files
    for file in &db_files {
        let dir_path = dir_id_to_path.get(&file.directory_id).cloned().unwrap_or_default();
        if !fs_file_set.contains(&(dir_path, file.filename.clone())) {
            db.delete_file(file.id)?;
            stats.files_removed += 1;
        }
    }

    // Delete removed directories (deepest first to avoid FK constraint on parent_id)
    let mut dirs_to_delete_sorted = dirs_to_delete;
    dirs_to_delete_sorted.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()));
    for (_, id) in dirs_to_delete_sorted {
        db.delete_directory(*id)?;
        stats.directories_removed += 1;
    }

    // === Phase 4: Add new directories ===

    // Collect new directories and sort by depth (parents before children)
    let mut new_dirs: Vec<_> = fs_dirs
        .iter()
        .filter(|(path, _)| !db_dirs.contains_key(*path))
        .collect();
    new_dirs.sort_by_key(|(path, _)| path.matches('/').count());

    let mut dir_path_to_id: HashMap<String, i64> = HashMap::new();
    for (path, mtime) in new_dirs {
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

        // Check if this directory was moved (has metadata to restore)
        let basename = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if let Some(metadata) = deleted_metadata.remove(&basename) {
            // Restore rating
            if let Some(rating) = metadata.rating {
                db.set_directory_rating(id, Some(rating))?;
            }
            // Restore tags
            for tag in &metadata.tags {
                db.add_directory_tag(id, tag)?;
            }

            // Move thumbnails for files in this directory
            for filename in &metadata.files {
                let old_file_path = library_path.join(&metadata.old_path).join(filename);
                let new_file_path = library_path.join(path).join(filename);

                // Get mtime from the new file location
                if let Ok(file_meta) = std::fs::metadata(&new_file_path) {
                    if let Ok(mtime) = file_meta.modified() {
                        // Move image thumbnail
                        if is_image_file(&new_file_path) {
                            if let (Some(old_thumb), Some(new_thumb)) = (
                                compute_thumbnail_path(&old_file_path, mtime),
                                compute_thumbnail_path(&new_file_path, mtime),
                            ) {
                                if old_thumb.exists() && old_thumb != new_thumb {
                                    let _ = std::fs::rename(&old_thumb, &new_thumb);
                                }
                            }
                        }
                        // Move video thumbnail
                        else if is_video_file(&new_file_path) {
                            if let (Some(old_thumb), Some(new_thumb)) = (
                                compute_video_thumbnail_path(&old_file_path, mtime),
                                compute_video_thumbnail_path(&new_file_path, mtime),
                            ) {
                                if old_thumb.exists() && old_thumb != new_thumb {
                                    let _ = std::fs::rename(&old_thumb, &new_thumb);
                                }
                            }
                        }
                    }
                }
            }

            stats.directories_moved += 1;
        }
    }

    // Also populate dir_path_to_id with existing directories
    for (path, id) in &db_dirs {
        dir_path_to_id.insert(path.clone(), *id);
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
    eprintln!(" done");

    eprint!("[sync] Committing to database...");
    db.commit()?;
    eprintln!(" done");

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

    #[test]
    fn test_sync_nested_directories_have_correct_parent_ids() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/image.jpg"), "data").unwrap();

        // Init
        run_init(root).unwrap();

        // Add deeply nested directories (simulating a move operation)
        fs::create_dir_all(root.join("Hongdan/Vol1")).unwrap();
        fs::create_dir_all(root.join("Hongdan/Vol2")).unwrap();
        fs::create_dir_all(root.join("Hongdan/Vol3")).unwrap();
        fs::write(root.join("Hongdan/Vol1/img.jpg"), "data").unwrap();
        fs::write(root.join("Hongdan/Vol2/img.jpg"), "data").unwrap();
        fs::write(root.join("Hongdan/Vol3/img.jpg"), "data").unwrap();

        // Sync
        let stats = run_sync(root, false, false).unwrap();
        assert_eq!(stats.directories_added, 4); // Hongdan + Vol1 + Vol2 + Vol3

        // Verify parent_id relationships are correct
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();

        let hongdan = db.get_directory_by_path("Hongdan").unwrap().unwrap();
        assert_eq!(hongdan.parent_id, None); // Root level

        let vol1 = db.get_directory_by_path("Hongdan/Vol1").unwrap().unwrap();
        assert_eq!(vol1.parent_id, Some(hongdan.id));

        let vol2 = db.get_directory_by_path("Hongdan/Vol2").unwrap().unwrap();
        assert_eq!(vol2.parent_id, Some(hongdan.id));

        let vol3 = db.get_directory_by_path("Hongdan/Vol3").unwrap().unwrap();
        assert_eq!(vol3.parent_id, Some(hongdan.id));
    }

    #[test]
    fn test_sync_move_preserves_metadata() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure: korean/Hongdan
        fs::create_dir_all(root.join("korean/Hongdan")).unwrap();
        fs::write(root.join("korean/Hongdan/image.jpg"), "data").unwrap();

        // Init
        run_init(root).unwrap();

        // Add rating and tags to the directory
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let hongdan = db.get_directory_by_path("korean/Hongdan").unwrap().unwrap();
        db.set_directory_rating(hongdan.id, Some(5)).unwrap();
        db.add_directory_tag(hongdan.id, "favorite").unwrap();
        db.add_directory_tag(hongdan.id, "kpop").unwrap();
        drop(db);

        // Move the directory on disk: korean/Hongdan -> artists/Hongdan
        fs::create_dir_all(root.join("artists")).unwrap();
        fs::rename(root.join("korean/Hongdan"), root.join("artists/Hongdan")).unwrap();

        // Sync
        let stats = run_sync(root, false, false).unwrap();

        // Should detect the move
        assert_eq!(stats.directories_moved, 1);
        assert_eq!(stats.directories_added, 2); // artists + artists/Hongdan
        assert_eq!(stats.directories_removed, 1); // korean/Hongdan

        // Verify metadata was preserved
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let new_hongdan = db.get_directory_by_path("artists/Hongdan").unwrap().unwrap();
        assert_eq!(new_hongdan.rating, Some(5));

        let tags = db.get_directory_tags(new_hongdan.id).unwrap();
        assert!(tags.contains(&"favorite".to_string()));
        assert!(tags.contains(&"kpop".to_string()));
    }

    #[test]
    fn test_sync_move_ambiguous_no_transfer() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create initial structure with two directories of the same name
        fs::create_dir_all(root.join("a/Photos")).unwrap();
        fs::create_dir_all(root.join("b/Photos")).unwrap();
        fs::write(root.join("a/Photos/img.jpg"), "data").unwrap();
        fs::write(root.join("b/Photos/img.jpg"), "data").unwrap();

        // Init
        run_init(root).unwrap();

        // Add rating to a/Photos
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let photos_a = db.get_directory_by_path("a/Photos").unwrap().unwrap();
        db.set_directory_rating(photos_a.id, Some(5)).unwrap();
        drop(db);

        // Move both directories to new locations
        fs::create_dir_all(root.join("c")).unwrap();
        fs::create_dir_all(root.join("d")).unwrap();
        fs::rename(root.join("a/Photos"), root.join("c/Photos")).unwrap();
        fs::rename(root.join("b/Photos"), root.join("d/Photos")).unwrap();

        // Sync - should NOT transfer metadata due to ambiguity
        let stats = run_sync(root, false, false).unwrap();
        assert_eq!(stats.directories_moved, 0); // Ambiguous, no transfer

        // Verify metadata was NOT transferred (both should have no rating)
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let photos_c = db.get_directory_by_path("c/Photos").unwrap().unwrap();
        let photos_d = db.get_directory_by_path("d/Photos").unwrap().unwrap();
        assert_eq!(photos_c.rating, None);
        assert_eq!(photos_d.rating, None);
    }

    #[test]
    fn test_sync_preserves_root_directory_with_files() {
        // Reproduces bug: sync tried to delete "" directory created for root-level files,
        // causing FK constraint failure since files still referenced it
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create a file directly in the root (not in a subdirectory)
        fs::write(root.join("root_image.jpg"), "data").unwrap();

        // Init - this creates a "" directory for root-level files
        run_init(root).unwrap();

        // Verify "" directory exists with the file
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let root_dir = db.get_directory_by_path("").unwrap();
        assert!(root_dir.is_some(), "Root directory should exist after init");
        let files = db.get_files_in_directory(root_dir.unwrap().id).unwrap();
        assert_eq!(files.len(), 1);
        drop(db);

        // Sync should NOT fail with FK constraint error
        let stats = run_sync(root, false, false).unwrap();

        // The "" directory should NOT be deleted
        assert_eq!(stats.directories_removed, 0);

        // Verify "" directory and file still exist
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let root_dir = db.get_directory_by_path("").unwrap();
        assert!(root_dir.is_some(), "Root directory should still exist after sync");
        let files = db.get_files_in_directory(root_dir.unwrap().id).unwrap();
        assert_eq!(files.len(), 1);
    }
}
