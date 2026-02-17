use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

use crate::db::Database;
use crate::thumbnails::{
    collect_preview_images_standalone, generate_dir_preview_from_paths, has_dir_preview,
    TempPreviewState,
};

/// Statistics from preview generation
pub struct PreviewStats {
    pub total: usize,
    pub generated: usize,
    pub skipped: usize,
}

/// Generate directory previews for all directories in the library
pub fn run_generate_previews(library_path: &Path) -> Result<PreviewStats> {
    let db_path = library_path.join(".picman.db");
    let db = Database::open(&db_path)?;

    let directories = db.get_all_directories()?;
    let total = directories.len();

    // Find directories that need preview generation
    let dirs_needing_preview: Vec<_> = directories
        .iter()
        .filter(|d| !has_dir_preview(d.id))
        .cloned()
        .collect();

    let to_generate = dirs_needing_preview.len();
    let skipped = total - to_generate;

    if to_generate == 0 {
        return Ok(PreviewStats {
            total,
            generated: 0,
            skipped,
        });
    }

    println!(
        "Generating {} previews ({} already exist)...",
        to_generate, skipped
    );

    // Create temp state for preview generation
    let temp_state = TempPreviewState {
        library_path: library_path.to_path_buf(),
        db,
        directories: directories.clone(),
    };

    // Phase 1: Collect image paths (sequential - needs DB access)
    println!("  Phase 1: Collecting image paths...");
    let preview_data: Vec<(i64, String, Vec<std::path::PathBuf>)> = dirs_needing_preview
        .iter()
        .enumerate()
        .map(|(i, dir)| {
            if (i + 1) % 100 == 0 {
                println!("    Scanned {} / {} directories", i + 1, to_generate);
            }
            let images = collect_preview_images_standalone(&temp_state, dir);
            (dir.id, dir.path.clone(), images)
        })
        .collect();
    println!("  Phase 1 complete: {} directories scanned", preview_data.len());

    // Phase 2: Generate previews in parallel
    println!("  Phase 2: Generating preview images...");

    let progress = ProgressBar::new(to_generate as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("    {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}")
            .unwrap()
            .progress_chars("██░"),
    );

    let generated = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    preview_data.par_iter().for_each(|(dir_id, _path, images)| {
        if !images.is_empty() {
            if generate_dir_preview_from_paths(*dir_id, images).is_some() {
                generated.fetch_add(1, Ordering::Relaxed);
            } else {
                failed.fetch_add(1, Ordering::Relaxed);
            }
        }
        let gen = generated.load(Ordering::Relaxed);
        let fail = failed.load(Ordering::Relaxed);
        progress.set_message(format!("{} generated, {} failed", gen, fail));
        progress.inc(1);
    });

    progress.finish_and_clear();

    let final_generated = generated.load(Ordering::Relaxed);
    let final_failed = failed.load(Ordering::Relaxed);
    println!(
        "  Phase 2 complete: {} generated, {} failed, {} empty",
        final_generated,
        final_failed,
        to_generate - final_generated - final_failed
    );

    Ok(PreviewStats {
        total,
        generated: final_generated,
        skipped,
    })
}

/// Check which directories are missing preview images without generating them
pub fn run_check_previews(library_path: &Path) -> Result<()> {
    let db_path = library_path.join(".picman.db");
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

    // Find directories missing previews in parallel
    let checked = AtomicUsize::new(0);
    let total = directories.len();

    spinner.set_message(format!("Checking previews... 0/{}", total));

    let missing_dirs: Vec<(String, String)> = directories
        .par_iter()
        .filter_map(|dir| {
            let count = checked.fetch_add(1, Ordering::Relaxed);
            if count % 100 == 0 {
                spinner.set_message(format!("Checking previews... {}/{}", count, total));
            }

            if !has_dir_preview(dir.id) {
                let top_dir = dir.path.split('/').next().unwrap_or("").to_string();
                let top_dir = if top_dir.is_empty() {
                    "(root)".to_string()
                } else {
                    top_dir
                };
                Some((top_dir, dir.path.clone()))
            } else {
                None
            }
        })
        .collect();

    spinner.finish_and_clear();

    // Group by top-level directory
    let mut missing_by_top_dir: HashMap<String, Vec<String>> = HashMap::new();
    for (top_dir, path) in missing_dirs {
        missing_by_top_dir.entry(top_dir).or_default().push(path);
    }

    let total_missing: usize = missing_by_top_dir.values().map(|v| v.len()).sum();

    if total_missing == 0 {
        println!("All directories have preview images.");
        return Ok(());
    }

    println!("Missing previews:");
    let mut sorted: Vec<_> = missing_by_top_dir.into_iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len())); // Sort by count descending

    for (dir, missing) in &sorted {
        println!("  {:<40} {} directories", format!("{}/", dir), missing.len());
    }
    println!("Total: {} directories", total_missing);

    Ok(())
}
