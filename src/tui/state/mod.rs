mod files;
mod filter;
mod navigation;
mod preview;
mod rename;
mod search;
mod tags;

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

use anyhow::Result;
use ratatui::layout::Rect;
use ratatui::widgets::{ListState, TableState};

use crate::db::{Database, Directory, File};
use crate::tui::preview_loader::PreviewLoader;

// Re-export dialog types so existing `use crate::tui::state::X` paths keep working
pub use super::dialogs::{
    FilterCriteria, FilterDialogFocus, FilterDialogState, OperationsMenuState, RatingFilter,
    RenameDialogState, SearchState, TagInputState,
};
pub use super::operations::{BackgroundProgress, OperationType};
pub use super::preview_cache::LruPreviewCache;

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
    pub list_state: ListState,
}

impl TreeState {
    pub fn new(directories: Vec<Directory>) -> Self {
        Self {
            directories,
            selected_index: 0,
            expanded: HashSet::new(),
            list_state: ListState::default().with_selected(Some(0)),
        }
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
        self.ancestor_ids(dir.id).count()
    }

    /// Iterate over ancestor directory IDs (parent, grandparent, ...).
    /// Does NOT include `dir_id` itself.
    pub fn ancestor_ids(&self, dir_id: i64) -> impl Iterator<Item = i64> + '_ {
        let first_parent = self
            .directories
            .iter()
            .find(|d| d.id == dir_id)
            .and_then(|d| d.parent_id);
        std::iter::successors(first_parent, |&pid| {
            self.directories
                .iter()
                .find(|d| d.id == pid)
                .and_then(|d| d.parent_id)
        })
    }

    /// Get directories visible based on expansion state, filtered by matching IDs
    pub fn visible_directories_filtered(&self, matching_ids: &HashSet<i64>) -> Vec<&Directory> {
        if matching_ids.is_empty() {
            // No filter active - return all visible
            return self.visible_directories();
        }
        let mut visible = Vec::new();
        self.collect_visible_filtered(None, matching_ids, &mut visible);
        visible
    }

    fn collect_visible_filtered<'a>(
        &'a self,
        parent_id: Option<i64>,
        matching_ids: &HashSet<i64>,
        visible: &mut Vec<&'a Directory>,
    ) {
        for dir in &self.directories {
            if dir.parent_id == parent_id && matching_ids.contains(&dir.id) {
                visible.push(dir);
                if self.expanded.contains(&dir.id) {
                    self.collect_visible_filtered(Some(dir.id), matching_ids, visible);
                }
            }
        }
    }

    /// Check if a directory has visible children (considering filter)
    pub fn has_visible_children(&self, dir_id: i64, matching_ids: &HashSet<i64>) -> bool {
        if matching_ids.is_empty() {
            return self.has_children(dir_id);
        }
        self.directories
            .iter()
            .any(|d| d.parent_id == Some(dir_id) && matching_ids.contains(&d.id))
    }
}

/// State for the file list
pub struct FileListState {
    pub files: Vec<FileWithTags>,
    pub selected_index: usize,
    pub table_state: TableState,
}

impl FileListState {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selected_index: 0,
            table_state: TableState::default().with_selected(Some(0)),
        }
    }

    /// Get the currently selected file
    pub fn selected_file(&self) -> Option<&FileWithTags> {
        self.files.get(self.selected_index)
    }
}

/// Default LRU cache size for file previews.
const DEFAULT_PREVIEW_CACHE_SIZE: usize = 50;

/// Default LRU cache size for directory composite previews.
/// These are larger images (~12 thumbnails composited) so we keep fewer.
const DEFAULT_DIR_PREVIEW_CACHE_SIZE: usize = 20;

/// Main application state
pub struct AppState {
    pub library_path: PathBuf,
    pub db: Database,
    pub focus: Focus,
    pub tree: TreeState,
    pub file_list: FileListState,
    pub show_help: bool,
    pub preview_cache: RefCell<LruPreviewCache>,
    pub dir_preview_cache: RefCell<LruPreviewCache>,
    pub tag_input: Option<TagInputState>,
    pub filter_dialog: Option<FilterDialogState>,
    pub rename_dialog: Option<RenameDialogState>,
    pub filter: FilterCriteria,
    /// Directory IDs that match the current filter (includes ancestors for tree structure)
    pub matching_dir_ids: HashSet<i64>,
    /// Operations menu popup
    pub operations_menu: Option<OperationsMenuState>,
    /// Status message to show temporarily
    pub status_message: Option<String>,
    /// Background operation progress
    pub background_progress: Option<BackgroundProgress>,
    /// Queue of pending operations (executed sequentially)
    pub operation_queue: VecDeque<OperationType>,
    /// Cache for missing preview check: (dir_id, is_missing)
    pub missing_preview_cache: RefCell<Option<(i64, bool)>>,
    /// True when file list needs to be reloaded (deferred loading for smooth scrolling)
    pub files_dirty: bool,
    /// Skip loading new previews during rapid navigation (show cached instead)
    pub skip_preview: bool,
    /// Background image loader - decodes images off the main thread
    pub preview_loader: RefCell<PreviewLoader>,
    /// Current directory ID for tracking stale preview loads
    pub current_dir_id: Option<i64>,
    /// Force a full terminal redraw on the next frame (needed after closing overlays
    /// that cover image protocol content — the protocol doesn't know its display was
    /// destroyed so terminal.clear() is required to make it re-send).
    pub force_redraw: bool,
    /// Layout rects saved each frame for mouse hit-testing
    pub tree_area: Rect,
    pub file_list_area: Rect,
    /// Path of the file whose protocol is currently being rendered.
    /// The actual protocol lives in the preview_cache entry. This path is used
    /// as a fallback reference during rapid navigation (skip_preview) so we know
    /// which cached protocol to keep showing.
    pub render_protocol: RefCell<Option<PathBuf>>,
    /// Incremental search state (/ key)
    pub search: SearchState,
    /// Whether the details panel is expanded (toggled with `i`)
    pub details_expanded: bool,
    /// Cached EXIF data for the current file (avoids re-reading on every frame)
    pub cached_exif: Option<(PathBuf, super::exif::ExifInfo)>,
}

