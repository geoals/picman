use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

use crate::db::Database;
use crate::tui::widgets::{
    generate_image_thumbnail, generate_video_thumbnail, has_thumbnail, is_image_file,
    is_video_file,
};

/// Statistics from thumbnail generation
pub struct ThumbnailStats {
    pub total: usize,
    pub generated: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Generate thumbnails for all media files in the library
pub fn run_generate_thumbnails(library_path: &Path) -> Result<ThumbnailStats> {
    let db_path = library_path.join(".picman.db");
    let db = Database::open(&db_path)?;

    let directories = db.get_all_directories()?;

    // Collect all media files that need thumbnails
    println!("Scanning for files needing thumbnails...");
    let mut files_needing_thumbnails: Vec<(String, std::path::PathBuf)> = Vec::new();

    for dir in &directories {
        let files = db.get_files_in_directory(dir.id)?;
        for file in files {
            let path = if dir.path.is_empty() {
                library_path.join(&file.filename)
            } else {
                library_path.join(&dir.path).join(&file.filename)
            };

            if (is_image_file(&path) || is_video_file(&path)) && !has_thumbnail(&path) {
                let display_path = if dir.path.is_empty() {
                    file.filename.clone()
                } else {
                    format!("{}/{}", dir.path, file.filename)
                };
                files_needing_thumbnails.push((display_path, path));
            }
        }
    }

    let total_files = files_needing_thumbnails.len();
    let total_in_db: usize = directories
        .iter()
        .filter_map(|d| db.get_files_in_directory(d.id).ok())
        .map(|f| f.len())
        .sum();

    let skipped = total_in_db - total_files;

    if total_files == 0 {
        println!("All {} files already have thumbnails.", total_in_db);
        return Ok(ThumbnailStats {
            total: total_in_db,
            generated: 0,
            skipped,
            failed: 0,
        });
    }

    println!(
        "Generating {} thumbnails ({} already exist)...",
        total_files, skipped
    );

    let mut generated = 0;
    let mut failed = 0;

    for (i, (display_path, path)) in files_needing_thumbnails.iter().enumerate() {
        let proc_count = i + 1;

        let result = if is_image_file(path) {
            generate_image_thumbnail(path).is_some()
        } else if is_video_file(path) {
            generate_video_thumbnail(path).is_some()
        } else {
            false
        };

        if result {
            generated += 1;
            println!(
                "    [{}/{}] {} - OK ({} generated)",
                proc_count, total_files, display_path, generated
            );
        } else {
            failed += 1;
            println!(
                "    [{}/{}] {} - FAILED",
                proc_count, total_files, display_path
            );
        }
    }

    println!("Done: {} generated, {} failed", generated, failed);

    Ok(ThumbnailStats {
        total: total_in_db,
        generated,
        skipped,
        failed,
    })
}

/// Check which directories have missing thumbnails without generating them
pub fn run_check_thumbnails(library_path: &Path) -> Result<()> {
    let db_path = library_path.join(".picman.db");
    let db = Database::open(&db_path)?;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.enable_steady_tick(Duration::from_millis(100));

    spinner.set_message("Loading files...");
    let directories = db.get_all_directories()?;
    let all_files = db.get_all_files()?;
    let total_files = all_files.len();

    // Build dir_id -> path lookup
    let dir_paths: HashMap<i64, String> = directories
        .iter()
        .map(|d| (d.id, d.path.clone()))
        .collect();

    // Check thumbnails in parallel
    let checked = AtomicUsize::new(0);

    spinner.set_message(format!("Checking thumbnails... 0/{}", total_files));

    let missing_files: Vec<String> = all_files
        .par_iter()
        .filter_map(|file| {
            let count = checked.fetch_add(1, Ordering::Relaxed);
            if count % 1000 == 0 {
                spinner.set_message(format!("Checking thumbnails... {}/{}", count, total_files));
            }

            let dir_path = dir_paths.get(&file.directory_id).map(|s| s.as_str()).unwrap_or("");
            let path = if dir_path.is_empty() {
                library_path.join(&file.filename)
            } else {
                library_path.join(dir_path).join(&file.filename)
            };

            if (is_image_file(&path) || is_video_file(&path)) && !has_thumbnail(&path) {
                // Return top-level directory
                let top_dir = dir_path.split('/').next().unwrap_or("").to_string();
                Some(if top_dir.is_empty() { "(root)".to_string() } else { top_dir })
            } else {
                None
            }
        })
        .collect();

    spinner.finish_and_clear();

    // Count by top-level directory
    let mut missing_by_top_dir: HashMap<String, usize> = HashMap::new();
    for top_dir in &missing_files {
        *missing_by_top_dir.entry(top_dir.clone()).or_insert(0) += 1;
    }
    let total_missing = missing_files.len();

    if total_missing == 0 {
        println!("All files have thumbnails.");
        return Ok(());
    }

    println!("Missing thumbnails:");
    let mut sorted: Vec<_> = missing_by_top_dir.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by count descending

    for (dir, count) in &sorted {
        println!("  {:<40} {} files", format!("{}/", dir), count);
    }
    println!("Total: {} files in {} directories", total_missing, sorted.len());

    Ok(())
}
