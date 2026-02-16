use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use ratatui::widgets::{ListState, TableState};
use ratatui_image::protocol::StatefulProtocol;

use crate::db::{Database, Directory, File};
use crate::scanner::detect_orientation;
use crate::suggestions::extract_suggested_words;

/// Rating filter options
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RatingFilter {
    #[default]
    Any,              // No rating filter
    Unrated,          // Only unrated items
    MinRating(i32),   // Minimum rating (1-5)
}

/// Active filter criteria for filtering directories and files
#[derive(Clone, Default)]
pub struct FilterCriteria {
    pub rating: RatingFilter,
    pub tags: Vec<String>,        // Empty = any tags, multiple = AND logic
    pub video_only: bool,         // If true, only show video files
}

impl FilterCriteria {
    pub fn is_active(&self) -> bool {
        self.rating != RatingFilter::Any || !self.tags.is_empty() || self.video_only
    }
}

/// Which element has focus in the filter dialog
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterDialogFocus {
    Rating,
    VideoOnly,
    Tag,
}

/// State for the rename dialog popup
pub struct RenameDialogState {
    pub dir_id: i64,
    pub original_path: String,
    pub new_name: String,
    pub cursor_pos: usize,
    pub suggested_words: Vec<String>,
    pub selected_suggestion: usize,
    pub scroll_offset: usize,
}

