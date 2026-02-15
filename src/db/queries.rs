use std::path::PathBuf;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::Database;

/// Represents a directory in the database
#[derive(Debug, Clone, PartialEq)]
pub struct Directory {
    pub id: i64,
    pub path: String,
    pub parent_id: Option<i64>,
    pub rating: Option<i32>,
    pub mtime: Option<i64>,
}

/// Represents a file in the database
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub id: i64,
    pub directory_id: i64,
    pub filename: String,
    pub size: i64,
    pub mtime: i64,
    pub hash: Option<String>,
    pub rating: Option<i32>,
    pub media_type: Option<String>,
}

/// Represents a file that needs hashing (id + full path)
#[derive(Debug, Clone)]
pub struct FileToHash {
    pub id: i64,
    pub path: PathBuf,
}

impl Database {
    // ==================== Directory Operations ====================

    /// Insert a new directory, returns its ID
    pub fn insert_directory(
        &self,
        path: &str,
        parent_id: Option<i64>,
        mtime: Option<i64>,
    ) -> Result<i64> {
        self.connection().execute(
            "INSERT INTO directories (path, parent_id, mtime) VALUES (?1, ?2, ?3)",
            params![path, parent_id, mtime],
        )?;
        Ok(self.connection().last_insert_rowid())
    }

