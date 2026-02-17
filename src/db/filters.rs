use std::collections::{HashMap, HashSet};

use anyhow::Result;
use tracing::{debug, instrument};

use super::Database;
use crate::tui::dialogs::RatingFilter;

impl Database {
    /// Get IDs of directories containing files that match the filter criteria,
    /// OR directories that themselves have matching tags.
    /// Also includes ancestor directories to maintain tree structure.
    /// For multiple tags, uses AND logic (must have ALL tags).
    #[instrument(skip(self))]
    pub fn get_directories_with_matching_files(
        &self,
        rating_filter: RatingFilter,
        tags: &[String],
        video_only: bool,
    ) -> Result<HashSet<i64>> {
        let mut matching_dir_ids: HashSet<i64> = HashSet::new();

        if rating_filter == RatingFilter::Any && tags.is_empty() && !video_only {
            // No filter - return empty set (caller should show all)
            return Ok(matching_dir_ids);
        }

        // === Part 1: Find directories with matching FILES ===
        let mut file_conditions = Vec::new();

        if video_only {
            file_conditions.push("f.media_type = 'video'".to_string());
        }

        let min_rating = match rating_filter {
            RatingFilter::Any => None,
            RatingFilter::Unrated => {
                file_conditions.push("f.rating IS NULL".to_string());
                None
            }
            RatingFilter::MinRating(r) => {
                file_conditions.push("f.rating >= ?1".to_string());
                Some(r)
            }
        };

        if !tags.is_empty() {
            let tag_param_start = if min_rating.is_some() { 2 } else { 1 };
            let tag_count_param = tag_param_start;
            let tag_placeholders = (0..tags.len())
                .map(|i| format!("?{}", tag_param_start + 1 + i))
                .collect::<Vec<_>>()
                .join(",");
            file_conditions.push(format!(
                "(SELECT COUNT(DISTINCT t.name) FROM file_tags ft JOIN tags t ON ft.tag_id = t.id WHERE ft.file_id = f.id AND t.name IN ({})) = ?{}",
                tag_placeholders, tag_count_param
            ));
        }

        // Only query files if we have conditions
        if !file_conditions.is_empty() {
            let query = format!(
                "SELECT DISTINCT f.directory_id FROM files f WHERE {}",
                file_conditions.join(" AND ")
            );

            let mut stmt = self.connection().prepare(&query)?;

            // Build parameters based on what filters are active
            let mut params: Vec<rusqlite::types::Value> = Vec::new();

            if let Some(rating) = min_rating {
                params.push(rating.into());
            }

            if !tags.is_empty() {
                let tag_count = tags.len() as i64;
                params.push(tag_count.into());
                params.extend(tags.iter().map(|t| t.clone().into()));
            }

            let dir_ids: Vec<i64> = stmt
                .query_map(rusqlite::params_from_iter(params), |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;

            matching_dir_ids.extend(dir_ids.iter());
        }

        // === Part 2: Find directories that match the DIRECTORY-LEVEL filter ===
        let all_dirs = self.get_all_directories()?;
        let mut dirs_matching_full_filter: Vec<i64> = Vec::new();

        // Build lookup maps for efficient access
        let dir_parent_map: HashMap<i64, Option<i64>> = all_dirs
            .iter()
            .map(|d| (d.id, d.parent_id))
            .collect();

        // Fetch all directory tags in one query (instead of N queries)
        let all_dir_tags = if !tags.is_empty() && !video_only {
            self.get_all_directory_tags()?
        } else {
            HashMap::new()
        };

        if !video_only {
            for dir in &all_dirs {
                // Check rating filter on directory
                let dir_matches_rating = match rating_filter {
                    RatingFilter::Any => true,
                    RatingFilter::Unrated => dir.rating.is_none(),
                    RatingFilter::MinRating(min) => dir.rating.map(|r| r >= min).unwrap_or(false),
                };

                // Check tag filter on directory (using pre-fetched tags)
                let dir_matches_tags = if tags.is_empty() {
                    true
                } else {
                    let dir_tags = all_dir_tags.get(&dir.id).map(|v| v.as_slice()).unwrap_or(&[]);
                    tags.iter().all(|t| dir_tags.contains(t))
                };

                // Directory matches if it passes both filters
                if dir_matches_rating && dir_matches_tags {
                    dirs_matching_full_filter.push(dir.id);
                    matching_dir_ids.insert(dir.id);
                }
            }
        }

        // === Part 3: Include ALL DESCENDANTS of directories that match the full filter ===
        for &matching_dir_id in &dirs_matching_full_filter {
            for dir in &all_dirs {
                let mut current_id = dir.parent_id;
                while let Some(pid) = current_id {
                    if pid == matching_dir_id {
                        matching_dir_ids.insert(dir.id);
                        break;
                    }
                    current_id = dir_parent_map.get(&pid).copied().flatten();
                }
            }
        }

        // === Part 4: Include ancestor directories to maintain tree structure ===
        let mut ancestors_to_add: HashSet<i64> = HashSet::new();

        for &dir_id in matching_dir_ids.clone().iter() {
            let mut current_id = Some(dir_id);
            while let Some(id) = current_id {
                if let Some(&parent_id_opt) = dir_parent_map.get(&id) {
                    if let Some(parent_id) = parent_id_opt {
                        if !matching_dir_ids.contains(&parent_id) {
                            ancestors_to_add.insert(parent_id);
                        }
                        current_id = Some(parent_id);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        matching_dir_ids.extend(ancestors_to_add);

        debug!(count = matching_dir_ids.len(), "found matching directories");
        Ok(matching_dir_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_directories_with_matching_files() {
        let db = Database::open_in_memory().unwrap();

        let root_id = db.insert_directory("", None, None).unwrap();
        let photos_id = db.insert_directory("photos", Some(root_id), None).unwrap();
        let vacation_id = db.insert_directory("photos/vacation", Some(photos_id), None).unwrap();
        let work_id = db.insert_directory("work", Some(root_id), None).unwrap();

        let file1_id = db.insert_file(photos_id, "photo1.jpg", 1024, 12345, Some("image")).unwrap();
        let file2_id = db.insert_file(vacation_id, "beach.jpg", 1024, 12346, Some("image")).unwrap();
        let file3_id = db.insert_file(work_id, "doc.jpg", 1024, 12347, Some("image")).unwrap();

        db.set_file_rating(file1_id, Some(3)).unwrap();
        db.set_file_rating(file2_id, Some(5)).unwrap();
        db.set_file_rating(file3_id, Some(2)).unwrap();

        db.add_file_tag(file1_id, "family").unwrap();
        db.add_file_tag(file2_id, "family").unwrap();
        db.add_file_tag(file2_id, "vacation").unwrap();

        // No filter returns empty set
        let result = db.get_directories_with_matching_files(RatingFilter::Any, &[], false).unwrap();
        assert!(result.is_empty());

        // Rating filter only
        let result = db.get_directories_with_matching_files(RatingFilter::MinRating(4), &[], false).unwrap();
        assert!(result.contains(&vacation_id));
        assert!(result.contains(&photos_id));
        assert!(result.contains(&root_id));
        assert!(!result.contains(&work_id));

        // Tag filter (single)
        let result = db.get_directories_with_matching_files(RatingFilter::Any, &["family".to_string()], false).unwrap();
        assert!(result.contains(&photos_id));
        assert!(result.contains(&vacation_id));
        assert!(!result.contains(&work_id));

        // Tag filter (multiple - AND logic)
        let result = db.get_directories_with_matching_files(
            RatingFilter::Any,
            &["family".to_string(), "vacation".to_string()],
            false,
        ).unwrap();
        assert!(result.contains(&vacation_id));

        // Combined rating and tag
        let result = db.get_directories_with_matching_files(
            RatingFilter::MinRating(4),
            &["family".to_string()],
            false,
        ).unwrap();
        assert!(result.contains(&vacation_id));
        assert!(result.contains(&photos_id));
        assert!(result.contains(&root_id));
    }

    #[test]
    fn test_directory_tag_includes_all_descendants() {
        let db = Database::open_in_memory().unwrap();

        let photos_id = db.insert_directory("photos", None, None).unwrap();
        let vacation_id = db.insert_directory("photos/vacation", Some(photos_id), None).unwrap();
        let beach_id = db.insert_directory("photos/vacation/beach", Some(vacation_id), None).unwrap();

        db.insert_file(photos_id, "root.jpg", 1024, 12345, Some("image")).unwrap();
        db.insert_file(vacation_id, "trip.jpg", 1024, 12346, Some("image")).unwrap();
        db.insert_file(beach_id, "sunset.jpg", 1024, 12347, Some("image")).unwrap();

        db.add_directory_tag(vacation_id, "travel").unwrap();

        let result = db.get_directories_with_matching_files(
            RatingFilter::Any, &["travel".to_string()], false,
        ).unwrap();

        assert!(result.contains(&vacation_id));
        assert!(result.contains(&beach_id));
        assert!(result.contains(&photos_id));
    }

    #[test]
    fn test_directory_rating_includes_all_descendants() {
        let db = Database::open_in_memory().unwrap();

        let photos_id = db.insert_directory("photos", None, None).unwrap();
        let vacation_id = db.insert_directory("photos/vacation", Some(photos_id), None).unwrap();
        let beach_id = db.insert_directory("photos/vacation/beach", Some(vacation_id), None).unwrap();

        db.insert_file(photos_id, "root.jpg", 1024, 12345, Some("image")).unwrap();
        db.insert_file(vacation_id, "trip.jpg", 1024, 12346, Some("image")).unwrap();
        db.insert_file(beach_id, "sunset.jpg", 1024, 12347, Some("image")).unwrap();

        db.set_directory_rating(vacation_id, Some(5)).unwrap();

        let result = db.get_directories_with_matching_files(
            RatingFilter::MinRating(4), &[], false,
        ).unwrap();

        assert!(result.contains(&vacation_id));
        assert!(result.contains(&beach_id));
        assert!(result.contains(&photos_id));
    }

    #[test]
    fn test_file_tag_only_shows_that_file_not_siblings() {
        let db = Database::open_in_memory().unwrap();

        let photos_id = db.insert_directory("photos", None, None).unwrap();

        let file1_id = db.insert_file(photos_id, "photo1.jpg", 1024, 12345, Some("image")).unwrap();
        db.insert_file(photos_id, "photo2.jpg", 1024, 12346, Some("image")).unwrap();
        db.insert_file(photos_id, "photo3.jpg", 1024, 12347, Some("image")).unwrap();

        db.add_file_tag(file1_id, "favorite").unwrap();

        let result = db.get_directories_with_matching_files(
            RatingFilter::Any, &["favorite".to_string()], false,
        ).unwrap();

        assert!(result.contains(&photos_id));
    }
}
