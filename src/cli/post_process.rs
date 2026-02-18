use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use tracing::{debug, info, instrument, warn};

use crate::db::Database;
use crate::hash::compute_file_hash;
use crate::scanner::{detect_orientation, read_dimensions_fast};

const HASH_BATCH_SIZE: usize = 1000;
const DIMENSION_BATCH_SIZE: usize = 1000;
const ORIENTATION_BATCH_SIZE: usize = 5000;

/// Detect image orientation and add landscape/portrait tags
#[instrument(skip(db, library_path))]
pub(super) fn tag_orientation(db: &Database, library_path: &Path) -> Result<usize> {
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
pub(super) fn backfill_dimensions(db: &Database, library_path: &Path) -> Result<usize> {
    let files = db.get_files_needing_dimensions()?;
    let total = files.len();

    if total == 0 {
        return Ok(0);
    }

    let progress = ProgressBar::new(total as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("    {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {elapsed_precise} | {msg}")
            .unwrap()
            .progress_chars("██░"),
    );
    progress.set_message("reading dimensions");

    let mut backfilled = 0usize;

    // Process in batches for resumability on interrupt
    for batch in files.chunks(DIMENSION_BATCH_SIZE) {
        db.begin_transaction()?;
        for file in batch {
            let full_path = library_path.join(&file.path);
            if let Some((w, h)) = read_dimensions_fast(&full_path) {
                db.set_file_dimensions(file.id, w, h)?;
                backfilled += 1;
            }
            progress.inc(1);
        }
        db.commit()?;
    }

    progress.finish_with_message(format!("{backfilled} dimensions backfilled"));

    Ok(backfilled)
}

/// Hash files that have NULL hash values
#[instrument(skip(db, library_path))]
pub(super) fn hash_files(db: &Database, library_path: &Path) -> Result<(usize, usize)> {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::TempDir;

    use super::super::init::DB_FILENAME;
    use crate::cli::{run_init, run_sync};
    use crate::db::Database;

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
}
