use std::path::Path;

use anyhow::Result;

use crate::db::Database;

/// Set or clear a file's rating
///
/// # Arguments
/// * `library_path` - Path to the library root
/// * `file_path` - Path to the file (relative to library root)
/// * `rating` - Rating 1-5, or None to clear
pub fn run_rate(library_path: &Path, file_path: &Path, rating: Option<i32>) -> Result<()> {
    // Validate rating
    if let Some(r) = rating {
        if !(1..=5).contains(&r) {
            anyhow::bail!("Rating must be between 1 and 5 (got {})", r);
        }
    }

    let db_path = library_path.join(".picman.db");
    if !db_path.exists() {
        anyhow::bail!(
            "No database found at {}. Run 'picman init' first.",
            library_path.display()
        );
    }

    let db = Database::open(&db_path)?;

    // Convert file_path to relative path string
    let relative_path = file_path.to_string_lossy();

    let file = db.get_file_by_path(&relative_path)?;
    let file = match file {
        Some(f) => f,
        None => anyhow::bail!("File not found in database: {}", relative_path),
    };

    db.set_file_rating(file.id, rating)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_library() -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let lib_path = temp_dir.path().to_path_buf();

        fs::write(lib_path.join("photo.jpg"), "fake jpeg").unwrap();
        crate::cli::run_init(&lib_path).unwrap();

        (temp_dir, lib_path)
    }

    #[test]
    fn test_rate_file() {
        let (_temp_dir, lib_path) = setup_test_library();

        run_rate(&lib_path, Path::new("photo.jpg"), Some(5)).unwrap();

        // Verify the rating was set
        let db = Database::open(&lib_path.join(".picman.db")).unwrap();
        let file = db.get_file_by_path("photo.jpg").unwrap().unwrap();
        assert_eq!(file.rating, Some(5));
    }

    #[test]
    fn test_rate_file_clear() {
        let (_temp_dir, lib_path) = setup_test_library();

        // Set a rating first
        run_rate(&lib_path, Path::new("photo.jpg"), Some(3)).unwrap();

        // Clear it
        run_rate(&lib_path, Path::new("photo.jpg"), None).unwrap();

        // Verify it was cleared
        let db = Database::open(&lib_path.join(".picman.db")).unwrap();
        let file = db.get_file_by_path("photo.jpg").unwrap().unwrap();
        assert_eq!(file.rating, None);
    }

    #[test]
    fn test_rate_invalid_rating_zero() {
        let (_temp_dir, lib_path) = setup_test_library();

        let result = run_rate(&lib_path, Path::new("photo.jpg"), Some(0));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("between 1 and 5"));
    }

    #[test]
    fn test_rate_invalid_rating_too_high() {
        let (_temp_dir, lib_path) = setup_test_library();

        let result = run_rate(&lib_path, Path::new("photo.jpg"), Some(6));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("between 1 and 5"));
    }

    #[test]
    fn test_rate_nonexistent_file() {
        let (_temp_dir, lib_path) = setup_test_library();

        let result = run_rate(&lib_path, Path::new("nonexistent.jpg"), Some(5));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_rate_no_database() {
        let temp_dir = TempDir::new().unwrap();
        let result = run_rate(temp_dir.path(), Path::new("photo.jpg"), Some(5));
        assert!(result.is_err());
    }
}
