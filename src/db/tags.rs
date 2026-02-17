use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use tracing::{debug, instrument};

use super::Database;

impl Database {
    /// Get or create a tag by name, returns its ID
    pub fn get_or_create_tag(&self, name: &str) -> Result<i64> {
        let existing: Option<i64> = self
            .connection()
            .query_row("SELECT id FROM tags WHERE name = ?1", [name], |row| {
                row.get(0)
            })
            .optional()?;

        if let Some(id) = existing {
            return Ok(id);
        }

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

    /// Get all directory tags as a map (directory_id -> list of tag names)
    #[instrument(skip(self))]
    pub fn get_all_directory_tags(&self) -> Result<HashMap<i64, Vec<String>>> {
        let mut stmt = self.connection().prepare(
            "SELECT dt.directory_id, t.name FROM directory_tags dt
             JOIN tags t ON dt.tag_id = t.id
             ORDER BY dt.directory_id, t.name",
        )?;

        let mut result: HashMap<i64, Vec<String>> = HashMap::new();

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (dir_id, tag_name) = row?;
            result.entry(dir_id).or_default().push(tag_name);
        }

        debug!(count = result.len(), "loaded all directory tags");
        Ok(result)
    }

    /// Get all file tags as a map (file_id -> list of tag names)
    #[instrument(skip(self))]
    pub fn get_all_file_tags(&self) -> Result<HashMap<i64, Vec<String>>> {
        let mut stmt = self.connection().prepare(
            "SELECT ft.file_id, t.name FROM file_tags ft
             JOIN tags t ON ft.tag_id = t.id
             ORDER BY ft.file_id, t.name",
        )?;

        let mut result: HashMap<i64, Vec<String>> = HashMap::new();

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (file_id, tag_name) = row?;
            result.entry(file_id).or_default().push(tag_name);
        }

        debug!(count = result.len(), "loaded all file tags");
        Ok(result)
    }

    /// Get file tags for all files in a directory as a map (file_id -> list of tag names)
    #[instrument(skip(self))]
    pub fn get_file_tags_for_directory(
        &self,
        directory_id: i64,
    ) -> Result<HashMap<i64, Vec<String>>> {
        let mut stmt = self.connection().prepare(
            "SELECT ft.file_id, t.name FROM file_tags ft
             JOIN tags t ON ft.tag_id = t.id
             JOIN files f ON ft.file_id = f.id
             WHERE f.directory_id = ?1
             ORDER BY ft.file_id, t.name",
        )?;

        let mut result: HashMap<i64, Vec<String>> = HashMap::new();

        let rows = stmt.query_map([directory_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (file_id, tag_name) = row?;
            result.entry(file_id).or_default().push(tag_name);
        }

        debug!(
            directory_id,
            count = result.len(),
            "loaded file tags for directory"
        );
        Ok(result)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_tags() {
        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos", None, None).unwrap();
        let file_id = db.insert_file(dir_id, "photo.jpg", 1024, 12345, Some("image")).unwrap();

        db.add_file_tag(file_id, "portrait").unwrap();
        db.add_file_tag(file_id, "outdoor").unwrap();

        let tags = db.get_file_tags(file_id).unwrap();
        assert_eq!(tags, vec!["outdoor", "portrait"]);

        db.add_file_tag(file_id, "portrait").unwrap();
        let tags = db.get_file_tags(file_id).unwrap();
        assert_eq!(tags.len(), 2);

        db.remove_file_tag(file_id, "portrait").unwrap();
        let tags = db.get_file_tags(file_id).unwrap();
        assert_eq!(tags, vec!["outdoor"]);
    }

    #[test]
    fn test_directory_tags() {
        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos/vacation", None, None).unwrap();

        db.add_directory_tag(dir_id, "travel").unwrap();
        db.add_directory_tag(dir_id, "2024").unwrap();

        let tags = db.get_directory_tags(dir_id).unwrap();
        assert_eq!(tags, vec!["2024", "travel"]);

        db.remove_directory_tag(dir_id, "travel").unwrap();
        let tags = db.get_directory_tags(dir_id).unwrap();
        assert_eq!(tags, vec!["2024"]);
    }

    #[test]
    fn test_get_all_file_tags() {
        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos", None, None).unwrap();

        let file1_id = db.insert_file(dir_id, "photo1.jpg", 1024, 12345, Some("image")).unwrap();
        let file2_id = db.insert_file(dir_id, "photo2.jpg", 1024, 12346, Some("image")).unwrap();
        let file3_id = db.insert_file(dir_id, "photo3.jpg", 1024, 12347, Some("image")).unwrap();

        db.add_file_tag(file1_id, "landscape").unwrap();
        db.add_file_tag(file1_id, "vacation").unwrap();
        db.add_file_tag(file2_id, "portrait").unwrap();

        let all_tags = db.get_all_file_tags().unwrap();
        assert_eq!(all_tags.get(&file1_id).unwrap().len(), 2);
        assert_eq!(all_tags.get(&file2_id).unwrap().len(), 1);
        assert!(all_tags.get(&file3_id).is_none());
    }

    #[test]
    fn test_get_file_tags_for_directory() {
        let db = Database::open_in_memory().unwrap();
        let dir1_id = db.insert_directory("photos", None, None).unwrap();
        let dir2_id = db.insert_directory("videos", None, None).unwrap();

        let file1_id = db.insert_file(dir1_id, "photo1.jpg", 1024, 12345, Some("image")).unwrap();
        let file2_id = db.insert_file(dir1_id, "photo2.jpg", 1024, 12346, Some("image")).unwrap();
        let file3_id = db.insert_file(dir2_id, "video1.mp4", 1024, 12347, Some("video")).unwrap();

        db.add_file_tag(file1_id, "landscape").unwrap();
        db.add_file_tag(file1_id, "vacation").unwrap();
        db.add_file_tag(file2_id, "portrait").unwrap();
        db.add_file_tag(file3_id, "action").unwrap();

        let dir1_tags = db.get_file_tags_for_directory(dir1_id).unwrap();
        assert_eq!(dir1_tags.len(), 2);
        assert!(dir1_tags.contains_key(&file1_id));
        assert!(dir1_tags.contains_key(&file2_id));
        assert!(!dir1_tags.contains_key(&file3_id));
    }
}
