use std::path::Path;

use anyhow::Result;

use crate::db::{Database, File};

/// Info about a file for display purposes
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub path: String,
    pub rating: Option<i32>,
    pub tags: Vec<String>,
}

/// Options for filtering the file list
#[derive(Debug, Default)]
pub struct ListOptions {
    pub min_rating: Option<i32>,
    pub tag: Option<String>,
}

/// List files from the library, optionally filtered
pub fn run_list(library_path: &Path, options: ListOptions) -> Result<Vec<FileInfo>> {
    let db_path = library_path.join(".picman.db");
    if !db_path.exists() {
        anyhow::bail!(
            "No database found at {}. Run 'picman init' first.",
            library_path.display()
        );
    }

    let db = Database::open(&db_path)?;

    // Get files based on filters
    let files_with_paths: Vec<(File, String)> = match (&options.min_rating, &options.tag) {
        (Some(rating), None) => db.get_files_by_rating(*rating)?,
        (None, Some(tag)) => db.get_files_by_tag(tag)?,
        (Some(rating), Some(tag)) => {
            // Both filters: get by tag, then filter by rating
            db.get_files_by_tag(tag)?
                .into_iter()
                .filter(|(f, _)| f.rating.map(|r| r >= *rating).unwrap_or(false))
                .collect()
        }
        (None, None) => db.get_all_files_with_paths()?,
    };

    // Convert to FileInfo with tags
    let mut result = Vec::with_capacity(files_with_paths.len());
    for (file, dir_path) in files_with_paths {
        let full_path = if dir_path.is_empty() {
            file.filename.clone()
        } else {
            format!("{}/{}", dir_path, file.filename)
        };

        let tags = db.get_file_tags(file.id)?;

        result.push(FileInfo {
            path: full_path,
            rating: file.rating,
            tags,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_library() -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let lib_path = temp_dir.path().to_path_buf();

        // Create test files
        fs::write(lib_path.join("photo1.jpg"), "fake jpeg").unwrap();
        fs::write(lib_path.join("photo2.jpg"), "fake jpeg").unwrap();
        fs::write(lib_path.join("photo3.jpg"), "fake jpeg").unwrap();

        // Initialize the database
        crate::cli::run_init(&lib_path).unwrap();

        // Set up ratings and tags
        let db = Database::open(&lib_path.join(".picman.db")).unwrap();
        let file1 = db.get_file_by_path("photo1.jpg").unwrap().unwrap();
        let file2 = db.get_file_by_path("photo2.jpg").unwrap().unwrap();
        let _file3 = db.get_file_by_path("photo3.jpg").unwrap().unwrap();

        db.set_file_rating(file1.id, Some(5)).unwrap();
        db.set_file_rating(file2.id, Some(4)).unwrap();
        // file3 unrated

        db.add_file_tag(file1.id, "portrait").unwrap();
        db.add_file_tag(file2.id, "portrait").unwrap();
        db.add_file_tag(file2.id, "outdoor").unwrap();

        (temp_dir, lib_path)
    }

    #[test]
    fn test_list_all_files() {
        let (_temp_dir, lib_path) = setup_test_library();

        let files = run_list(&lib_path, ListOptions::default()).unwrap();
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn test_list_filter_by_rating() {
        let (_temp_dir, lib_path) = setup_test_library();

        let files = run_list(
            &lib_path,
            ListOptions {
                min_rating: Some(4),
                tag: None,
            },
        )
        .unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.rating.unwrap() >= 4));
    }

    #[test]
    fn test_list_filter_by_tag() {
        let (_temp_dir, lib_path) = setup_test_library();

        let files = run_list(
            &lib_path,
            ListOptions {
                min_rating: None,
                tag: Some("portrait".to_string()),
            },
        )
        .unwrap();

        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_list_filter_by_rating_and_tag() {
        let (_temp_dir, lib_path) = setup_test_library();

        let files = run_list(
            &lib_path,
            ListOptions {
                min_rating: Some(5),
                tag: Some("portrait".to_string()),
            },
        )
        .unwrap();

        // Only photo1 has rating 5 AND portrait tag
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "photo1.jpg");
    }

    #[test]
    fn test_list_includes_tags() {
        let (_temp_dir, lib_path) = setup_test_library();

        let files = run_list(&lib_path, ListOptions::default()).unwrap();
        let photo2 = files.iter().find(|f| f.path == "photo2.jpg").unwrap();

        assert_eq!(photo2.tags, vec!["outdoor", "portrait"]); // sorted
    }

    #[test]
    fn test_list_nonexistent_db_errors() {
        let temp_dir = TempDir::new().unwrap();
        let result = run_list(temp_dir.path(), ListOptions::default());
        assert!(result.is_err());
    }
}