impl AppState {
    pub fn new(library_path: PathBuf, db: Database) -> Result<Self> {
        let directories = db.get_all_directories()?;
        let tree = TreeState::new(directories);

        let mut state = Self {
            library_path,
            db,
            focus: Focus::DirectoryTree,
            tree,
            file_list: FileListState::new(),
            show_help: false,
            preview_cache: RefCell::new(LruPreviewCache::new(DEFAULT_PREVIEW_CACHE_SIZE)),
            dir_preview_cache: RefCell::new(LruPreviewCache::new(DEFAULT_DIR_PREVIEW_CACHE_SIZE)),
            tag_input: None,
            filter_dialog: None,
            rename_dialog: None,
            filter: FilterCriteria::default(),
            matching_dir_ids: HashSet::new(),
            operations_menu: None,
            status_message: None,
            background_progress: None,
            operation_queue: VecDeque::new(),
            missing_preview_cache: RefCell::new(None),
            files_dirty: false,
            skip_preview: false,
            preview_loader: RefCell::new(PreviewLoader::new()),
            current_dir_id: None,
            force_redraw: false,
            tree_area: Rect::default(),
            file_list_area: Rect::default(),
            render_protocol: RefCell::new(None),
            search: SearchState::new(),
            details_expanded: false,
            cached_exif: None,
        };

        // Load files for initial selection
        state.load_files_for_selected_directory()?;

        Ok(state)
    }

    // ==================== Core Getters ====================

    /// Get visible directories considering the current filter
    pub fn get_visible_directories(&self) -> Vec<&Directory> {
        let mut dirs = self.tree.visible_directories_filtered(&self.matching_dir_ids);

        // When Unrated filter is active, also hide directories that have a rating
        if self.filter.rating == RatingFilter::Unrated {
            dirs.retain(|d| d.rating.is_none());
        }

        dirs
    }

    /// Get the currently selected directory considering the filter
    pub fn get_selected_directory(&self) -> Option<&Directory> {
        self.get_visible_directories()
            .get(self.tree.selected_index)
            .copied()
    }

    /// Get the full path to the currently selected file
    pub fn selected_file_path(&self) -> Option<PathBuf> {
        let file_with_tags = self.file_list.selected_file()?;
        let dir = self.get_selected_directory()?;
        Some(dir.file_path(&self.library_path, &file_with_tags.file.filename))
    }

    pub fn set_rating(&mut self, rating: Option<i32>) -> Result<()> {
        match self.focus {
            Focus::DirectoryTree => {
                if let Some(dir) = self.get_selected_directory() {
                    let dir_id = dir.id;
                    self.db.set_directory_rating(dir_id, rating)?;
                    // Update in-memory state
                    if let Some(dir) = self.tree.directories.iter_mut().find(|d| d.id == dir_id) {
                        dir.rating = rating;
                    }
                }
            }
            Focus::FileList => {
                if let Some(file_with_tags) =
                    self.file_list.files.get_mut(self.file_list.selected_index)
                {
                    self.db.set_file_rating(file_with_tags.file.id, rating)?;
                    file_with_tags.file.rating = rating;
                }
            }
        }
        Ok(())
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// Clear status message
    pub fn clear_status_message(&mut self) {
        self.status_message = None;
    }
}

/// Shared test helpers for all state sub-module tests.
#[cfg(test)]
pub(super) mod test_helpers {
    use crate::db::Database;
    use super::AppState;

    pub fn create_test_directories() -> Vec<crate::db::Directory> {
        vec![
            crate::db::Directory { id: 1, path: "photos".to_string(), parent_id: None, rating: None, mtime: Some(0) },
            crate::db::Directory { id: 2, path: "photos/vacation".to_string(), parent_id: Some(1), rating: None, mtime: Some(0) },
            crate::db::Directory { id: 3, path: "photos/vacation/beach".to_string(), parent_id: Some(2), rating: None, mtime: Some(0) },
            crate::db::Directory { id: 4, path: "videos".to_string(), parent_id: None, rating: Some(5), mtime: Some(0) },
        ]
    }

