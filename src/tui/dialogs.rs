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

    /// Check whether a single file passes this filter.
    ///
    /// `file_tags` are tags on the file itself; `dir_tags` are inherited from
    /// the directory and its ancestors. When `ancestor_matches` is true the
    /// directory already satisfies rating+tag criteria, so only `video_only`
    /// is enforced.
    pub fn matches_file(
        &self,
        file: &crate::db::File,
        file_tags: &[String],
        dir_tags: &[String],
        ancestor_matches: bool,
    ) -> bool {
        // video_only always applies, even when ancestor matches
        if self.video_only && file.media_type.as_deref() != Some("video") {
            return false;
        }

        // When ancestor matches, skip rating and tag checks
        if ancestor_matches || !self.is_active() {
            return true;
        }

        // Rating filter
        match self.rating {
            RatingFilter::Any => {}
            RatingFilter::Unrated => {
                if file.rating.is_some() {
                    return false;
                }
            }
            RatingFilter::MinRating(min) => match file.rating {
                Some(r) if r >= min => {}
                _ => return false,
            },
        }

        // Tag filter (AND logic) — file tags + inherited directory tags
        if !self.tags.is_empty() {
            let has_all = self
                .tags
                .iter()
                .all(|t| file_tags.contains(t) || dir_tags.contains(t));
            if !has_all {
                return false;
            }
        }

        true
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
    pub tag_input_selected: bool,      // True when the input line is the selected item
    pub tag_editing: bool,             // True when actively typing in tag input
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
            tag_input_selected: true,
            tag_editing: false,
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
        sort_prefix_first(&mut self.filtered_tags, &query);
        self.tag_list_index = 0;
        self.tag_scroll_offset = 0;
    }

    pub fn selected_autocomplete_tag(&self) -> Option<&String> {
        self.filtered_tags.get(self.tag_list_index)
    }

    /// Move up in tag section. Navigates: tag list → input line → leave section.
    /// Returns true if moved within section, false if at top (should leave section).
    pub fn move_tag_list_up(&mut self) -> bool {
        if self.tag_input_selected {
            // Already at input line (top of section), signal to leave
            false
        } else if self.tag_list_index > 0 {
            self.tag_list_index -= 1;
            if self.tag_list_index < self.tag_scroll_offset {
                self.tag_scroll_offset = self.tag_list_index;
            }
            true
        } else {
            // At first tag, move up to input line
            self.tag_input_selected = true;
            true
        }
    }

    /// Move down in tag section. Navigates: input line → tag list.
    /// Returns true if moved, false if already at bottom.
    pub fn move_tag_list_down(&mut self) -> bool {
        if self.tag_input_selected {
            // Move from input line to first tag
            self.tag_input_selected = false;
            self.tag_list_index = 0;
            self.tag_scroll_offset = 0;
            true
        } else if !self.filtered_tags.is_empty()
            && self.tag_list_index < self.filtered_tags.len() - 1
        {
            self.tag_list_index += 1;
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

    /// Push a character into the tag input (only when Tag section is focused)
    pub fn char_input(&mut self, c: char) {
        if self.focus == FilterDialogFocus::Tag {
            self.tag_input.push(c);
            self.update_tag_filter();
        }
    }

    /// Handle backspace: remove char from input, or pop last selected tag if empty
    pub fn backspace(&mut self) {
        if self.focus != FilterDialogFocus::Tag {
            return;
        }
        if self.tag_input.is_empty() {
            self.selected_tags.pop();
        } else {
            self.tag_input.pop();
        }
        self.update_tag_filter();
    }

    /// Navigate up across sections (Tag list → VideoOnly → Rating)
    pub fn navigate_up(&mut self) {
        match self.focus {
            FilterDialogFocus::Tag => {
                if !self.move_tag_list_up() {
                    self.focus = FilterDialogFocus::VideoOnly;
                }
            }
            FilterDialogFocus::VideoOnly => {
                self.focus = FilterDialogFocus::Rating;
            }
            FilterDialogFocus::Rating => {}
        }
    }

    /// Navigate down across sections (Rating → VideoOnly → Tag list)
    pub fn navigate_down(&mut self) {
        match self.focus {
            FilterDialogFocus::Rating => {
                self.focus = FilterDialogFocus::VideoOnly;
            }
            FilterDialogFocus::VideoOnly => {
                self.focus = FilterDialogFocus::Tag;
            }
            FilterDialogFocus::Tag => {
                self.move_tag_list_down();
            }
        }
    }

    /// Cycle rating left (Any ← Unrated ← 1 ← 2 ← 3 ← 4 ← 5)
    pub fn navigate_rating_left(&mut self) {
        if self.focus != FilterDialogFocus::Rating {
            return;
        }
        self.rating_filter = match self.rating_filter {
            RatingFilter::Any => RatingFilter::MinRating(5),
            RatingFilter::Unrated => RatingFilter::Any,
            RatingFilter::MinRating(1) => RatingFilter::Unrated,
            RatingFilter::MinRating(n) => RatingFilter::MinRating(n - 1),
        };
    }

    /// Cycle rating right (Any → Unrated → 1 → 2 → 3 → 4 → 5)
    pub fn navigate_rating_right(&mut self) {
        if self.focus != FilterDialogFocus::Rating {
            return;
        }
        self.rating_filter = match self.rating_filter {
            RatingFilter::Any => RatingFilter::Unrated,
            RatingFilter::Unrated => RatingFilter::MinRating(1),
            RatingFilter::MinRating(5) => RatingFilter::Any,
            RatingFilter::MinRating(n) => RatingFilter::MinRating(n + 1),
        };
    }

    /// Tab: cycle focus to next section (wraps). Clears tag_editing.
    pub fn cycle_focus_down(&mut self) {
        self.tag_editing = false;
        self.focus = match self.focus {
            FilterDialogFocus::Rating => FilterDialogFocus::VideoOnly,
            FilterDialogFocus::VideoOnly => FilterDialogFocus::Tag,
            FilterDialogFocus::Tag => FilterDialogFocus::Rating,
        };
        if self.focus == FilterDialogFocus::Tag {
            self.tag_input_selected = true;
        }
    }

    /// BackTab: cycle focus to previous section (wraps). Clears tag_editing.
    pub fn cycle_focus_up(&mut self) {
        self.tag_editing = false;
        self.focus = match self.focus {
            FilterDialogFocus::Rating => FilterDialogFocus::Tag,
            FilterDialogFocus::VideoOnly => FilterDialogFocus::Rating,
            FilterDialogFocus::Tag => FilterDialogFocus::VideoOnly,
        };
        if self.focus == FilterDialogFocus::Tag {
            self.tag_input_selected = true;
        }
    }

    /// Set a specific minimum rating (1-5). Only works when Rating section focused.
    pub fn set_rating(&mut self, rating: i32) {
        if self.focus == FilterDialogFocus::Rating {
            self.rating_filter = RatingFilter::MinRating(rating);
        }
    }

    /// Toggle video-only filter
    pub fn toggle_video(&mut self) {
        self.video_only = !self.video_only;
    }

    /// Set the unrated filter. Only works when Rating section focused.
    pub fn set_unrated(&mut self) {
        if self.focus == FilterDialogFocus::Rating {
            self.rating_filter = RatingFilter::Unrated;
        }
    }

    /// Add the highlighted autocomplete tag to selected_tags
    pub fn add_tag(&mut self) {
        if let Some(tag) = self.selected_autocomplete_tag().cloned() {
            if !self.selected_tags.contains(&tag) {
                self.selected_tags.push(tag);
                self.selected_tags.sort();
            }
            self.tag_input.clear();
            self.tag_editing = false;
            self.tag_input_selected = true;
            self.update_tag_filter();
        }
    }
}

/// Sort tags so prefix matches come before substring-only matches.
/// Preserves alphabetical order within each group.
fn sort_prefix_first(tags: &mut [String], query: &str) {
    tags.sort_by_key(|tag| !tag.to_lowercase().starts_with(query));
}

/// State for the tag input popup
pub struct TagInputState {
    pub input: String,
    pub all_tags: Vec<String>,
    pub filtered_tags: Vec<String>,
    pub selected_index: usize,
    /// True when the input line is the selected navigable item
    pub input_selected: bool,
    /// True when actively typing in the input field
    pub editing: bool,
    /// Tags currently applied to the selected item (for toggle display)
    pub current_tags: Vec<String>,
}

impl TagInputState {
    pub fn new(all_tags: Vec<String>) -> Self {
        Self::new_with_current(all_tags, Vec::new())
    }

    pub fn new_with_current(all_tags: Vec<String>, current_tags: Vec<String>) -> Self {
        let filtered_tags = all_tags.clone();
        Self {
            input: String::new(),
            all_tags,
            filtered_tags,
            selected_index: 0,
            input_selected: true,
            editing: true,
            current_tags,
        }
    }

    /// Check if a tag is currently applied to the selected item
    pub fn is_applied(&self, tag: &str) -> bool {
        self.current_tags.iter().any(|t| t == tag)
    }

    pub fn update_filter(&mut self) {
        let query = self.input.to_lowercase();
        self.filtered_tags = self
            .all_tags
            .iter()
            .filter(|tag| tag.to_lowercase().contains(&query))
            .cloned()
            .collect();
        sort_prefix_first(&mut self.filtered_tags, &query);
        self.selected_index = 0;
    }

    pub fn selected_tag(&self) -> Option<&String> {
        self.filtered_tags.get(self.selected_index)
    }

    pub fn move_up(&mut self) {
        if self.editing {
            // Editing mode: navigate autocomplete list only (wrap within list)
            if !self.filtered_tags.is_empty() {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                } else {
                    self.selected_index = self.filtered_tags.len() - 1;
                }
            }
        } else if self.input_selected {
            // Browse mode at input line: wrap to bottom of list
            if !self.filtered_tags.is_empty() {
                self.input_selected = false;
                self.selected_index = self.filtered_tags.len() - 1;
            }
        } else if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            // At first tag: move up to input line
            self.input_selected = true;
        }
    }

    /// Push a character and update the filter in one call
    pub fn push_char_and_filter(&mut self, c: char) {
        self.input.push(c);
        self.update_filter();
    }

    /// Pop a character and update the filter in one call
    pub fn pop_char_and_filter(&mut self) {
        self.input.pop();
        self.update_filter();
    }

    /// Update popup state after a tag toggle (add/remove).
    /// Clears input, updates current_tags / all_tags, and preserves browse-mode cursor.
    pub fn apply_toggle(&mut self, tag: &str, was_applied: bool) {
        if was_applied {
            self.current_tags.retain(|t| t != tag);
        } else {
            self.current_tags.push(tag.to_string());
            self.current_tags.sort();
        }
        // Register brand-new tags
        if !self.all_tags.contains(&tag.to_string()) {
            self.all_tags.push(tag.to_string());
            self.all_tags.sort();
        }
        // Clear input and rebuild filter list, but preserve selection when browsing
        let had_input = !self.input.is_empty();
        self.input.clear();
        let prev_index = self.selected_index;
        self.update_filter();
        if !had_input {
            // Browsing mode: keep cursor where it was (clamped to list bounds)
            self.selected_index = prev_index.min(self.filtered_tags.len().saturating_sub(1));
        }
    }

    pub fn move_down(&mut self) {
        if self.editing {
            // Editing mode: navigate autocomplete list only (wrap within list)
            if !self.filtered_tags.is_empty() {
                if self.selected_index < self.filtered_tags.len() - 1 {
                    self.selected_index += 1;
                } else {
                    self.selected_index = 0;
                }
            }
        } else if self.input_selected {
            // Browse mode at input line: move to first tag
            if !self.filtered_tags.is_empty() {
                self.input_selected = false;
                self.selected_index = 0;
            }
        } else if self.selected_index < self.filtered_tags.len() - 1 {
            self.selected_index += 1;
        } else {
            // At last tag: wrap to input line
            self.input_selected = true;
        }
    }
}

