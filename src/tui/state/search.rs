use std::collections::HashSet;

use super::{AppState, FileWithTags, Focus};

impl AppState {
    /// Get visible directories considering both filter and search.
    /// When search is active on the tree, filters by directory name and keeps ancestors.
    pub fn get_search_visible_directories(&self) -> Vec<&crate::db::Directory> {
        let dirs = self.get_visible_directories();
        if self.search.query.is_empty() || self.focus != Focus::DirectoryTree {
            return dirs;
        }

        // Find IDs of directories whose name matches the search
        let matching_ids: HashSet<i64> = dirs
            .iter()
            .filter(|d| {
                let name = d.path.rsplit('/').next().unwrap_or(&d.path);
                self.search.matches(name)
            })
            .map(|d| d.id)
            .collect();

        // Include ancestors of matching directories to preserve tree structure
        let mut visible_ids = matching_ids.clone();
        for &id in &matching_ids {
            for pid in self.tree.ancestor_ids(id) {
                if !visible_ids.insert(pid) {
                    break; // Already visited this ancestor
                }
            }
        }

        dirs.into_iter()
            .filter(|d| visible_ids.contains(&d.id))
            .collect()
    }

    /// Get visible files considering the current search filter
    pub fn get_visible_files(&self) -> Vec<&FileWithTags> {
        if self.search.query.is_empty() {
            self.file_list.files.iter().collect()
        } else {
            self.file_list
                .files
                .iter()
                .filter(|f| self.search.matches(&f.file.filename))
                .collect()
        }
    }

    /// Get the display index mapping from visible (search-filtered) indices to underlying file_list indices
    pub fn visible_file_indices(&self) -> Vec<usize> {
        if self.search.query.is_empty() {
            (0..self.file_list.files.len()).collect()
        } else {
            self.file_list
                .files
                .iter()
                .enumerate()
                .filter(|(_, f)| self.search.matches(&f.file.filename))
                .map(|(i, _)| i)
                .collect()
        }
    }
}