    pub fn create_test_app_state() -> (AppState, tempfile::TempDir) {
        use std::fs;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let db_path = root.join(".picman.db");

        // Create directory structure
        fs::create_dir_all(root.join("photos")).unwrap();
        fs::create_dir_all(root.join("videos")).unwrap();
        fs::write(root.join("photos/img1.jpg"), "data").unwrap();
        fs::write(root.join("photos/img2.jpg"), "data").unwrap();
        fs::write(root.join("videos/vid1.mp4"), "data").unwrap();

        // Create and populate database
        let db = Database::open(&db_path).unwrap();
        db.insert_directory("photos", None, None).unwrap();
        db.insert_directory("videos", None, None).unwrap();

        let photos_dir = db.get_directory_by_path("photos").unwrap().unwrap();
        db.insert_file(photos_dir.id, "img1.jpg", 4, 0, Some("image")).unwrap();
        db.insert_file(photos_dir.id, "img2.jpg", 4, 0, Some("image")).unwrap();

        let videos_dir = db.get_directory_by_path("videos").unwrap().unwrap();
        db.insert_file(videos_dir.id, "vid1.mp4", 4, 0, Some("video")).unwrap();

        let state = AppState::new(root, db).unwrap();
        (state, temp)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use super::*;
    use test_helpers::*;

    // ==================== TreeState Tests ====================

    #[test]
    fn test_tree_visible_directories_respects_expansion() {
        let dirs = create_test_directories();
        let mut tree = TreeState::new(dirs);

        // Initially only root level visible
        let visible = tree.visible_directories();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].path, "photos");
        assert_eq!(visible[1].path, "videos");

        // Expand photos
        tree.expanded.insert(1);
        let visible = tree.visible_directories();
        assert_eq!(visible.len(), 3);
        assert_eq!(visible[0].path, "photos");
        assert_eq!(visible[1].path, "photos/vacation");
        assert_eq!(visible[2].path, "videos");
    }

    #[test]
    fn test_tree_has_children() {
        let dirs = create_test_directories();
        let tree = TreeState::new(dirs);

        assert!(tree.has_children(1));  // photos has vacation
        assert!(tree.has_children(2));  // vacation has beach
        assert!(!tree.has_children(3)); // beach has no children
        assert!(!tree.has_children(4)); // videos has no children
    }

    #[test]
    fn test_tree_depth_calculation() {
        let dirs = create_test_directories();
        let tree = TreeState::new(dirs);

        assert_eq!(tree.depth(&tree.directories[0]), 0); // photos
        assert_eq!(tree.depth(&tree.directories[1]), 1); // photos/vacation
        assert_eq!(tree.depth(&tree.directories[2]), 2); // photos/vacation/beach
        assert_eq!(tree.depth(&tree.directories[3]), 0); // videos
    }

    #[test]
    fn test_tree_ancestor_ids() {
        let dirs = create_test_directories();
        let tree = TreeState::new(dirs);

        // beach (id=3) -> vacation (id=2) -> photos (id=1)
        let ancestors: Vec<i64> = tree.ancestor_ids(3).collect();
        assert_eq!(ancestors, vec![2, 1]);

        // vacation (id=2) -> photos (id=1)
        let ancestors: Vec<i64> = tree.ancestor_ids(2).collect();
        assert_eq!(ancestors, vec![1]);

        // photos (id=1) has no ancestors
        let ancestors: Vec<i64> = tree.ancestor_ids(1).collect();
        assert!(ancestors.is_empty());

        // Non-existent ID returns empty
        let ancestors: Vec<i64> = tree.ancestor_ids(999).collect();
        assert!(ancestors.is_empty());
    }

    #[test]
    fn test_tree_filtered_visibility() {
        let dirs = create_test_directories();
        let mut tree = TreeState::new(dirs);
        tree.expanded.insert(1);
        tree.expanded.insert(2);

        // Only show directories 1 and 3 (photos and beach)
        let matching: HashSet<i64> = [1, 3].iter().copied().collect();
        let visible = tree.visible_directories_filtered(&matching);

        assert_eq!(visible.len(), 1); // Only photos visible (beach is under vacation which isn't in filter)
    }

    // ==================== Rating Tests ====================

    #[test]
    fn test_app_state_set_rating_on_directory() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::DirectoryTree;

        state.set_rating(Some(4)).unwrap();

        let dir = state.get_selected_directory().unwrap();
        assert_eq!(dir.rating, Some(4));
    }

    #[test]
    fn test_app_state_set_rating_on_file() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::FileList;

        // First ensure files are loaded
        assert!(!state.file_list.files.is_empty());

        state.set_rating(Some(3)).unwrap();

        let file = state.file_list.selected_file().unwrap();
        assert_eq!(file.file.rating, Some(3));
    }
}
