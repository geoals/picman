use anyhow::Result;

use super::{AppState, FilterCriteria, FilterDialogState, RatingFilter};

impl AppState {
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
    pub(super) fn update_matching_directories(&mut self) -> Result<()> {
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
}
