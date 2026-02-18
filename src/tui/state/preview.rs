use super::AppState;

impl AppState {
    /// Poll for completed background preview loads and insert into cache.
    /// Called at the start of each render cycle.
    /// Routes results to the appropriate cache based on `is_dir_preview`.
    /// Also preloads other files in the current directory.
    pub fn poll_preview_results(&self) {
        let results = self.preview_loader.borrow_mut().poll_results();

        for result in results {
            let mut cache = if result.is_dir_preview {
                self.dir_preview_cache.borrow_mut()
            } else {
                self.preview_cache.borrow_mut()
            };

            if let Some(image) = result.image {
                cache.insert(result.path.clone(), image, result.protocol);
            } else if let Some(protocol) = result.protocol {
                cache.set_protocol(&result.path, protocol);
            }
        }

        // Preload other files in the directory (runs on every poll, not just after loads,
        // so that entering a new directory triggers preloading immediately)
        self.preload_directory_files();
    }

    /// Preload image/video files near the selected file in the current directory.
    ///
    /// Starts from the selected index and wraps forward, so the selected file is
    /// always first in the queue. Limits total pending to the LRU cache size to
    /// avoid wasted decode work and the infinite preload-evict-requeue loop.
    ///
    /// When the selection moves to an uncached file, bumps the load generation
    /// so the worker skips stale requests from the previous position instantly.
    ///
    /// No disk I/O happens here — extension checks are pure string ops, and
    /// thumbnail path resolution is deferred to the worker thread.
    fn preload_directory_files(&self) {
        use crate::thumbnails::{is_image_file, is_video_file};

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
        let total = self.file_list.files.len();
        if total == 0 {
            return;
        }

        let selected_idx = self.file_list.selected_index;
        let max_pending = cache.max_size();

        // If selected file isn't cached, bump generation to invalidate stale
        // preloads from a previous position — the worker skips them instantly.
        let selected_file = &self.file_list.files[selected_idx];
        let selected_path = dir.file_path(&self.library_path, &selected_file.file.filename);

        if !cache.contains(&selected_path) && !loader.is_pending(&selected_path) {
            loader.bump_load_generation();
        }

        // Queue files starting from selected index, wrapping around.
        // Selected file is always first → processed before neighbors.
        // Limit total (cached + pending) to cache size to avoid an eviction cycle
        // where preloads evict the selected file's protocol causing flicker.
        let indices = (selected_idx..total).chain(0..selected_idx);
        for idx in indices {
            if cache.len() + loader.pending_count() >= max_pending {
                break;
            }

            let file_with_tags = &self.file_list.files[idx];
            let file_path = dir.file_path(&self.library_path, &file_with_tags.file.filename);

            // Extension-only filter — no stat() calls
            if !is_image_file(&file_path) && !is_video_file(&file_path) {
                continue;
            }

            if cache.contains(&file_path) || loader.is_pending(&file_path) {
                continue;
            }

            loader.queue_file_load(file_path, dir_id);
        }
    }

    /// Update EXIF cache if details are expanded and selection changed
    pub fn refresh_exif_cache(&mut self) {
        if !self.details_expanded {
            return;
        }
        let Some(path) = self.selected_file_path() else {
            return;
        };
        let needs_read = self
            .cached_exif
            .as_ref()
            .map(|(p, _)| p != &path)
            .unwrap_or(true);
        if needs_read {
            let info = super::super::exif::read_exif(&path);
            self.cached_exif = Some((path, info));
        }
    }
}
