use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use ratatui::layout::Rect;
use ratatui::widgets::{ListState, TableState};

use crate::db::{Database, Directory, File};
use crate::suggestions::extract_suggested_words;
use crate::tui::preview_loader::PreviewLoader;

// Re-export dialog types so existing `use crate::tui::state::X` paths keep working
pub use super::dialogs::{
    FilterCriteria, FilterDialogFocus, FilterDialogState, OperationsMenuState, RatingFilter,
    RenameDialogState, TagInputState,
};
pub use super::operations::{BackgroundProgress, OperationType};
pub use super::preview_cache::{DirectoryPreviewCache, LruPreviewCache};

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

/// Default LRU cache size (~200 decoded images, ~1GB memory)
const DEFAULT_PREVIEW_CACHE_SIZE: usize = 200;

/// Main application state
pub struct AppState {
    pub library_path: PathBuf,
    pub db: Database,
    pub focus: Focus,
    pub tree: TreeState,
    pub file_list: FileListState,
    pub show_help: bool,
    pub preview_cache: RefCell<LruPreviewCache>,
    pub directory_preview_cache: RefCell<Option<DirectoryPreviewCache>>,
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
            directory_preview_cache: RefCell::new(None),
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
        };

        // Load files for initial selection
        state.load_files_for_selected_directory()?;

        Ok(state)
    }

    /// Load files for the currently selected directory, applying current filter
    fn load_files_for_selected_directory(&mut self) -> Result<()> {
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

                // Apply filter if active AND no ancestor matches the full filter
                if self.filter.is_active() && !ancestor_matches_filter {
                    // Check video filter (always applies even if ancestor matches)
                    if self.filter.video_only {
                        if file.media_type.as_deref() != Some("video") {
                            continue;
                        }
                    }
                    // Check rating filter
                    match self.filter.rating {
                        RatingFilter::Any => {}
                        RatingFilter::Unrated => {
                            if file.rating.is_some() {
                                continue;
                            }
                        }
                        RatingFilter::MinRating(min) => {
                            match file.rating {
                                Some(r) if r >= min => {}
                                _ => continue,
                            }
                        }
                    }
                    // Check tag filter (AND logic) - include directory tags
                    if !self.filter.tags.is_empty() {
                        let has_all_tags = self.filter.tags.iter().all(|t| {
                            tags.contains(t) || dir_tags.contains(t)
                        });
                        if !has_all_tags {
                            continue;
                        }
                    }
                } else if self.filter.video_only {
                    // Even when ancestor matches, video_only filter still applies
                    if file.media_type.as_deref() != Some("video") {
                        continue;
                    }
                }

                self.file_list.files.push(FileWithTags { file, tags });
            }
        }

        Ok(())
    }

    pub fn move_down(&mut self) -> Result<()> {
        match self.focus {
            Focus::DirectoryTree => {
                let visible_count = self.get_visible_directories().len();
                if visible_count > 0 {
                    if self.tree.selected_index < visible_count - 1 {
                        self.tree.selected_index += 1;
                    } else {
                        self.tree.selected_index = 0; // Wrap to top
                    }
                    self.tree.list_state.select(Some(self.tree.selected_index));
                    // Defer file loading until after rapid navigation stops
                    self.files_dirty = true;
                }
            }
            Focus::FileList => {
                if !self.file_list.files.is_empty() {
                    if self.file_list.selected_index < self.file_list.files.len() - 1 {
                        self.file_list.selected_index += 1;
                    } else {
                        self.file_list.selected_index = 0; // Wrap to top
                    }
                    self.file_list.table_state.select(Some(self.file_list.selected_index));
                    // Skip loading new preview during rapid navigation
                    self.skip_preview = true;
                }
            }
        }
        Ok(())
    }

    pub fn move_up(&mut self) -> Result<()> {
        match self.focus {
            Focus::DirectoryTree => {
                let visible_count = self.get_visible_directories().len();
                if visible_count > 0 {
                    if self.tree.selected_index > 0 {
                        self.tree.selected_index -= 1;
                    } else {
                        self.tree.selected_index = visible_count - 1; // Wrap to bottom
                    }
                    self.tree.list_state.select(Some(self.tree.selected_index));
                    // Defer file loading until after rapid navigation stops
                    self.files_dirty = true;
                }
            }
            Focus::FileList => {
                if !self.file_list.files.is_empty() {
                    if self.file_list.selected_index > 0 {
                        self.file_list.selected_index -= 1;
                    } else {
                        self.file_list.selected_index = self.file_list.files.len() - 1; // Wrap to bottom
                    }
                    self.file_list.table_state.select(Some(self.file_list.selected_index));
                    // Skip loading new preview during rapid navigation
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

    /// Poll for completed background preview loads and insert into cache.
    /// Called at the start of each render cycle.
    /// Also preloads other files in the current directory.
    pub fn poll_preview_results(&self) {
        let results = self.preview_loader.borrow_mut().poll_results();

        for result in results {
            if let Some(protocol) = result.protocol {
                self.preview_cache.borrow_mut().insert(result.path, protocol);
            }
        }

        // Preload other files in the directory (runs on every poll, not just after loads,
        // so that entering a new directory triggers preloading immediately)
        self.preload_directory_files();
    }

    /// Preload all image/video files in the current directory that aren't already cached or pending.
    fn preload_directory_files(&self) {
        use crate::thumbnails::get_preview_path_for_file;

        let dir_id = match self.current_dir_id {
            Some(id) => id,
            None => return,
        };

        let dir = match self.get_selected_directory() {
            Some(d) => d.clone(),
            None => return,
        };

        let mut loader = self.preview_loader.borrow_mut();
        let cache = self.preview_cache.borrow();

        for file_with_tags in &self.file_list.files {
            let file_path = if dir.path.is_empty() {
                self.library_path.join(&file_with_tags.file.filename)
            } else {
                self.library_path.join(&dir.path).join(&file_with_tags.file.filename)
            };

            // Skip if already cached or pending
            if cache.contains(&file_path) || loader.is_pending(&file_path) {
                continue;
            }

            // Determine preview path (thumbnail or original)
            // Skip if no preview available (non-image/video file or missing video thumbnail)
            let (preview_path, is_thumbnail) = match get_preview_path_for_file(&file_path) {
                Some(result) => result,
                None => continue,
            };

            loader.queue_load(file_path, preview_path, is_thumbnail, dir_id);
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
                        self.focus = Focus::FileList;
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

    /// Get the full path to the currently selected file
    pub fn selected_file_path(&self) -> Option<PathBuf> {
        let file_with_tags = self.file_list.selected_file()?;
        let dir = self.get_selected_directory()?;

        let relative_path = if dir.path.is_empty() {
            PathBuf::from(&file_with_tags.file.filename)
        } else {
            PathBuf::from(&dir.path).join(&file_with_tags.file.filename)
        };

        Some(self.library_path.join(relative_path))
    }

    /// Open the tag input popup
    pub fn open_tag_input(&mut self) -> Result<()> {
        let all_tags = self.db.get_all_tags()?;
        self.tag_input = Some(TagInputState::new(all_tags));
        Ok(())
    }

    /// Close the tag input popup without applying
    pub fn close_tag_input(&mut self) {
        self.tag_input = None;
        self.force_redraw = true;
    }

    /// Apply the selected or entered tag
    pub fn apply_tag(&mut self) -> Result<()> {
        let tag = if let Some(ref input) = self.tag_input {
            // Use selected tag if available, otherwise use input text
            input
                .selected_tag()
                .cloned()
                .unwrap_or_else(|| input.input.clone())
        } else {
            return Ok(());
        };

        if tag.is_empty() {
            self.tag_input = None;
            return Ok(());
        }

        match self.focus {
            Focus::DirectoryTree => {
                if let Some(dir) = self.get_selected_directory() {
                    self.db.add_directory_tag(dir.id, &tag)?;
                }
            }
            Focus::FileList => {
                if let Some(file_with_tags) =
                    self.file_list.files.get_mut(self.file_list.selected_index)
                {
                    self.db.add_file_tag(file_with_tags.file.id, &tag)?;
                    if !file_with_tags.tags.contains(&tag) {
                        file_with_tags.tags.push(tag);
                        file_with_tags.tags.sort();
                    }
                }
            }
        }

        self.tag_input = None;
        self.force_redraw = true;
        Ok(())
    }

    /// Handle character input for tag popup
    pub fn tag_input_char(&mut self, c: char) {
        if let Some(ref mut input) = self.tag_input {
            input.input.push(c);
            input.update_filter();
        }
    }

    /// Handle backspace for tag popup
    pub fn tag_input_backspace(&mut self) {
        if let Some(ref mut input) = self.tag_input {
            input.input.pop();
            input.update_filter();
        }
    }

    /// Move selection up in tag popup
    pub fn tag_input_up(&mut self) {
        if let Some(ref mut input) = self.tag_input {
            input.move_up();
        }
    }

    /// Move selection down in tag popup
    pub fn tag_input_down(&mut self) {
        if let Some(ref mut input) = self.tag_input {
            input.move_down();
        }
    }

    // ==================== Filter Methods ====================

    /// Open the filter dialog
    pub fn open_filter_dialog(&mut self) -> Result<()> {
        let all_tags = self.db.get_all_tags()?;
        self.filter_dialog = Some(FilterDialogState::new(all_tags, &self.filter));
        Ok(())
    }

    /// Close the filter dialog without applying
    pub fn close_filter_dialog(&mut self) {
        self.filter_dialog = None;
        self.force_redraw = true;
    }

    /// Apply the filter from the dialog and close it
    pub fn apply_filter(&mut self) -> Result<()> {
        if let Some(ref dialog) = self.filter_dialog {
            self.filter = dialog.to_criteria();
            self.update_matching_directories()?;
        }
        self.filter_dialog = None;
        self.force_redraw = true;
        // Reload files with filter applied
        self.load_files_for_selected_directory()?;
        Ok(())
    }

    /// Clear the entire filter
    pub fn clear_filter(&mut self) -> Result<()> {
        self.filter = FilterCriteria::default();
        self.matching_dir_ids.clear();
        if let Some(ref mut dialog) = self.filter_dialog {
            dialog.rating_filter = RatingFilter::Any;
            dialog.selected_tags.clear();
            dialog.video_only = false;
            dialog.update_tag_filter();
        }
        self.force_redraw = true;
        // Reload files without filter
        self.load_files_for_selected_directory()?;
        Ok(())
    }

    /// Update the set of matching directory IDs based on current filter
    fn update_matching_directories(&mut self) -> Result<()> {
        if self.filter.is_active() {
            // Store the currently selected directory ID before updating
            let current_dir_id = self.get_selected_directory().map(|d| d.id);

            self.matching_dir_ids = self.db.get_directories_with_matching_files(
                self.filter.rating,
                &self.filter.tags,
                self.filter.video_only,
            )?;

            // Reset selection if current directory is not visible
            if let Some(dir_id) = current_dir_id {
                let visible = self.get_visible_directories();
                if !visible.iter().any(|d| d.id == dir_id) {
                    self.tree.selected_index = 0;
                    self.tree.list_state.select(Some(0));
                }
            }
        } else {
            self.matching_dir_ids.clear();
        }
        Ok(())
    }

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

    /// Check if a directory or any of its ancestors matches the full filter criteria
    /// (both rating and tags, when both are active)
    fn directory_or_ancestor_matches_filter(&self, dir_id: i64) -> Result<bool> {
        let mut current_id = Some(dir_id);

        while let Some(id) = current_id {
            // Find the directory
            let dir = self.tree.directories.iter().find(|d| d.id == id);

            if let Some(dir) = dir {
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

                // Directory matches if it passes both filters
                if dir_matches_rating && dir_matches_tags {
                    return Ok(true);
                }

                // Move to parent directory
                current_id = dir.parent_id;
            } else {
                break;
            }
        }

        Ok(false)
    }

    /// Get all tags from a directory and all its ancestors
    fn get_directory_and_ancestor_tags(&self, dir_id: i64) -> Result<Vec<String>> {
        let mut all_tags = Vec::new();
        let mut current_id = Some(dir_id);

        while let Some(id) = current_id {
            // Get tags for this directory
            let tags = self.db.get_directory_tags(id)?;
            all_tags.extend(tags);

            // Find parent
            current_id = self
                .tree
                .directories
                .iter()
                .find(|d| d.id == id)
                .and_then(|d| d.parent_id);
        }

        Ok(all_tags)
    }

    /// Handle character input for filter dialog
    pub fn filter_dialog_char(&mut self, c: char) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Tag {
                dialog.tag_input.push(c);
                dialog.update_tag_filter();
            }
        }
    }

    /// Handle backspace for filter dialog
    pub fn filter_dialog_backspace(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Tag {
                if dialog.tag_input.is_empty() {
                    // Remove last selected tag if input is empty
                    dialog.selected_tags.pop();
                    dialog.update_tag_filter();
                } else {
                    dialog.tag_input.pop();
                    dialog.update_tag_filter();
                }
            }
        }
    }

    /// Move selection up in filter dialog (crosses sections at boundaries)
    pub fn filter_dialog_up(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            match dialog.focus {
                FilterDialogFocus::Tag => {
                    // If at top of tag list, move to VideoOnly section
                    if !dialog.move_tag_list_up() {
                        dialog.focus = FilterDialogFocus::VideoOnly;
                    }
                }
                FilterDialogFocus::VideoOnly => {
                    dialog.focus = FilterDialogFocus::Rating;
                }
                FilterDialogFocus::Rating => {
                    // Stay at top (Rating section)
                }
            }
        }
    }

    /// Move selection down in filter dialog (crosses sections at boundaries)
    pub fn filter_dialog_down(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            match dialog.focus {
                FilterDialogFocus::Rating => {
                    dialog.focus = FilterDialogFocus::VideoOnly;
                }
                FilterDialogFocus::VideoOnly => {
                    dialog.focus = FilterDialogFocus::Tag;
                }
                FilterDialogFocus::Tag => {
                    // If at bottom of tag list, stay there
                    dialog.move_tag_list_down();
                }
            }
        }
    }

    /// Navigate rating left in filter dialog
    /// Navigate rating left in filter dialog (order: Any <- Unrated <- 1 <- 2 <- 3 <- 4 <- 5)
    pub fn filter_dialog_left(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Rating {
                dialog.rating_filter = match dialog.rating_filter {
                    RatingFilter::Any => RatingFilter::MinRating(5),
                    RatingFilter::Unrated => RatingFilter::Any,
                    RatingFilter::MinRating(1) => RatingFilter::Unrated,
                    RatingFilter::MinRating(n) => RatingFilter::MinRating(n - 1),
                };
            }
        }
    }

    /// Navigate rating right in filter dialog (order: Any -> Unrated -> 1 -> 2 -> 3 -> 4 -> 5)
    pub fn filter_dialog_right(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Rating {
                dialog.rating_filter = match dialog.rating_filter {
                    RatingFilter::Any => RatingFilter::Unrated,
                    RatingFilter::Unrated => RatingFilter::MinRating(1),
                    RatingFilter::MinRating(5) => RatingFilter::Any,
                    RatingFilter::MinRating(n) => RatingFilter::MinRating(n + 1),
                };
            }
        }
    }

    /// Move focus down in filter dialog
    pub fn filter_dialog_focus_down(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            dialog.tag_editing = false;
            dialog.focus = match dialog.focus {
                FilterDialogFocus::Rating => FilterDialogFocus::VideoOnly,
                FilterDialogFocus::VideoOnly => FilterDialogFocus::Tag,
                FilterDialogFocus::Tag => FilterDialogFocus::Rating,
            };
            // Enter tag section at the input line (top)
            if dialog.focus == FilterDialogFocus::Tag {
                dialog.tag_input_selected = true;
            }
        }
    }

    /// Move focus up in filter dialog
    pub fn filter_dialog_focus_up(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            dialog.tag_editing = false;
            dialog.focus = match dialog.focus {
                FilterDialogFocus::Rating => FilterDialogFocus::Tag,
                FilterDialogFocus::VideoOnly => FilterDialogFocus::Rating,
                FilterDialogFocus::Tag => FilterDialogFocus::VideoOnly,
            };
            // Enter tag section at the input line (top)
            if dialog.focus == FilterDialogFocus::Tag {
                dialog.tag_input_selected = true;
            }
        }
    }

    /// Get current focus in filter dialog
    pub fn filter_dialog_focus(&self) -> Option<FilterDialogFocus> {
        self.filter_dialog.as_ref().map(|d| d.focus)
    }

    /// Set rating directly in filter dialog (1-5)
    pub fn filter_dialog_set_rating(&mut self, rating: i32) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Rating {
                dialog.rating_filter = RatingFilter::MinRating(rating);
            }
        }
    }

    /// Toggle video-only filter in dialog
    pub fn filter_dialog_toggle_video(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            dialog.video_only = !dialog.video_only;
        }
    }

    /// Set unrated filter in dialog
    pub fn filter_dialog_set_unrated(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Rating {
                dialog.rating_filter = RatingFilter::Unrated;
            }
        }
    }

    /// Add the highlighted tag from the autocomplete list to the filter
    pub fn filter_dialog_add_tag(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if let Some(tag) = dialog.selected_autocomplete_tag().cloned() {
                if !dialog.selected_tags.contains(&tag) {
                    dialog.selected_tags.push(tag);
                    dialog.selected_tags.sort();
                }
                dialog.tag_input.clear();
                dialog.tag_editing = false;
                dialog.tag_input_selected = true;
                dialog.update_tag_filter();
            }
        }
    }

    /// Auto-apply filter changes without closing the dialog
    pub fn auto_apply_filter(&mut self) -> Result<()> {
        if let Some(ref dialog) = self.filter_dialog {
            self.filter = dialog.to_criteria();
            self.update_matching_directories()?;
        }
        // Reload files with filter applied
        self.load_files_for_selected_directory()?;
        Ok(())
    }

    // ==================== Rename Dialog Methods ====================

    /// Open the rename dialog for the selected directory
    pub fn open_rename_dialog(&mut self) -> Result<()> {
        if self.focus != Focus::DirectoryTree {
            return Ok(());
        }

        if let Some(dir) = self.get_selected_directory().cloned() {
            // Collect all descendant directory IDs
            let mut dir_ids = vec![dir.id];
            self.collect_descendant_dir_ids(dir.id, &mut dir_ids);

            // Build a combined path string from all subdirectory names
            let mut all_paths = String::new();
            for d in &self.tree.directories {
                if dir_ids.contains(&d.id) {
                    all_paths.push_str(&d.path);
                    all_paths.push('/');
                }
            }

            // Collect tags from files in all these directories
            let mut file_tags: Vec<String> = Vec::new();
            for did in &dir_ids {
                if let Ok(files) = self.db.get_files_in_directory(*did) {
                    for f in &files {
                        if let Ok(tags) = self.db.get_file_tags(f.id) {
                            file_tags.extend(tags);
                        }
                    }
                }
            }

            let suggested_words = extract_suggested_words(&all_paths, &file_tags);
            self.rename_dialog = Some(RenameDialogState::new(dir.id, dir.path.clone(), suggested_words));
        }
        Ok(())
    }

    /// Close the rename dialog without applying
    pub fn close_rename_dialog(&mut self) {
        self.rename_dialog = None;
        self.force_redraw = true;
    }

    /// Apply the rename and close the dialog
    pub fn apply_rename(&mut self) -> Result<()> {
        let (dir_id, old_path, new_name) = if let Some(ref dialog) = self.rename_dialog {
            let new_name = dialog.new_name.trim().to_string();
            if new_name.is_empty() {
                self.rename_dialog = None;
                return Ok(());
            }
            (dialog.dir_id, dialog.original_path.clone(), new_name)
        } else {
            return Ok(());
        };

        // Build new path: replace the last component
        let new_path = if let Some(parent) = old_path.rsplit_once('/') {
            format!("{}/{}", parent.0, new_name)
        } else {
            new_name.clone()
        };

        // Don't rename if nothing changed
        if old_path == new_path {
            self.rename_dialog = None;
            return Ok(());
        }

        // Perform filesystem rename
        let old_fs_path = self.library_path.join(&old_path);
        let new_fs_path = self.library_path.join(&new_path);

        if new_fs_path.exists() {
            self.status_message = Some(format!("Error: '{}' already exists", new_name));
            self.rename_dialog = None;
            return Ok(());
        }

        std::fs::rename(&old_fs_path, &new_fs_path)?;

        // Update database: this directory and all descendants
        self.db.rename_directory(dir_id, &old_path, &new_path)?;

        // Update in-memory state
        for dir in &mut self.tree.directories {
            if dir.id == dir_id {
                dir.path = new_path.clone();
            } else if dir.path.starts_with(&format!("{}/", old_path)) {
                dir.path = dir.path.replacen(&old_path, &new_path, 1);
            }
        }

        self.status_message = Some(format!("Renamed to '{}'", new_name));
        self.rename_dialog = None;

        // Invalidate caches
        self.preview_cache.borrow_mut().clear();
        *self.directory_preview_cache.borrow_mut() = None;

        Ok(())
    }

    /// Clear status message
    pub fn clear_status_message(&mut self) {
        self.status_message = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== TreeState Tests ====================

    fn create_test_directories() -> Vec<Directory> {
        vec![
            Directory { id: 1, path: "photos".to_string(), parent_id: None, rating: None, mtime: Some(0) },
            Directory { id: 2, path: "photos/vacation".to_string(), parent_id: Some(1), rating: None, mtime: Some(0) },
            Directory { id: 3, path: "photos/vacation/beach".to_string(), parent_id: Some(2), rating: None, mtime: Some(0) },
            Directory { id: 4, path: "videos".to_string(), parent_id: None, rating: Some(5), mtime: Some(0) },
        ]
    }

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

    // ==================== AppState Navigation Tests (require in-memory DB) ====================

    fn create_test_app_state() -> (AppState, tempfile::TempDir) {
        use tempfile::TempDir;
        use std::fs;

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

    #[test]
    fn test_app_state_move_down_wraps() {
        // Keep _tempdir alive - dropping it deletes the test files
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
        // Keep _tempdir alive - dropping it deletes the test files
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
        // Keep _tempdir alive - dropping it deletes the test files
let (mut state, _tempdir) = create_test_app_state();

        assert_eq!(state.focus, Focus::DirectoryTree);
        state.toggle_focus();
        assert_eq!(state.focus, Focus::FileList);
        state.toggle_focus();
        assert_eq!(state.focus, Focus::DirectoryTree);
    }

    #[test]
    fn test_app_state_set_rating_on_directory() {
        // Keep _tempdir alive - dropping it deletes the test files
let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::DirectoryTree;

        state.set_rating(Some(4)).unwrap();

        let dir = state.get_selected_directory().unwrap();
        assert_eq!(dir.rating, Some(4));
    }

    #[test]
    fn test_app_state_set_rating_on_file() {
        // Keep _tempdir alive - dropping it deletes the test files
let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::FileList;

        // First ensure files are loaded
        assert!(!state.file_list.files.is_empty());

        state.set_rating(Some(3)).unwrap();

        let file = state.file_list.selected_file().unwrap();
        assert_eq!(file.file.rating, Some(3));
    }

    #[test]
    fn test_filter_by_file_tag_and_directory_tag() {
        // Bug: filtering by tag1 (on file) AND tag2 (on directory) should show the file
        use tempfile::TempDir;
        use std::fs;

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
        use tempfile::TempDir;
        use std::fs;

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
