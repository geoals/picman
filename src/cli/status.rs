use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

use crate::db::Database;
use crate::tui::widgets::{has_dir_preview, has_thumbnail, is_image_file, is_video_file};

/// Show library status and health information
pub fn run_status(library_path: &Path) -> Result<()> {
    let db_path = library_path.join(".picman.db");
    if !db_path.exists() {
        anyhow::bail!(
            "No database found at {}. Run 'picman init' first.",
            db_path.display()
        );
    }

    let db = Database::open(&db_path)?;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.enable_steady_tick(Duration::from_millis(100));

    spinner.set_message("Loading directories...");
    let directories = db.get_all_directories()?;

    spinner.set_message("Loading files...");
    let files = db.get_all_files()?;

    // Count files without hash
    let files_without_hash = files.iter().filter(|f| f.hash.is_none()).count();

    // Build dir_id -> path lookup
    let dir_paths: HashMap<i64, String> = directories
        .iter()
        .map(|d| (d.id, d.path.clone()))
        .collect();

    // Check thumbnails in parallel
    let checked = AtomicUsize::new(0);
    let total_files = files.len();

    spinner.set_message(format!("Checking thumbnails... 0/{}", total_files));

    let missing_thumbnail_dirs: Vec<String> = files
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
                let top_dir = dir_path.split('/').next().unwrap_or("(root)").to_string();
                Some(if top_dir.is_empty() { "(root)".to_string() } else { top_dir })
            } else {
                None
            }
        })
        .collect();

    // Count by top-level directory
    let mut missing_thumbnails_by_dir: HashMap<String, usize> = HashMap::new();
    for top_dir in &missing_thumbnail_dirs {
        *missing_thumbnails_by_dir.entry(top_dir.clone()).or_insert(0) += 1;
    }
    let total_missing_thumbnails = missing_thumbnail_dirs.len();

    // Count missing previews by top-level directory
    let mut missing_previews_by_dir: HashMap<String, usize> = HashMap::new();
    let mut total_missing_previews = 0usize;

    spinner.set_message("Checking previews...");
    for dir in &directories {
        if !has_dir_preview(dir.id) {
            total_missing_previews += 1;

            let top_dir = dir.path.split('/').next().unwrap_or("(root)").to_string();
            let top_dir = if top_dir.is_empty() {
                "(root)".to_string()
            } else {
                top_dir
            };
            *missing_previews_by_dir.entry(top_dir).or_insert(0) += 1;
        }
    }

    spinner.finish_and_clear();

    // Print status
    println!("Library: {}", library_path.display());
    println!("  Directories: {}", directories.len());
    println!("  Files: {}", files.len());
    println!();

    // Missing thumbnails
    if total_missing_thumbnails == 0 {
        println!("  Missing thumbnails: none");
    } else {
        let dir_count = missing_thumbnails_by_dir.len();
        println!(
            "  Missing thumbnails: {} files in {} directories",
            total_missing_thumbnails, dir_count
        );
    }

    // Missing previews
    if total_missing_previews == 0 {
        println!("  Missing previews: none");
    } else {
        let dir_count = missing_previews_by_dir.len();
        println!(
            "  Missing previews: {} directories (in {} top-level)",
            total_missing_previews, dir_count
        );
    }

    // Files without hash
    if files_without_hash == 0 {
        println!("  Files without hash: none");
    } else {
        println!("  Files without hash: {}", files_without_hash);
    }

    Ok(())
}
