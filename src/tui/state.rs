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

/// Detect image orientation from EXIF data
fn detect_orientation(path: &std::path::Path) -> Option<&'static str> {
    let size = imagesize::size(path).ok()?;
    let (mut width, mut height) = (size.width, size.height);

    // Check EXIF orientation - values 5-8 involve 90° rotation, swapping dimensions
    if let Ok(file) = std::fs::File::open(path) {
        let mut bufreader = std::io::BufReader::new(file);
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut bufreader) {
            if let Some(orientation) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
                if let exif::Value::Short(ref vals) = orientation.value {
                    if let Some(&val) = vals.first() {
                        if val >= 5 && val <= 8 {
                            std::mem::swap(&mut width, &mut height);
                        }
                    }
                }
            }
        }
    }

    if width > height {
        Some("landscape")
    } else if height > width {
        Some("portrait")
    } else {
        None
    }
}

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

/// Extract meaningful words from a directory path for rename suggestions
pub fn extract_suggested_words(path: &str, file_tags: &[String]) -> Vec<String> {
    use std::collections::HashMap;

    let mut word_counts: HashMap<String, usize> = HashMap::new();

    // Delimiters for splitting
    let delimiters = [' ', '-', '_', '@', '(', ')', '[', ']', '{', '}',
                      '【', '】', '「', '」', '『', '』', '/', '\\', '&', '+'];

    // Noise words to filter out
    let noise: std::collections::HashSet<&str> = [
        "no", "vol", "p", "v", "gb", "mb", "kb", "pic", "video", "gif",
        "cosplay", "coser", "ver", "version", "normal", "bonus", "set",
        "part", "作品", "月", "年", "订阅", "特典", "合集",
    ].iter().copied().collect();

    // Process path segments
    for segment in path.split('/') {
        let mut current_word = String::new();

        for c in segment.chars() {
            if delimiters.contains(&c) {
                if !current_word.is_empty() {
                    process_word(&current_word, &noise, &mut word_counts);
                    current_word.clear();
                }
            } else {
                current_word.push(c);
            }
        }

        if !current_word.is_empty() {
            process_word(&current_word, &noise, &mut word_counts);
        }
    }

    // Add file tags with high weight
    for tag in file_tags {
        *word_counts.entry(tag.clone()).or_insert(0) += 3;
    }

    // Sort by frequency, then alphabetically
    let mut words: Vec<(String, usize)> = word_counts.into_iter().collect();
    words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    words.into_iter().map(|(w, _)| w).take(30).collect()
}

fn process_word(
    word: &str,
    noise: &std::collections::HashSet<&str>,
    counts: &mut std::collections::HashMap<String, usize>
) {
    let trimmed = word.trim();
    let lower = trimmed.to_lowercase();

    // Skip if too short
    if trimmed.len() < 2 {
        return;
    }

    // Skip if noise word
    if noise.contains(lower.as_str()) {
        return;
    }

    // Skip if purely numeric
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return;
    }

    // Skip if looks like file size (e.g., "1.37GB", "350MB")
    if trimmed.ends_with("GB") || trimmed.ends_with("MB") || trimmed.ends_with("KB") {
        return;
    }

    // Skip patterns like "73P1V" or "45P"
    if trimmed.chars().any(|c| c.is_ascii_digit())
       && (trimmed.contains('P') || trimmed.contains('V'))
       && trimmed.len() < 10 {
        return;
    }

    *counts.entry(trimmed.to_string()).or_insert(0) += 1;
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

    pub fn move_tag_list_up(&mut self) {
        if !self.filtered_tags.is_empty() {
            if self.tag_list_index > 0 {
                self.tag_list_index -= 1;
            } else {
                self.tag_list_index = self.filtered_tags.len() - 1;
                // Scroll to show the last item
                self.tag_scroll_offset = self.filtered_tags.len().saturating_sub(5);
            }
            // Ensure selection is visible (scroll up if needed)
            if self.tag_list_index < self.tag_scroll_offset {
                self.tag_scroll_offset = self.tag_list_index;
            }
        }
    }

    pub fn move_tag_list_down(&mut self) {
        if !self.filtered_tags.is_empty() {
            if self.tag_list_index < self.filtered_tags.len() - 1 {
                self.tag_list_index += 1;
                // Scroll down if selection goes below visible area (assume ~5 visible items)
                let visible_height = 5;
                if self.tag_list_index >= self.tag_scroll_offset + visible_height {
                    self.tag_scroll_offset = self.tag_list_index - visible_height + 1;
                }
            } else {
                self.tag_list_index = 0;
                self.tag_scroll_offset = 0;
            }
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

            // Check if this directory or any ancestor has all the filter tags
            // If so, show all files without checking individual file tags/ratings
            let ancestor_has_all_filter_tags = if !self.filter.tags.is_empty() {
                self.directory_or_ancestor_has_tags(dir.id, &self.filter.tags)?
            } else {
                false
            };

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
                    // Check rating filter (skip if directory/ancestor has matching tags)
                    if !ancestor_has_all_filter_tags {
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
                    }
                    // Check tag filter (AND logic)
                    // Skip this check if directory or ancestor has all the filter tags
                    if !self.filter.tags.is_empty() && !ancestor_has_all_filter_tags {
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

    /// Check if a directory or any of its ancestors has all the specified tags
    fn directory_or_ancestor_has_tags(&self, dir_id: i64, tags: &[String]) -> Result<bool> {
        let mut current_id = Some(dir_id);

        while let Some(id) = current_id {
            let dir_tags = self.db.get_directory_tags(id)?;
            if tags.iter().all(|t| dir_tags.contains(t)) {
                return Ok(true);
            }

            // Move to parent directory
            current_id = self.tree.directories
                .iter()
                .find(|d| d.id == id)
                .and_then(|d| d.parent_id);
        }

        Ok(false)
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
                if was_cancelled {
                    self.status_message = Some(format!("Cancelled - {} {}", completed, progress.operation.done_label()));
                } else {
                    self.status_message = Some(format!("{} {}", completed, progress.operation.done_label()));
                }
                self.background_progress = None;
                // Clear preview cache to reload (for thumbnails)
                *self.preview_cache.borrow_mut() = None;
            }
        }
    }

    /// Clear status message
    pub fn clear_status_message(&mut self) {
        self.status_message = None;
    }
}
