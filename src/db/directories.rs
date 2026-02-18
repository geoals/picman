use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::params;

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

impl Directory {
    /// Build the absolute path for a file inside this directory
    pub fn file_path(&self, library_path: &Path, filename: &str) -> PathBuf {
        if self.path.is_empty() {
            library_path.join(filename)
        } else {
            library_path.join(&self.path).join(filename)
        }
    }

    /// Build the absolute path for this directory itself
    pub fn full_path(&self, library_path: &Path) -> PathBuf {
        if self.path.is_empty() {
            library_path.to_path_buf()
        } else {
            library_path.join(&self.path)
        }
    }
}

impl Database {
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
        use rusqlite::OptionalExtension;
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
        use rusqlite::OptionalExtension;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directory_crud() {
        let db = Database::open_in_memory().unwrap();

        let root_id = db.insert_directory("photos", None, Some(12345)).unwrap();
        assert!(root_id > 0);

        let child_id = db
            .insert_directory("photos/vacation", Some(root_id), Some(12346))
            .unwrap();
        assert!(child_id > 0);

        let root = db.get_directory_by_path("photos").unwrap().unwrap();
        assert_eq!(root.path, "photos");
        assert_eq!(root.parent_id, None);
        assert_eq!(root.mtime, Some(12345));

        let children = db.get_child_directories(Some(root_id)).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].path, "photos/vacation");

        let roots = db.get_child_directories(None).unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, "photos");

        db.set_directory_rating(root_id, Some(4)).unwrap();
        let updated = db.get_directory(root_id).unwrap().unwrap();
        assert_eq!(updated.rating, Some(4));

        db.delete_directory(child_id).unwrap();
        let children = db.get_child_directories(Some(root_id)).unwrap();
        assert!(children.is_empty());
    }

    #[test]
    fn test_file_path_with_nonempty_dir() {
        let dir = Directory {
            id: 1,
            path: "photos/vacation".to_string(),
            parent_id: None,
            rating: None,
            mtime: None,
        };
        let lib = std::path::Path::new("/library");
        let result = dir.file_path(lib, "img.jpg");
        assert_eq!(result, std::path::PathBuf::from("/library/photos/vacation/img.jpg"));
    }

    #[test]
    fn test_file_path_with_empty_dir() {
        let dir = Directory {
            id: 1,
            path: String::new(),
            parent_id: None,
            rating: None,
            mtime: None,
        };
        let lib = std::path::Path::new("/library");
        let result = dir.file_path(lib, "img.jpg");
        assert_eq!(result, std::path::PathBuf::from("/library/img.jpg"));
    }

    #[test]
    fn test_full_path_with_nonempty_dir() {
        let dir = Directory {
            id: 1,
            path: "photos".to_string(),
            parent_id: None,
            rating: None,
            mtime: None,
        };
        let lib = std::path::Path::new("/library");
        let result = dir.full_path(lib);
        assert_eq!(result, std::path::PathBuf::from("/library/photos"));
    }

    #[test]
    fn test_full_path_with_empty_dir() {
        let dir = Directory {
            id: 1,
            path: String::new(),
            parent_id: None,
            rating: None,
            mtime: None,
        };
        let lib = std::path::Path::new("/library");
        let result = dir.full_path(lib);
        assert_eq!(result, std::path::PathBuf::from("/library"));
    }

    #[test]
    fn test_repair_directory_parents() {
        let db = Database::open_in_memory().unwrap();

        let hongdan_id = db.insert_directory("Hongdan", None, None).unwrap();
        let vol1_id = db.insert_directory("Hongdan/Vol1", None, None).unwrap();
        let vol2_id = db.insert_directory("Hongdan/Vol2", None, None).unwrap();
        let _vol3_id = db.insert_directory("Hongdan/Vol3", Some(hongdan_id), None).unwrap();

        let vol1 = db.get_directory(vol1_id).unwrap().unwrap();
        assert_eq!(vol1.parent_id, None);

        let fixed = db.repair_directory_parents().unwrap();
        assert_eq!(fixed, 2);

        let vol1 = db.get_directory(vol1_id).unwrap().unwrap();
        assert_eq!(vol1.parent_id, Some(hongdan_id));

        let vol2 = db.get_directory(vol2_id).unwrap().unwrap();
        assert_eq!(vol2.parent_id, Some(hongdan_id));

        let fixed = db.repair_directory_parents().unwrap();
        assert_eq!(fixed, 0);
    }
}
