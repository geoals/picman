use std::path::Path;

use anyhow::Result;

use crate::db::Database;
use crate::tui::widgets::{
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

    // Phase 2: Generate previews sequentially
    println!("  Phase 2: Generating preview images...");
    let mut generated = 0;

    for (i, (dir_id, path, images)) in preview_data.iter().enumerate() {
        let proc_count = i + 1;

        if images.is_empty() {
            println!("    [{}/{}] {} (no images)", proc_count, to_generate, path);
            continue;
        }

        let result = if generate_dir_preview_from_paths(*dir_id, images).is_some() {
            generated += 1;
            "OK"
        } else {
            "FAILED"
        };

        println!("    [{}/{}] {} - {} ({} generated)", proc_count, to_generate, path, result, generated);
    }

    let final_generated = generated;
    println!("  Phase 2 complete: {} previews generated", final_generated);

    Ok(PreviewStats {
        total,
        generated: final_generated,
        skipped,
    })
}
