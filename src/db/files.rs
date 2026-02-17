use std::path::PathBuf;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::Database;

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

    /// Get a file by its relative path (e.g., "photos/vacation/beach.jpg")
    pub fn get_file_by_path(&self, relative_path: &str) -> Result<Option<File>> {
        let path = std::path::Path::new(relative_path);
        let filename = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => return Ok(None),
        };
        let dir_path = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let dir = self.get_directory_by_path(&dir_path)?;
        let dir = match dir {
            Some(d) => d,
            None => return Ok(None),
        };

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
    fn test_file_crud() {
        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos", None, None).unwrap();

        let file_id = db
            .insert_file(dir_id, "photo.jpg", 1024, 12345, Some("image"))
            .unwrap();
        assert!(file_id > 0);

        let file = db.get_file_by_name(dir_id, "photo.jpg").unwrap().unwrap();
        assert_eq!(file.filename, "photo.jpg");
        assert_eq!(file.size, 1024);
        assert_eq!(file.media_type, Some("image".to_string()));

        let files = db.get_files_in_directory(dir_id).unwrap();
        assert_eq!(files.len(), 1);

        db.set_file_hash(file_id, "abc123").unwrap();
        let updated = db.get_file_by_name(dir_id, "photo.jpg").unwrap().unwrap();
        assert_eq!(updated.hash, Some("abc123".to_string()));

        db.set_file_rating(file_id, Some(5)).unwrap();
        let updated = db.get_file_by_name(dir_id, "photo.jpg").unwrap().unwrap();
        assert_eq!(updated.rating, Some(5));

        db.delete_file(file_id).unwrap();
        let files = db.get_files_in_directory(dir_id).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_find_duplicates() {
        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos", None, None).unwrap();

        let file1_id = db.insert_file(dir_id, "photo1.jpg", 1024, 12345, Some("image")).unwrap();
        let file2_id = db.insert_file(dir_id, "photo2.jpg", 1024, 12346, Some("image")).unwrap();
        db.set_file_hash(file1_id, "samehash").unwrap();
        db.set_file_hash(file2_id, "samehash").unwrap();

        let file3_id = db.insert_file(dir_id, "photo3.jpg", 2048, 12347, Some("image")).unwrap();
        db.set_file_hash(file3_id, "uniquehash").unwrap();

        let dupes = db.find_duplicates().unwrap();
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].len(), 2);
    }

    #[test]
    fn test_get_file_by_path() {
        let db = Database::open_in_memory().unwrap();
        let dir1_id = db.insert_directory("photos", None, None).unwrap();
        let dir2_id = db.insert_directory("photos/vacation", Some(dir1_id), None).unwrap();

        db.insert_file(dir1_id, "image1.jpg", 1024, 12345, Some("image")).unwrap();
        db.insert_file(dir2_id, "beach.jpg", 2048, 12346, Some("image")).unwrap();

        let file = db.get_file_by_path("photos/image1.jpg").unwrap();
        assert!(file.is_some());
        assert_eq!(file.unwrap().filename, "image1.jpg");

        let file = db.get_file_by_path("photos/vacation/beach.jpg").unwrap();
        assert!(file.is_some());

        assert!(db.get_file_by_path("photos/nonexistent.jpg").unwrap().is_none());
        assert!(db.get_file_by_path("nonexistent/image.jpg").unwrap().is_none());
    }

    #[test]
    fn test_get_files_needing_hash() {
        let db = Database::open_in_memory().unwrap();
        let dir1_id = db.insert_directory("photos", None, None).unwrap();
        let dir2_id = db.insert_directory("photos/vacation", Some(dir1_id), None).unwrap();

        let file1_id = db.insert_file(dir1_id, "image1.jpg", 1024, 12345, Some("image")).unwrap();
        let file2_id = db.insert_file(dir1_id, "image2.jpg", 2048, 12346, Some("image")).unwrap();
        let file3_id = db.insert_file(dir2_id, "vacation.jpg", 3072, 12347, Some("image")).unwrap();

        db.set_file_hash(file1_id, "abc123").unwrap();

        let files = db.get_files_needing_hash().unwrap();
        assert_eq!(files.len(), 2);

        let ids: Vec<i64> = files.iter().map(|f| f.id).collect();
        assert!(ids.contains(&file2_id));
        assert!(ids.contains(&file3_id));
    }

    #[test]
    fn test_get_all_files_with_paths() {
        let db = Database::open_in_memory().unwrap();
        let dir1_id = db.insert_directory("photos", None, None).unwrap();
        let dir2_id = db.insert_directory("photos/vacation", Some(dir1_id), None).unwrap();

        db.insert_file(dir1_id, "image1.jpg", 1024, 12345, Some("image")).unwrap();
        db.insert_file(dir2_id, "beach.jpg", 2048, 12346, Some("image")).unwrap();

        let files = db.get_all_files_with_paths().unwrap();
        assert_eq!(files.len(), 2);

        let file_paths: Vec<(&str, &str)> = files
            .iter()
            .map(|(f, dir)| (f.filename.as_str(), dir.as_str()))
            .collect();
        assert!(file_paths.contains(&("image1.jpg", "photos")));
        assert!(file_paths.contains(&("beach.jpg", "photos/vacation")));
    }

    #[test]
    fn test_get_files_by_rating() {
        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos", None, None).unwrap();

        let file1_id = db.insert_file(dir_id, "great.jpg", 1024, 12345, Some("image")).unwrap();
        let file2_id = db.insert_file(dir_id, "good.jpg", 1024, 12346, Some("image")).unwrap();
        let file3_id = db.insert_file(dir_id, "ok.jpg", 1024, 12347, Some("image")).unwrap();
        db.insert_file(dir_id, "unrated.jpg", 1024, 12348, Some("image")).unwrap();

        db.set_file_rating(file1_id, Some(5)).unwrap();
        db.set_file_rating(file2_id, Some(4)).unwrap();
        db.set_file_rating(file3_id, Some(3)).unwrap();

        let files = db.get_files_by_rating(4).unwrap();
        assert_eq!(files.len(), 2);

        let files = db.get_files_by_rating(5).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0.filename, "great.jpg");

        let files = db.get_files_by_rating(3).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].1, "photos");
    }

    #[test]
    fn test_get_files_by_tag() {
        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos", None, None).unwrap();

        let file1_id = db.insert_file(dir_id, "portrait1.jpg", 1024, 12345, Some("image")).unwrap();
        let file2_id = db.insert_file(dir_id, "portrait2.jpg", 1024, 12346, Some("image")).unwrap();
        let file3_id = db.insert_file(dir_id, "landscape.jpg", 1024, 12347, Some("image")).unwrap();

        db.add_file_tag(file1_id, "portrait").unwrap();
        db.add_file_tag(file2_id, "portrait").unwrap();
        db.add_file_tag(file2_id, "outdoor").unwrap();
        db.add_file_tag(file3_id, "landscape").unwrap();

        let files = db.get_files_by_tag("portrait").unwrap();
        assert_eq!(files.len(), 2);

        let files = db.get_files_by_tag("outdoor").unwrap();
        assert_eq!(files.len(), 1);

        assert!(db.get_files_by_tag("nonexistent").unwrap().is_empty());
    }
}
