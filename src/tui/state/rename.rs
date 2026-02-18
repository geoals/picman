use anyhow::Result;

use crate::suggestions::extract_suggested_words;

use super::{AppState, Focus, RenameDialogState};

impl AppState {
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

        // Invalidate caches (protocols are in preview_cache, cleared with it)
        self.preview_cache.borrow_mut().clear();
        self.dir_preview_cache.borrow_mut().clear();
        *self.render_protocol.borrow_mut() = None;

        Ok(())
    }
}
