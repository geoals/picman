use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;

use crate::db::{Database, Directory, File};

/// Which pane has focus
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    DirectoryTree,
    FileList,
}

/// A file with its associated tags
#[derive(Debug, Clone)]
pub struct FileWithTags {
    pub file: File,
    pub tags: Vec<String>,
}

/// State for the directory tree
pub struct TreeState {
    pub directories: Vec<Directory>,
    pub selected_index: usize,
    pub expanded: HashSet<i64>,
}

impl TreeState {
    pub fn new(directories: Vec<Directory>) -> Self {
        Self {
            directories,
            selected_index: 0,
            expanded: HashSet::new(),
        }
    }

    /// Get the currently selected directory
    pub fn selected_directory(&self) -> Option<&Directory> {
        self.visible_directories()
            .get(self.selected_index)
            .copied()
    }

    /// Get directories visible based on expansion state
    pub fn visible_directories(&self) -> Vec<&Directory> {
        let mut visible = Vec::new();
        self.collect_visible(None, &mut visible);
        visible
    }

    fn collect_visible<'a>(&'a self, parent_id: Option<i64>, visible: &mut Vec<&'a Directory>) {
        for dir in &self.directories {
            if dir.parent_id == parent_id {
                visible.push(dir);
                if self.expanded.contains(&dir.id) {
                    self.collect_visible(Some(dir.id), visible);
                }
            }
        }
    }

    /// Check if a directory has children
    pub fn has_children(&self, dir_id: i64) -> bool {
        self.directories.iter().any(|d| d.parent_id == Some(dir_id))
    }

    /// Get depth of a directory in the tree
    pub fn depth(&self, dir: &Directory) -> usize {
        let mut depth = 0;
        let mut current_parent = dir.parent_id;
        while let Some(parent_id) = current_parent {
            depth += 1;
            current_parent = self
                .directories
                .iter()
                .find(|d| d.id == parent_id)
                .and_then(|d| d.parent_id);
        }
        depth
    }

}

/// State for the file list
pub struct FileListState {
    pub files: Vec<FileWithTags>,
    pub selected_index: usize,
}

impl FileListState {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selected_index: 0,
        }
    }

    /// Get the currently selected file
    pub fn selected_file(&self) -> Option<&FileWithTags> {
        self.files.get(self.selected_index)
    }
}

/// Main application state
pub struct AppState {
    pub library_path: PathBuf,
    pub db: Database,
    pub focus: Focus,
    pub tree: TreeState,
    pub file_list: FileListState,
    pub show_help: bool,
}

impl AppState {
    pub fn new(library_path: PathBuf, db: Database) -> Result<Self> {
        let directories = db.get_all_directories()?;
        let mut tree = TreeState::new(directories);

        // Expand root directories by default
        let root_dirs: Vec<i64> = tree
            .directories
            .iter()
            .filter(|d| d.parent_id.is_none())
            .map(|d| d.id)
            .collect();
        for id in root_dirs {
            tree.expanded.insert(id);
        }

        let mut state = Self {
            library_path,
            db,
            focus: Focus::DirectoryTree,
            tree,
            file_list: FileListState::new(),
            show_help: false,
        };

        // Load files for initial selection
        state.load_files_for_selected_directory()?;

        Ok(state)
    }

    /// Load files for the currently selected directory
    fn load_files_for_selected_directory(&mut self) -> Result<()> {
        self.file_list.files.clear();
        self.file_list.selected_index = 0;

        if let Some(dir) = self.tree.selected_directory() {
            let files = self.db.get_files_in_directory(dir.id)?;
            for file in files {
                let tags = self.db.get_file_tags(file.id)?;
                self.file_list.files.push(FileWithTags { file, tags });
            }
        }

        Ok(())
    }

    pub fn move_down(&mut self) {
        match self.focus {
            Focus::DirectoryTree => {
                let visible_count = self.tree.visible_directories().len();
                if visible_count > 0 && self.tree.selected_index < visible_count - 1 {
                    self.tree.selected_index += 1;
                }
            }
            Focus::FileList => {
                if !self.file_list.files.is_empty()
                    && self.file_list.selected_index < self.file_list.files.len() - 1
                {
                    self.file_list.selected_index += 1;
                }
            }
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            Focus::DirectoryTree => {
                if self.tree.selected_index > 0 {
                    self.tree.selected_index -= 1;
                }
            }
            Focus::FileList => {
                if self.file_list.selected_index > 0 {
                    self.file_list.selected_index -= 1;
                }
            }
        }
    }

    pub fn move_left(&mut self) {
        match self.focus {
            Focus::DirectoryTree => {
                // Collapse current directory or move to parent
                if let Some(dir) = self.tree.selected_directory().cloned() {
                    if self.tree.expanded.contains(&dir.id) {
                        self.tree.expanded.remove(&dir.id);
                    } else if let Some(parent_id) = dir.parent_id {
                        // Move to parent
                        let visible = self.tree.visible_directories();
                        if let Some(pos) = visible.iter().position(|d| d.id == parent_id) {
                            self.tree.selected_index = pos;
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
                if let Some(dir) = self.tree.selected_directory().cloned() {
                    if self.tree.has_children(dir.id) && !self.tree.expanded.contains(&dir.id) {
                        self.tree.expanded.insert(dir.id);
                    } else {
                        self.focus = Focus::FileList;
                    }
                } else {
                    self.focus = Focus::FileList;
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
                // Load files for selected directory
                self.load_files_for_selected_directory()?;
            }
            Focus::FileList => {
                // Preview is automatic based on selection
            }
        }
        Ok(())
    }

    pub fn set_rating(&mut self, rating: Option<i32>) -> Result<()> {
        if self.focus != Focus::FileList {
            return Ok(());
        }

        if let Some(file_with_tags) = self.file_list.files.get_mut(self.file_list.selected_index) {
            self.db.set_file_rating(file_with_tags.file.id, rating)?;
            file_with_tags.file.rating = rating;
        }

        Ok(())
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// Get the full path to the currently selected file
    pub fn selected_file_path(&self) -> Option<PathBuf> {
        let file_with_tags = self.file_list.selected_file()?;
        let dir = self.tree.selected_directory()?;

        let relative_path = if dir.path.is_empty() {
            PathBuf::from(&file_with_tags.file.filename)
        } else {
            PathBuf::from(&dir.path).join(&file_with_tags.file.filename)
        };

        Some(self.library_path.join(relative_path))
    }
}