    /// Get a directory by its path
    pub fn get_directory_by_path(&self, path: &str) -> Result<Option<Directory>> {
        let result = self
            .connection()
            .query_row(
                "SELECT id, path, parent_id, rating, mtime FROM directories WHERE path = ?1",
                [path],
                |row| {
                    Ok(Directory {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        parent_id: row.get(2)?,
                        rating: row.get(3)?,
                        mtime: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    /// Get a directory by its ID
    pub fn get_directory(&self, id: i64) -> Result<Option<Directory>> {
        let result = self
            .connection()
            .query_row(
                "SELECT id, path, parent_id, rating, mtime FROM directories WHERE id = ?1",
                [id],
                |row| {
                    Ok(Directory {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        parent_id: row.get(2)?,
                        rating: row.get(3)?,
                        mtime: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    /// Get all child directories of a parent
    pub fn get_child_directories(&self, parent_id: Option<i64>) -> Result<Vec<Directory>> {
        let row_mapper = |row: &rusqlite::Row| {
            Ok(Directory {
                id: row.get(0)?,
                path: row.get(1)?,
                parent_id: row.get(2)?,
                rating: row.get(3)?,
                mtime: row.get(4)?,
            })
        };

        if let Some(pid) = parent_id {
            let mut stmt = self.connection().prepare(
                "SELECT id, path, parent_id, rating, mtime FROM directories WHERE parent_id = ?1 ORDER BY path",
            )?;
            let result: Vec<Directory> = stmt
                .query_map([pid], row_mapper)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(result)
        } else {
            let mut stmt = self.connection().prepare(
                "SELECT id, path, parent_id, rating, mtime FROM directories WHERE parent_id IS NULL ORDER BY path",
            )?;
            let result: Vec<Directory> = stmt
                .query_map([], row_mapper)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(result)
        }
    }

    /// Update directory rating
    pub fn set_directory_rating(&self, id: i64, rating: Option<i32>) -> Result<()> {
        self.connection().execute(
            "UPDATE directories SET rating = ?1 WHERE id = ?2",
            params![rating, id],
        )?;
        Ok(())
    }

    /// Update directory mtime
    pub fn set_directory_mtime(&self, id: i64, mtime: i64) -> Result<()> {
        self.connection().execute(
            "UPDATE directories SET mtime = ?1 WHERE id = ?2",
            params![mtime, id],
        )?;
        Ok(())
    }

    /// Delete a directory by ID
    pub fn delete_directory(&self, id: i64) -> Result<()> {
        self.connection()
            .execute("DELETE FROM directories WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Rename a directory and update all descendant paths
    pub fn rename_directory(&self, id: i64, old_path: &str, new_path: &str) -> Result<()> {
        let conn = self.connection();

        // Update the directory itself
        conn.execute(
            "UPDATE directories SET path = ?1 WHERE id = ?2",
            params![new_path, id],
        )?;

        // Update all descendants: replace old_path prefix with new_path
        conn.execute(
            "UPDATE directories SET path = ?1 || substr(path, ?2)
             WHERE path LIKE ?3 AND id != ?4",
            params![
                new_path,
                old_path.len() + 1,  // +1 to skip the old prefix
                format!("{}/%", old_path),
                id
            ],
        )?;

        Ok(())
    }

    /// Get recursive stats for a directory (file count, total size)
    /// Includes the directory itself and all descendants
    pub fn get_directory_stats(&self, dir_id: i64) -> Result<(i64, i64)> {
        // Use recursive CTE to find all descendant directory IDs
        let mut stmt = self.connection().prepare(
            "WITH RECURSIVE descendants(id) AS (
                SELECT ?1
                UNION ALL
                SELECT d.id FROM directories d
                JOIN descendants ON d.parent_id = descendants.id
            )
            SELECT COUNT(*), COALESCE(SUM(f.size), 0)
            FROM files f
            WHERE f.directory_id IN (SELECT id FROM descendants)",
        )?;

        let (count, size): (i64, i64) = stmt.query_row([dir_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;

        Ok((count, size))
    }

    /// Repair parent_id values based on path strings.
    /// Returns the number of directories fixed.
    pub fn repair_directory_parents(&self) -> Result<usize> {
        let all_dirs = self.get_all_directories()?;

        // Build path -> id mapping
        let path_to_id: std::collections::HashMap<&str, i64> = all_dirs
            .iter()
            .map(|d| (d.path.as_str(), d.id))
            .collect();

        let mut fixed = 0;

        for dir in &all_dirs {
            // Derive expected parent path from this directory's path
            let expected_parent_path = std::path::Path::new(&dir.path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|s| !s.is_empty());

            // Get expected parent_id
            let expected_parent_id = expected_parent_path
                .as_ref()
                .and_then(|p| path_to_id.get(p.as_str()).copied());

            // Check if current parent_id matches expected
            if dir.parent_id != expected_parent_id {
                self.connection().execute(
                    "UPDATE directories SET parent_id = ?1 WHERE id = ?2",
                    rusqlite::params![expected_parent_id, dir.id],
                )?;
                fixed += 1;
            }
        }

        Ok(fixed)
    }

    /// Get all directories
    pub fn get_all_directories(&self) -> Result<Vec<Directory>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, path, parent_id, rating, mtime FROM directories ORDER BY path",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(Directory {
                id: row.get(0)?,
                path: row.get(1)?,
                parent_id: row.get(2)?,
                rating: row.get(3)?,
                mtime: row.get(4)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ==================== File Operations ====================

    /// Insert a new file, returns its ID
    pub fn insert_file(
        &self,
        directory_id: i64,
        filename: &str,
        size: i64,
        mtime: i64,
        media_type: Option<&str>,
    ) -> Result<i64> {
        self.connection().execute(
            "INSERT INTO files (directory_id, filename, size, mtime, media_type) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![directory_id, filename, size, mtime, media_type],
        )?;
        Ok(self.connection().last_insert_rowid())
    }

    /// Get a file by directory ID and filename
    pub fn get_file_by_name(&self, directory_id: i64, filename: &str) -> Result<Option<File>> {
        let result = self
            .connection()
            .query_row(
                "SELECT id, directory_id, filename, size, mtime, hash, rating, media_type
                 FROM files WHERE directory_id = ?1 AND filename = ?2",
                params![directory_id, filename],
                |row| {
                    Ok(File {
                        id: row.get(0)?,
                        directory_id: row.get(1)?,
                        filename: row.get(2)?,
                        size: row.get(3)?,
                        mtime: row.get(4)?,
                        hash: row.get(5)?,
                        rating: row.get(6)?,
                        media_type: row.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    /// Get all files in a directory
    pub fn get_files_in_directory(&self, directory_id: i64) -> Result<Vec<File>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, directory_id, filename, size, mtime, hash, rating, media_type
             FROM files WHERE directory_id = ?1 ORDER BY filename",
        )?;

        let rows = stmt.query_map([directory_id], |row| {
            Ok(File {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                filename: row.get(2)?,
                size: row.get(3)?,
                mtime: row.get(4)?,
                hash: row.get(5)?,
                rating: row.get(6)?,
                media_type: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get all files in the database
    pub fn get_all_files(&self) -> Result<Vec<File>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, directory_id, filename, size, mtime, hash, rating, media_type
             FROM files ORDER BY directory_id, filename",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(File {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                filename: row.get(2)?,
                size: row.get(3)?,
                mtime: row.get(4)?,
                hash: row.get(5)?,
                rating: row.get(6)?,
                media_type: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Update file hash
    pub fn set_file_hash(&self, id: i64, hash: &str) -> Result<()> {
        self.connection()
            .execute("UPDATE files SET hash = ?1 WHERE id = ?2", params![hash, id])?;
        Ok(())
    }

    /// Update file rating
    pub fn set_file_rating(&self, id: i64, rating: Option<i32>) -> Result<()> {
        self.connection().execute(
            "UPDATE files SET rating = ?1 WHERE id = ?2",
            params![rating, id],
        )?;
        Ok(())
    }

    /// Update file mtime and size (for sync)
    pub fn update_file_metadata(&self, id: i64, size: i64, mtime: i64) -> Result<()> {
        self.connection().execute(
            "UPDATE files SET size = ?1, mtime = ?2, hash = NULL WHERE id = ?3",
            params![size, mtime, id],
        )?;
        Ok(())
    }

    /// Delete a file by ID
    pub fn delete_file(&self, id: i64) -> Result<()> {
        self.connection()
            .execute("DELETE FROM files WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Find duplicate files (files with same hash)
    pub fn find_duplicates(&self) -> Result<Vec<Vec<File>>> {
        let mut stmt = self.connection().prepare(
            "SELECT hash FROM files WHERE hash IS NOT NULL
             GROUP BY hash HAVING COUNT(*) > 1",
        )?;

        let hashes: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut duplicates = Vec::new();
        for hash in hashes {
            let mut stmt = self.connection().prepare(
                "SELECT id, directory_id, filename, size, mtime, hash, rating, media_type
                 FROM files WHERE hash = ?1",
            )?;

            let files: Vec<File> = stmt
                .query_map([&hash], |row| {
                    Ok(File {
                        id: row.get(0)?,
                        directory_id: row.get(1)?,
                        filename: row.get(2)?,
                        size: row.get(3)?,
                        mtime: row.get(4)?,
                        hash: row.get(5)?,
                        rating: row.get(6)?,
                        media_type: row.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            duplicates.push(files);
        }

        Ok(duplicates)
    }

    // ==================== Tag Operations ====================

    /// Get or create a tag by name, returns its ID
    pub fn get_or_create_tag(&self, name: &str) -> Result<i64> {
        // Try to get existing tag
        let existing: Option<i64> = self
            .connection()
            .query_row("SELECT id FROM tags WHERE name = ?1", [name], |row| {
                row.get(0)
            })
            .optional()?;

        if let Some(id) = existing {
            return Ok(id);
        }

        // Create new tag
        self.connection()
            .execute("INSERT INTO tags (name) VALUES (?1)", [name])?;
        Ok(self.connection().last_insert_rowid())
    }

    /// Add a tag to a file
    pub fn add_file_tag(&self, file_id: i64, tag_name: &str) -> Result<()> {
        let tag_id = self.get_or_create_tag(tag_name)?;
        self.connection().execute(
            "INSERT OR IGNORE INTO file_tags (file_id, tag_id) VALUES (?1, ?2)",
            params![file_id, tag_id],
        )?;
        Ok(())
    }

    /// Remove a tag from a file
    pub fn remove_file_tag(&self, file_id: i64, tag_name: &str) -> Result<()> {
        self.connection().execute(
            "DELETE FROM file_tags WHERE file_id = ?1 AND tag_id = (SELECT id FROM tags WHERE name = ?2)",
            params![file_id, tag_name],
        )?;
        Ok(())
    }

    /// Get all tags for a file
    pub fn get_file_tags(&self, file_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.connection().prepare(
            "SELECT t.name FROM tags t
             JOIN file_tags ft ON t.id = ft.tag_id
             WHERE ft.file_id = ?1 ORDER BY t.name",
        )?;

        let tags: Vec<String> = stmt
            .query_map([file_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tags)
    }

    /// Add a tag to a directory
    pub fn add_directory_tag(&self, directory_id: i64, tag_name: &str) -> Result<()> {
        let tag_id = self.get_or_create_tag(tag_name)?;
        self.connection().execute(
            "INSERT OR IGNORE INTO directory_tags (directory_id, tag_id) VALUES (?1, ?2)",
            params![directory_id, tag_id],
        )?;
        Ok(())
    }

    /// Remove a tag from a directory
    pub fn remove_directory_tag(&self, directory_id: i64, tag_name: &str) -> Result<()> {
        self.connection().execute(
            "DELETE FROM directory_tags WHERE directory_id = ?1 AND tag_id = (SELECT id FROM tags WHERE name = ?2)",
            params![directory_id, tag_name],
        )?;
        Ok(())
    }

    /// Get all tags for a directory
    pub fn get_directory_tags(&self, directory_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.connection().prepare(
            "SELECT t.name FROM tags t
             JOIN directory_tags dt ON t.id = dt.tag_id
             WHERE dt.directory_id = ?1 ORDER BY t.name",
        )?;

        let tags: Vec<String> = stmt
            .query_map([directory_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tags)
    }

    /// Get all tags in the database
    pub fn get_all_tags(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .connection()
            .prepare("SELECT name FROM tags ORDER BY name")?;

        let tags: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tags)
    }

    // ==================== Query Operations ====================

    /// Get a file by its relative path (e.g., "photos/vacation/beach.jpg")
    pub fn get_file_by_path(&self, relative_path: &str) -> Result<Option<File>> {
        // Split into directory path and filename
        let path = std::path::Path::new(relative_path);
        let filename = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => return Ok(None),
        };
        let dir_path = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // Look up directory
        let dir = self.get_directory_by_path(&dir_path)?;
        let dir = match dir {
            Some(d) => d,
            None => return Ok(None),
        };

        // Look up file in that directory
        self.get_file_by_name(dir.id, &filename)
    }

    /// Get all files with their directory paths
    pub fn get_all_files_with_paths(&self) -> Result<Vec<(File, String)>> {
        let mut stmt = self.connection().prepare(
            "SELECT f.id, f.directory_id, f.filename, f.size, f.mtime, f.hash, f.rating, f.media_type, d.path
             FROM files f
             JOIN directories d ON f.directory_id = d.id
             ORDER BY d.path, f.filename",
        )?;

        let rows = stmt.query_map([], |row| {
            let file = File {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                filename: row.get(2)?,
                size: row.get(3)?,
                mtime: row.get(4)?,
                hash: row.get(5)?,
                rating: row.get(6)?,
                media_type: row.get(7)?,
            };
            let dir_path: String = row.get(8)?;
            Ok((file, dir_path))
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get files filtered by minimum rating
    pub fn get_files_by_rating(&self, min_rating: i32) -> Result<Vec<(File, String)>> {
        let mut stmt = self.connection().prepare(
            "SELECT f.id, f.directory_id, f.filename, f.size, f.mtime, f.hash, f.rating, f.media_type, d.path
             FROM files f
             JOIN directories d ON f.directory_id = d.id
             WHERE f.rating >= ?1
             ORDER BY f.rating DESC, d.path, f.filename",
        )?;

        let rows = stmt.query_map([min_rating], |row| {
            let file = File {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                filename: row.get(2)?,
                size: row.get(3)?,
                mtime: row.get(4)?,
                hash: row.get(5)?,
                rating: row.get(6)?,
                media_type: row.get(7)?,
            };
            let dir_path: String = row.get(8)?;
            Ok((file, dir_path))
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get files that have a specific tag
    pub fn get_files_by_tag(&self, tag: &str) -> Result<Vec<(File, String)>> {
        let mut stmt = self.connection().prepare(
            "SELECT f.id, f.directory_id, f.filename, f.size, f.mtime, f.hash, f.rating, f.media_type, d.path
             FROM files f
             JOIN directories d ON f.directory_id = d.id
             JOIN file_tags ft ON f.id = ft.file_id
             JOIN tags t ON ft.tag_id = t.id
             WHERE t.name = ?1
             ORDER BY d.path, f.filename",
        )?;

        let rows = stmt.query_map([tag], |row| {
            let file = File {
                id: row.get(0)?,
                directory_id: row.get(1)?,
                filename: row.get(2)?,
                size: row.get(3)?,
                mtime: row.get(4)?,
                hash: row.get(5)?,
                rating: row.get(6)?,
                media_type: row.get(7)?,
            };
            let dir_path: String = row.get(8)?;
            Ok((file, dir_path))
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // ==================== Filter Operations ====================

    /// Get IDs of directories containing files that match the filter criteria,
    /// OR directories that themselves have matching tags.
    /// Also includes ancestor directories to maintain tree structure.
    /// For multiple tags, uses AND logic (must have ALL tags).
    pub fn get_directories_with_matching_files(
        &self,
        rating_filter: crate::tui::state::RatingFilter,
        tags: &[String],
        video_only: bool,
    ) -> Result<std::collections::HashSet<i64>> {
        use std::collections::HashSet;
        use crate::tui::state::RatingFilter;

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
        // A directory matches if it has matching tags AND rating (when both filters active)
        let all_dirs = self.get_all_directories()?;
        let mut dirs_matching_full_filter: Vec<i64> = Vec::new();

        if !video_only {
            for dir in &all_dirs {
                // Check rating filter on directory
                let dir_matches_rating = match rating_filter {
                    RatingFilter::Any => true,
                    RatingFilter::Unrated => dir.rating.is_none(),
                    RatingFilter::MinRating(min) => dir.rating.map(|r| r >= min).unwrap_or(false),
                };

                // Check tag filter on directory
                let dir_matches_tags = if tags.is_empty() {
                    true
                } else {
                    let dir_tags = self.get_directory_tags(dir.id)?;
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
            // Find all directories that have this directory as an ancestor
            for dir in &all_dirs {
                let mut current_id = dir.parent_id;
                while let Some(pid) = current_id {
                    if pid == matching_dir_id {
                        matching_dir_ids.insert(dir.id);
                        break;
                    }
                    current_id = all_dirs.iter()
                        .find(|d| d.id == pid)
                        .and_then(|d| d.parent_id);
                }
            }
        }

        // === Part 4: Include ancestor directories to maintain tree structure ===
        let mut ancestors_to_add: HashSet<i64> = HashSet::new();

        for &dir_id in &matching_dir_ids {
            let mut current_id = dir_id;
            while let Some(dir) = all_dirs.iter().find(|d| d.id == current_id) {
                if let Some(parent_id) = dir.parent_id {
                    if !matching_dir_ids.contains(&parent_id) {
                        ancestors_to_add.insert(parent_id);
                    }
                    current_id = parent_id;
                } else {
                    break;
                }
            }
        }

        matching_dir_ids.extend(ancestors_to_add);

        Ok(matching_dir_ids)
    }

    // ==================== Orientation Operations ====================

    /// Get image files that don't have landscape/portrait tags yet
    pub fn get_files_needing_orientation(&self) -> Result<Vec<FileToHash>> {
        let mut stmt = self.connection().prepare(
            "SELECT f.id, d.path, f.filename
             FROM files f
             JOIN directories d ON f.directory_id = d.id
             WHERE f.media_type = 'image'
               AND NOT EXISTS (
                   SELECT 1 FROM file_tags ft
                   JOIN tags t ON ft.tag_id = t.id
                   WHERE ft.file_id = f.id AND t.name IN ('landscape', 'portrait')
               )
             ORDER BY d.path, f.filename",
        )?;

        let files: Vec<FileToHash> = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let dir_path: String = row.get(1)?;
                let filename: String = row.get(2)?;

                let path = if dir_path.is_empty() {
                    PathBuf::from(&filename)
                } else {
                    PathBuf::from(&dir_path).join(&filename)
                };

                Ok(FileToHash { id, path })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(files)
    }

    // ==================== Hash Operations ====================

    /// Get all files that need hashing (where hash IS NULL)
    pub fn get_files_needing_hash(&self) -> Result<Vec<FileToHash>> {
        let mut stmt = self.connection().prepare(
            "SELECT f.id, d.path, f.filename
             FROM files f
             JOIN directories d ON f.directory_id = d.id
             WHERE f.hash IS NULL
             ORDER BY d.path, f.filename",
        )?;

        let files: Vec<FileToHash> = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let dir_path: String = row.get(1)?;
                let filename: String = row.get(2)?;

                let path = if dir_path.is_empty() {
                    PathBuf::from(&filename)
                } else {
                    PathBuf::from(&dir_path).join(&filename)
                };

                Ok(FileToHash { id, path })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directory_crud() {
        let db = Database::open_in_memory().unwrap();

        // Insert root directory
        let root_id = db.insert_directory("photos", None, Some(12345)).unwrap();
        assert!(root_id > 0);

        // Insert child directory
        let child_id = db
            .insert_directory("photos/vacation", Some(root_id), Some(12346))
            .unwrap();
        assert!(child_id > 0);

        // Get by path
        let root = db.get_directory_by_path("photos").unwrap().unwrap();
        assert_eq!(root.path, "photos");
        assert_eq!(root.parent_id, None);
        assert_eq!(root.mtime, Some(12345));

        // Get children
        let children = db.get_child_directories(Some(root_id)).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].path, "photos/vacation");

        // Get root directories (no parent)
        let roots = db.get_child_directories(None).unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, "photos");

        // Update rating
        db.set_directory_rating(root_id, Some(4)).unwrap();
        let updated = db.get_directory(root_id).unwrap().unwrap();
        assert_eq!(updated.rating, Some(4));

        // Delete
        db.delete_directory(child_id).unwrap();
        let children = db.get_child_directories(Some(root_id)).unwrap();
        assert!(children.is_empty());
    }

    #[test]
    fn test_file_crud() {
        let db = Database::open_in_memory().unwrap();

        // Create directory first
        let dir_id = db.insert_directory("photos", None, None).unwrap();

        // Insert file
        let file_id = db
            .insert_file(dir_id, "photo.jpg", 1024, 12345, Some("image"))
            .unwrap();
        assert!(file_id > 0);

        // Get by name
        let file = db.get_file_by_name(dir_id, "photo.jpg").unwrap().unwrap();
        assert_eq!(file.filename, "photo.jpg");
        assert_eq!(file.size, 1024);
        assert_eq!(file.media_type, Some("image".to_string()));

        // Get files in directory
        let files = db.get_files_in_directory(dir_id).unwrap();
        assert_eq!(files.len(), 1);

        // Update hash
        db.set_file_hash(file_id, "abc123").unwrap();
        let updated = db.get_file_by_name(dir_id, "photo.jpg").unwrap().unwrap();
        assert_eq!(updated.hash, Some("abc123".to_string()));

        // Update rating
        db.set_file_rating(file_id, Some(5)).unwrap();
        let updated = db.get_file_by_name(dir_id, "photo.jpg").unwrap().unwrap();
        assert_eq!(updated.rating, Some(5));

        // Delete
        db.delete_file(file_id).unwrap();
        let files = db.get_files_in_directory(dir_id).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_find_duplicates() {
        let db = Database::open_in_memory().unwrap();

        let dir_id = db.insert_directory("photos", None, None).unwrap();

        // Insert files with same hash (duplicates)
        let file1_id = db
            .insert_file(dir_id, "photo1.jpg", 1024, 12345, Some("image"))
            .unwrap();
        let file2_id = db
            .insert_file(dir_id, "photo2.jpg", 1024, 12346, Some("image"))
            .unwrap();

        db.set_file_hash(file1_id, "samehash").unwrap();
        db.set_file_hash(file2_id, "samehash").unwrap();

        // Insert unique file
        let file3_id = db
            .insert_file(dir_id, "photo3.jpg", 2048, 12347, Some("image"))
            .unwrap();
        db.set_file_hash(file3_id, "uniquehash").unwrap();

        // Find duplicates
        let dupes = db.find_duplicates().unwrap();
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].len(), 2);
    }

    #[test]
    fn test_file_tags() {
        let db = Database::open_in_memory().unwrap();

        let dir_id = db.insert_directory("photos", None, None).unwrap();
        let file_id = db
            .insert_file(dir_id, "photo.jpg", 1024, 12345, Some("image"))
            .unwrap();

        // Add tags
        db.add_file_tag(file_id, "portrait").unwrap();
        db.add_file_tag(file_id, "outdoor").unwrap();

        // Get tags
        let tags = db.get_file_tags(file_id).unwrap();
        assert_eq!(tags, vec!["outdoor", "portrait"]); // sorted

        // Add same tag again (should be idempotent)
        db.add_file_tag(file_id, "portrait").unwrap();
        let tags = db.get_file_tags(file_id).unwrap();
        assert_eq!(tags.len(), 2);

        // Remove tag
        db.remove_file_tag(file_id, "portrait").unwrap();
        let tags = db.get_file_tags(file_id).unwrap();
        assert_eq!(tags, vec!["outdoor"]);
    }

    #[test]
    fn test_directory_tags() {
        let db = Database::open_in_memory().unwrap();

        let dir_id = db.insert_directory("photos/vacation", None, None).unwrap();

        // Add tags
        db.add_directory_tag(dir_id, "travel").unwrap();
        db.add_directory_tag(dir_id, "2024").unwrap();

        // Get tags
        let tags = db.get_directory_tags(dir_id).unwrap();
        assert_eq!(tags, vec!["2024", "travel"]); // sorted

        // Remove tag
        db.remove_directory_tag(dir_id, "travel").unwrap();
        let tags = db.get_directory_tags(dir_id).unwrap();
        assert_eq!(tags, vec!["2024"]);
    }

    #[test]
    fn test_get_files_needing_hash() {
        let db = Database::open_in_memory().unwrap();

        // Create directory structure
        let dir1_id = db.insert_directory("photos", None, None).unwrap();
        let dir2_id = db.insert_directory("photos/vacation", Some(dir1_id), None).unwrap();

        // Insert files - some with hash, some without
        let file1_id = db.insert_file(dir1_id, "image1.jpg", 1024, 12345, Some("image")).unwrap();
        let file2_id = db.insert_file(dir1_id, "image2.jpg", 2048, 12346, Some("image")).unwrap();
        let file3_id = db.insert_file(dir2_id, "vacation.jpg", 3072, 12347, Some("image")).unwrap();

        // Set hash on file1 only
        db.set_file_hash(file1_id, "abc123").unwrap();

        // Get files needing hash
        let files = db.get_files_needing_hash().unwrap();

        // Should return file2 and file3 (both have NULL hash)
        assert_eq!(files.len(), 2);

        // Verify we get correct file IDs
        let ids: Vec<i64> = files.iter().map(|f| f.id).collect();
        assert!(ids.contains(&file2_id));
        assert!(ids.contains(&file3_id));

        // Verify paths are constructed correctly
        let paths: Vec<String> = files.iter().map(|f| f.path.to_string_lossy().to_string()).collect();
        assert!(paths.iter().any(|p| p.ends_with("photos/image2.jpg")));
        assert!(paths.iter().any(|p| p.ends_with("photos/vacation/vacation.jpg")));
    }

    #[test]
    fn test_get_file_by_path() {
        let db = Database::open_in_memory().unwrap();

        // Create directory structure
        let dir1_id = db.insert_directory("photos", None, None).unwrap();
        let dir2_id = db
            .insert_directory("photos/vacation", Some(dir1_id), None)
            .unwrap();

        // Insert files
        db.insert_file(dir1_id, "image1.jpg", 1024, 12345, Some("image"))
            .unwrap();
        db.insert_file(dir2_id, "beach.jpg", 2048, 12346, Some("image"))
            .unwrap();

        // Find file in root dir
        let file = db.get_file_by_path("photos/image1.jpg").unwrap();
        assert!(file.is_some());
        assert_eq!(file.unwrap().filename, "image1.jpg");

        // Find file in nested dir
        let file = db.get_file_by_path("photos/vacation/beach.jpg").unwrap();
        assert!(file.is_some());
        assert_eq!(file.unwrap().filename, "beach.jpg");

        // Non-existent file returns None
        let file = db.get_file_by_path("photos/nonexistent.jpg").unwrap();
        assert!(file.is_none());

        // Non-existent directory returns None
        let file = db.get_file_by_path("nonexistent/image.jpg").unwrap();
        assert!(file.is_none());
    }

    #[test]
    fn test_get_files_by_rating() {
        let db = Database::open_in_memory().unwrap();

        let dir_id = db.insert_directory("photos", None, None).unwrap();

        // Insert files with various ratings
        let file1_id = db
            .insert_file(dir_id, "great.jpg", 1024, 12345, Some("image"))
            .unwrap();
        let file2_id = db
            .insert_file(dir_id, "good.jpg", 1024, 12346, Some("image"))
            .unwrap();
        let file3_id = db
            .insert_file(dir_id, "ok.jpg", 1024, 12347, Some("image"))
            .unwrap();
        db.insert_file(dir_id, "unrated.jpg", 1024, 12348, Some("image"))
            .unwrap();

        db.set_file_rating(file1_id, Some(5)).unwrap();
        db.set_file_rating(file2_id, Some(4)).unwrap();
        db.set_file_rating(file3_id, Some(3)).unwrap();

        // Get 4+ rated files
        let files = db.get_files_by_rating(4).unwrap();
        assert_eq!(files.len(), 2);
        let names: Vec<&str> = files.iter().map(|(f, _)| f.filename.as_str()).collect();
        assert!(names.contains(&"great.jpg"));
        assert!(names.contains(&"good.jpg"));

        // Get 5+ rated files
        let files = db.get_files_by_rating(5).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0.filename, "great.jpg");

        // Get 3+ rated files (excludes unrated)
        let files = db.get_files_by_rating(3).unwrap();
        assert_eq!(files.len(), 3);

        // Directory path is included
        assert_eq!(files[0].1, "photos");
    }

    #[test]
    fn test_get_files_by_tag() {
        let db = Database::open_in_memory().unwrap();

        let dir_id = db.insert_directory("photos", None, None).unwrap();

        let file1_id = db
            .insert_file(dir_id, "portrait1.jpg", 1024, 12345, Some("image"))
            .unwrap();
        let file2_id = db
            .insert_file(dir_id, "portrait2.jpg", 1024, 12346, Some("image"))
            .unwrap();
        let file3_id = db
            .insert_file(dir_id, "landscape.jpg", 1024, 12347, Some("image"))
            .unwrap();

        db.add_file_tag(file1_id, "portrait").unwrap();
        db.add_file_tag(file2_id, "portrait").unwrap();
        db.add_file_tag(file2_id, "outdoor").unwrap();
        db.add_file_tag(file3_id, "landscape").unwrap();

        // Find by portrait tag
        let files = db.get_files_by_tag("portrait").unwrap();
        assert_eq!(files.len(), 2);
        let names: Vec<&str> = files.iter().map(|(f, _)| f.filename.as_str()).collect();
        assert!(names.contains(&"portrait1.jpg"));
        assert!(names.contains(&"portrait2.jpg"));

        // Find by outdoor tag
        let files = db.get_files_by_tag("outdoor").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0.filename, "portrait2.jpg");

        // Non-existent tag returns empty
        let files = db.get_files_by_tag("nonexistent").unwrap();
        assert!(files.is_empty());

        // Directory path is included
        let files = db.get_files_by_tag("portrait").unwrap();
        assert_eq!(files[0].1, "photos");
    }

    #[test]
    fn test_get_all_files_with_paths() {
        let db = Database::open_in_memory().unwrap();

        let dir1_id = db.insert_directory("photos", None, None).unwrap();
        let dir2_id = db
            .insert_directory("photos/vacation", Some(dir1_id), None)
            .unwrap();

        db.insert_file(dir1_id, "image1.jpg", 1024, 12345, Some("image"))
            .unwrap();
        db.insert_file(dir2_id, "beach.jpg", 2048, 12346, Some("image"))
            .unwrap();

        let files = db.get_all_files_with_paths().unwrap();
        assert_eq!(files.len(), 2);

        // Check paths are included correctly
        let file_paths: Vec<(&str, &str)> = files
            .iter()
            .map(|(f, dir)| (f.filename.as_str(), dir.as_str()))
            .collect();
        assert!(file_paths.contains(&("image1.jpg", "photos")));
        assert!(file_paths.contains(&("beach.jpg", "photos/vacation")));
    }

    #[test]
    fn test_get_directories_with_matching_files() {
        let db = Database::open_in_memory().unwrap();

        // Create directory structure: root -> photos -> vacation
        let root_id = db.insert_directory("", None, None).unwrap();
        let photos_id = db.insert_directory("photos", Some(root_id), None).unwrap();
        let vacation_id = db.insert_directory("photos/vacation", Some(photos_id), None).unwrap();
        let work_id = db.insert_directory("work", Some(root_id), None).unwrap();

        // Add files with different ratings and tags
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
        let result = db.get_directories_with_matching_files(crate::tui::state::RatingFilter::Any, &[], false).unwrap();
        assert!(result.is_empty());

        // Rating filter only
        let result = db.get_directories_with_matching_files(crate::tui::RatingFilter::MinRating(4), &[], false).unwrap();
        assert!(result.contains(&vacation_id)); // file2 has rating 5
        assert!(result.contains(&photos_id));   // ancestor of vacation
        assert!(result.contains(&root_id));     // ancestor of photos
        assert!(!result.contains(&work_id));    // file3 has rating 2

        // Tag filter only (single tag)
        let result = db.get_directories_with_matching_files(crate::tui::RatingFilter::Any, &["family".to_string()], false).unwrap();
        assert!(result.contains(&photos_id));   // file1 has "family" tag
        assert!(result.contains(&vacation_id)); // file2 has "family" tag
        assert!(result.contains(&root_id));     // ancestor
        assert!(!result.contains(&work_id));    // no matching files

        // Tag filter only (multiple tags - AND logic)
        let result = db.get_directories_with_matching_files(
            crate::tui::RatingFilter::Any,
            &["family".to_string(), "vacation".to_string()],
            false,
        ).unwrap();
        assert!(result.contains(&vacation_id)); // file2 has both tags
        assert!(result.contains(&photos_id));   // ancestor
        assert!(result.contains(&root_id));     // ancestor
        assert!(!result.contains(&photos_id) || result.len() >= 3); // photos_id is included as ancestor

        // Combined rating and tag filter
        let result = db.get_directories_with_matching_files(
            crate::tui::RatingFilter::MinRating(4),
            &["family".to_string()],
            false,
        ).unwrap();
        assert!(result.contains(&vacation_id)); // file2 has rating 5 and "family" tag
        assert!(result.contains(&photos_id));   // ancestor
        assert!(result.contains(&root_id));     // ancestor
        // photos_id is only included as ancestor (file1 has rating 3 < 4)
    }

    #[test]
    fn test_repair_directory_parents() {
        let db = Database::open_in_memory().unwrap();

        // Insert directories with correct paths but wrong parent_ids
        let hongdan_id = db.insert_directory("Hongdan", None, None).unwrap();
        // Intentionally insert with wrong parent_id (None instead of hongdan_id)
        let vol1_id = db.insert_directory("Hongdan/Vol1", None, None).unwrap();
        let vol2_id = db.insert_directory("Hongdan/Vol2", None, None).unwrap();
        // This one has correct parent_id
        let vol3_id = db.insert_directory("Hongdan/Vol3", Some(hongdan_id), None).unwrap();

        // Verify initial state - Vol1 and Vol2 have wrong parent_id
        let vol1 = db.get_directory(vol1_id).unwrap().unwrap();
        assert_eq!(vol1.parent_id, None); // Wrong!

        let vol2 = db.get_directory(vol2_id).unwrap().unwrap();
        assert_eq!(vol2.parent_id, None); // Wrong!

        // Run repair
        let fixed = db.repair_directory_parents().unwrap();
        assert_eq!(fixed, 2); // Vol1 and Vol2 should be fixed

        // Verify parent_ids are now correct
        let hongdan = db.get_directory(hongdan_id).unwrap().unwrap();
        assert_eq!(hongdan.parent_id, None); // Correct - root level

        let vol1 = db.get_directory(vol1_id).unwrap().unwrap();
        assert_eq!(vol1.parent_id, Some(hongdan_id)); // Fixed!

        let vol2 = db.get_directory(vol2_id).unwrap().unwrap();
        assert_eq!(vol2.parent_id, Some(hongdan_id)); // Fixed!

        let vol3 = db.get_directory(vol3_id).unwrap().unwrap();
        assert_eq!(vol3.parent_id, Some(hongdan_id)); // Was already correct

        // Run repair again - should fix 0 since all are correct now
        let fixed = db.repair_directory_parents().unwrap();
        assert_eq!(fixed, 0);
    }
}
