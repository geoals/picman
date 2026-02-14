# Picman Roadmap

## Implemented

- [x] Database schema (directories, files, tags)
- [x] Init command (scan filesystem, populate DB)
- [x] Transaction-based bulk inserts (fast init)

## In Progress

- [ ] Sync command (detect filesystem changes)

## Planned (Core)

### Phase 2: Core Operations
- [ ] Sync command with change detection (added/removed/modified)
- [ ] Hash computation (xxHash, parallel, resumable)
- [ ] CLI commands: list, rate, tag

### Phase 3: Basic TUI
- [ ] App shell with event loop
- [ ] Directory browser (tree navigation)
- [ ] Inline image preview (Kitty graphics protocol)
- [ ] Vim-style keyboard navigation

### Phase 4: TUI Actions
- [ ] Rating in TUI (0-9 keys)
- [ ] Tagging in TUI
- [ ] Filtering by rating/tag

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
- [ ] Video thumbnail extraction
- [ ] Video preview in TUI
- [ ] GIF handling

## Won't Implement (Out of Scope)

- Cloud sync
- Photo editing
- Social features
