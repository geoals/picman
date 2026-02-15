use std::path::Path;

use anyhow::Result;

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
