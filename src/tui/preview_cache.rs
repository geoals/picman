use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::DynamicImage;

/// Cached image preview state — stores the decoded image behind an Arc
/// so it can be cheaply shared with the background worker for protocol creation.
pub struct PreviewCache {
    pub path: PathBuf,
    pub image: Arc<DynamicImage>,
}

impl PreviewCache {
    pub fn new(path: PathBuf, image: Arc<DynamicImage>) -> Self {
        Self { path, image }
    }
}

/// LRU cache for decoded image previews
/// Holds up to `max_size` entries, evicting least-recently-used when full.
/// Designed for ~200 decoded images (~1GB memory).
pub struct LruPreviewCache {
    /// Map of path -> cached preview
    entries: HashMap<PathBuf, PreviewCache>,
    /// Access order: most recently used at back, least at front
    access_order: VecDeque<PathBuf>,
    /// Maximum number of entries
    max_size: usize,
}

impl LruPreviewCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::new(),
            access_order: VecDeque::new(),
            max_size,
        }
    }

    /// Insert a preview into the cache, evicting oldest if over capacity
    pub fn insert(&mut self, path: PathBuf, image: Arc<DynamicImage>) {
        // If already in cache, remove from access order (will re-add at end)
        if self.entries.contains_key(&path) {
            self.access_order.retain(|p| p != &path);
        }

        // Evict if at capacity
        while self.entries.len() >= self.max_size && !self.access_order.is_empty() {
            if let Some(oldest) = self.access_order.pop_front() {
                self.entries.remove(&oldest);
            }
        }

        // Insert new entry
        self.entries.insert(path.clone(), PreviewCache::new(path.clone(), image));
        self.access_order.push_back(path);
    }

    /// Get a cached preview without updating access order (read-only peek)
    pub fn get(&self, path: &Path) -> Option<&PreviewCache> {
        self.entries.get(path)
    }

    /// Get a cached preview, updating access order
    pub fn get_mut(&mut self, path: &Path) -> Option<&mut PreviewCache> {
        if self.entries.contains_key(path) {
            // Update access order: move to back (most recent)
            self.access_order.retain(|p| p.as_path() != path);
            self.access_order.push_back(path.to_path_buf());
            self.entries.get_mut(path)
        } else {
            None
        }
    }

    /// Check if cache contains a path
    pub fn contains(&self, path: &Path) -> bool {
        self.entries.contains_key(path)
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Maximum number of entries this cache can hold
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Get the most recently accessed entry (for showing stale preview during rapid scroll)
    pub fn get_last_accessed_mut(&mut self) -> Option<&mut PreviewCache> {
        self.access_order.back().and_then(|path| self.entries.get_mut(path))
    }
}

/// Cached directory preview state (composite image)
pub struct DirectoryPreviewCache {
    pub dir_id: i64,
    pub image: Arc<DynamicImage>,
}

impl DirectoryPreviewCache {
    pub fn new(dir_id: i64, image: Arc<DynamicImage>) -> Self {
        Self { dir_id, image }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_image() -> Arc<DynamicImage> {
        Arc::new(DynamicImage::new_rgb8(1, 1))
    }

    #[test]
    fn test_lru_preview_cache_basic() {
        let cache = LruPreviewCache::new(3);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        assert!(!cache.contains(&PathBuf::from("test.jpg")));
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = LruPreviewCache::new(3);

        cache.insert(PathBuf::from("a.jpg"), mock_image());
        cache.insert(PathBuf::from("b.jpg"), mock_image());
        cache.insert(PathBuf::from("c.jpg"), mock_image());
        assert_eq!(cache.len(), 3);

        // Inserting a 4th should evict the oldest ("a.jpg")
        cache.insert(PathBuf::from("d.jpg"), mock_image());
        assert_eq!(cache.len(), 3);
        assert!(!cache.contains(&PathBuf::from("a.jpg")));
        assert!(cache.contains(&PathBuf::from("b.jpg")));
        assert!(cache.contains(&PathBuf::from("c.jpg")));
        assert!(cache.contains(&PathBuf::from("d.jpg")));
    }

    #[test]
    fn test_lru_access_order() {
        let mut cache = LruPreviewCache::new(3);

        cache.insert(PathBuf::from("a.jpg"), mock_image());
        cache.insert(PathBuf::from("b.jpg"), mock_image());
        cache.insert(PathBuf::from("c.jpg"), mock_image());

        // Access "a.jpg" — makes it most-recently-used
        cache.get_mut(Path::new("a.jpg"));

        // Insert a 4th — should evict "b.jpg" (the least recently used)
        cache.insert(PathBuf::from("d.jpg"), mock_image());
        assert_eq!(cache.len(), 3);
        assert!(cache.contains(&PathBuf::from("a.jpg")));
        assert!(!cache.contains(&PathBuf::from("b.jpg")));
        assert!(cache.contains(&PathBuf::from("c.jpg")));
        assert!(cache.contains(&PathBuf::from("d.jpg")));
    }

    #[test]
    fn test_get_last_accessed() {
        let mut cache = LruPreviewCache::new(3);

        cache.insert(PathBuf::from("a.jpg"), mock_image());
        cache.insert(PathBuf::from("b.jpg"), mock_image());

        // Last inserted is "b.jpg"
        let last = cache.get_last_accessed_mut().unwrap();
        assert_eq!(last.path, PathBuf::from("b.jpg"));

        // Access "a.jpg" — now it becomes the last accessed
        cache.get_mut(Path::new("a.jpg"));
        let last = cache.get_last_accessed_mut().unwrap();
        assert_eq!(last.path, PathBuf::from("a.jpg"));
    }
}
