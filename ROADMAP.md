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
- [x] Auto-tagging orientation (landscape/portrait) during sync

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
- [ ] EXIF metadata extraction

### Organization
- [ ] Directory reorganization tools (rename with metadata preservation)
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
