use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

/// Database wrapper for picman
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create database at the given path
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.initialize_schema()?;
        Ok(db)
    }

    /// Create in-memory database (for testing)
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.initialize_schema()?;
        Ok(db)
    }

    fn initialize_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS directories (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                parent_id INTEGER REFERENCES directories(id),
                rating INTEGER CHECK (rating IS NULL OR (rating >= 1 AND rating <= 5)),
                mtime INTEGER
            );

            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                directory_id INTEGER NOT NULL REFERENCES directories(id),
                filename TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                hash TEXT,
                rating INTEGER CHECK (rating IS NULL OR (rating >= 1 AND rating <= 5)),
                media_type TEXT CHECK (media_type IN ('image', 'video', 'other')),
                UNIQUE(directory_id, filename)
            );

            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY,
                name TEXT UNIQUE NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_tags (
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (file_id, tag_id)
            );

            CREATE TABLE IF NOT EXISTS directory_tags (
                directory_id INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,
                tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (directory_id, tag_id)
            );

            CREATE INDEX IF NOT EXISTS idx_files_hash ON files(hash);
            CREATE INDEX IF NOT EXISTS idx_files_directory ON files(directory_id);
            CREATE INDEX IF NOT EXISTS idx_directories_parent ON directories(parent_id);
            "#,
        )?;
        Ok(())
    }

    /// Get a reference to the underlying connection (for testing)
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Begin a transaction for bulk operations
    pub fn begin_transaction(&self) -> Result<()> {
        self.conn.execute("BEGIN TRANSACTION", [])?;
        Ok(())
    }

    /// Commit the current transaction
    pub fn commit(&self) -> Result<()> {
        self.conn.execute("COMMIT", [])?;
        Ok(())
    }

    /// Rollback the current transaction
    pub fn rollback(&self) -> Result<()> {
        self.conn.execute("ROLLBACK", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_in_memory_database() {
        let db = Database::open_in_memory().expect("Failed to create database");

        // Verify tables exist by querying them
        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"directories".to_string()));
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"tags".to_string()));
        assert!(tables.contains(&"file_tags".to_string()));
        assert!(tables.contains(&"directory_tags".to_string()));
    }

    #[test]
    fn test_directory_rating_constraints() {
        let db = Database::open_in_memory().unwrap();

        // Valid ratings (1-5)
        for rating in 1..=5 {
            db.conn
                .execute(
                    &format!(
                        "INSERT INTO directories (path, rating) VALUES ('test{}', {})",
                        rating, rating
                    ),
                    [],
                )
                .expect(&format!("Should accept rating {}", rating));
        }

        // NULL rating is allowed
        db.conn
            .execute(
                "INSERT INTO directories (path, rating) VALUES ('test_null', NULL)",
                [],
            )
            .expect("Should accept NULL rating");

        // Rating too high (6)
        let result = db.conn.execute(
            "INSERT INTO directories (path, rating) VALUES ('test_high', 6)",
            [],
        );
        assert!(result.is_err(), "Should reject rating > 5");

        // Rating too low (0)
        let result = db.conn.execute(
            "INSERT INTO directories (path, rating) VALUES ('test_zero', 0)",
            [],
        );
        assert!(result.is_err(), "Should reject rating < 1");

        // Negative rating
        let result = db.conn.execute(
            "INSERT INTO directories (path, rating) VALUES ('test_neg', -1)",
            [],
        );
        assert!(result.is_err(), "Should reject negative rating");
    }

    #[test]
    fn test_file_media_type_constraint() {
        let db = Database::open_in_memory().unwrap();

        // First create a directory
        db.conn
            .execute("INSERT INTO directories (path) VALUES ('dir')", [])
            .unwrap();

        // Valid media types
        for media_type in ["image", "video", "other"] {
            db.conn
                .execute(
                    "INSERT INTO files (directory_id, filename, size, mtime, media_type)
                     VALUES (1, ?, 100, 12345, ?)",
                    [format!("file_{}.jpg", media_type), media_type.to_string()],
                )
                .expect(&format!("Should accept media_type '{}'", media_type));
        }

        // Invalid media type
        let result = db.conn.execute(
            "INSERT INTO files (directory_id, filename, size, mtime, media_type)
             VALUES (1, 'bad.txt', 100, 12345, 'invalid')",
            [],
        );
        assert!(result.is_err(), "Should reject invalid media_type");
    }
}
