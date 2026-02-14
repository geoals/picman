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
        db.set_directory_rating(root_id, Some(8)).unwrap();
        let updated = db.get_directory(root_id).unwrap().unwrap();
        assert_eq!(updated.rating, Some(8));

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
        db.set_file_rating(file_id, Some(9)).unwrap();
        let updated = db.get_file_by_name(dir_id, "photo.jpg").unwrap().unwrap();
        assert_eq!(updated.rating, Some(9));

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
}
