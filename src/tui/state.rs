use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use ratatui::widgets::{ListState, TableState};
use ratatui_image::protocol::StatefulProtocol;

use crate::db::{Database, Directory, File};

/// Active filter criteria for filtering directories and files
#[derive(Clone, Default)]
pub struct FilterCriteria {
    pub min_rating: Option<i32>,  // None = any rating (including unrated)
    pub tags: Vec<String>,        // Empty = any tags, multiple = AND logic
    pub video_only: bool,         // If true, only show video files
}

impl FilterCriteria {
    pub fn is_active(&self) -> bool {
        self.min_rating.is_some() || !self.tags.is_empty() || self.video_only
    }
}

/// Which element has focus in the filter dialog
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterDialogFocus {
    Rating,
    Tag,
}

/// State for the filter dialog popup
pub struct FilterDialogState {
    pub all_tags: Vec<String>,
    pub rating_selected: Option<i32>,  // 1-5 or None for "any"
    pub selected_tags: Vec<String>,    // Tags already added to filter
    pub tag_input: String,             // Current text input for adding new tag
    pub filtered_tags: Vec<String>,    // Autocomplete suggestions
    pub tag_list_index: usize,
    pub focus: FilterDialogFocus,
    pub video_only: bool,              // Filter to show only videos
}

impl FilterDialogState {
    pub fn new(all_tags: Vec<String>, current_filter: &FilterCriteria) -> Self {
        let filtered_tags = all_tags.clone();
        Self {
            all_tags,
            rating_selected: current_filter.min_rating,
            selected_tags: current_filter.tags.clone(),
            tag_input: String::new(),
            filtered_tags,
            tag_list_index: 0,
            focus: FilterDialogFocus::Rating,
            video_only: current_filter.video_only,
        }
    }

    pub fn update_tag_filter(&mut self) {
        let query = self.tag_input.to_lowercase();
        self.filtered_tags = self
            .all_tags
            .iter()
            .filter(|tag| {
                tag.to_lowercase().contains(&query)
                    && !self.selected_tags.contains(tag)
            })
            .cloned()
            .collect();
        self.tag_list_index = 0;
    }

    pub fn selected_autocomplete_tag(&self) -> Option<&String> {
        self.filtered_tags.get(self.tag_list_index)
    }

    pub fn move_tag_list_up(&mut self) {
        if !self.filtered_tags.is_empty() {
            if self.tag_list_index > 0 {
                self.tag_list_index -= 1;
            } else {
                self.tag_list_index = self.filtered_tags.len() - 1;
            }
        }
    }

    pub fn move_tag_list_down(&mut self) {
        if !self.filtered_tags.is_empty() {
            if self.tag_list_index < self.filtered_tags.len() - 1 {
                self.tag_list_index += 1;
            } else {
                self.tag_list_index = 0;
            }
        }
    }

    pub fn to_criteria(&self) -> FilterCriteria {
        FilterCriteria {
            min_rating: self.rating_selected,
            tags: self.selected_tags.clone(),
            video_only: self.video_only,
        }
    }
}

/// Cached image preview state
pub struct PreviewCache {
    pub path: PathBuf,
    pub protocol: Box<dyn StatefulProtocol>,
}

impl PreviewCache {
    pub fn new(path: PathBuf, protocol: Box<dyn StatefulProtocol>) -> Self {
        Self { path, protocol }
    }
}

/// Which pane has focus
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    DirectoryTree,
    FileList,
}

/// State for the tag input popup
pub struct TagInputState {
    pub input: String,
    pub all_tags: Vec<String>,
    pub filtered_tags: Vec<String>,
    pub selected_index: usize,
}

impl TagInputState {
    pub fn new(all_tags: Vec<String>) -> Self {
        let filtered_tags = all_tags.clone();
        Self {
            input: String::new(),
            all_tags,
            filtered_tags,
            selected_index: 0,
        }
    }

    pub fn update_filter(&mut self) {
        let query = self.input.to_lowercase();
        self.filtered_tags = self
            .all_tags
            .iter()
            .filter(|tag| tag.to_lowercase().contains(&query))
            .cloned()
            .collect();
        self.selected_index = 0;
    }

    pub fn selected_tag(&self) -> Option<&String> {
        self.filtered_tags.get(self.selected_index)
    }

