use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::db::Database;
use crate::perceptual_hash::{group_by_similarity, hamming_distance};

use super::init::DB_FILENAME;

/// JSON output format for duplicate groups
#[derive(Serialize)]
struct DupesOutput {
    exact: Vec<ExactGroup>,
    similar: Vec<SimilarGroup>,
    summary: Summary,
}

#[derive(Serialize)]
struct ExactGroup {
    hash: String,
    size: i64,
    files: Vec<String>,
}

#[derive(Serialize)]
struct SimilarGroup {
    max_distance: u32,
    files: Vec<SimilarFile>,
}

#[derive(Serialize)]
struct SimilarFile {
    path: String,
    size: i64,
    width: Option<i32>,
    height: Option<i32>,
}

#[derive(Serialize)]
struct Summary {
    exact_groups: usize,
    exact_files: usize,
    similar_groups: usize,
    similar_files: usize,
}

/// Run the dupes command: find exact and perceptual duplicates
pub fn run_dupes(
    library_path: &Path,
    subdir: Option<&Path>,
    json: bool,
    threshold: u32,
) -> Result<()> {
    let library_path = library_path
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("Library path does not exist: {}", library_path.display()))?;

    let db_path = library_path.join(DB_FILENAME);
    if !db_path.exists() {
        anyhow::bail!(
            "No database found at {}. Run 'picman init' first.",
            db_path.display()
        );
    }

    let db = Database::open(&db_path)?;

    // Warn about unhashed files
    let unhashed = db.get_files_needing_hash()?;
    if !unhashed.is_empty() {
        eprintln!(
            "Warning: {} files have no content hash. Run 'picman sync --hash' first.",
            unhashed.len()
        );
    }

    // Warn about files without perceptual hash
    let unphashed = db.get_files_needing_perceptual_hash()?;
    if !unphashed.is_empty() {
        eprintln!(
            "Warning: {} image files have no perceptual hash. Run 'picman sync --perceptual' first.",
            unphashed.len()
        );
    }

    // === Exact duplicates ===
    let exact_groups = db.find_duplicates_with_paths()?;
    let exact_groups: Vec<_> = if let Some(sub) = subdir {
        let sub_str = sub.to_string_lossy();
        exact_groups
            .into_iter()
            .filter(|g| g.files.iter().any(|(_, p)| path_in_subdir(p, &sub_str)))
            .collect()
    } else {
        exact_groups
    };

    // Collect file IDs from exact groups to exclude from perceptual results
    let exact_file_ids: HashSet<i64> = exact_groups
        .iter()
        .flat_map(|g| g.files.iter().map(|(f, _)| f.id))
        .collect();

    // === Perceptual duplicates ===
    let all_hashes = db.get_all_perceptual_hashes()?;
    // Convert i64 → u64 for comparison
    let hashes_u64: Vec<(i64, u64)> = all_hashes
        .iter()
        .map(|(id, h)| (*id, *h as u64))
        .collect();

    let similar_groups_raw = group_by_similarity(&hashes_u64, threshold);

    // Fetch file details for each perceptual group, filter out exact duplicates
    let mut similar_groups: Vec<SimilarGroupInfo> = Vec::new();
    for group_ids in &similar_groups_raw {
        // Skip groups where all members are already in exact duplicate groups
        if group_ids.iter().all(|id| exact_file_ids.contains(id)) {
            continue;
        }

        let mut files = Vec::new();
        let mut group_hashes: Vec<u64> = Vec::new();
        for &file_id in group_ids {
            if let Some((file, dir_path)) = db.get_file_with_path(file_id)? {
                let full_path = format_path(&dir_path, &file.filename);
                // Filter by subdir if specified
                if let Some(sub) = subdir {
                    let sub_str = sub.to_string_lossy();
                    if !path_in_subdir(&dir_path, &sub_str) && !path_in_subdir(&full_path, &sub_str) {
                        continue;
                    }
                }
                if let Some(phash) = file.perceptual_hash {
                    group_hashes.push(phash as u64);
                }
                files.push((file, dir_path));
            }
        }

        if files.len() >= 2 {
            // Calculate max distance within group
            let max_dist = max_pairwise_distance(&group_hashes);
            similar_groups.push(SimilarGroupInfo {
                max_distance: max_dist,
                files,
            });
        }
    }

    if json {
        print_json(&exact_groups, &similar_groups)?;
    } else {
        print_human(&exact_groups, &similar_groups);
    }

    Ok(())
}

struct SimilarGroupInfo {
    max_distance: u32,
    files: Vec<(crate::db::File, String)>,
}

fn format_path(dir_path: &str, filename: &str) -> String {
    if dir_path.is_empty() {
        filename.to_string()
    } else {
        format!("{}/{}", dir_path, filename)
    }
}

fn path_in_subdir(path: &str, subdir: &str) -> bool {
    path == subdir || path.starts_with(&format!("{}/", subdir))
}

