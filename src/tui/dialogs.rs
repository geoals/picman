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

/// State for operations menu popup
pub struct OperationsMenuState {
    pub directory_path: String,
    pub file_count: usize,
    pub selected: usize,
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

    // ==================== FilterCriteria Tests ====================

    #[test]
    fn test_filter_criteria_is_active() {
        let filter = FilterCriteria::default();
        assert!(!filter.is_active());

        let filter = FilterCriteria {
            rating: RatingFilter::MinRating(3),
            ..Default::default()
        };
        assert!(filter.is_active());

        let filter = FilterCriteria {
            tags: vec!["landscape".to_string()],
            ..Default::default()
        };
        assert!(filter.is_active());

        let filter = FilterCriteria {
            video_only: true,
            ..Default::default()
        };
        assert!(filter.is_active());
    }

    // ==================== FilterDialogState Tests ====================

    #[test]
    fn test_filter_dialog_update_tag_filter() {
        let all_tags = vec![
            "landscape".to_string(),
            "portrait".to_string(),
            "vacation".to_string(),
        ];
        let mut dialog = FilterDialogState::new(all_tags, &FilterCriteria::default());

        dialog.tag_input = "port".to_string();
        dialog.update_tag_filter();
        assert_eq!(dialog.filtered_tags, vec!["portrait"]);
    }

    #[test]
    fn test_filter_dialog_excludes_selected_tags() {
        let all_tags = vec![
            "landscape".to_string(),
            "portrait".to_string(),
        ];
        let filter = FilterCriteria {
            tags: vec!["landscape".to_string()],
            ..Default::default()
        };
        let mut dialog = FilterDialogState::new(all_tags, &filter);

        dialog.update_tag_filter();
        assert_eq!(dialog.filtered_tags, vec!["portrait"]);
    }

    #[test]
    fn test_filter_dialog_to_criteria() {
        let all_tags = vec!["landscape".to_string()];
        let mut dialog = FilterDialogState::new(all_tags, &FilterCriteria::default());
        dialog.rating_filter = RatingFilter::MinRating(3);
        dialog.selected_tags = vec!["landscape".to_string()];
        dialog.video_only = true;

        let criteria = dialog.to_criteria();
        assert_eq!(criteria.rating, RatingFilter::MinRating(3));
        assert_eq!(criteria.tags, vec!["landscape"]);
        assert!(criteria.video_only);
    }

    // ==================== TagInputState Tests ====================

    #[test]
    fn test_tag_input_filter_by_query() {
        let mut state = TagInputState::new(vec![
            "landscape".to_string(),
            "portrait".to_string(),
            "vacation".to_string(),
        ]);
        state.input = "port".to_string();
        state.update_filter();
        assert_eq!(state.filtered_tags, vec!["portrait"]);
    }

    #[test]
    fn test_tag_input_navigation_wraps() {
        let mut state = TagInputState::new(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(state.selected_index, 0);

        state.move_up();
        assert_eq!(state.selected_index, 1); // Wraps to bottom

        state.move_down();
        assert_eq!(state.selected_index, 0); // Wraps to top
    }
}