    pub fn move_up(&mut self) {
        if !self.filtered_tags.is_empty() {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            } else {
                self.selected_index = self.filtered_tags.len() - 1; // Wrap to bottom
            }
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered_tags.is_empty() {
            if self.selected_index < self.filtered_tags.len() - 1 {
                self.selected_index += 1;
            } else {
                self.selected_index = 0; // Wrap to top
            }
        }
    }
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

/// State for thumbnail generation confirmation dialog
pub struct ThumbnailConfirmState {
    pub directory_path: String,
    pub file_count: usize,
}

/// Progress tracker for background thumbnail generation
pub struct ThumbnailProgress {
    pub total: usize,
    pub completed: Arc<AtomicUsize>,
    pub done: Arc<AtomicBool>,
}

/// Main application state
pub struct AppState {
    pub library_path: PathBuf,
    pub db: Database,
    pub focus: Focus,
    pub tree: TreeState,
    pub file_list: FileListState,
    pub show_help: bool,
    pub preview_cache: RefCell<Option<PreviewCache>>,
    pub tag_input: Option<TagInputState>,
    pub filter_dialog: Option<FilterDialogState>,
    pub filter: FilterCriteria,
    /// Directory IDs that match the current filter (includes ancestors for tree structure)
    pub matching_dir_ids: HashSet<i64>,
    /// Confirmation dialog for directory thumbnail generation
    pub thumbnail_confirm: Option<ThumbnailConfirmState>,
    /// Status message to show temporarily
    pub status_message: Option<String>,
    /// Background thumbnail generation progress
    pub thumbnail_progress: Option<ThumbnailProgress>,
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
            preview_cache: RefCell::new(None),
            tag_input: None,
            filter_dialog: None,
            filter: FilterCriteria::default(),
            matching_dir_ids: HashSet::new(),
            thumbnail_confirm: None,
            status_message: None,
            thumbnail_progress: None,
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

        if let Some(dir) = self.get_selected_directory() {
            let files = self.db.get_files_in_directory(dir.id)?;
            for file in files {
                let tags = self.db.get_file_tags(file.id)?;

                // Apply filter if active
                if self.filter.is_active() {
                    // Check video filter
                    if self.filter.video_only {
                        if file.media_type.as_deref() != Some("video") {
                            continue;
                        }
                    }
                    // Check rating filter
                    if let Some(min_rating) = self.filter.min_rating {
                        match file.rating {
                            Some(r) if r >= min_rating => {}
                            _ => continue,
                        }
                    }
                    // Check tag filter (AND logic)
                    if !self.filter.tags.is_empty() {
                        let has_all_tags = self.filter.tags.iter().all(|t| tags.contains(t));
                        if !has_all_tags {
                            continue;
                        }
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
                    self.load_files_for_selected_directory()?;
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
                    self.load_files_for_selected_directory()?;
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
                }
            }
        }
        Ok(())
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
    }

    /// Apply the filter from the dialog and close it
    pub fn apply_filter(&mut self) -> Result<()> {
        if let Some(ref dialog) = self.filter_dialog {
            self.filter = dialog.to_criteria();
            self.update_matching_directories()?;
        }
        self.filter_dialog = None;
        // Reload files with filter applied
        self.load_files_for_selected_directory()?;
        Ok(())
    }

    /// Clear the entire filter
    pub fn clear_filter(&mut self) -> Result<()> {
        self.filter = FilterCriteria::default();
        self.matching_dir_ids.clear();
        if let Some(ref mut dialog) = self.filter_dialog {
            dialog.rating_selected = None;
            dialog.selected_tags.clear();
            dialog.video_only = false;
            dialog.update_tag_filter();
        }
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
                self.filter.min_rating,
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
        self.tree.visible_directories_filtered(&self.matching_dir_ids)
    }

    /// Get the currently selected directory considering the filter
    pub fn get_selected_directory(&self) -> Option<&Directory> {
        self.get_visible_directories()
            .get(self.tree.selected_index)
            .copied()
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

    /// Move selection up in filter dialog
    pub fn filter_dialog_up(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Tag {
                dialog.move_tag_list_up();
            }
        }
    }

    /// Move selection down in filter dialog
    pub fn filter_dialog_down(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Tag {
                dialog.move_tag_list_down();
            }
        }
    }

