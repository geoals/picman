use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use tracing::{debug, info, instrument, warn};

use crate::db::Database;
use crate::hash::compute_file_hash;
use crate::scanner::{detect_orientation, read_dimensions, MediaType, Scanner};
use crate::thumbnails::{compute_thumbnail_path, compute_video_thumbnail_path, is_image_file, is_video_file};

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
    pub dimensions_backfilled: usize,
}

/// Metadata to preserve when a file is moved (as part of directory move)
struct FileMetadata {
    rating: Option<i32>,
    tags: Vec<String>,
}

/// Metadata to preserve when a directory is moved
struct DirectoryMetadata {
    rating: Option<i32>,
    tags: Vec<String>,
    /// Files in this directory with their metadata (keyed by filename)
    files: HashMap<String, FileMetadata>,
    /// The old directory path (for computing old thumbnail paths)
    old_path: String,
}

/// Run the sync command: update database to match filesystem
///
/// If `incremental` is true, only scans files in directories whose mtime changed.
/// This is much faster for large libraries on slow storage (HDD).
pub fn run_sync(library_path: &Path, compute_hashes: bool, tag_orientation_flag: bool) -> Result<SyncStats> {
    run_sync_impl(library_path, compute_hashes, tag_orientation_flag, false)
}

/// Run an incremental sync - only scan files in changed directories.
/// Much faster than full sync on HDD.
pub fn run_sync_incremental(library_path: &Path) -> Result<SyncStats> {
    run_sync_impl(library_path, false, false, true)
}

fn run_sync_impl(
    library_path: &Path,
    compute_hashes: bool,
    tag_orientation_flag: bool,
    incremental: bool,
) -> Result<SyncStats> {
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
    let mut stats = if incremental {
        sync_database_incremental(&db, &scanner, &library_path)?
    } else {
        sync_database(&db, &scanner, &library_path)?
    };

    // Backfill dimensions for existing image files with NULL width/height
    stats.dimensions_backfilled = backfill_dimensions(&db, &library_path)?;

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
#[instrument(skip(db, library_path))]
fn tag_orientation(db: &Database, library_path: &Path) -> Result<usize> {
    info!("finding images needing orientation");
    let files = db.get_files_needing_orientation()?;
    let total = files.len();
    info!(total, "found images needing orientation");

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
        debug!(processed, total, "orientation progress");
    }

    info!(total_tagged, "orientation tagging complete");

    Ok(total_tagged)
}

/// Backfill dimensions for image files with NULL width/height.
/// Reads only the file header (via imagesize), so it's fast even on HDD.
#[instrument(skip(db, library_path))]
fn backfill_dimensions(db: &Database, library_path: &Path) -> Result<usize> {
    let files = db.get_files_needing_dimensions()?;
    let total = files.len();

    if total == 0 {
        return Ok(0);
    }

    info!(total, "backfilling dimensions for existing images");

    let mut backfilled = 0usize;

    // Process in a single transaction (header reads are fast, no need for batching)
    db.begin_transaction()?;
    for file in &files {
        let full_path = library_path.join(&file.path);
        if let Some((w, h)) = read_dimensions(&full_path) {
            db.set_file_dimensions(file.id, w, h)?;
            backfilled += 1;
        }
    }
    db.commit()?;

    if backfilled > 0 {
        info!(backfilled, total, "dimension backfill complete");
    }

    Ok(backfilled)
}

/// Hash files that have NULL hash values
#[instrument(skip(db, library_path))]
fn hash_files(db: &Database, library_path: &Path) -> Result<(usize, usize)> {
    let files_to_hash = db.get_files_needing_hash()?;
    let total = files_to_hash.len();
    info!(total, "files needing hash");

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
                debug!(current, total, "hashing progress");

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
                    warn!(error = %e, "failed to hash file");
                    total_errors += 1;
                }
            }
        }
    }

    info!(total_hashed, total_errors, "hashing complete");

    Ok((total_hashed, total_errors))
}