impl RenameDialogState {
    pub fn new(dir_id: i64, original_path: String, suggested_words: Vec<String>) -> Self {
        // Extract just the directory name from the path
        let name = original_path
            .rsplit('/')
            .next()
            .unwrap_or(&original_path)
            .to_string();
        let cursor_pos = name.len();
        Self {
            dir_id,
            original_path,
            new_name: name,
            cursor_pos,
            suggested_words,
            selected_suggestion: 0,
            scroll_offset: 0,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.new_name.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            // Find the previous character boundary
            let mut new_pos = self.cursor_pos - 1;
            while new_pos > 0 && !self.new_name.is_char_boundary(new_pos) {
                new_pos -= 1;
            }
            self.new_name.remove(new_pos);
            self.cursor_pos = new_pos;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.new_name.len() {
            self.new_name.remove(self.cursor_pos);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let mut new_pos = self.cursor_pos - 1;
            while new_pos > 0 && !self.new_name.is_char_boundary(new_pos) {
                new_pos -= 1;
            }
            self.cursor_pos = new_pos;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.new_name.len() {
            let mut new_pos = self.cursor_pos + 1;
            while new_pos < self.new_name.len() && !self.new_name.is_char_boundary(new_pos) {
                new_pos += 1;
            }
            self.cursor_pos = new_pos;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.new_name.len();
    }

    pub fn select_prev_suggestion(&mut self, visible_count: usize) {
        if !self.suggested_words.is_empty() {
            if self.selected_suggestion > 0 {
                self.selected_suggestion -= 1;
            } else {
                self.selected_suggestion = self.suggested_words.len() - 1;
            }
            self.adjust_scroll(visible_count);
        }
    }

    pub fn select_next_suggestion(&mut self, visible_count: usize) {
        if !self.suggested_words.is_empty() {
            if self.selected_suggestion < self.suggested_words.len() - 1 {
                self.selected_suggestion += 1;
            } else {
                self.selected_suggestion = 0;
            }
            self.adjust_scroll(visible_count);
        }
    }

    fn adjust_scroll(&mut self, visible_count: usize) {
        if visible_count == 0 {
            return;
        }
        // Scroll up if selection is above visible area
        if self.selected_suggestion < self.scroll_offset {
            self.scroll_offset = self.selected_suggestion;
        }
        // Scroll down if selection is below visible area
        if self.selected_suggestion >= self.scroll_offset + visible_count {
            self.scroll_offset = self.selected_suggestion - visible_count + 1;
        }
    }

    pub fn use_suggestion(&mut self) {
        if let Some(word) = self.suggested_words.get(self.selected_suggestion) {
            self.new_name = word.clone();
            self.cursor_pos = self.new_name.len();
        }
    }

    pub fn append_suggestion(&mut self) {
        if let Some(word) = self.suggested_words.get(self.selected_suggestion) {
            self.new_name.push_str(word);
            self.cursor_pos = self.new_name.len();
        }
    }
}

/// State for the filter dialog popup
pub struct FilterDialogState {
    pub all_tags: Vec<String>,
    pub rating_filter: RatingFilter,
    pub selected_tags: Vec<String>,    // Tags already added to filter
    pub tag_input: String,             // Current text input for adding new tag
    pub filtered_tags: Vec<String>,    // Autocomplete suggestions
    pub tag_list_index: usize,
    pub tag_scroll_offset: usize,      // Scroll offset for tag list
    pub focus: FilterDialogFocus,
    pub video_only: bool,              // Filter to show only videos
}

impl FilterDialogState {
    pub fn new(all_tags: Vec<String>, current_filter: &FilterCriteria) -> Self {
        let filtered_tags = all_tags.clone();
        Self {
            all_tags,
            rating_filter: current_filter.rating,
            selected_tags: current_filter.tags.clone(),
            tag_input: String::new(),
            filtered_tags,
            tag_list_index: 0,
            tag_scroll_offset: 0,
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
        self.tag_scroll_offset = 0;
    }

    pub fn selected_autocomplete_tag(&self) -> Option<&String> {
        self.filtered_tags.get(self.tag_list_index)
    }

    /// Move up in tag list. Returns true if moved, false if already at top.
    pub fn move_tag_list_up(&mut self) -> bool {
        if !self.filtered_tags.is_empty() && self.tag_list_index > 0 {
            self.tag_list_index -= 1;
            // Ensure selection is visible (scroll up if needed)
            if self.tag_list_index < self.tag_scroll_offset {
                self.tag_scroll_offset = self.tag_list_index;
            }
            true
        } else {
            false
        }
    }

    /// Move down in tag list. Returns true if moved, false if already at bottom.
    pub fn move_tag_list_down(&mut self) -> bool {
        if !self.filtered_tags.is_empty()
            && self.tag_list_index < self.filtered_tags.len() - 1
        {
            self.tag_list_index += 1;
            // Scroll down if selection goes below visible area (assume ~5 visible items)
            let visible_height = 5;
            if self.tag_list_index >= self.tag_scroll_offset + visible_height {
                self.tag_scroll_offset = self.tag_list_index - visible_height + 1;
            }
            true
        } else {
            false
        }
    }

    /// Adjust scroll offset to keep selection visible within given height
    pub fn adjust_tag_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        // Ensure selection is visible
        if self.tag_list_index < self.tag_scroll_offset {
            self.tag_scroll_offset = self.tag_list_index;
        } else if self.tag_list_index >= self.tag_scroll_offset + visible_height {
            self.tag_scroll_offset = self.tag_list_index - visible_height + 1;
        }
    }

    pub fn to_criteria(&self) -> FilterCriteria {
        FilterCriteria {
            rating: self.rating_filter,
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

/// Cached directory preview state (2x2 composite image)
pub struct DirectoryPreviewCache {
    pub dir_id: i64,
    pub protocol: Box<dyn StatefulProtocol>,
}

impl DirectoryPreviewCache {
    pub fn new(dir_id: i64, protocol: Box<dyn StatefulProtocol>) -> Self {
        Self { dir_id, protocol }
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

/// Types of background operations
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    Thumbnails,
    Orientation,
    Hash,
    DirPreview,
    DirPreviewRecursive,
}

impl OperationType {
    pub fn label(&self) -> &'static str {
        match self {
            OperationType::Thumbnails => "Generating thumbnails",
            OperationType::Orientation => "Tagging orientation",
            OperationType::Hash => "Computing hashes",
            OperationType::DirPreview => "Generating dir preview",
            OperationType::DirPreviewRecursive => "Generating dir previews",
        }
    }

    pub fn done_label(&self) -> &'static str {
        match self {
            OperationType::Thumbnails => "thumbnails generated",
            OperationType::Orientation => "files tagged",
            OperationType::Hash => "files hashed",
            OperationType::DirPreview => "dir preview generated",
            OperationType::DirPreviewRecursive => "dir previews generated",
        }
    }
}

/// State for operations menu popup
pub struct OperationsMenuState {
    pub directory_path: String,
    pub file_count: usize,
    pub selected: usize,
}

/// Progress tracker for background operations
pub struct BackgroundProgress {
    pub operation: OperationType,
    pub total: usize,
    pub completed: Arc<AtomicUsize>,
    pub done: Arc<AtomicBool>,
    pub cancelled: Arc<AtomicBool>,
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
    /// Directory IDs where files are missing thumbnails (checked in background)
    pub dirs_missing_file_thumbnails: Arc<Mutex<HashSet<i64>>>,
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
            directory_preview_cache: RefCell::new(None),
            tag_input: None,
            filter_dialog: None,
            rename_dialog: None,
            filter: FilterCriteria::default(),
            matching_dir_ids: HashSet::new(),
            operations_menu: None,
            status_message: None,
            background_progress: None,
            dirs_missing_file_thumbnails: Arc::new(Mutex::new(HashSet::new())),
        };

        // Load files for initial selection
        state.load_files_for_selected_directory()?;

        // Start background check for missing file thumbnails
        state.start_thumbnail_check();

        Ok(state)
    }

    /// Load files for the currently selected directory, applying current filter
    fn load_files_for_selected_directory(&mut self) -> Result<()> {
        self.file_list.files.clear();
        self.file_list.selected_index = 0;
        self.file_list.table_state.select(Some(0));

        if let Some(dir) = self.get_selected_directory() {
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

            for file in files {
                let tags = self.db.get_file_tags(file.id)?;

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
        // Invalidate preview cache to force full redraw
        *self.preview_cache.borrow_mut() = None;
        *self.directory_preview_cache.borrow_mut() = None;
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
        // Invalidate preview cache to force full redraw
        *self.preview_cache.borrow_mut() = None;
        *self.directory_preview_cache.borrow_mut() = None;
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
        // Invalidate preview cache to force full redraw
        *self.preview_cache.borrow_mut() = None;
        *self.directory_preview_cache.borrow_mut() = None;
    }

    /// Apply the filter from the dialog and close it
    pub fn apply_filter(&mut self) -> Result<()> {
        if let Some(ref dialog) = self.filter_dialog {
            self.filter = dialog.to_criteria();
            self.update_matching_directories()?;
        }
        self.filter_dialog = None;
        // Invalidate preview cache to force full redraw (fixes dialog remnants)
        *self.preview_cache.borrow_mut() = None;
        *self.directory_preview_cache.borrow_mut() = None;
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
        // Invalidate preview cache to force full redraw
        *self.preview_cache.borrow_mut() = None;
        *self.directory_preview_cache.borrow_mut() = None;
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
            dialog.focus = match dialog.focus {
                FilterDialogFocus::Rating => FilterDialogFocus::VideoOnly,
                FilterDialogFocus::VideoOnly => FilterDialogFocus::Tag,
                FilterDialogFocus::Tag => FilterDialogFocus::Rating,
            };
        }
    }

    /// Move focus up in filter dialog
    pub fn filter_dialog_focus_up(&mut self) {
        if let Some(ref mut dialog) = self.filter_dialog {
            dialog.focus = match dialog.focus {
                FilterDialogFocus::Rating => FilterDialogFocus::Tag,
                FilterDialogFocus::VideoOnly => FilterDialogFocus::Rating,
                FilterDialogFocus::Tag => FilterDialogFocus::VideoOnly,
            };
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

    /// Add selected tag to filter
    pub fn filter_dialog_add_tag(&mut self) {
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
                }
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
        // Invalidate preview cache to force full redraw
        *self.preview_cache.borrow_mut() = None;
        *self.directory_preview_cache.borrow_mut() = None;
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
        *self.preview_cache.borrow_mut() = None;
        *self.directory_preview_cache.borrow_mut() = None;

        Ok(())
    }

    // ==================== Operations Menu Methods ====================

    /// Open the operations menu (triggered by 'o')
    pub fn open_operations_menu(&mut self) {
        if let Some(dir) = self.get_selected_directory().cloned() {
            // Count files recursively
            let mut dir_ids = vec![dir.id];
            self.collect_descendant_dir_ids(dir.id, &mut dir_ids);

            let mut file_count = 0;
            for dir_id in &dir_ids {
                if let Ok(files) = self.db.get_files_in_directory(*dir_id) {
                    file_count += files.len();
                }
            }

            self.operations_menu = Some(OperationsMenuState {
                directory_path: dir.path.clone(),
                file_count,
                selected: 0,
            });
        }
    }

    /// Close the operations menu
    pub fn close_operations_menu(&mut self) {
        self.operations_menu = None;
        // Invalidate preview cache to force full redraw
        *self.preview_cache.borrow_mut() = None;
        *self.directory_preview_cache.borrow_mut() = None;
    }

    /// Move selection up in operations menu
    pub fn operations_menu_up(&mut self) {
        if let Some(ref mut menu) = self.operations_menu {
            if menu.selected > 0 {
                menu.selected -= 1;
            } else {
                menu.selected = 2; // Wrap to bottom
            }
        }
    }

    /// Move selection down in operations menu
    pub fn operations_menu_down(&mut self) {
        if let Some(ref mut menu) = self.operations_menu {
            if menu.selected < 2 {
                menu.selected += 1;
            } else {
                menu.selected = 0; // Wrap to top
            }
        }
    }

    /// Recursively collect all descendant directory IDs
    fn collect_descendant_dir_ids(&self, parent_id: i64, result: &mut Vec<i64>) {
        for dir in &self.tree.directories {
            if dir.parent_id == Some(parent_id) {
                result.push(dir.id);
                self.collect_descendant_dir_ids(dir.id, result);
            }
        }
    }

    /// Execute the selected operation from menu
    pub fn operations_menu_select(&mut self) {
        let operation = if let Some(ref menu) = self.operations_menu {
            match menu.selected {
                0 => OperationType::Thumbnails,
                1 => OperationType::Orientation,
                2 => OperationType::Hash,
                _ => return,
            }
        } else {
            return;
        };

        self.operations_menu = None;
        self.run_operation(operation);
    }

    /// Run a background operation on current directory and all subdirectories
    pub fn run_operation(&mut self, operation: OperationType) {
        // Handle directory preview operations separately
        if matches!(operation, OperationType::DirPreview | OperationType::DirPreviewRecursive) {
            self.run_dir_preview_operation(operation);
            return;
        }

        use crate::tui::widgets::{has_thumbnail, is_image_file, is_video_file};

        let selected_dir = match self.get_selected_directory() {
            Some(d) => d.clone(),
            None => return,
        };

        // Collect all directory IDs under selected directory (including itself)
        let mut dir_ids = vec![selected_dir.id];
        self.collect_descendant_dir_ids(selected_dir.id, &mut dir_ids);

        let library_path = self.library_path.clone();

        // Collect files from all directories, skipping already-processed ones
        let mut file_data: Vec<(i64, PathBuf)> = Vec::new();
        for dir_id in &dir_ids {
            if let Ok(files) = self.db.get_files_in_directory(*dir_id) {
                // Get directory path for this dir_id
                let dir_path = self.tree.directories.iter()
                    .find(|d| d.id == *dir_id)
                    .map(|d| d.path.clone())
                    .unwrap_or_default();

                for file in files {
                    let path = if dir_path.is_empty() {
                        library_path.join(&file.filename)
                    } else {
                        library_path.join(&dir_path).join(&file.filename)
                    };

                    // Filter based on operation type and skip already-processed
                    let include = match operation {
                        OperationType::Thumbnails => {
                            (is_image_file(&path) || is_video_file(&path)) && !has_thumbnail(&path)
                        }
                        OperationType::Orientation => {
                            if !is_image_file(&path) {
                                false
                            } else {
                                // Check if already has orientation tag
                                let tags = self.db.get_file_tags(file.id).unwrap_or_default();
                                !tags.contains(&"landscape".to_string()) && !tags.contains(&"portrait".to_string())
                            }
                        }
                        OperationType::Hash => file.hash.is_none(),
                        OperationType::DirPreview | OperationType::DirPreviewRecursive => false,
                    };

                    if include {
                        file_data.push((file.id, path));
                    }
                }
            }
        }

        let total = file_data.len();
        if total == 0 {
            self.status_message = Some("Nothing to do - all files already processed".to_string());
            return;
        }

        // Set up progress tracking
        let completed = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));

        self.background_progress = Some(BackgroundProgress {
            operation,
            total,
            completed: Arc::clone(&completed),
            done: Arc::clone(&done),
            cancelled: Arc::clone(&cancelled),
        });

        // Get db path for operations that need it
        let db_path = self.library_path.join(".picman.db");

        // Spawn background thread for parallel processing
        std::thread::spawn(move || {
            use rayon::prelude::*;

            match operation {
                OperationType::Thumbnails => {
                    use crate::tui::widgets::{generate_image_thumbnail, generate_video_thumbnail, is_image_file, is_video_file};

                    file_data.par_iter().for_each(|(_, path)| {
                        if cancelled.load(Ordering::Relaxed) {
                            return;
                        }
                        if is_image_file(path) {
                            generate_image_thumbnail(path);
                        } else if is_video_file(path) {
                            generate_video_thumbnail(path);
                        }
                        completed.fetch_add(1, Ordering::Relaxed);
                    });
                }
                OperationType::Orientation => {
                    use crate::db::Database;

                    // Process in parallel, collect results
                    let results: Vec<_> = file_data.par_iter()
                        .filter_map(|(file_id, path)| {
                            if cancelled.load(Ordering::Relaxed) {
                                return None;
                            }
                            let orientation = detect_orientation(path);
                            completed.fetch_add(1, Ordering::Relaxed);
                            Some((*file_id, orientation))
                        })
                        .collect();

                    // Update DB serially (SQLite is not thread-safe)
                    if !cancelled.load(Ordering::Relaxed) {
                        if let Ok(db) = Database::open(&db_path) {
                            for (file_id, orientation) in results {
                                if let Some(tag) = orientation {
                                    let _ = db.add_file_tag(file_id, tag);
                                }
                            }
                        }
                    }
                }
                OperationType::Hash => {
                    use crate::db::Database;
                    use crate::hash::compute_file_hash;

                    // Process in parallel, collect results
                    let results: Vec<_> = file_data.par_iter()
                        .filter_map(|(file_id, path)| {
                            if cancelled.load(Ordering::Relaxed) {
                                return None;
                            }
                            let hash = compute_file_hash(path).ok();
                            completed.fetch_add(1, Ordering::Relaxed);
                            Some((*file_id, hash))
                        })
                        .collect();

                    // Update DB serially
                    if !cancelled.load(Ordering::Relaxed) {
                        if let Ok(db) = Database::open(&db_path) {
                            for (file_id, hash) in results {
                                if let Some(h) = hash {
                                    let _ = db.set_file_hash(file_id, &h);
                                }
                            }
                        }
                    }
                }
                OperationType::DirPreview | OperationType::DirPreviewRecursive => {
                    // Handled by run_dir_preview_operation
                }
            }

            done.store(true, Ordering::Relaxed);
        });
    }

    /// Run directory preview generation (single or recursive)
    fn run_dir_preview_operation(&mut self, operation: OperationType) {
        use crate::db::Database;
        use crate::tui::widgets::{
            collect_preview_images_standalone, generate_dir_preview,
            generate_dir_preview_from_paths, TempPreviewState,
        };

        let selected_dir = match self.get_selected_directory() {
            Some(d) => d.clone(),
            None => return,
        };

        // Collect directories to process
        let dir_data: Vec<Directory> = if operation == OperationType::DirPreview {
            // Single directory only
            vec![selected_dir]
        } else {
            // Recursive: selected + all descendants
            let mut dir_ids = vec![selected_dir.id];
            self.collect_descendant_dir_ids(selected_dir.id, &mut dir_ids);
            self.tree
                .directories
                .iter()
                .filter(|d| dir_ids.contains(&d.id))
                .cloned()
                .collect()
        };

        let total = dir_data.len();
        if total == 0 {
            return;
        }

        // For single directory, run synchronously (fast enough)
        if operation == OperationType::DirPreview {
            generate_dir_preview(self, &dir_data[0]);
            // Clear cache to reload
            *self.directory_preview_cache.borrow_mut() = None;
            self.status_message = Some("Dir preview generated".to_string());
            return;
        }

        // For recursive, run in background with progress
        let completed = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));

        self.background_progress = Some(BackgroundProgress {
            operation,
            total,
            completed: Arc::clone(&completed),
            done: Arc::clone(&done),
            cancelled: Arc::clone(&cancelled),
        });

        let db_path = self.library_path.join(".picman.db");
        let library_path = self.library_path.clone();
        let all_directories = self.tree.directories.clone();

        std::thread::spawn(move || {
            use rayon::prelude::*;

            // Open DB connection for collecting image paths
            let db = match Database::open(&db_path) {
                Ok(db) => db,
                Err(_) => {
                    done.store(true, Ordering::Relaxed);
                    return;
                }
            };

            let temp_state = TempPreviewState {
                library_path,
                db,
                directories: all_directories,
            };

            // Step 1: Collect all image paths (sequential - needs DB)
            let preview_data: Vec<(i64, Vec<PathBuf>)> = dir_data
                .iter()
                .map(|dir| {
                    let images = collect_preview_images_standalone(&temp_state, dir);
                    (dir.id, images)
                })
                .collect();

            // Step 2: Generate previews in parallel (no DB needed)
            preview_data.par_iter().for_each(|(dir_id, images)| {
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                generate_dir_preview_from_paths(*dir_id, images);
                completed.fetch_add(1, Ordering::Relaxed);
            });

            done.store(true, Ordering::Relaxed);
        });
    }

    /// Cancel any running background operation
    pub fn cancel_background_operation(&mut self) {
        if let Some(ref progress) = self.background_progress {
            progress.cancelled.store(true, Ordering::Relaxed);
        }
    }

    /// Check if a background operation is running
    pub fn has_background_operation(&self) -> bool {
        self.background_progress.is_some()
    }

    /// Check and update background operation progress
    pub fn update_background_progress(&mut self) {
        if let Some(ref progress) = self.background_progress {
            if progress.done.load(Ordering::Relaxed) {
                let completed = progress.completed.load(Ordering::Relaxed);
                let was_cancelled = progress.cancelled.load(Ordering::Relaxed);
                let operation = progress.operation;
                if was_cancelled {
                    self.status_message = Some(format!("Cancelled - {} {}", completed, progress.operation.done_label()));
                } else {
                    self.status_message = Some(format!("{} {}", completed, progress.operation.done_label()));
                }
                self.background_progress = None;
                // Clear preview cache to reload (for thumbnails)
                *self.preview_cache.borrow_mut() = None;

                // After thumbnail generation, refresh the missing thumbnails check
                if operation == OperationType::Thumbnails {
                    if let Ok(mut set) = self.dirs_missing_file_thumbnails.lock() {
                        set.clear();
                    }
                    self.start_thumbnail_check();
                }
            }
        }
    }

    /// Clear status message
    pub fn clear_status_message(&mut self) {
        self.status_message = None;
    }

    /// Check if a directory is missing file thumbnails
    pub fn dir_missing_file_thumbnails(&self, dir_id: i64) -> bool {
        self.dirs_missing_file_thumbnails
            .lock()
            .map(|set| set.contains(&dir_id))
            .unwrap_or(false)
    }

    /// Start background check for directories with missing file thumbnails
    /// Checks only the first media file in each directory
    fn start_thumbnail_check(&self) {
        use crate::db::Database;
        use crate::tui::widgets::{has_thumbnail, is_image_file, is_video_file};

        let db_path = self.library_path.join(".picman.db");
        let library_path = self.library_path.clone();
        let directories = self.tree.directories.clone();
        let missing_set = Arc::clone(&self.dirs_missing_file_thumbnails);

        std::thread::spawn(move || {
            let db = match Database::open(&db_path) {
                Ok(db) => db,
                Err(_) => return,
            };

            for dir in &directories {
                // Get files in this directory
                let files = match db.get_files_in_directory(dir.id) {
                    Ok(f) => f,
                    Err(_) => continue,
                };

                // Find the first media file (image or video)
                let first_media = files.iter().find(|f| {
                    let path = if dir.path.is_empty() {
                        library_path.join(&f.filename)
                    } else {
                        library_path.join(&dir.path).join(&f.filename)
                    };
                    is_image_file(&path) || is_video_file(&path)
                });

                // If there's a media file but it doesn't have a thumbnail, mark as missing
                if let Some(file) = first_media {
                    let path = if dir.path.is_empty() {
                        library_path.join(&file.filename)
                    } else {
                        library_path.join(&dir.path).join(&file.filename)
                    };

                    if !has_thumbnail(&path) {
                        if let Ok(mut set) = missing_set.lock() {
                            set.insert(dir.id);
                        }
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== RenameDialogState Tests ====================

    #[test]
    fn test_rename_dialog_extracts_name_from_path() {
        let state = RenameDialogState::new(1, "photos/vacation/beach".to_string(), vec![]);
        assert_eq!(state.new_name, "beach");
        assert_eq!(state.cursor_pos, 5); // "beach".len()
    }

    #[test]
    fn test_rename_dialog_insert_char() {
        let mut state = RenameDialogState::new(1, "test".to_string(), vec![]);
        state.cursor_pos = 2;
        state.insert_char('X');
        assert_eq!(state.new_name, "teXst");
        assert_eq!(state.cursor_pos, 3);
    }

    #[test]
    fn test_rename_dialog_backspace() {
        let mut state = RenameDialogState::new(1, "test".to_string(), vec![]);
        state.cursor_pos = 2;
        state.backspace();
        assert_eq!(state.new_name, "tst");
        assert_eq!(state.cursor_pos, 1);
    }

    #[test]
    fn test_rename_dialog_backspace_at_start() {
        let mut state = RenameDialogState::new(1, "test".to_string(), vec![]);
        state.cursor_pos = 0;
        state.backspace();
        assert_eq!(state.new_name, "test");
        assert_eq!(state.cursor_pos, 0);
    }

    #[test]
    fn test_rename_dialog_cursor_movement() {
        let mut state = RenameDialogState::new(1, "test".to_string(), vec![]);
        assert_eq!(state.cursor_pos, 4);

        state.move_cursor_left();
        assert_eq!(state.cursor_pos, 3);

        state.move_cursor_home();
        assert_eq!(state.cursor_pos, 0);

        state.move_cursor_right();
        assert_eq!(state.cursor_pos, 1);

        state.move_cursor_end();
        assert_eq!(state.cursor_pos, 4);
    }

    #[test]
    fn test_rename_dialog_use_suggestion() {
        let mut state = RenameDialogState::new(
            1,
            "test".to_string(),
            vec!["alpha".to_string(), "beta".to_string()],
        );
        state.selected_suggestion = 1;
        state.use_suggestion();
        assert_eq!(state.new_name, "beta");
        assert_eq!(state.cursor_pos, 4);
    }

    // ==================== FilterDialogState Tests ====================

    #[test]
    fn test_filter_dialog_update_tag_filter() {
        let all_tags = vec!["landscape".to_string(), "portrait".to_string(), "nature".to_string()];
        let mut state = FilterDialogState::new(all_tags, &FilterCriteria::default());

        state.tag_input = "port".to_string();
        state.update_tag_filter();

        assert_eq!(state.filtered_tags.len(), 1);
        assert_eq!(state.filtered_tags[0], "portrait");
    }

    #[test]
    fn test_filter_dialog_excludes_selected_tags() {
        let all_tags = vec!["landscape".to_string(), "portrait".to_string()];
        let mut state = FilterDialogState::new(all_tags, &FilterCriteria::default());

        state.selected_tags.push("landscape".to_string());
        state.update_tag_filter();

        assert_eq!(state.filtered_tags.len(), 1);
        assert_eq!(state.filtered_tags[0], "portrait");
    }

    #[test]
    fn test_filter_dialog_to_criteria() {
        let all_tags = vec!["landscape".to_string()];
        let mut state = FilterDialogState::new(all_tags, &FilterCriteria::default());

        state.rating_filter = RatingFilter::MinRating(3);
        state.selected_tags = vec!["nature".to_string()];
        state.video_only = true;

        let criteria = state.to_criteria();

        assert_eq!(criteria.rating, RatingFilter::MinRating(3));
        assert_eq!(criteria.tags, vec!["nature".to_string()]);
        assert!(criteria.video_only);
    }

    // ==================== TagInputState Tests ====================

    #[test]
    fn test_tag_input_filter_by_query() {
        let all_tags = vec!["landscape".to_string(), "portrait".to_string(), "beach".to_string()];
        let mut state = TagInputState::new(all_tags);

        state.input = "port".to_string();
        state.update_filter();

        assert_eq!(state.filtered_tags.len(), 1);
        assert_eq!(state.filtered_tags[0], "portrait");
    }

    #[test]
    fn test_tag_input_navigation_wraps() {
        let all_tags = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut state = TagInputState::new(all_tags);

        assert_eq!(state.selected_index, 0);

        state.move_up();
        assert_eq!(state.selected_index, 2); // Wrapped to bottom

        state.move_down();
        assert_eq!(state.selected_index, 0); // Wrapped to top
    }

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

    // ==================== FilterCriteria Tests ====================

    #[test]
    fn test_filter_criteria_is_active() {
        let mut criteria = FilterCriteria::default();
        assert!(!criteria.is_active());

        criteria.rating = RatingFilter::MinRating(3);
        assert!(criteria.is_active());

        criteria = FilterCriteria::default();
        criteria.tags = vec!["test".to_string()];
        assert!(criteria.is_active());

        criteria = FilterCriteria::default();
        criteria.video_only = true;
        assert!(criteria.is_active());
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
