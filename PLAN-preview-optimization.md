# Plan: Preview Loading Optimization

## Goals
1. Smooth scrolling through files (no stutter when holding j/k)
2. Responsive directory navigation (no blocking when entering folders)
3. Bounded memory usage (don't cache unlimited previews)
4. Preload nearby files for instant display when scrolling

## Key Insights from First Attempt

1. **The main thread must never call `image::open()`** - This is the primary source of blocking/stutter
2. **Track directory, not file** - For skip logic, we care about "is this from the current directory?" not "is this the exact current file?"
3. **Clear file cache on directory change** - Prevents showing wrong preview when entering a new folder
4. **LRU eviction is essential** - Without it, memory grows unbounded
5. **Deferred file loading helps** - Wait until all keypresses are processed before loading file list

## Implementation Steps (in order)

### Step 1: Batch Tag Fetching (isolated, safe)

**File:** `src/db/queries.rs`

Add `get_file_tags_for_directory(dir_id) -> HashMap<file_id, Vec<String>>` that fetches all tags for files in a directory with a single query instead of N queries.

**Why first:** Completely isolated change, no risk, immediate performance benefit for directories with many files.

**Test:** Load a directory with 100+ tagged files, verify it's faster.

---

### Step 2: LRU Cache Structures (isolated, safe)

**File:** `src/tui/state.rs`

Replace:
```rust
pub preview_cache: RefCell<Option<PreviewCache>>  // Single item
```

With:
```rust
pub preview_cache: RefCell<PreviewCache>  // HashMap + access_order VecDeque
```

Implement:
- `insert(path, protocol)` - Add to cache, evict oldest if > 300 items
- `get_mut(path)` - Get item, update access order
- `contains(path)` - Check if cached
- `clear()` - Reset cache

**Why second:** Just data structure change, no behavioral change yet. The render code will still load synchronously, but now it caches multiple items.

**Test:** Scroll through files, verify they stay cached (no re-load when scrolling back).

---

### Step 3: Deferred File List Loading

**File:** `src/tui/state.rs`, `src/tui/app.rs`

Add `files_dirty: bool` flag. When navigating directories:
- Set `files_dirty = true` instead of calling `load_files_for_selected_directory()`
- After event loop drains all pending keypresses, call `load_files_if_dirty()`

**Why third:** Reduces redundant DB queries when scrolling fast through directory tree. Still loads synchronously, but only once per "rest" position.

**Test:** Hold j/k in directory tree, verify smooth scrolling (file list updates after you stop).

---

### Step 4: Skip Preview During Navigation

**File:** `src/tui/state.rs`, `src/tui/widgets/preview.rs`

Add `skip_preview: bool` flag. When navigating:
- Set `skip_preview = true`
- Render function checks flag: if true, render cached image (don't load new one)
- Clear flag after render

**Why fourth:** Prevents blocking during rapid navigation. User sees stale preview while scrolling, but it doesn't stutter.

**Test:** Hold j/k in file list, verify smooth scrolling (preview updates after you stop).

---

### Step 5: Clear Cache on Directory Change

**File:** `src/tui/state.rs`

In `load_files_for_selected_directory()`:
```rust
*self.preview_cache.borrow_mut() = PreviewCache::new();
```

**Why fifth:** Prevents showing file preview from previous directory when entering a new folder.

**Test:** Enter folder A, scroll to file, go back, enter folder B - verify you don't see folder A's file preview.

---

### Step 6: Background Image Loading

**File:** `src/tui/widgets/preview.rs`

Create background loader:
```rust
struct BackgroundLoader {
    request_tx: Sender<LoadRequest>,
    result_rx: Receiver<LoadResult>,
    pending: HashSet<PathBuf>,
    current_dir_id: Arc<RwLock<Option<i64>>>,
}
```

Worker thread:
- Receives `LoadRequest { path, preview_path, apply_exif, dir_id }`
- Checks if `dir_id == current_dir_id` before loading (skip if stale)
- Sends back `LoadResult { path, image }` or `Skipped { path }`

Render function:
- Never calls `image::open()` directly
- If not cached: queue for background load, show fallback (last cached from same dir)
- Process completed loads at start of render

**Why sixth:** This is the big change. Do it last when everything else is stable.

**Test:**
- Scroll through files - should be smooth
- Enter new directory - should show folder preview until first file loads
- Hold j/k rapidly - should skip intermediate files, load final selection

---

### Step 7: Preload Adjacent Files

**File:** `src/tui/widgets/preview.rs`

After rendering current file, queue all files in directory for background loading:
```rust
fn queue_all_file_previews(state: &AppState, dir_id: i64) {
    for file in &state.file_list.files {
        if !cached && !pending {
            queue_file_load(path, preview_path, apply_exif, dir_id);
        }
    }
}
```

Worker processes sequentially. Current file loads first (queued first), then others preload.

**Why seventh:** Nice-to-have optimization. Only add after core loading works.

**Test:** Scroll to middle of directory, wait, scroll forward - images should appear instantly (preloaded).

---

## Architecture Diagram

```
┌─────────────────┐     ┌─────────────────┐
│   Main Thread   │     │  Worker Thread  │
├─────────────────┤     ├─────────────────┤
│                 │     │                 │
│ render_preview()│     │ loop {          │
│   │             │     │   recv request  │
│   ├─ check cache│     │   check dir_id  │
│   │             │     │   if stale: skip│
│   ├─ if miss:   │     │   image::open() │
│   │   queue_load├────►│   send result   │
│   │             │     │ }               │
│   ├─ process    │◄────┤                 │
│   │   results   │     │                 │
│   │             │     │                 │
│   └─ render     │     │                 │
│      cached img │     │                 │
│                 │     │                 │
└─────────────────┘     └─────────────────┘
        │                       │
        ▼                       │
┌─────────────────┐             │
│   LRU Cache     │◄────────────┘
│ (max 300 items) │  insert loaded images
└─────────────────┘
```

## Testing Checklist

After each step, verify:
- [ ] No regression in basic functionality
- [ ] Memory usage stays reasonable (watch with `htop`)
- [ ] Scrolling feels responsive
- [ ] Correct preview shows for selected item

## Notes

- Each step should be a separate commit
- Test thoroughly before moving to next step
- Steps 1-5 are low-risk, step 6-7 are higher complexity
- If step 6 causes issues, steps 1-5 still provide value independently
