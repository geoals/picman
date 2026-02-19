use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::db::Directory;
use crate::scanner::detect_orientation;

use super::dialogs::OperationsMenuState;
use super::state::AppState;

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

/// Progress tracker for background operations
pub struct BackgroundProgress {
    pub operation: OperationType,
    pub total: usize,
    pub completed: Arc<AtomicUsize>,
    pub done: Arc<AtomicBool>,
    pub cancelled: Arc<AtomicBool>,
    pub start_time: Instant,
}

/// Background operation and operations menu methods on AppState
impl AppState {
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
        self.force_redraw = true;
    }

    /// Recursively collect all descendant directory IDs
    pub(crate) fn collect_descendant_dir_ids(&self, parent_id: i64, result: &mut Vec<i64>) {
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
                3 => OperationType::DirPreview,
                4 => OperationType::DirPreviewRecursive,
                _ => return,
            }
        } else {
            return;
        };

        self.operations_menu = None;
        self.run_operation(operation);
    }

    // ==================== Background Operation Execution ====================

    /// Run a background operation on current directory and all subdirectories
    pub fn run_operation(&mut self, operation: OperationType) {
        // If an operation is already running, queue this one
        if self.background_progress.is_some() {
            self.operation_queue.push_back(operation);
            self.status_message = Some(format!(
                "Queued {} ({} in queue)",
                operation.label(),
                self.operation_queue.len()
            ));
            return;
        }

        // Handle directory preview operations separately
        if matches!(operation, OperationType::DirPreview | OperationType::DirPreviewRecursive) {
            self.run_dir_preview_operation(operation);
            return;
        }

        // Collect files that need processing (skips already-processed ones)
        let file_data = self.collect_files_for_operation(operation);
        if file_data.is_empty() {
            self.status_message = Some("Nothing to do - all files already processed".to_string());
            return;
        }

        // Set up progress tracking
        let completed = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));

        self.background_progress = Some(BackgroundProgress {
            operation,
            total: file_data.len(),
            completed: Arc::clone(&completed),
            done: Arc::clone(&done),
            cancelled: Arc::clone(&cancelled),
            start_time: Instant::now(),
        });

        let db_path = self.library_path.join(".picman.db");

        // Spawn background thread for parallel processing
        std::thread::spawn(move || {
            use rayon::prelude::*;

            match operation {
                OperationType::Thumbnails => {
                    use crate::thumbnails::{generate_image_thumbnail, generate_video_thumbnail, is_image_file, is_video_file};

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
                    parallel_compute_serial_write(
                        &file_data, &cancelled, &completed, &db_path,
                        |path| detect_orientation(path),
                        |db, file_id, tag| { let _ = db.add_file_tag(file_id, tag); },
                    );
                }
                OperationType::Hash => {
                    use crate::hash::compute_file_hash;
                    parallel_compute_serial_write(
                        &file_data, &cancelled, &completed, &db_path,
                        |path| compute_file_hash(path).ok(),
                        |db, file_id, hash| { let _ = db.set_file_hash(file_id, &hash); },
                    );
                }
                OperationType::DirPreview | OperationType::DirPreviewRecursive => {}
            }

            done.store(true, Ordering::Relaxed);
        });
    }

    /// Collect files from selected directory and descendants that need processing.
    ///
    /// Iterates all files under the selected directory, builds full paths,
    /// and filters based on what the operation needs (e.g., skipping files
    /// that already have thumbnails, hashes, or orientation tags).
    fn collect_files_for_operation(&self, operation: OperationType) -> Vec<(i64, PathBuf)> {
        use crate::thumbnails::{has_thumbnail, is_image_file, is_video_file};

        let selected_dir = match self.get_selected_directory() {
            Some(d) => d.clone(),
            None => return Vec::new(),
        };

        let mut dir_ids = vec![selected_dir.id];
        self.collect_descendant_dir_ids(selected_dir.id, &mut dir_ids);

        let mut file_data = Vec::new();
        for dir_id in &dir_ids {
            let Ok(files) = self.db.get_files_in_directory(*dir_id) else {
                continue;
            };

            let dir_path = self.tree.directories.iter()
                .find(|d| d.id == *dir_id)
                .map(|d| d.path.clone())
                .unwrap_or_default();

            for file in files {
                let path = if dir_path.is_empty() {
                    self.library_path.join(&file.filename)
                } else {
                    self.library_path.join(&dir_path).join(&file.filename)
                };

                let include = match operation {
                    OperationType::Thumbnails => {
                        (is_image_file(&path) || is_video_file(&path)) && !has_thumbnail(&path)
                    }
                    OperationType::Orientation => {
                        if !is_image_file(&path) {
                            false
                        } else {
                            let tags = self.db.get_file_tags(file.id).unwrap_or_default();
                            !tags.contains(&"landscape".to_string())
                                && !tags.contains(&"portrait".to_string())
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

        file_data
    }

    /// Run directory preview generation (single or recursive)
    fn run_dir_preview_operation(&mut self, operation: OperationType) {
        use crate::db::Database;
        use crate::thumbnails::{
            collect_preview_images_standalone, generate_dir_preview_from_paths, TempPreviewState,
        };
        use crate::tui::widgets::generate_dir_preview;

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
            self.dir_preview_cache.borrow_mut().clear();
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
            start_time: Instant::now(),
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

    // ==================== Background Progress Management ====================

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
                let queue_len = self.operation_queue.len();

                if was_cancelled {
                    // Clear queue on cancel
                    self.operation_queue.clear();
                    self.status_message = Some(format!(
                        "Cancelled - {} {}",
                        completed,
                        progress.operation.done_label()
                    ));
                } else if queue_len > 0 {
                    self.status_message = Some(format!(
                        "{} {} ({} more queued)",
                        completed,
                        progress.operation.done_label(),
                        queue_len
                    ));
                } else {
                    self.status_message = Some(format!(
                        "{} {}",
                        completed,
                        progress.operation.done_label()
                    ));
                }
                self.background_progress = None;
                // Clear preview caches to reload
                self.preview_cache.borrow_mut().clear();
                *self.missing_preview_cache.borrow_mut() = None;

                // Start next queued operation if any
                if let Some(next_op) = self.operation_queue.pop_front() {
                    self.run_operation(next_op);
                }
            }
        }
    }
}

/// Process files in parallel and write results to DB serially.
///
/// This is the shared pattern for Orientation and Hash operations:
/// 1. Compute a value for each file using rayon (respecting cancellation)
/// 2. Collect successful results
/// 3. Write them to the database serially (SQLite is single-writer)
fn parallel_compute_serial_write<T: Send>(
    file_data: &[(i64, PathBuf)],
    cancelled: &AtomicBool,
    completed: &AtomicUsize,
    db_path: &Path,
    compute: impl Fn(&Path) -> Option<T> + Send + Sync,
    write: impl Fn(&crate::db::Database, i64, T),
) {
    use rayon::prelude::*;

    let results: Vec<(i64, T)> = file_data
        .par_iter()
        .filter_map(|(file_id, path)| {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            let value = compute(path);
            completed.fetch_add(1, Ordering::Relaxed);
            value.map(|v| (*file_id, v))
        })
        .collect();

    if !cancelled.load(Ordering::Relaxed) {
        if let Ok(db) = crate::db::Database::open(db_path) {
            for (file_id, value) in results {
                write(&db, file_id, value);
            }
        }
    }
}