fn max_pairwise_distance(hashes: &[u64]) -> u32 {
    let mut max = 0;
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            let d = hamming_distance(hashes[i], hashes[j]);
            if d > max {
                max = d;
            }
        }
    }
    max
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn print_human(
    exact_groups: &[crate::db::DuplicateGroup],
    similar_groups: &[SimilarGroupInfo],
) {
    if exact_groups.is_empty() && similar_groups.is_empty() {
        println!("No duplicates found.");
        return;
    }

    // Exact duplicates
    if !exact_groups.is_empty() {
        println!("Exact copies (same file content):");
        for (i, group) in exact_groups.iter().enumerate() {
            let size = group.files.first().map(|(f, _)| f.size).unwrap_or(0);
            println!(
                "  Group {} ({} files, {} each):",
                i + 1,
                group.files.len(),
                format_size(size)
            );
            for (file, dir_path) in &group.files {
                println!("    {}", format_path(dir_path, &file.filename));
            }
        }
        println!();
    }

    // Perceptual duplicates
    if !similar_groups.is_empty() {
        println!("Visually similar (different compression/resolution):");
        for (i, group) in similar_groups.iter().enumerate() {
            println!(
                "  Group {} ({} files, distance {}):",
                i + 1,
                group.files.len(),
                group.max_distance
            );
            for (file, dir_path) in &group.files {
                let dims = match (file.width, file.height) {
                    (Some(w), Some(h)) => format!(", {}×{}", w, h),
                    _ => String::new(),
                };
                println!(
                    "    {} ({}{})",
                    format_path(dir_path, &file.filename),
                    format_size(file.size),
                    dims
                );
            }
        }
        println!();
    }

    // Summary
    let exact_files: usize = exact_groups.iter().map(|g| g.files.len()).sum();
    let similar_files: usize = similar_groups.iter().map(|g| g.files.len()).sum();
    println!(
        "Summary: {} exact group(s) ({} files), {} similar group(s) ({} files)",
        exact_groups.len(),
        exact_files,
        similar_groups.len(),
        similar_files
    );
}

fn print_json(
    exact_groups: &[crate::db::DuplicateGroup],
    similar_groups: &[SimilarGroupInfo],
) -> Result<()> {
    let exact: Vec<ExactGroup> = exact_groups
        .iter()
        .map(|g| ExactGroup {
            hash: g.hash.clone(),
            size: g.files.first().map(|(f, _)| f.size).unwrap_or(0),
            files: g
                .files
                .iter()
                .map(|(f, p)| format_path(p, &f.filename))
                .collect(),
        })
        .collect();

    let similar: Vec<SimilarGroup> = similar_groups
        .iter()
        .map(|g| SimilarGroup {
            max_distance: g.max_distance,
            files: g
                .files
                .iter()
                .map(|(f, p)| SimilarFile {
                    path: format_path(p, &f.filename),
                    size: f.size,
                    width: f.width,
                    height: f.height,
                })
                .collect(),
        })
        .collect();

    let exact_files: usize = exact.iter().map(|g| g.files.len()).sum();
    let similar_files: usize = similar.iter().map(|g| g.files.len()).sum();

    let output = DupesOutput {
        summary: Summary {
            exact_groups: exact.len(),
            exact_files,
            similar_groups: similar.len(),
            similar_files,
        },
        exact,
        similar,
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    use crate::cli::run_init;
    use crate::cli::run_sync;

    fn setup_library_with_dupes() -> (TempDir, std::path::PathBuf) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        // Create directories
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::create_dir_all(root.join("backup")).unwrap();

        // Create files with same content (exact duplicates)
        fs::write(root.join("photos/beach.jpg"), "identical content here").unwrap();
        fs::write(root.join("backup/beach_copy.jpg"), "identical content here").unwrap();

        // Create a unique file
        fs::write(root.join("photos/unique.jpg"), "completely different").unwrap();

        // Init and hash
        run_init(&root).unwrap();
        run_sync(&root, true, false, true).unwrap();

        (temp, root)
    }

    #[test]
    fn test_run_dupes_finds_exact_duplicates() {
        let (_temp, root) = setup_library_with_dupes();
        // This should not error
        let result = run_dupes(&root, None, false, 8);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_dupes_json_output() {
        let (_temp, root) = setup_library_with_dupes();
        let result = run_dupes(&root, None, true, 8);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_dupes_no_db_errors() {
        let temp = TempDir::new().unwrap();
        let result = run_dupes(temp.path(), None, false, 8);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_dupes_subdir_filter() {
        let (_temp, root) = setup_library_with_dupes();
        let subdir = std::path::Path::new("photos");
        let result = run_dupes(&root, Some(subdir), false, 8);
        assert!(result.is_ok());
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1_500_000), "1.4 MB");
        assert_eq!(format_size(2_500_000_000), "2.3 GB");
    }

    #[test]
    fn test_path_in_subdir() {
        assert!(path_in_subdir("photos/vacation", "photos"));
        assert!(path_in_subdir("photos", "photos"));
        assert!(!path_in_subdir("backup/photos", "photos"));
        assert!(!path_in_subdir("photography", "photos"));
    }

    #[test]
    fn test_max_pairwise_distance() {
        assert_eq!(max_pairwise_distance(&[]), 0);
        assert_eq!(max_pairwise_distance(&[0xFF]), 0);
        assert_eq!(max_pairwise_distance(&[0, 0xFF]), 8);
        assert_eq!(max_pairwise_distance(&[0, 3, 0xFF]), 8);
    }
}
