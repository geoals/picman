use std::process::Command;

use anyhow::Result;

use super::{AppState, FileWithTags, RatingFilter};

impl AppState {
    /// Load files for the currently selected directory, applying current filter
    pub(super) fn load_files_for_selected_directory(&mut self) -> Result<()> {
        self.file_list.files.clear();
        self.file_list.selected_index = 0;
        self.file_list.table_state.select(Some(0));

        let selected_dir = self.get_selected_directory().cloned();
        if let Some(dir) = selected_dir {
            // Update current directory ID and notify preview loader
            self.current_dir_id = Some(dir.id);
            self.preview_loader.borrow_mut().set_current_dir(dir.id);
            let files = self.db.get_files_in_directory(dir.id)?;

            // Check if this directory or any ancestor matches the full filter criteria
            // If so, show all files without checking individual file filters
            let ancestor_matches_filter = self.directory_or_ancestor_matches_filter(dir.id)?;

            // Get tags from this directory and all ancestors (for tag filter inheritance)
            let dir_tags = if !self.filter.tags.is_empty() {
                self.get_directory_and_ancestor_tags(dir.id)?
            } else {
                vec![]
            };

            // Batch fetch all tags for files in this directory (single query instead of N)
            let all_file_tags = self.db.get_file_tags_for_directory(dir.id)?;

            for file in files {
                let tags = all_file_tags
                    .get(&file.id)
                    .cloned()
                    .unwrap_or_default();

                if !self.filter.matches_file(&file, &tags, &dir_tags, ancestor_matches_filter) {
                    continue;
                }

                self.file_list.files.push(FileWithTags { file, tags });
            }
        }

        Ok(())
    }

    /// Load files for the selected directory if marked as dirty.
    /// Called after event loop drains all pending keypresses.
    pub fn load_files_if_dirty(&mut self) -> Result<()> {
        if self.files_dirty {
            self.load_files_for_selected_directory()?;
            self.files_dirty = false;
        }
        Ok(())
    }

    /// Clear the skip_preview flag after rapid navigation stops.
    /// Called after event loop drains all pending keypresses.
    pub fn clear_skip_preview(&mut self) {
        self.skip_preview = false;
    }

    /// Open the selected file with the default system viewer
    pub fn open_selected_file(&self) -> Result<()> {
        if let Some(path) = self.selected_file_path() {
            #[cfg(target_os = "linux")]
            {
                Command::new("xdg-open")
                    .arg(&path)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?;
            }
            #[cfg(target_os = "macos")]
            {
                Command::new("open")
                    .arg(&path)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?;
            }
        }
        Ok(())
    }

    /// Check if a directory or any of its ancestors matches the full filter criteria
    /// (both rating and tags, when both are active)
    fn directory_or_ancestor_matches_filter(&self, dir_id: i64) -> Result<bool> {
        let ids = std::iter::once(dir_id).chain(self.tree.ancestor_ids(dir_id));

        for id in ids {
            let Some(dir) = self.tree.directories.iter().find(|d| d.id == id) else {
                break;
            };

            // Check rating filter on directory
            let dir_matches_rating = match self.filter.rating {
                RatingFilter::Any => true,
                RatingFilter::Unrated => dir.rating.is_none(),
                RatingFilter::MinRating(min) => dir.rating.map(|r| r >= min).unwrap_or(false),
            };

            // Check tag filter on directory
            let dir_matches_tags = if self.filter.tags.is_empty() {
                true
            } else {
                let dir_tags = self.db.get_directory_tags(id)?;
                self.filter.tags.iter().all(|t| dir_tags.contains(t))
            };

            if dir_matches_rating && dir_matches_tags {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get all tags from a directory and all its ancestors
    fn get_directory_and_ancestor_tags(&self, dir_id: i64) -> Result<Vec<String>> {
        let mut all_tags = Vec::new();
        for id in std::iter::once(dir_id).chain(self.tree.ancestor_ids(dir_id)) {
            let tags = self.db.get_directory_tags(id)?;
            all_tags.extend(tags);
        }
        Ok(all_tags)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::TempDir;

    use crate::db::Database;
    use super::super::AppState;

    #[test]
    fn test_filter_by_file_tag_and_directory_tag() {
        // Bug: filtering by tag1 (on file) AND tag2 (on directory) should show the file
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let db_path = root.join(".picman.db");

        // Create directory and file
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/img1.jpg"), "data").unwrap();

        // Create and populate database
        let db = Database::open(&db_path).unwrap();
        let dir_id = db.insert_directory("photos", None, None).unwrap();
        let file_id = db.insert_file(dir_id, "img1.jpg", 4, 0, Some("image")).unwrap();

        // Add tag to file
        db.add_file_tag(file_id, "file_tag").unwrap();
        // Add different tag to directory
        db.add_directory_tag(dir_id, "dir_tag").unwrap();

        let mut state = AppState::new(root, db).unwrap();

        // Select the photos directory
        state.tree.selected_index = 0;
        state.tree.list_state.select(Some(0));
        state.load_files_for_selected_directory().unwrap();

        // Without filter, file should be visible
        assert_eq!(state.file_list.files.len(), 1);

        // Set filter to require both tags
        state.filter.tags = vec!["file_tag".to_string(), "dir_tag".to_string()];
        state.load_files_for_selected_directory().unwrap();

        // File has file_tag directly and inherits dir_tag from directory
        // So it should match the filter and be visible
        assert_eq!(
            state.file_list.files.len(),
            1,
            "File should be visible when filtering by file tag AND directory tag"
        );
    }

    #[test]
    fn test_filter_inherits_tags_from_grandparent() {
        // Tags should be inherited from all ancestors, not just direct parent
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let db_path = root.join(".picman.db");

        // Create nested directory structure
        fs::create_dir_all(root.join("photos/vacation/beach")).unwrap();
        fs::write(root.join("photos/vacation/beach/img1.jpg"), "data").unwrap();

        // Create and populate database
        let db = Database::open(&db_path).unwrap();
        let photos_id = db.insert_directory("photos", None, None).unwrap();
        let vacation_id = db.insert_directory("photos/vacation", Some(photos_id), None).unwrap();
        let beach_id = db.insert_directory("photos/vacation/beach", Some(vacation_id), None).unwrap();
        let file_id = db.insert_file(beach_id, "img1.jpg", 4, 0, Some("image")).unwrap();

        // Add tag to file
        db.add_file_tag(file_id, "sunset").unwrap();
        // Add tag to grandparent (photos), not parent (beach)
        db.add_directory_tag(photos_id, "family").unwrap();

        let mut state = AppState::new(root, db).unwrap();

        // Expand tree to show beach directory
        state.tree.expanded.insert(photos_id);
        state.tree.expanded.insert(vacation_id);

        // Select the beach directory (index 2: photos=0, vacation=1, beach=2)
        state.tree.selected_index = 2;
        state.tree.list_state.select(Some(2));
        state.load_files_for_selected_directory().unwrap();

        // Set filter to require both tags
        state.filter.tags = vec!["sunset".to_string(), "family".to_string()];
        state.load_files_for_selected_directory().unwrap();

        // File has sunset directly and inherits family from grandparent
        assert_eq!(
            state.file_list.files.len(),
            1,
            "File should inherit tags from grandparent directory"
        );
    }
}
