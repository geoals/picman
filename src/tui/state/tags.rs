use anyhow::Result;

use super::{AppState, Focus, TagInputState};

impl AppState {
    /// Open the tag input popup with current item's tags for toggle display
    pub fn open_tag_input(&mut self) -> Result<()> {
        let all_tags = self.db.get_all_tags()?;
        let current_tags = match self.focus {
            Focus::FileList => {
                self.file_list
                    .selected_file()
                    .map(|f| f.tags.clone())
                    .unwrap_or_default()
            }
            Focus::DirectoryTree => {
                if let Some(dir) = self.get_selected_directory() {
                    self.db.get_directory_tags(dir.id)?
                } else {
                    Vec::new()
                }
            }
        };
        self.tag_input = Some(TagInputState::new_with_current(all_tags, current_tags));
        Ok(())
    }

    /// Close the tag input popup without applying
    pub fn close_tag_input(&mut self) {
        self.tag_input = None;
        self.force_redraw = true;
    }

    /// Toggle the selected or entered tag (add if not present, remove if present).
    /// Keeps the popup open for multi-tag editing; closes only on empty input.
    pub fn toggle_tag(&mut self) -> Result<()> {
        let tag = if let Some(ref input) = self.tag_input {
            input
                .selected_tag()
                .cloned()
                .unwrap_or_else(|| input.input.clone())
        } else {
            return Ok(());
        };

        if tag.is_empty() {
            self.tag_input = None;
            self.force_redraw = true;
            return Ok(());
        }

        let is_applied = self
            .tag_input
            .as_ref()
            .is_some_and(|input| input.is_applied(&tag));

        match self.focus {
            Focus::DirectoryTree => {
                if let Some(dir) = self.get_selected_directory() {
                    let dir_id = dir.id;
                    if is_applied {
                        self.db.remove_directory_tag(dir_id, &tag)?;
                    } else {
                        self.db.add_directory_tag(dir_id, &tag)?;
                    }
                }
            }
            Focus::FileList => {
                if let Some(file_with_tags) =
                    self.file_list.files.get_mut(self.file_list.selected_index)
                {
                    if is_applied {
                        self.db.remove_file_tag(file_with_tags.file.id, &tag)?;
                        file_with_tags.tags.retain(|t| t != &tag);
                    } else {
                        self.db.add_file_tag(file_with_tags.file.id, &tag)?;
                        if !file_with_tags.tags.contains(&tag) {
                            file_with_tags.tags.push(tag.clone());
                            file_with_tags.tags.sort();
                        }
                    }
                }
            }
        }

        // Update popup state to reflect the toggle
        if let Some(ref mut input) = self.tag_input {
            input.apply_toggle(&tag, is_applied);
        }

        // Keep popup open (don't set self.tag_input = None)
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use super::super::Focus;

    #[test]
    fn test_open_tag_input_populates_current_tags_from_file() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::FileList;

        // Add a tag to the first file
        let file_id = state.file_list.files[0].file.id;
        state.db.add_file_tag(file_id, "outdoor").unwrap();
        state.file_list.files[0].tags = vec!["outdoor".to_string()];

        state.open_tag_input().unwrap();

        let tag_input = state.tag_input.as_ref().unwrap();
        assert_eq!(tag_input.current_tags, vec!["outdoor"]);
    }

    #[test]
    fn test_open_tag_input_populates_current_tags_from_directory() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::DirectoryTree;

        // Add a tag to the selected directory
        let dir_id = state.get_selected_directory().unwrap().id;
        state.db.add_directory_tag(dir_id, "vacation").unwrap();

        state.open_tag_input().unwrap();

        let tag_input = state.tag_input.as_ref().unwrap();
        assert_eq!(tag_input.current_tags, vec!["vacation"]);
    }

    #[test]
    fn test_toggle_tag_removes_existing_tag() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::FileList;

        // Add a tag to the first file
        let file_id = state.file_list.files[0].file.id;
        state.db.add_file_tag(file_id, "outdoor").unwrap();
        state.file_list.files[0].tags = vec!["outdoor".to_string()];

        // Open tag input and select "outdoor"
        state.open_tag_input().unwrap();
        // Type to filter to "outdoor" or navigate to it
        if let Some(ref mut input) = state.tag_input {
            input.input = "outdoor".to_string();
            input.update_filter();
        }

        state.toggle_tag().unwrap();

        // Tag should be removed from DB
        let db_tags = state.db.get_file_tags(file_id).unwrap();
        assert!(!db_tags.contains(&"outdoor".to_string()));

        // Tag should be removed from in-memory state
        assert!(!state.file_list.files[0].tags.contains(&"outdoor".to_string()));

        // Popup should remain open
        assert!(state.tag_input.is_some());

        // current_tags on popup should be updated
        let tag_input = state.tag_input.as_ref().unwrap();
        assert!(!tag_input.current_tags.contains(&"outdoor".to_string()));
    }

    #[test]
    fn test_toggle_tag_adds_new_tag() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::FileList;
        let file_id = state.file_list.files[0].file.id;

        // Open tag input and type a new tag
        state.open_tag_input().unwrap();
        if let Some(ref mut input) = state.tag_input {
            input.input = "vacation".to_string();
            input.update_filter();
        }

        state.toggle_tag().unwrap();

        // Tag should be added to DB
        let db_tags = state.db.get_file_tags(file_id).unwrap();
        assert!(db_tags.contains(&"vacation".to_string()));

        // Tag should be added to in-memory state
        assert!(state.file_list.files[0].tags.contains(&"vacation".to_string()));

        // Popup should remain open
        assert!(state.tag_input.is_some());

        // current_tags on popup should be updated
        let tag_input = state.tag_input.as_ref().unwrap();
        assert!(tag_input.current_tags.contains(&"vacation".to_string()));
    }

    #[test]
    fn test_toggle_tag_empty_input_closes_popup() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::FileList;

        state.open_tag_input().unwrap();
        // Leave input empty
        if let Some(ref mut input) = state.tag_input {
            input.input.clear();
            // No selected tag either — clear filtered tags
            input.filtered_tags.clear();
        }

        state.toggle_tag().unwrap();

        // Popup should close on empty input
        assert!(state.tag_input.is_none());
    }

    #[test]
    fn test_toggle_tag_preserves_selection_position() {
        let (mut state, _tempdir) = create_test_app_state();
        state.focus = Focus::FileList;

        // Create several tags so we have a list to navigate
        let file_id = state.file_list.files[0].file.id;
        state.db.add_file_tag(file_id, "alpha").unwrap();
        state.db.add_file_tag(file_id, "beta").unwrap();
        state.file_list.files[0].tags = vec!["alpha".to_string(), "beta".to_string()];

        state.open_tag_input().unwrap();

        // Navigate to the second tag in browse mode (no text input)
        if let Some(ref mut input) = state.tag_input {
            input.editing = false;
            input.input_selected = false;
            input.selected_index = 1;
        }

        let tag_at_1 = state.tag_input.as_ref().unwrap().filtered_tags[1].clone();
        state.toggle_tag().unwrap();

        // Selection should stay at the same index, not jump to 0
        let tag_input = state.tag_input.as_ref().unwrap();
        assert_eq!(
            tag_input.selected_index, 1,
            "Selection should stay at index 1 after toggle, but was {}",
            tag_input.selected_index,
        );
        // The tag at that index should still exist in the list
        assert_eq!(tag_input.filtered_tags[1], tag_at_1);
    }
}
