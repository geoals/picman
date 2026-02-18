use anyhow::Result;

use super::{AppState, Focus};

/// Navigation direction for wrapping list movement
#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
}

/// Advance an index within a wrapping list
fn wrap_index(current: usize, count: usize, direction: Direction) -> usize {
    match direction {
        Direction::Down => {
            if current < count - 1 { current + 1 } else { 0 }
        }
        Direction::Up => {
            if current > 0 { current - 1 } else { count - 1 }
        }
    }
}

impl AppState {
    pub fn move_down(&mut self) -> Result<()> {
        self.navigate(Direction::Down)
    }

    pub fn move_up(&mut self) -> Result<()> {
        self.navigate(Direction::Up)
    }

    fn navigate(&mut self, direction: Direction) -> Result<()> {
        match self.focus {
            Focus::DirectoryTree => {
                let count = self.get_visible_directories().len();
                if count > 0 {
                    self.tree.selected_index = wrap_index(self.tree.selected_index, count, direction);
                    self.tree.list_state.select(Some(self.tree.selected_index));
                    self.files_dirty = true;
                }
            }
            Focus::FileList => {
                let count = self.file_list.files.len();
                if count > 0 {
                    self.file_list.selected_index = wrap_index(self.file_list.selected_index, count, direction);
                    self.file_list.table_state.select(Some(self.file_list.selected_index));
                    self.skip_preview = true;
                }
            }
        }
        Ok(())
    }

    /// Select a specific index in the directory tree (for mouse clicks).
    /// No-op if index is out of bounds.
    pub fn select_tree_index(&mut self, index: usize) {
        let visible_count = self.get_visible_directories().len();
        if index < visible_count {
            self.tree.selected_index = index;
            self.tree.list_state.select(Some(index));
            self.files_dirty = true;
        }
    }

    /// Select a specific index in the file list (for mouse clicks).
    /// No-op if index is out of bounds.
    pub fn select_file_index(&mut self, index: usize) {
        if index < self.file_list.files.len() {
            self.file_list.selected_index = index;
            self.file_list.table_state.select(Some(index));
            self.skip_preview = true;
        }
    }

    pub fn move_left(&mut self) {
        match self.focus {
            Focus::DirectoryTree => {
                // Collapse current directory or move to parent
                if let Some(dir) = self.get_selected_directory().cloned() {
                    if self.tree.expanded.contains(&dir.id) {
                        self.tree.expanded.remove(&dir.id);
                    } else if let Some(parent_id) = dir.parent_id {
                        // Move to parent
                        let visible = self.get_visible_directories();
                        if let Some(pos) = visible.iter().position(|d| d.id == parent_id) {
                            self.tree.selected_index = pos;
                            self.tree.list_state.select(Some(pos));
                        }
                    }
                }
            }
            Focus::FileList => {
                // Switch to directory tree
                self.focus = Focus::DirectoryTree;
            }
        }
    }

    pub fn move_right(&mut self) {
        match self.focus {
            Focus::DirectoryTree => {
                // Expand current directory or switch to file list
                if let Some(dir) = self.get_selected_directory().cloned() {
                    let has_children = self.tree.has_visible_children(dir.id, &self.matching_dir_ids);
                    if has_children && !self.tree.expanded.contains(&dir.id) {
                        self.tree.expanded.insert(dir.id);
                    } else if !self.file_list.files.is_empty() {
                        self.focus = Focus::FileList;
                    }
                }
            }
            Focus::FileList => {
                // Already at rightmost pane
            }
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::DirectoryTree => Focus::FileList,
            Focus::FileList => Focus::DirectoryTree,
        };
    }

    pub fn select(&mut self) -> Result<()> {
        match self.focus {
            Focus::DirectoryTree => {
                if let Some(dir) = self.get_selected_directory().cloned() {
                    let has_children = self.tree.has_visible_children(dir.id, &self.matching_dir_ids);
                    if has_children {
                        // Directory has children - expand if needed and select first child
                        if !self.tree.expanded.contains(&dir.id) {
                            self.tree.expanded.insert(dir.id);
                        }
                        // Move to first child
                        self.tree.selected_index += 1;
                        self.tree.list_state.select(Some(self.tree.selected_index));
                        self.load_files_for_selected_directory()?;
                    } else {
                        // Leaf directory - load files and move to file list
                        self.load_files_for_selected_directory()?;
                        if self.file_list.files.is_empty() {
                            self.status_message = Some("No files in directory".to_string());
                        } else {
                            self.focus = Focus::FileList;
                        }
                    }
                }
            }
            Focus::FileList => {
                // Open file with default viewer
                self.open_selected_file()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use super::super::Focus;

    #[test]
    fn test_app_state_move_down_wraps() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::DirectoryTree;

        // Move to last directory
        state.tree.selected_index = state.get_visible_directories().len() - 1;

        // Move down should wrap to top
        state.move_down().unwrap();
        assert_eq!(state.tree.selected_index, 0);
    }

    #[test]
    fn test_app_state_move_up_wraps() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::DirectoryTree;
        state.tree.selected_index = 0;

        // Move up should wrap to bottom
        state.move_up().unwrap();
        let visible_count = state.get_visible_directories().len();
        assert_eq!(state.tree.selected_index, visible_count - 1);
    }

    #[test]
    fn test_app_state_toggle_focus() {
        let (mut state, _tempdir) = create_test_app_state();

        assert_eq!(state.focus, Focus::DirectoryTree);
        state.toggle_focus();
        assert_eq!(state.focus, Focus::FileList);
        state.toggle_focus();
        assert_eq!(state.focus, Focus::DirectoryTree);
    }

    #[test]
    fn test_select_empty_leaf_directory_stays_on_tree() {
        use std::fs;
        use tempfile::TempDir;

        use crate::db::Database;
        use super::super::AppState;

        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let db_path = root.join(".picman.db");

        // Create directory structure: "empty" has no files
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::write(root.join("photos/img.jpg"), "data").unwrap();

        let db = Database::open(&db_path).unwrap();
        db.insert_directory("empty", None, None).unwrap();
        db.insert_directory("photos", None, None).unwrap();

        let photos_dir = db.get_directory_by_path("photos").unwrap().unwrap();
        db.insert_file(photos_dir.id, "img.jpg", 4, 0, Some("image")).unwrap();

        let mut state = AppState::new(root, db).unwrap();

        // Select "empty" (first directory in sorted order)
        state.tree.selected_index = 0;
        state.tree.list_state.select(Some(0));
        state.focus = Focus::DirectoryTree;

        // Enter on a leaf directory with no files should NOT switch to FileList
        state.select().unwrap();
        assert_eq!(state.focus, Focus::DirectoryTree, "Focus should stay on tree for empty directory");
        assert!(state.status_message.is_some(), "Should show a status message for empty directory");
    }

    #[test]
    fn test_move_right_empty_directory_stays_on_tree() {
        use std::fs;
        use tempfile::TempDir;

        use crate::db::Database;
        use super::super::AppState;

        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let db_path = root.join(".picman.db");

        // Create directory with no files
        fs::create_dir_all(root.join("empty")).unwrap();

        let db = Database::open(&db_path).unwrap();
        db.insert_directory("empty", None, None).unwrap();

        let mut state = AppState::new(root, db).unwrap();

        state.tree.selected_index = 0;
        state.tree.list_state.select(Some(0));
        state.focus = Focus::DirectoryTree;

        // Expand it first (it has no children, so move_right would try to switch focus)
        state.move_right();
        assert_eq!(state.focus, Focus::DirectoryTree, "move_right should not enter empty file list");
    }
}