/// State for incremental search (LazyVim-style `/` search)
pub struct SearchState {
    pub query: String,
    pub active: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            active: false,
        }
    }
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.query.clear();
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.query.clear();
    }

    /// Accept the current search: deactivate input but keep the query as filter
    pub fn accept(&mut self) {
        self.active = false;
        // Don't clear query — it stays as a filter until next search
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
    }

    /// Case-insensitive substring match. Empty query matches everything.
    pub fn matches(&self, text: &str) -> bool {
        if self.query.is_empty() {
            return true;
        }
        text.to_lowercase().contains(&self.query.to_lowercase())
    }
}

/// State for operations menu popup
pub struct OperationsMenuState {
    pub directory_path: String,
    pub file_count: usize,
    pub selected: usize,
}

impl OperationsMenuState {
    const ITEM_COUNT: usize = 5;

    /// Move selection up (wraps to bottom)
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = Self::ITEM_COUNT - 1;
        }
    }

    /// Move selection down (wraps to top)
    pub fn move_down(&mut self) {
        if self.selected < Self::ITEM_COUNT - 1 {
            self.selected += 1;
        } else {
            self.selected = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== SearchState Tests ====================

    #[test]
    fn test_search_empty_query_matches_everything() {
        let search = SearchState::new();
        assert!(search.matches("anything"));
        assert!(search.matches(""));
    }

    #[test]
    fn test_search_case_insensitive_substring() {
        let mut search = SearchState::new();
        search.query = "beach".to_string();
        assert!(search.matches("Beach Party"));
        assert!(search.matches("BEACH"));
        assert!(search.matches("the beach"));
        assert!(!search.matches("shore"));
    }

    #[test]
    fn test_search_activate_deactivate() {
        let mut search = SearchState::new();
        assert!(!search.active);

        search.activate();
        assert!(search.active);
        assert!(search.query.is_empty());

        search.push_char('a');
        search.push_char('b');
        assert_eq!(search.query, "ab");

        search.pop_char();
        assert_eq!(search.query, "a");

        search.deactivate();
        assert!(!search.active);
        assert!(search.query.is_empty());
    }

    #[test]
    fn test_search_accept_keeps_filter() {
        let mut search = SearchState::new();
        search.activate();
        search.push_char('f');
        search.push_char('o');
        search.push_char('o');
        search.accept();

        assert!(!search.active);
        assert_eq!(search.query, "foo"); // Query preserved as filter
    }

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
    fn test_tag_input_filter_prefers_prefix_matches() {
        let mut state = TagInputState::new(vec![
            "landscape".to_string(),
            "screenshot".to_string(),
        ]);
        state.input = "sc".to_string();
        state.update_filter();
        // "screenshot" starts with "sc", "landscape" only contains it
        assert_eq!(state.filtered_tags, vec!["screenshot", "landscape"]);
    }

    #[test]
    fn test_filter_dialog_prefers_prefix_matches() {
        let all_tags = vec![
            "landscape".to_string(),
            "screenshot".to_string(),
        ];
        let mut dialog = FilterDialogState::new(all_tags, &FilterCriteria::default());
        dialog.tag_input = "sc".to_string();
        dialog.update_tag_filter();
        assert_eq!(dialog.filtered_tags, vec!["screenshot", "landscape"]);
    }

    #[test]
    fn test_tag_input_editing_navigation_wraps_within_list() {
        let mut state = TagInputState::new(vec!["a".to_string(), "b".to_string()]);
        assert!(state.editing);
        assert_eq!(state.selected_index, 0);

        state.move_up();
        assert_eq!(state.selected_index, 1); // Wraps to bottom within list

        state.move_down();
        assert_eq!(state.selected_index, 0); // Wraps to top within list
    }

    #[test]
    fn test_tag_input_starts_in_editing_mode() {
        let state = TagInputState::new(vec!["a".to_string()]);
        assert!(state.editing);
        assert!(state.input_selected);
    }

    #[test]
    fn test_tag_input_browse_down_cycles_through_input_and_list() {
        let mut state = TagInputState::new(vec!["a".to_string(), "b".to_string()]);
        state.editing = false;
        assert!(state.input_selected);

        // Input → first tag
        state.move_down();
        assert!(!state.input_selected);
        assert_eq!(state.selected_index, 0);

        // First tag → second tag
        state.move_down();
        assert!(!state.input_selected);
        assert_eq!(state.selected_index, 1);

        // Last tag → wraps to input
        state.move_down();
        assert!(state.input_selected);
    }

    #[test]
    fn test_tag_input_browse_up_wraps_from_input_to_bottom() {
        let mut state = TagInputState::new(vec!["a".to_string(), "b".to_string()]);
        state.editing = false;
        assert!(state.input_selected);

        state.move_up();
        assert!(!state.input_selected);
        assert_eq!(state.selected_index, 1); // Wrapped to last tag
    }

    #[test]
    fn test_tag_input_browse_up_from_first_tag_goes_to_input() {
        let mut state = TagInputState::new(vec!["a".to_string(), "b".to_string()]);
        state.editing = false;
        state.input_selected = false;
        state.selected_index = 0;

        state.move_up();
        assert!(state.input_selected);
    }

    #[test]
    fn test_tag_input_browse_empty_list_stays_at_input() {
        let mut state = TagInputState::new(vec![]);
        state.editing = false;
        assert!(state.input_selected);

        state.move_down();
        assert!(state.input_selected); // Can't move into empty list

        state.move_up();
        assert!(state.input_selected); // Can't move into empty list
    }

    #[test]
    fn test_tag_input_is_applied_returns_correct_values() {
        let state = TagInputState::new_with_current(
            vec!["outdoor".to_string(), "portrait".to_string(), "landscape".to_string()],
            vec!["outdoor".to_string(), "portrait".to_string()],
        );
        assert!(state.is_applied("outdoor"));
        assert!(state.is_applied("portrait"));
        assert!(!state.is_applied("landscape"));
        assert!(!state.is_applied("nonexistent"));
    }

    #[test]
    fn test_tag_input_new_with_current_preserves_all_tags() {
        let state = TagInputState::new_with_current(
            vec!["a".to_string(), "b".to_string()],
            vec!["a".to_string()],
        );
        assert_eq!(state.all_tags, vec!["a", "b"]);
        assert_eq!(state.current_tags, vec!["a"]);
        assert_eq!(state.filtered_tags, vec!["a", "b"]);
    }

    // ==================== FilterDialogState Navigation Tests ====================

    fn make_filter_dialog() -> FilterDialogState {
        let all_tags = vec![
            "landscape".to_string(),
            "portrait".to_string(),
            "vacation".to_string(),
        ];
        FilterDialogState::new(all_tags, &FilterCriteria::default())
    }

    #[test]
    fn test_filter_dialog_char_input_on_tag_focus() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.char_input('a');
        assert_eq!(dialog.tag_input, "a");
        // Also updates filter
        assert!(dialog.filtered_tags.iter().all(|t| t.contains('a')));
    }

    #[test]
    fn test_filter_dialog_char_input_ignored_on_rating_focus() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;
        dialog.char_input('a');
        assert!(dialog.tag_input.is_empty());
    }

    #[test]
    fn test_filter_dialog_backspace_removes_char() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.tag_input = "ab".to_string();
        dialog.backspace();
        assert_eq!(dialog.tag_input, "a");
    }

    #[test]
    fn test_filter_dialog_backspace_empty_removes_last_tag() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.selected_tags = vec!["landscape".to_string(), "portrait".to_string()];
        dialog.backspace();
        assert_eq!(dialog.selected_tags, vec!["landscape"]);
    }

    #[test]
    fn test_filter_dialog_backspace_ignored_on_rating_focus() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;
        dialog.tag_input = "ab".to_string();
        dialog.backspace();
        assert_eq!(dialog.tag_input, "ab"); // unchanged
    }

    #[test]
    fn test_filter_dialog_navigate_up_from_tag_to_video() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.tag_input_selected = true; // at top of tag section
        dialog.navigate_up();
        assert_eq!(dialog.focus, FilterDialogFocus::VideoOnly);
    }

    #[test]
    fn test_filter_dialog_navigate_up_within_tag_list() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.tag_input_selected = false;
        dialog.tag_list_index = 1;
        dialog.navigate_up();
        assert_eq!(dialog.focus, FilterDialogFocus::Tag);
        assert_eq!(dialog.tag_list_index, 0);
    }

    #[test]
    fn test_filter_dialog_navigate_up_video_to_rating() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::VideoOnly;
        dialog.navigate_up();
        assert_eq!(dialog.focus, FilterDialogFocus::Rating);
    }

    #[test]
    fn test_filter_dialog_navigate_up_stays_at_rating() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;
        dialog.navigate_up();
        assert_eq!(dialog.focus, FilterDialogFocus::Rating);
    }

    #[test]
    fn test_filter_dialog_navigate_down_from_rating() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;
        dialog.navigate_down();
        assert_eq!(dialog.focus, FilterDialogFocus::VideoOnly);
    }

    #[test]
    fn test_filter_dialog_navigate_down_from_video() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::VideoOnly;
        dialog.navigate_down();
        assert_eq!(dialog.focus, FilterDialogFocus::Tag);
    }

    #[test]
    fn test_filter_dialog_navigate_down_within_tag_list() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.tag_input_selected = true;
        dialog.navigate_down();
        // Should move into tag list
        assert_eq!(dialog.focus, FilterDialogFocus::Tag);
        assert!(!dialog.tag_input_selected);
    }

    #[test]
    fn test_filter_dialog_navigate_rating_left_cycles() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;

        // Any -> 5
        dialog.navigate_rating_left();
        assert_eq!(dialog.rating_filter, RatingFilter::MinRating(5));

        // 5 -> 4
        dialog.navigate_rating_left();
        assert_eq!(dialog.rating_filter, RatingFilter::MinRating(4));

        // ... all the way to 1 -> Unrated -> Any
        dialog.rating_filter = RatingFilter::MinRating(1);
        dialog.navigate_rating_left();
        assert_eq!(dialog.rating_filter, RatingFilter::Unrated);
        dialog.navigate_rating_left();
        assert_eq!(dialog.rating_filter, RatingFilter::Any);
    }

    #[test]
    fn test_filter_dialog_navigate_rating_left_noop_on_other_focus() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.rating_filter = RatingFilter::Any;
        dialog.navigate_rating_left();
        assert_eq!(dialog.rating_filter, RatingFilter::Any); // unchanged
    }

    #[test]
    fn test_filter_dialog_navigate_rating_right_cycles() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;

        // Any -> Unrated -> 1 -> 2 -> ... -> 5 -> Any
        dialog.navigate_rating_right();
        assert_eq!(dialog.rating_filter, RatingFilter::Unrated);
        dialog.navigate_rating_right();
        assert_eq!(dialog.rating_filter, RatingFilter::MinRating(1));
        dialog.rating_filter = RatingFilter::MinRating(5);
        dialog.navigate_rating_right();
        assert_eq!(dialog.rating_filter, RatingFilter::Any);
    }

    #[test]
    fn test_filter_dialog_cycle_focus_down() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;
        dialog.tag_editing = true;

        dialog.cycle_focus_down();
        assert_eq!(dialog.focus, FilterDialogFocus::VideoOnly);
        assert!(!dialog.tag_editing); // editing cleared on focus change

        dialog.cycle_focus_down();
        assert_eq!(dialog.focus, FilterDialogFocus::Tag);
        assert!(dialog.tag_input_selected); // enters tag at input line

        dialog.cycle_focus_down();
        assert_eq!(dialog.focus, FilterDialogFocus::Rating); // wraps
    }

    #[test]
    fn test_filter_dialog_cycle_focus_up() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;
        dialog.tag_editing = true;

        dialog.cycle_focus_up();
        assert_eq!(dialog.focus, FilterDialogFocus::Tag);
        assert!(!dialog.tag_editing);
        assert!(dialog.tag_input_selected);

        dialog.cycle_focus_up();
        assert_eq!(dialog.focus, FilterDialogFocus::VideoOnly);

        dialog.cycle_focus_up();
        assert_eq!(dialog.focus, FilterDialogFocus::Rating);
    }

    #[test]
    fn test_filter_dialog_set_rating() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;
        dialog.set_rating(3);
        assert_eq!(dialog.rating_filter, RatingFilter::MinRating(3));
    }

    #[test]
    fn test_filter_dialog_set_rating_ignored_on_other_focus() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.set_rating(3);
        assert_eq!(dialog.rating_filter, RatingFilter::Any); // unchanged
    }

    #[test]
    fn test_filter_dialog_toggle_video() {
        let mut dialog = make_filter_dialog();
        assert!(!dialog.video_only);
        dialog.toggle_video();
        assert!(dialog.video_only);
        dialog.toggle_video();
        assert!(!dialog.video_only);
    }

    #[test]
    fn test_filter_dialog_set_unrated() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Rating;
        dialog.set_unrated();
        assert_eq!(dialog.rating_filter, RatingFilter::Unrated);
    }

    #[test]
    fn test_filter_dialog_set_unrated_ignored_on_other_focus() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.set_unrated();
        assert_eq!(dialog.rating_filter, RatingFilter::Any); // unchanged
    }

    #[test]
    fn test_filter_dialog_add_tag() {
        let mut dialog = make_filter_dialog();
        dialog.focus = FilterDialogFocus::Tag;
        dialog.tag_list_index = 0; // "landscape" is first
        dialog.tag_input_selected = false;

        dialog.add_tag();

        assert!(dialog.selected_tags.contains(&"landscape".to_string()));
        assert!(dialog.tag_input.is_empty());
        assert!(!dialog.tag_editing);
        assert!(dialog.tag_input_selected);
    }

    #[test]
    fn test_filter_dialog_add_tag_no_duplicates() {
        let mut dialog = make_filter_dialog();
        dialog.selected_tags = vec!["landscape".to_string()];
        dialog.tag_list_index = 0;
        dialog.update_tag_filter(); // "landscape" now excluded from filtered_tags

        // First tag in filtered list is now "portrait"
        dialog.add_tag();
        assert_eq!(dialog.selected_tags, vec!["landscape", "portrait"]);
    }

    // ==================== TagInputState Convenience Method Tests ====================

    #[test]
    fn test_tag_input_push_char_and_filter() {
        let mut state = TagInputState::new(vec![
            "landscape".to_string(),
            "portrait".to_string(),
        ]);
        state.push_char_and_filter('p');
        assert_eq!(state.input, "p");
        assert_eq!(state.filtered_tags, vec!["portrait", "landscape"]);
    }

    #[test]
    fn test_tag_input_pop_char_and_filter() {
        let mut state = TagInputState::new(vec![
            "landscape".to_string(),
            "portrait".to_string(),
        ]);
        state.input = "po".to_string();
        state.update_filter();
        state.pop_char_and_filter();
        assert_eq!(state.input, "p");
        // "p" matches both portrait (prefix) and landscape (contains)
        assert_eq!(state.filtered_tags, vec!["portrait", "landscape"]);
    }

    // ==================== OperationsMenuState Tests ====================

    #[test]
    fn test_operations_menu_move_down() {
        let mut menu = OperationsMenuState {
            directory_path: String::new(),
            file_count: 0,
            selected: 0,
        };
        menu.move_down();
        assert_eq!(menu.selected, 1);
        menu.move_down();
        assert_eq!(menu.selected, 2);
    }

    #[test]
    fn test_operations_menu_move_down_wraps() {
        let mut menu = OperationsMenuState {
            directory_path: String::new(),
            file_count: 0,
            selected: 4,
        };
        menu.move_down();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn test_operations_menu_move_up() {
        let mut menu = OperationsMenuState {
            directory_path: String::new(),
            file_count: 0,
            selected: 3,
        };
        menu.move_up();
        assert_eq!(menu.selected, 2);
    }

    #[test]
    fn test_operations_menu_move_up_wraps() {
        let mut menu = OperationsMenuState {
            directory_path: String::new(),
            file_count: 0,
            selected: 0,
        };
        menu.move_up();
        assert_eq!(menu.selected, 4);
    }

    // ==================== FilterCriteria::matches_file Tests ====================

    use crate::db::File;

    fn make_file(media_type: Option<&str>, rating: Option<i32>) -> File {
        File {
            id: 1,
            directory_id: 1,
            filename: "test.jpg".to_string(),
            size: 100,
            mtime: 0,
            hash: None,
            rating,
            media_type: media_type.map(|s| s.to_string()),
            width: None,
            height: None,
            perceptual_hash: None,
        }
    }

    #[test]
    fn test_matches_file_no_filter_passes_all() {
        let filter = FilterCriteria::default();
        let file = make_file(Some("image"), Some(3));
        assert!(filter.matches_file(&file, &[], &[], false));
    }

    #[test]
    fn test_matches_file_ancestor_match_bypasses_rating_and_tag_filters() {
        let filter = FilterCriteria {
            rating: RatingFilter::MinRating(5),
            tags: vec!["rare".to_string()],
            video_only: false,
        };
        let file = make_file(Some("image"), None);
        // ancestor_matches=true should bypass rating and tag checks
        assert!(filter.matches_file(&file, &[], &[], true));
    }

    #[test]
    fn test_matches_file_video_only_applies_even_with_ancestor_match() {
        let filter = FilterCriteria {
            rating: RatingFilter::Any,
            tags: vec![],
            video_only: true,
        };
        let image_file = make_file(Some("image"), None);
        let video_file = make_file(Some("video"), None);
        // video_only should always apply, even with ancestor match
        assert!(!filter.matches_file(&image_file, &[], &[], true));
        assert!(filter.matches_file(&video_file, &[], &[], true));
    }

    #[test]
    fn test_matches_file_rating_unrated() {
        let filter = FilterCriteria {
            rating: RatingFilter::Unrated,
            ..Default::default()
        };
        assert!(filter.matches_file(&make_file(Some("image"), None), &[], &[], false));
        assert!(!filter.matches_file(&make_file(Some("image"), Some(3)), &[], &[], false));
    }

    #[test]
    fn test_matches_file_rating_min() {
        let filter = FilterCriteria {
            rating: RatingFilter::MinRating(3),
            ..Default::default()
        };
        assert!(!filter.matches_file(&make_file(Some("image"), None), &[], &[], false));
        assert!(!filter.matches_file(&make_file(Some("image"), Some(2)), &[], &[], false));
        assert!(filter.matches_file(&make_file(Some("image"), Some(3)), &[], &[], false));
        assert!(filter.matches_file(&make_file(Some("image"), Some(5)), &[], &[], false));
    }

    #[test]
    fn test_matches_file_tag_and_logic_with_dir_tags() {
        let filter = FilterCriteria {
            tags: vec!["sunset".to_string(), "family".to_string()],
            ..Default::default()
        };
        let file = make_file(Some("image"), None);
        // File has "sunset", dir has "family" → AND satisfied
        assert!(filter.matches_file(&file, &["sunset".to_string()], &["family".to_string()], false));
        // File has "sunset" only → AND not satisfied
        assert!(!filter.matches_file(&file, &["sunset".to_string()], &[], false));
    }

    // ==================== TagInputState::apply_toggle Tests ====================

    #[test]
    fn test_apply_toggle_removes_tag() {
        let mut input = TagInputState::new_with_current(
            vec!["outdoor".to_string(), "portrait".to_string()],
            vec!["outdoor".to_string()],
        );
        input.apply_toggle("outdoor", true);
        assert!(!input.current_tags.contains(&"outdoor".to_string()));
    }

    #[test]
    fn test_apply_toggle_adds_tag() {
        let mut input = TagInputState::new_with_current(
            vec!["outdoor".to_string(), "portrait".to_string()],
            vec![],
        );
        input.apply_toggle("portrait", false);
        assert!(input.current_tags.contains(&"portrait".to_string()));
        assert!(input.current_tags.is_sorted());
    }

    #[test]
    fn test_apply_toggle_registers_new_tag_in_all_tags() {
        let mut input = TagInputState::new_with_current(
            vec!["outdoor".to_string()],
            vec![],
        );
        input.apply_toggle("brand_new", false);
        assert!(input.all_tags.contains(&"brand_new".to_string()));
        assert!(input.current_tags.contains(&"brand_new".to_string()));
    }

    #[test]
    fn test_apply_toggle_clears_input_and_preserves_browse_selection() {
        let mut input = TagInputState::new_with_current(
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
            vec![],
        );
        input.editing = false;
        input.input_selected = false;
        input.selected_index = 1;
        input.input = "be".to_string();

        input.apply_toggle("beta", false);

        assert!(input.input.is_empty());
        // Was browsing (input was not empty before), so selected_index resets via update_filter
        // But since input was non-empty, it acts like typing mode → selected_index resets to 0
    }

    #[test]
    fn test_apply_toggle_browse_mode_keeps_cursor_position() {
        let mut input = TagInputState::new_with_current(
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
            vec![],
        );
        input.editing = false;
        input.input_selected = false;
        // Empty input = browse mode
        input.input.clear();
        input.update_filter();
        // update_filter resets selected_index, so set position after
        input.selected_index = 1;

        input.apply_toggle("beta", false);

        // Browse mode (empty input): cursor position preserved (clamped)
        assert_eq!(input.selected_index, 1);
    }
}
