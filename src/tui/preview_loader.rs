use image::DynamicImage;
use ratatui::layout::Rect;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{FilterType, Resize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;

use super::widgets::{apply_exif_orientation, create_protocol, is_image_file};

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
    pub protocol: Option<Box<dyn StatefulProtocol>>,
}

fn pack_area(width: u16, height: u16) -> u32 {
    (width as u32) << 16 | height as u32
}

fn unpack_area(packed: u32) -> (u16, u16) {
    ((packed >> 16) as u16, packed as u16)
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
    /// Last-known preview render area, packed as (width << 16 | height).
    /// Shared with the worker so it can pre-encode images at the right size.
    preview_area: Arc<AtomicU32>,
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
        let preview_area = Arc::new(AtomicU32::new(0));
        let dir_id_clone = Arc::clone(&current_dir_id);
        let area_clone = Arc::clone(&preview_area);

        // Spawn worker thread
        thread::spawn(move || {
            worker_loop(request_rx, result_tx, dir_id_clone, area_clone);
        });

        Self {
            request_tx,
            result_rx,
            current_dir_id,
            preview_area,
            pending: std::collections::HashSet::new(),
        }
    }

    /// Create a PreviewLoader with injected channels (for testing without a worker thread).
    #[cfg(test)]
    fn with_channels(
        request_tx: Sender<LoadRequest>,
        result_rx: Receiver<LoadResult>,
    ) -> Self {
        Self {
            request_tx,
            result_rx,
            current_dir_id: Arc::new(AtomicI64::new(-1)),
            preview_area: Arc::new(AtomicU32::new(0)),
            pending: std::collections::HashSet::new(),
        }
    }

    /// Set the current directory ID. Loads from other directories will be skipped.
    pub fn set_current_dir(&mut self, dir_id: i64) {
        self.current_dir_id.store(dir_id, Ordering::Relaxed);
        // Clear pending set since we're in a new directory
        self.pending.clear();
    }

    /// Update the preview render area hint so the worker can pre-encode at the right size.
    pub fn set_preview_area(&self, width: u16, height: u16) {
        self.preview_area.store(pack_area(width, height), Ordering::Relaxed);
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
    pub fn is_pending(&self, path: &Path) -> bool {
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
    preview_area: Arc<AtomicU32>,
) {
    while let Ok(request) = request_rx.recv() {
        // Check if this request is still relevant (same directory)
        let current = current_dir_id.load(Ordering::Relaxed);
        if request.dir_id != current {
            // Stale request - skip it but still notify so pending is cleared
            let _ = result_tx.send(LoadResult {
                path: request.path,
                protocol: None,
            });
            continue;
        }

        // Load and decode the image
        let image = load_image(&request.preview_path, request.is_thumbnail);

        // Check again before sending - directory might have changed while loading
        let current = current_dir_id.load(Ordering::Relaxed);
        if request.dir_id != current {
            let _ = result_tx.send(LoadResult {
                path: request.path,
                protocol: None,
            });
            continue;
        }

        // Create protocol and pre-encode for the current preview area
        let protocol = image.and_then(|img| {
            let mut proto = create_protocol(img)?;

            // Pre-encode at the last-known preview area size to avoid
            // resize_encode jank on the first render
            let packed = preview_area.load(Ordering::Relaxed);
            if packed != 0 {
                let (w, h) = unpack_area(packed);
                proto.resize_encode(
                    &Resize::Fit(Some(FilterType::Lanczos3)),
                    None,
                    Rect::new(0, 0, w, h),
                );
            }

            Some(proto)
        });

        let _ = result_tx.send(LoadResult {
            path: request.path,
            protocol,
        });
    }
}

/// Load an image, applying EXIF orientation if needed
fn load_image(preview_path: &Path, is_thumbnail: bool) -> Option<DynamicImage> {
    let img = image::open(preview_path).ok()?;

    // Apply EXIF orientation only for original files (thumbnails have it baked in)
    Some(if is_thumbnail || !is_image_file(preview_path) {
        img
    } else {
        apply_exif_orientation(preview_path, img)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    /// Create a test loader with injected channels, returning the loader
    /// plus the request receiver and result sender for driving tests.
    fn test_loader() -> (
        PreviewLoader,
        Receiver<LoadRequest>,
        Sender<LoadResult>,
    ) {
        let (request_tx, request_rx) = channel::<LoadRequest>();
        let (result_tx, result_rx) = channel::<LoadResult>();
        let loader = PreviewLoader::with_channels(request_tx, result_rx);
        (loader, request_rx, result_tx)
    }

    #[test]
    fn test_queue_load_tracks_pending() {
        let (mut loader, _request_rx, _result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        let queued = loader.queue_load(
            path.clone(),
            path.clone(),
            false,
            1,
        );

        assert!(queued);
        assert!(loader.is_pending(&path));
    }

    #[test]
    fn test_queue_load_deduplicates() {
        let (mut loader, _request_rx, _result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        assert!(loader.queue_load(path.clone(), path.clone(), false, 1));
        assert!(!loader.queue_load(path.clone(), path.clone(), false, 1));
    }

    #[test]
    fn test_set_current_dir_clears_pending() {
        let (mut loader, _request_rx, _result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        loader.queue_load(path.clone(), path.clone(), false, 1);
        assert!(loader.is_pending(&path));

        loader.set_current_dir(2);
        assert!(!loader.is_pending(&path));
    }

    #[test]
    fn test_poll_results_clears_pending() {
        let (mut loader, _request_rx, result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        loader.queue_load(path.clone(), path.clone(), false, 1);
        assert!(loader.is_pending(&path));

        // Simulate the worker sending back a result
        result_tx
            .send(LoadResult {
                path: path.clone(),
                protocol: None,
            })
            .unwrap();

        let results = loader.poll_results();
        assert_eq!(results.len(), 1);
        assert!(!loader.is_pending(&path));
    }

    #[test]
    fn test_pack_unpack_area_round_trip() {
        assert_eq!(unpack_area(pack_area(120, 40)), (120, 40));
        assert_eq!(unpack_area(pack_area(0, 0)), (0, 0));
        assert_eq!(unpack_area(pack_area(u16::MAX, u16::MAX)), (u16::MAX, u16::MAX));
    }

    #[test]
    fn test_set_preview_area() {
        let (loader, _request_rx, _result_tx) = test_loader();

        // Initially zero (unknown)
        assert_eq!(loader.preview_area.load(Ordering::Relaxed), 0);

        loader.set_preview_area(120, 40);
        let packed = loader.preview_area.load(Ordering::Relaxed);
        assert_eq!(unpack_area(packed), (120, 40));
    }
}