/// Incremental sync: only scan files in directories whose mtime changed.
/// Much faster than full sync on HDD (2500 dir stats vs 93k file stats).
#[instrument(skip_all)]
fn sync_database_incremental(
    db: &Database,
    scanner: &Scanner,
    _library_path: &Path,
) -> Result<SyncStats> {
    let mut stats = SyncStats::default();

    db.begin_transaction()?;

    // === Phase 1: Scan directories only (fast - no file stats) ===
    info!("scanning directories only (incremental mode)");
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message("Scanning directories...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let mut fs_dirs: HashMap<String, i64> = HashMap::new();
    for dir in scanner.scan_directories() {
        fs_dirs.insert(dir.relative_path, dir.mtime);
        if fs_dirs.len() % 100 == 0 {
            spinner.set_message(format!("Scanning directories... {}", fs_dirs.len()));
        }
    }
    spinner.finish_with_message(format!("Scanned {} directories", fs_dirs.len()));
    info!(dirs = fs_dirs.len(), "directory scan complete");

    // === Phase 2: Load directories from database ===
    debug!("loading directories from database");
    let db_dirs: HashMap<String, (i64, Option<i64>)> = db
        .get_all_directories()?
        .into_iter()
        .map(|d| (d.path, (d.id, d.mtime)))
        .collect();

    // === Phase 3: Determine which directories need file scanning ===
    let mut dirs_to_scan_files: HashSet<String> = HashSet::new();

    // New directories (in filesystem but not in DB)
    for (path, _mtime) in &fs_dirs {
        if !db_dirs.contains_key(path) {
            dirs_to_scan_files.insert(path.clone());
        }
    }

    // Directories with changed mtime
    for (path, fs_mtime) in &fs_dirs {
        if let Some((_id, db_mtime)) = db_dirs.get(path) {
            if db_mtime.map(|m| m != *fs_mtime).unwrap_or(true) {
                dirs_to_scan_files.insert(path.clone());
            }
        }
    }

    // Deleted directories (in DB but not in filesystem)
    // Note: Skip "" directory (root-level files)
    let dirs_to_delete: Vec<_> = db_dirs
        .iter()
        .filter(|(path, _)| !path.is_empty() && !fs_dirs.contains_key(*path))
        .map(|(path, (id, _))| (path.clone(), *id))
        .collect();

    info!(
        new_dirs = dirs_to_scan_files.len(),
        deleted_dirs = dirs_to_delete.len(),
        "incremental change detection complete"
    );

    // === Phase 4: If no changes detected, quick exit ===
    if dirs_to_scan_files.is_empty() && dirs_to_delete.is_empty() {
        debug!("no directory changes detected, skipping file scan");
        db.commit()?;
        return Ok(stats);
    }

    // === Phase 5: Scan files only in changed directories ===
    let fs_files = if dirs_to_scan_files.is_empty() {
        Vec::new()
    } else {
        info!(dirs = dirs_to_scan_files.len(), "scanning files in changed directories");
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        spinner.set_message(format!("Scanning files in {} directories...", dirs_to_scan_files.len()));
        spinner.enable_steady_tick(std::time::Duration::from_millis(100));

        let files = scanner.scan_files_in_directories(&dirs_to_scan_files);
        spinner.finish_with_message(format!("Found {} files in {} directories", files.len(), dirs_to_scan_files.len()));
        files
    };
    debug!(files = fs_files.len(), "file scan complete");

    // Build lookup for new files
    let fs_file_set: HashSet<(String, String)> = fs_files
        .iter()
        .map(|f| (f.directory.clone(), f.filename.clone()))
        .collect();

    // === Phase 6: Handle deleted directories ===
    // Delete files first (FK constraint), then directories
    for (path, id) in &dirs_to_delete {
        let files_in_dir = db.get_files_in_directory(*id)?;
        for file in files_in_dir {
            db.delete_file(file.id)?;
            stats.files_removed += 1;
        }
        debug!(path, "deleted directory");
    }

    // Delete directories deepest first
    let mut dirs_to_delete_sorted = dirs_to_delete;
    dirs_to_delete_sorted.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()));
    for (_path, id) in dirs_to_delete_sorted {
        db.delete_directory(id)?;
        stats.directories_removed += 1;
    }

    // === Phase 7: Add new directories ===
    let mut dir_path_to_id: HashMap<String, i64> = db_dirs
        .iter()
        .map(|(path, (id, _))| (path.clone(), *id))
        .collect();

    // Sort new directories by depth (parents first)
    let mut new_dirs: Vec<_> = fs_dirs
        .iter()
        .filter(|(path, _)| !db_dirs.contains_key(*path))
        .collect();
    new_dirs.sort_by_key(|(path, _)| path.matches('/').count());

    for (path, mtime) in new_dirs {
        let parent_path = Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| !s.is_empty());

        let parent_id = parent_path.as_ref().and_then(|p| {
            dir_path_to_id
                .get(p)
                .copied()
                .or_else(|| db.get_directory_by_path(p).ok().flatten().map(|d| d.id))
        });

        let id = db.insert_directory(path, parent_id, Some(*mtime))?;
        dir_path_to_id.insert(path.clone(), id);
        stats.directories_added += 1;
    }

    // === Phase 8: Update mtime for existing directories that changed ===
    for (path, fs_mtime) in &fs_dirs {
        if let Some((id, db_mtime)) = db_dirs.get(path) {
            if db_mtime.map(|m| m != *fs_mtime).unwrap_or(true) {
                db.set_directory_mtime(*id, *fs_mtime)?;
            }
        }
    }

    // === Phase 9: Handle files in changed directories ===
    // Delete files that no longer exist in scanned directories
    for dir_path in &dirs_to_scan_files {
        if let Some(id) = dir_path_to_id.get(dir_path) {
            let db_files = db.get_files_in_directory(*id)?;
            for file in db_files {
                if !fs_file_set.contains(&(dir_path.clone(), file.filename.clone())) {
                    db.delete_file(file.id)?;
                    stats.files_removed += 1;
                }
            }
        }
    }

    // Add/update files
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
            continue;
        }

        match db.get_file_by_name(dir_id, &file.filename)? {
            Some(db_file) => {
                if db_file.mtime != file.mtime || db_file.size != file.size as i64 {
                    db.update_file_metadata(db_file.id, file.size as i64, file.mtime)?;
                    stats.files_modified += 1;
                }
            }
            None => {
                let (width, height) = if file.media_type == MediaType::Image {
                    read_dimensions(&file.path)
                        .map(|(w, h)| (Some(w), Some(h)))
                        .unwrap_or((None, None))
                } else {
                    (None, None)
                };
                db.insert_file_with_dimensions(
                    dir_id,
                    &file.filename,
                    file.size as i64,
                    file.mtime,
                    Some(file.media_type.as_str()),
                    width,
                    height,
                )?;
                stats.files_added += 1;
            }
        }
    }

    debug!("committing to database");
    db.commit()?;
    info!(
        dirs_added = stats.directories_added,
        dirs_removed = stats.directories_removed,
        files_added = stats.files_added,
        files_removed = stats.files_removed,
        files_modified = stats.files_modified,
        "incremental sync complete"
    );

    Ok(stats)
}

