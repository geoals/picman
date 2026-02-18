# Picman Roadmap

## Implemented

### Phase 1: Foundation
- [x] Database schema (directories, files, tags)
- [x] Init command (scan filesystem, populate DB)
- [x] Transaction-based bulk inserts (fast init)

### Phase 2: Core Operations
- [x] Sync command with change detection (added/removed/modified)
- [x] Hash computation (xxHash, streaming, resumable)
- [x] CLI commands: list, rate, tag (1-5 star ratings)

### Phase 3: Basic TUI
- [x] App shell with event loop
- [x] Directory browser (tree navigation with expand/collapse)
- [x] Inline image preview (Kitty graphics protocol)
- [x] Vim-style keyboard navigation (j/k/h/l)
- [x] 3-column layout (tree | files | preview)
- [x] File list with scrolling
- [x] Wrap-around navigation

### Phase 4: TUI Actions
- [x] Rating in TUI (1-5/asdfg keys, works on files and directories)
- [x] Tagging in TUI (t key, popup with autocomplete)
- [x] Filtering by rating/tag/video (m key, AND logic for multiple tags)
- [x] Orientation tagging (landscape/portrait, EXIF-aware)

### Phase 4.5: Batch Operations
- [x] Operations menu (o key) for batch processing
- [x] Thumbnail generation with disk cache (~/.cache/picman/thumbnails/)
- [x] Orientation tagging via TUI (recursive, parallel)
- [x] Hash computation via TUI (recursive, parallel)
- [x] Background processing with progress bar
- [x] Graceful cancellation on quit

### Phase 4.6: Directory Management
- [x] Directory preview generation (composite image from subdirs/files)
- [x] Directory preview CLI command (`picman previews`)
- [x] Thumbnail CLI command (`picman thumbnails`)
- [x] Directory rename with word suggestions from subdirs
- [x] Filter popup UX improvements (auto-apply, better navigation)

### Phase 4.7: Performance & Polish
- [x] Background preview loading (non-blocking image decode in worker thread)
- [x] LRU preview cache (bounded memory, 200 items)
- [x] Smooth scrolling with deferred file loading and skip-preview
- [x] Preload adjacent files for instant display
- [x] Batch tag fetching (single query per directory)
- [x] Mouse support (click, double-click, scroll wheel)
- [x] Vim-style navigation in tag and filter popups (j/k/h/l/i modes)
- [x] Operation queue (sequential execution, +N queued indicator)
- [x] Rich progress indicators (visual bar, spinner, elapsed, ETA)
- [x] Status CLI command (`picman status`)
- [x] Repair CLI command (`picman repair`)
- [x] `--check` flag for thumbnails and previews commands
- [x] Tracing infrastructure for debugging

### Phase 4.8: Details & Search
- [x] Empty directory guard (no focus switch to empty file list)
- [x] Thumbnail indicator in file list (`*` marker)
- [x] Image dimensions stored in database (header-only read via `imagesize`)
- [x] Database migration system (`PRAGMA user_version`)
- [x] Compact details panel reformat (one item per line, dimensions display)
- [x] Expanded details panel (`i` key) with EXIF data (camera, lens, exposure, GPS)
- [x] Incremental search (`/` key, LazyVim-style) for directories and files

## Planned (Core)

### Phase 5: Culling Workflow
- [ ] Culling mode for directories
- [ ] Random sample display
- [ ] Keep/cull actions with visual state

### Phase 6: Duplicate Detection
- [ ] Find duplicates by hash
- [ ] Duplicates panel in TUI
- [ ] CLI output for scripting

### Phase 7: Views & Portability
- [ ] Symlink view creation (preserve hierarchy)
- [ ] Export to sidecar JSON
- [ ] Import from sidecar JSON

## Planned (Extended Features)

### Subject Management
- [ ] **Subject aliasing** - Link multiple names/aliases to same subject
  - Database: `subjects` table + `subject_aliases` table
  - UI: Manage aliases, view all content for a subject regardless of folder name
  - Auto-detect: Suggest potential aliases based on folder proximity/similarity

### Smart Tagging
- [ ] **Auto-suggest tags from folder names**
  - Extract keywords and named entities from directory names
  - Pattern matching for common naming formats
  - Present suggestions during rating/browsing, user confirms with single key

### Image Analysis (Future)
- [ ] Similar image detection (perceptual hashing - pHash/dHash)
- [ ] Same scene detection (ML-based, different angles)
- [ ] Face detection/grouping
- [x] EXIF orientation reading (for correct preview/tagging)

### Organization
- [x] Directory rename with metadata preservation (r key in TUI)
- [ ] Batch rename based on patterns
- [ ] Move/merge duplicate directories

### Media Support
- [x] Video thumbnail extraction (via ffmpeg)
- [x] Video preview in TUI (thumbnails)
- [ ] GIF handling

## Won't Implement (Out of Scope)

- Cloud sync
- Photo editing
- Social features