    /// Navigate rating left in filter dialog
    pub fn filter_dialog_left(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Rating {
                dialog.rating_selected = match dialog.rating_selected {
                    None => Some(5),
                    Some(1) => None,
                    Some(n) => Some(n - 1),
                };
            }
        }
    }

    /// Navigate rating right in filter dialog
    pub fn filter_dialog_right(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Rating {
                dialog.rating_selected = match dialog.rating_selected {
                    None => Some(1),
                    Some(5) => None,
                    Some(n) => Some(n + 1),
                };
            }
        }
    }

    /// Toggle focus in filter dialog
    pub fn filter_dialog_toggle_focus(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            dialog.focus = match dialog.focus {
                FilterDialogFocus::Rating => FilterDialogFocus::Tag,
                FilterDialogFocus::Tag => FilterDialogFocus::Rating,
            };
        }
    }

    /// Set rating directly in filter dialog (1-5)
    pub fn filter_dialog_set_rating(&mut self, rating: i32) {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Rating {
                dialog.rating_selected = Some(rating);
            }
        }
    }

    /// Toggle video-only filter in dialog
    pub fn filter_dialog_toggle_video(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            dialog.video_only = !dialog.video_only;
        }
    }

    /// Add selected tag to filter (or apply if in rating focus)
    pub fn filter_dialog_enter(&mut self) -> Result<bool> {
        if let Some(ref mut dialog) = self.filter_dialog {
            if dialog.focus == FilterDialogFocus::Tag {
                // Add selected tag from autocomplete, or input text
                let tag_to_add = dialog
                    .selected_autocomplete_tag()
                    .cloned()
                    .or_else(|| {
                        let input = dialog.tag_input.trim().to_string();
                        if input.is_empty() { None } else { Some(input) }
                    });

                if let Some(tag) = tag_to_add {
                    if !dialog.selected_tags.contains(&tag) {
                        dialog.selected_tags.push(tag);
                        dialog.selected_tags.sort();
                    }
                    dialog.tag_input.clear();
                    dialog.update_tag_filter();
                    return Ok(false); // Don't close dialog
                }
            }
            // Apply filter and close
            return Ok(true);
        }
        Ok(false)
    }

    // ==================== Thumbnail Methods ====================

    /// Handle Shift+T: generate thumbnail for file or show confirm for directory
    pub fn trigger_thumbnail_generation(&mut self) {
        use crate::tui::widgets::{generate_image_thumbnail, generate_video_thumbnail, is_image_file, is_video_file};

        match self.focus {
            Focus::FileList => {
                // Generate thumbnail for selected file
                if let Some(path) = self.selected_file_path() {
                    if is_image_file(&path) {
                        if generate_image_thumbnail(&path).is_some() {
                            self.status_message = Some("Thumbnail generated".to_string());
                            // Clear preview cache to reload
                            *self.preview_cache.borrow_mut() = None;
                        } else {
                            self.status_message = Some("Failed to generate thumbnail".to_string());
                        }
                    } else if is_video_file(&path) {
                        if generate_video_thumbnail(&path).is_some() {
                            self.status_message = Some("Thumbnail generated".to_string());
                            *self.preview_cache.borrow_mut() = None;
                        } else {
                            self.status_message = Some("Failed to generate thumbnail".to_string());
                        }
                    }
                }
            }
            Focus::DirectoryTree => {
                // Show confirmation dialog for directory
                if let Some(dir) = self.get_selected_directory() {
                    let file_count = self.file_list.files.len();
                    self.thumbnail_confirm = Some(ThumbnailConfirmState {
                        directory_path: dir.path.clone(),
                        file_count,
                    });
                }
            }
        }
    }

    /// Confirm and generate thumbnails for all files in current directory
    pub fn confirm_thumbnail_generation(&mut self) {
        use crate::tui::widgets::{is_image_file, is_video_file};

        self.thumbnail_confirm = None;

        // Collect paths first (need owned data for parallel processing)
        let dir_path = self.get_selected_directory().map(|d| d.path.clone());
        let paths: Vec<PathBuf> = self.file_list.files.iter()
            .filter_map(|file_with_tags| {
                let dir = dir_path.as_ref()?;
                let path = if dir.is_empty() {
                    self.library_path.join(&file_with_tags.file.filename)
                } else {
                    self.library_path.join(dir).join(&file_with_tags.file.filename)
                };
                // Only include image/video files
                if is_image_file(&path) || is_video_file(&path) {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        let total = paths.len();
        if total == 0 {
            self.status_message = Some("No media files to process".to_string());
            return;
        }

        // Set up progress tracking
        let completed = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));

        self.thumbnail_progress = Some(ThumbnailProgress {
            total,
            completed: Arc::clone(&completed),
            done: Arc::clone(&done),
        });

        // Spawn background thread for parallel generation
        std::thread::spawn(move || {
            use crate::tui::widgets::{generate_image_thumbnail, generate_video_thumbnail, is_image_file, is_video_file};
            use rayon::prelude::*;

            paths.par_iter().for_each(|path| {
                if is_image_file(path) {
                    generate_image_thumbnail(path);
                } else if is_video_file(path) {
                    generate_video_thumbnail(path);
                }
                completed.fetch_add(1, Ordering::Relaxed);
            });

            done.store(true, Ordering::Relaxed);
        });
    }

    /// Check and update thumbnail generation progress
    pub fn update_thumbnail_progress(&mut self) {
        if let Some(ref progress) = self.thumbnail_progress {
            if progress.done.load(Ordering::Relaxed) {
                let completed = progress.completed.load(Ordering::Relaxed);
                self.status_message = Some(format!("Generated {} thumbnails", completed));
                self.thumbnail_progress = None;
                // Clear preview cache to reload
                *self.preview_cache.borrow_mut() = None;
            }
        }
    }

    /// Cancel thumbnail generation dialog
    pub fn cancel_thumbnail_generation(&mut self) {
        self.thumbnail_confirm = None;
    }

    /// Clear status message
    pub fn clear_status_message(&mut self) {
        self.status_message = None;
    }
}