/// Sync database with filesystem
#[instrument(skip_all)]
fn sync_database(db: &Database, scanner: &Scanner, library_path: &Path) -> Result<SyncStats> {
    let mut stats = SyncStats::default();

    db.begin_transaction()?;

    // === Phase 1: Gather current state ===

    // Single-pass filesystem scan for both directories and files
    info!("scanning filesystem");
    let scan_result = scanner.scan_all();
    let fs_dirs: HashMap<String, i64> = scan_result
        .directories
        .iter()
        .map(|d| (d.relative_path.clone(), d.mtime))
        .collect();
    let fs_files = scan_result.files;
    info!(dirs = fs_dirs.len(), files = fs_files.len(), "scan complete");

    // Get all directories from database
    debug!("loading directories from database");
    let db_dirs: HashMap<String, i64> = db.get_all_directories()?
        .into_iter()
        .map(|d| (d.path, d.id))
        .collect();
    let fs_file_set: HashSet<(String, String)> = fs_files
        .iter()
        .map(|f| (f.directory.clone(), f.filename.clone()))
        .collect();

    // Get all files from database
    debug!("loading files from database");
    let db_files = db.get_all_files()?;
    debug!(count = db_files.len(), "loaded files from database");

    // Create lookup from directory_id to path
    let dir_id_to_path: HashMap<i64, String> = db.get_all_directories()?
        .into_iter()
        .map(|d| (d.id, d.path))
        .collect();

    // === Phase 2: Detect moved directories and collect metadata ===
    debug!("detecting changes");

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

    // Early exit optimization: skip expensive metadata collection if no directories to delete
    let mut deleted_metadata: HashMap<String, DirectoryMetadata> = HashMap::new();
    if !dirs_to_delete.is_empty() {
        // Bulk-load all directory tags and file tags upfront (avoids N+1 queries)
        let all_dir_tags = db.get_all_directory_tags()?;
        let all_file_tags = db.get_all_file_tags()?;

        // Collect metadata from directories being deleted, keyed by basename
        // Also collect file metadata for preservation during moves
        for (path, &id) in &dirs_to_delete {
            let dir = db.get_directory(id)?;
            let dir_tags = all_dir_tags.get(&id).cloned().unwrap_or_default();
            let files_in_dir = db.get_files_in_directory(id)?;

            let has_dir_metadata = dir.as_ref().map(|d| d.rating.is_some()).unwrap_or(false)
                || !dir_tags.is_empty();
            let has_files = !files_in_dir.is_empty();

            // Preserve if has metadata OR has files (for thumbnail/metadata migration)
            if has_dir_metadata || has_files {
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
                    // Collect file metadata (ratings and tags) for each file
                    let mut files_metadata: HashMap<String, FileMetadata> = HashMap::new();
                    for file in files_in_dir {
                        let file_tags = all_file_tags.get(&file.id).cloned().unwrap_or_default();
                        // Only store if file has metadata worth preserving
                        if file.rating.is_some() || !file_tags.is_empty() {
                            files_metadata.insert(
                                file.filename.clone(),
                                FileMetadata {
                                    rating: file.rating,
                                    tags: file_tags,
                                },
                            );
                        } else {
                            // Still track file for thumbnail migration (with empty metadata)
                            files_metadata.insert(
                                file.filename.clone(),
                                FileMetadata {
                                    rating: None,
                                    tags: Vec::new(),
                                },
                            );
                        }
                    }

                    deleted_metadata.insert(
                        basename,
                        DirectoryMetadata {
                            rating: dir.and_then(|d| d.rating),
                            tags: dir_tags,
                            files: files_metadata,
                            old_path: path.to_string(),
                        },
                    );
                }
            }
        }
    }
    info!(
        dirs_to_delete = dirs_to_delete.len(),
        dirs_to_add = new_dir_paths.len(),
        "change detection complete"
    );

    // === Phase 3: Delete removed items (files first due to FK constraint) ===
    debug!("applying changes");

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

    // Track file metadata from moved directories (keyed by new dir path -> filename -> metadata)
    let mut moved_file_metadata: HashMap<String, HashMap<String, FileMetadata>> = HashMap::new();

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
            for filename in metadata.files.keys() {
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

            // Store file metadata for restoration after files are inserted
            moved_file_metadata.insert(path.clone(), metadata.files);

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
                let (width, height) = if file.media_type == MediaType::Image {
                    read_dimensions(&file.path)
                        .map(|(w, h)| (Some(w), Some(h)))
                        .unwrap_or((None, None))
                } else {
                    (None, None)
                };
                let file_id = db.insert_file_with_dimensions(
                    dir_id,
                    &file.filename,
                    file.size as i64,
                    file.mtime,
                    Some(file.media_type.as_str()),
                    width,
                    height,
                )?;
                stats.files_added += 1;

                // Check if this file was part of a moved directory and restore metadata
                if let Some(dir_files) = moved_file_metadata.get(&file.directory) {
                    if let Some(file_meta) = dir_files.get(&file.filename) {
                        // Restore rating
                        if let Some(rating) = file_meta.rating {
                            db.set_file_rating(file_id, Some(rating))?;
                        }
                        // Restore tags
                        for tag in &file_meta.tags {
                            db.add_file_tag(file_id, tag)?;
                        }
                    }
                }
            }
        }
    }
    debug!("changes applied");

    debug!("committing to database");
    db.commit()?;
    info!(
        dirs_added = stats.directories_added,
        dirs_removed = stats.directories_removed,
        dirs_moved = stats.directories_moved,
        files_added = stats.files_added,
        files_removed = stats.files_removed,
        files_modified = stats.files_modified,
        "sync complete"
    );

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

    #[test]
    fn test_sync_backfills_dimensions_for_existing_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create an image with known dimensions
        fs::create_dir_all(root.join("photos")).unwrap();
        let img = image::RgbImage::new(200, 100);
        img.save(root.join("photos/landscape.jpg")).unwrap();

        // Init — this should populate dimensions
        run_init(root).unwrap();

        // Verify init got the dimensions
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let dir = db.get_directory_by_path("photos").unwrap().unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();
        assert_eq!(files[0].width, Some(200));
        assert_eq!(files[0].height, Some(100));

        // Now clear them to simulate a pre-migration database
        db.connection()
            .execute("UPDATE files SET width = NULL, height = NULL", [])
            .unwrap();
        drop(db);

        // Sync should backfill the NULL dimensions
        let stats = run_sync(root, false, false).unwrap();
        assert_eq!(stats.dimensions_backfilled, 1);

        // Verify dimensions are populated again
        let db = Database::open(&root.join(DB_FILENAME)).unwrap();
        let files = db.get_files_in_directory(dir.id).unwrap();
        assert_eq!(files[0].width, Some(200));
        assert_eq!(files[0].height, Some(100));

        // Second sync should not backfill (already populated)
        drop(db);
        let stats2 = run_sync(root, false, false).unwrap();
        assert_eq!(stats2.dimensions_backfilled, 0);
    }
}
