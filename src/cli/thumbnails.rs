use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

use crate::db::Database;
use crate::thumbnails::{
    generate_image_thumbnail, generate_video_thumbnail, generate_web_thumbnail,
    generate_web_video_thumbnail, has_thumbnail, has_web_thumbnail, is_image_file, is_video_file,
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

    // Build dir_id -> path lookup
    let dir_paths: HashMap<i64, String> = directories
        .iter()
        .map(|d| (d.id, d.path.clone()))
        .collect();

    // Get all files from DB (sequential - SQLite)
    let all_files = db.get_all_files()?;
    let total_files = all_files.len();

    // Filter to media files and check thumbnails in parallel
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.enable_steady_tick(Duration::from_millis(100));

    let checked = AtomicUsize::new(0);

    let files_needing_thumbnails: Vec<(String, std::path::PathBuf)> = all_files
        .par_iter()
        .filter_map(|file| {
            let count = checked.fetch_add(1, Ordering::Relaxed);
            if count % 500 == 0 {
                spinner.set_message(format!("Checking files... {}/{}", count, total_files));
            }

            let dir_path = dir_paths.get(&file.directory_id).map(|s| s.as_str()).unwrap_or("");
            let path = if dir_path.is_empty() {
                library_path.join(&file.filename)
            } else {
                library_path.join(dir_path).join(&file.filename)
            };

            if (is_image_file(&path) || is_video_file(&path)) && !has_thumbnail(&path) {
                let display_path = if dir_path.is_empty() {
                    file.filename.clone()
                } else {
                    format!("{}/{}", dir_path, file.filename)
                };
                Some((display_path, path))
            } else {
                None
            }
        })
        .collect();

    spinner.finish_and_clear();

    let needing_count = files_needing_thumbnails.len();
    let skipped = total_files - needing_count;

    if needing_count == 0 {
        println!("All {} files already have thumbnails.", total_files);
        return Ok(ThumbnailStats {
            total: total_files,
            generated: 0,
            skipped,
            failed: 0,
        });
    }

    println!(
        "Generating {} thumbnails ({} already exist)...",
        needing_count, skipped
    );

    let progress = ProgressBar::new(needing_count as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("    {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}")
            .unwrap()
            .progress_chars("██░"),
    );

    let generated = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    files_needing_thumbnails
        .par_iter()
        .for_each(|(_display_path, path)| {
            let result = if is_image_file(path) {
                generate_image_thumbnail(path).is_some()
            } else if is_video_file(path) {
                generate_video_thumbnail(path).is_some()
            } else {
                false
            };

            if result {
                generated.fetch_add(1, Ordering::Relaxed);
            } else {
                failed.fetch_add(1, Ordering::Relaxed);
            }

            let gen = generated.load(Ordering::Relaxed);
            let fail = failed.load(Ordering::Relaxed);
            progress.set_message(format!("{} generated, {} failed", gen, fail));
            progress.inc(1);
        });

    progress.finish_and_clear();

    let generated = generated.load(Ordering::Relaxed);
    let failed = failed.load(Ordering::Relaxed);
    println!("Done: {} generated, {} failed", generated, failed);

    Ok(ThumbnailStats {
        total: total_files,
        generated,
        skipped,
        failed,
    })
}

/// Generate small (400px) web thumbnails for all media files in the library
pub fn run_generate_web_thumbnails(library_path: &Path) -> Result<ThumbnailStats> {
    let db_path = library_path.join(".picman.db");
    let db = Database::open(&db_path)?;

    let directories = db.get_all_directories()?;

    let dir_paths: HashMap<i64, String> = directories
        .iter()
        .map(|d| (d.id, d.path.clone()))
        .collect();

    let all_files = db.get_all_files()?;
    let total_files = all_files.len();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.enable_steady_tick(Duration::from_millis(100));

    let checked = AtomicUsize::new(0);

    let files_needing_thumbnails: Vec<(String, std::path::PathBuf)> = all_files
        .par_iter()
        .filter_map(|file| {
            let count = checked.fetch_add(1, Ordering::Relaxed);
            if count % 500 == 0 {
                spinner.set_message(format!("Checking web thumbnails... {}/{}", count, total_files));
            }

            let dir_path = dir_paths.get(&file.directory_id).map(|s| s.as_str()).unwrap_or("");
            let path = if dir_path.is_empty() {
                library_path.join(&file.filename)
            } else {
                library_path.join(dir_path).join(&file.filename)
            };

            if (is_image_file(&path) || is_video_file(&path)) && !has_web_thumbnail(&path) {
                let display_path = if dir_path.is_empty() {
                    file.filename.clone()
                } else {
                    format!("{}/{}", dir_path, file.filename)
                };
                Some((display_path, path))
            } else {
                None
            }
        })
        .collect();

    spinner.finish_and_clear();

    let needing_count = files_needing_thumbnails.len();
    let skipped = total_files - needing_count;

    if needing_count == 0 {
        println!("All {} files already have web thumbnails.", total_files);
        return Ok(ThumbnailStats {
            total: total_files,
            generated: 0,
            skipped,
            failed: 0,
        });
    }

    println!(
        "Generating {} web thumbnails ({} already exist)...",
        needing_count, skipped
    );

    let progress = ProgressBar::new(needing_count as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("    {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}")
            .unwrap()
            .progress_chars("██░"),
    );

    let generated = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    files_needing_thumbnails
        .par_iter()
        .for_each(|(_display_path, path)| {
            let result = if is_image_file(path) {
                generate_web_thumbnail(path).is_some()
            } else if is_video_file(path) {
                generate_web_video_thumbnail(path).is_some()
            } else {
                false
            };

            if result {
                generated.fetch_add(1, Ordering::Relaxed);
            } else {
                failed.fetch_add(1, Ordering::Relaxed);
            }

            let gen = generated.load(Ordering::Relaxed);
            let fail = failed.load(Ordering::Relaxed);
            progress.set_message(format!("{} generated, {} failed", gen, fail));
            progress.inc(1);
        });

    progress.finish_and_clear();

    let generated = generated.load(Ordering::Relaxed);
    let failed = failed.load(Ordering::Relaxed);
    println!("Done: {} generated, {} failed", generated, failed);

    Ok(ThumbnailStats {
        total: total_files,
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
