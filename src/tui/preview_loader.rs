use image::DynamicImage;
use ratatui::layout::Rect;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{FilterType, Resize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;

use crate::thumbnails::{apply_exif_orientation, is_image_file};
use super::widgets::create_protocol;

/// Request to load an image in the background
pub struct LoadRequest {
    pub path: PathBuf,
    pub preview_path: PathBuf,
    pub is_thumbnail: bool,
    pub dir_id: i64,
    generation: u64,
}

/// Result of a background load — always carries both an image and a protocol
/// (or neither, on decode failure).
pub struct LoadResult {
    pub path: PathBuf,
    pub image: Option<Arc<DynamicImage>>,
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
/// - Never blocks the UI thread on `image::open()` or `resize_encode()`
/// - Skips stale loads from previous directories or navigation positions
/// - Processes requests sequentially in a dedicated thread
/// - Always creates a render protocol after decoding (stored in cache for instant render)
pub struct PreviewLoader {
    load_tx: Sender<LoadRequest>,
    result_rx: Receiver<LoadResult>,
    current_dir_id: Arc<AtomicI64>,
    /// Last-known preview render area, packed as (width << 16 | height).
    /// Shared with the worker so it can pre-encode images at the right size.
    preview_area: Arc<AtomicU32>,
    /// Monotonic generation counter for within-directory staleness detection.
    /// Bumped when the selection moves to an uncached file, causing the worker
    /// to skip loads queued for the previous position.
    load_generation: Arc<AtomicU64>,
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
        let (load_tx, load_rx) = channel::<LoadRequest>();
        let (result_tx, result_rx) = channel::<LoadResult>();
        let current_dir_id = Arc::new(AtomicI64::new(-1));
        let preview_area = Arc::new(AtomicU32::new(0));
        let load_generation = Arc::new(AtomicU64::new(0));
        let dir_id_clone = Arc::clone(&current_dir_id);
        let area_clone = Arc::clone(&preview_area);
        let gen_clone = Arc::clone(&load_generation);

        // Spawn worker thread
        thread::spawn(move || {
            worker_loop(load_rx, result_tx, dir_id_clone, area_clone, gen_clone);
        });

        Self {
            load_tx,
            result_rx,
            current_dir_id,
            preview_area,
            load_generation,
            pending: std::collections::HashSet::new(),
        }
    }

    /// Create a PreviewLoader with injected channels (for testing without a worker thread).
    #[cfg(test)]
    fn with_channels(
        load_tx: Sender<LoadRequest>,
        result_rx: Receiver<LoadResult>,
    ) -> Self {
        Self {
            load_tx,
            result_rx,
            current_dir_id: Arc::new(AtomicI64::new(-1)),
            preview_area: Arc::new(AtomicU32::new(0)),
            load_generation: Arc::new(AtomicU64::new(0)),
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

    /// Bump the load generation, invalidating all pending load requests.
    /// The worker will skip stale-generation loads without decoding.
    /// Called when the selection moves to an uncached file.
    pub fn bump_load_generation(&mut self) {
        self.load_generation.fetch_add(1, Ordering::Relaxed);
        self.pending.clear();
    }

    /// Number of in-flight load requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Queue an image to be loaded in the background.
    /// The worker always creates a render protocol after decoding.
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

        let generation = self.load_generation.load(Ordering::Relaxed);
        let request = LoadRequest {
            path: path.clone(),
            preview_path,
            is_thumbnail,
            dir_id,
            generation,
        };

        if self.load_tx.send(request).is_ok() {
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

/// Worker thread that processes load requests.
/// Blocks on recv() — no busy-polling needed since there's only one channel.
fn worker_loop(
    load_rx: Receiver<LoadRequest>,
    result_tx: Sender<LoadResult>,
    current_dir_id: Arc<AtomicI64>,
    preview_area: Arc<AtomicU32>,
    load_generation: Arc<AtomicU64>,
) {
    while let Ok(req) = load_rx.recv() {
        handle_load(req, &result_tx, &current_dir_id, &load_generation, &preview_area);
    }
}

/// Maximum cached image dimensions (pixels). Images larger than this are
/// downscaled before caching to keep protocol creation fast.
/// 1920×1440 covers any realistic terminal preview area with headroom.
const MAX_CACHE_WIDTH: u32 = 1920;
const MAX_CACHE_HEIGHT: u32 = 1440;

/// Downscale an image if it exceeds the maximum cache dimensions.
/// Thumbnails and small images pass through unchanged.
fn downscale_for_cache(image: DynamicImage) -> DynamicImage {
    if image.width() > MAX_CACHE_WIDTH || image.height() > MAX_CACHE_HEIGHT {
        image.resize(
            MAX_CACHE_WIDTH,
            MAX_CACHE_HEIGHT,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    }
}

/// Handle a load-from-disk request: decode image and create a render protocol.
/// Stale requests (wrong directory or generation) are silently dropped.
fn handle_load(
    request: LoadRequest,
    result_tx: &Sender<LoadResult>,
    current_dir_id: &Arc<AtomicI64>,
    load_generation: &Arc<AtomicU64>,
    preview_area: &Arc<AtomicU32>,
) {
    // Skip stale requests — pending was already cleared by the caller
    if request.dir_id != current_dir_id.load(Ordering::Relaxed) {
        return;
    }
    if request.generation != load_generation.load(Ordering::Relaxed) {
        return;
    }

    // Load and decode the image
    let image = load_image(&request.preview_path, request.is_thumbnail);

    // Re-check after decode — directory or generation may have changed
    if request.dir_id != current_dir_id.load(Ordering::Relaxed) {
        return;
    }
    if request.generation != load_generation.load(Ordering::Relaxed) {
        return;
    }

    let arc_image = image.map(|img| Arc::new(downscale_for_cache(img)));

    // Always create a protocol so the cache entry is render-ready
    let protocol = arc_image
        .as_ref()
        .and_then(|img| make_pre_encoded_protocol(img, preview_area));

    let _ = result_tx.send(LoadResult {
        path: request.path,
        image: arc_image,
        protocol,
    });
}

/// Create a protocol from an image and pre-encode at the current preview area size
fn make_pre_encoded_protocol(
    image: &DynamicImage,
    preview_area: &Arc<AtomicU32>,
) -> Option<Box<dyn StatefulProtocol>> {
    // clone() on DynamicImage is required here — the protocol takes ownership.
    // This runs on the worker thread so it doesn't block the UI.
    let mut proto = create_protocol(image.clone())?;

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
        let (load_tx, load_rx) = channel::<LoadRequest>();
        let (result_tx, result_rx) = channel::<LoadResult>();
        let loader = PreviewLoader::with_channels(load_tx, result_rx);
        (loader, load_rx, result_tx)
    }

    #[test]
    fn test_queue_load_tracks_pending() {
        let (mut loader, _load_rx, _result_tx) = test_loader();

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
        let (mut loader, _load_rx, _result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        assert!(loader.queue_load(path.clone(), path.clone(), false, 1));
        assert!(!loader.queue_load(path.clone(), path.clone(), false, 1));
    }

    #[test]
    fn test_set_current_dir_clears_pending() {
        let (mut loader, _load_rx, _result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        loader.queue_load(path.clone(), path.clone(), false, 1);
        assert!(loader.is_pending(&path));

        loader.set_current_dir(2);
        assert!(!loader.is_pending(&path));
    }

    #[test]
    fn test_poll_results_clears_pending() {
        let (mut loader, _load_rx, result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        loader.queue_load(path.clone(), path.clone(), false, 1);
        assert!(loader.is_pending(&path));

        // Simulate the worker sending back a result
        result_tx
            .send(LoadResult {
                path: path.clone(),
                image: None,
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
        let (loader, _load_rx, _result_tx) = test_loader();

        // Initially zero (unknown)
        assert_eq!(loader.preview_area.load(Ordering::Relaxed), 0);

        loader.set_preview_area(120, 40);
        let packed = loader.preview_area.load(Ordering::Relaxed);
        assert_eq!(unpack_area(packed), (120, 40));
    }

    #[test]
    fn test_queue_load_sends_to_channel() {
        let (mut loader, load_rx, _result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        loader.queue_load(path.clone(), path.clone(), false, 1);

        // Should appear on load channel with current generation
        let req = load_rx.try_recv().unwrap();
        assert_eq!(req.path, path);
        assert_eq!(req.generation, 0);
    }

    #[test]
    fn test_downscale_for_cache_shrinks_large_images() {
        let large = DynamicImage::new_rgb8(4000, 3000);
        let result = downscale_for_cache(large);
        assert!(result.width() <= MAX_CACHE_WIDTH);
        assert!(result.height() <= MAX_CACHE_HEIGHT);
        // Aspect ratio preserved: 4000×3000 → 1920×1440
        assert_eq!(result.width(), 1920);
        assert_eq!(result.height(), 1440);
    }

    #[test]
    fn test_downscale_for_cache_preserves_small_images() {
        let small = DynamicImage::new_rgb8(200, 150);
        let result = downscale_for_cache(small);
        assert_eq!(result.width(), 200);
        assert_eq!(result.height(), 150);
    }

    #[test]
    fn test_bump_load_generation_clears_pending_and_increments() {
        let (mut loader, _load_rx, _result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        loader.queue_load(path.clone(), path.clone(), false, 1);
        assert!(loader.is_pending(&path));
        assert_eq!(loader.pending_count(), 1);

        loader.bump_load_generation();

        assert!(!loader.is_pending(&path));
        assert_eq!(loader.pending_count(), 0);
    }

    #[test]
    fn test_bump_generation_allows_requeue() {
        let (mut loader, load_rx, _result_tx) = test_loader();

        let path = PathBuf::from("/photos/img001.jpg");
        assert!(loader.queue_load(path.clone(), path.clone(), false, 1));
        // Duplicate blocked
        assert!(!loader.queue_load(path.clone(), path.clone(), false, 1));

        // Bump clears pending — same file can be re-queued with new generation
        loader.bump_load_generation();
        assert!(loader.queue_load(path.clone(), path.clone(), false, 1));

        // First request has generation 0, second has generation 1
        let req1 = load_rx.try_recv().unwrap();
        assert_eq!(req1.generation, 0);
        let req2 = load_rx.try_recv().unwrap();
        assert_eq!(req2.generation, 1);
    }
}
