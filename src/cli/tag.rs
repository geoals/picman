use std::path::Path;

use anyhow::Result;

use crate::db::Database;

/// Options for the tag command
#[derive(Debug, Default)]
pub struct TagOptions {
    pub add: Vec<String>,
    pub remove: Vec<String>,
    pub list: bool,
}

/// Add, remove, or list tags on a file
///
/// # Arguments
/// * `library_path` - Path to the library root
/// * `file_path` - Path to the file (relative to library root)
/// * `options` - What tags to add/remove, or whether to list
///
/// # Returns
/// Current tags after any modifications
pub fn run_tag(library_path: &Path, file_path: &Path, options: TagOptions) -> Result<Vec<String>> {
    let db_path = library_path.join(".picman.db");
    if !db_path.exists() {
        anyhow::bail!(
            "No database found at {}. Run 'picman init' first.",
            library_path.display()
        );
    }

    let db = Database::open(&db_path)?;

    let relative_path = file_path.to_string_lossy();

    let file = db.get_file_by_path(&relative_path)?;
    let file = match file {
        Some(f) => f,
        None => anyhow::bail!("File not found in database: {}", relative_path),
    };

    // Add tags
    for tag in &options.add {
        db.add_file_tag(file.id, tag)?;
    }

    // Remove tags
    for tag in &options.remove {
        db.remove_file_tag(file.id, tag)?;
    }

    // Return current tags
    db.get_file_tags(file.id)
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
    fn test_tag_add() {
        let (_temp_dir, lib_path) = setup_test_library();

        let tags = run_tag(
            &lib_path,
            Path::new("photo.jpg"),
            TagOptions {
                add: vec!["portrait".to_string(), "outdoor".to_string()],
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(tags, vec!["outdoor", "portrait"]); // sorted
    }

    #[test]
    fn test_tag_remove() {
        let (_temp_dir, lib_path) = setup_test_library();

        // Add tags first
        run_tag(
            &lib_path,
            Path::new("photo.jpg"),
            TagOptions {
                add: vec!["portrait".to_string(), "outdoor".to_string()],
                ..Default::default()
            },
        )
        .unwrap();

        // Remove one
        let tags = run_tag(
            &lib_path,
            Path::new("photo.jpg"),
            TagOptions {
                remove: vec!["portrait".to_string()],
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(tags, vec!["outdoor"]);
    }

    #[test]
    fn test_tag_list() {
        let (_temp_dir, lib_path) = setup_test_library();

        // Add tags
        run_tag(
            &lib_path,
            Path::new("photo.jpg"),
            TagOptions {
                add: vec!["portrait".to_string()],
                ..Default::default()
            },
        )
        .unwrap();

        // List tags
        let tags = run_tag(
            &lib_path,
            Path::new("photo.jpg"),
            TagOptions {
                list: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(tags, vec!["portrait"]);
    }

    #[test]
    fn test_tag_add_duplicate_idempotent() {
        let (_temp_dir, lib_path) = setup_test_library();

        // Add same tag twice
        run_tag(
            &lib_path,
            Path::new("photo.jpg"),
            TagOptions {
                add: vec!["portrait".to_string()],
                ..Default::default()
            },
        )
        .unwrap();

        let tags = run_tag(
            &lib_path,
            Path::new("photo.jpg"),
            TagOptions {
                add: vec!["portrait".to_string()],
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(tags, vec!["portrait"]); // still just one
    }

    #[test]
    fn test_tag_nonexistent_file() {
        let (_temp_dir, lib_path) = setup_test_library();

        let result = run_tag(
            &lib_path,
            Path::new("nonexistent.jpg"),
            TagOptions {
                list: true,
                ..Default::default()
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_tag_no_database() {
        let temp_dir = TempDir::new().unwrap();
        let result = run_tag(
            temp_dir.path(),
            Path::new("photo.jpg"),
            TagOptions::default(),
        );
        assert!(result.is_err());
    }
}
