use image::DynamicImage;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;

use super::widgets::{apply_exif_orientation, is_image_file};

/// Request to load an image in the background
pub struct LoadRequest {
    pub path: PathBuf,
    pub preview_path: PathBuf,
    pub is_thumbnail: bool,
    pub dir_id: i64,
}

/// Result of a background image load
pub struct LoadResult {
    pub path: PathBuf,
    pub image: Option<DynamicImage>,
}

/// Background image loader that runs image decoding off the main thread.
///
/// Key features:
/// - Never blocks the UI thread on `image::open()`
/// - Skips stale loads from previous directories
/// - Processes requests sequentially in a dedicated thread
pub struct PreviewLoader {
    request_tx: Sender<LoadRequest>,
    result_rx: Receiver<LoadResult>,
    current_dir_id: Arc<AtomicI64>,
    /// Paths that are currently being loaded (to avoid duplicate requests)
    pending: std::collections::HashSet<PathBuf>,
}

impl Default for PreviewLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewLoader {
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<LoadRequest>();
        let (result_tx, result_rx) = channel::<LoadResult>();
        let current_dir_id = Arc::new(AtomicI64::new(-1));
        let dir_id_clone = Arc::clone(&current_dir_id);

        // Spawn worker thread
        thread::spawn(move || {
            worker_loop(request_rx, result_tx, dir_id_clone);
        });

        Self {
            request_tx,
            result_rx,
            current_dir_id,
            pending: std::collections::HashSet::new(),
        }
    }

    /// Set the current directory ID. Loads from other directories will be skipped.
    pub fn set_current_dir(&mut self, dir_id: i64) {
        self.current_dir_id.store(dir_id, Ordering::Relaxed);
        // Clear pending set since we're in a new directory
        self.pending.clear();
    }

    /// Queue an image to be loaded in the background.
    /// Returns true if queued, false if already pending.
    pub fn queue_load(
        &mut self,
        path: PathBuf,
        preview_path: PathBuf,
        is_thumbnail: bool,
        dir_id: i64,
    ) -> bool {
        if self.pending.contains(&path) {
            return false;
        }

        let request = LoadRequest {
            path: path.clone(),
            preview_path,
            is_thumbnail,
            dir_id,
        };

        if self.request_tx.send(request).is_ok() {
            self.pending.insert(path);
            true
        } else {
            false
        }
    }

    /// Check if a path is currently being loaded
    pub fn is_pending(&self, path: &PathBuf) -> bool {
        self.pending.contains(path)
    }

    /// Poll for completed image loads. Returns all available results.
    pub fn poll_results(&mut self) -> Vec<LoadResult> {
        let mut results = Vec::new();
        loop {
            match self.result_rx.try_recv() {
                Ok(result) => {
                    self.pending.remove(&result.path);
                    results.push(result);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        results
    }
}

/// Worker thread that processes load requests
fn worker_loop(
    request_rx: Receiver<LoadRequest>,
    result_tx: Sender<LoadResult>,
    current_dir_id: Arc<AtomicI64>,
) {
    while let Ok(request) = request_rx.recv() {
        // Check if this request is still relevant (same directory)
        let current = current_dir_id.load(Ordering::Relaxed);
        if request.dir_id != current {
            // Stale request - skip it but still notify so pending is cleared
            let _ = result_tx.send(LoadResult {
                path: request.path,
                image: None,
            });
            continue;
        }

        // Load the image
        let image = load_image(&request.preview_path, request.is_thumbnail);

        // Check again before sending - directory might have changed while loading
        let current = current_dir_id.load(Ordering::Relaxed);
        if request.dir_id != current {
            let _ = result_tx.send(LoadResult {
                path: request.path,
                image: None,
            });
            continue;
        }

        let _ = result_tx.send(LoadResult {
            path: request.path,
            image,
        });
    }
}

/// Load an image, applying EXIF orientation if needed
fn load_image(preview_path: &PathBuf, is_thumbnail: bool) -> Option<DynamicImage> {
    let img = image::open(preview_path).ok()?;

    // Apply EXIF orientation only for original files (thumbnails have it baked in)
    Some(if is_thumbnail || !is_image_file(preview_path) {
        img
    } else {
        apply_exif_orientation(preview_path, img)
    })
}
