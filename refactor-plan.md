# Picman Refactoring Plan

## Code Size Audit (updated after state.rs refactor)

All line counts use `cloc` (code lines only — excludes blanks and comments).

Total Rust: **11,638 lines**

### Module Distribution

| Module | Files | Code | % of total |
|--------|-------|------|------------|
| TUI (non-widgets) | 15 | 4,415 | 38% |
| CLI | 10 | 2,030 | 17% |
| TUI widgets | 9 | 1,529 | 13% |
| Database | 6 | 1,413 | 12% |
| Standalone (scanner, thumbnails, hash, suggestions) | 4 | 1,085 | 9% |
| Web server | 3 | 849 | 7% |
| Core (main, lib, logging) | 3 | 317 | 3% |

### Largest Files

| File | Code | Concern |
|------|------|---------|
| `tui/dialogs.rs` | 1,186 | Grew after state.rs refactor; multiple dialog types |
| `cli/sync.rs` | 940 | Post-processing ops don't belong here; duplicated patterns |
| `serve/handlers.rs` | 524 | REST handlers |
| `tui/app.rs` | 500 | Key dispatch |
| `scanner.rs` | 456 | Filesystem traversal |
| `thumbnails.rs` | 446 | Image + ffmpeg processing |
| `db/files.rs` | 445 | File queries |
| `tui/preview_loader.rs` | 415 | Background image loading |
| `tui/operations.rs` | 371 | Background operation queue + AppState methods |
| `tui/state/mod.rs` | 360 | Core AppState (down from 1,361) |

## Completed Refactorings

### 1. `tui/state.rs` — ✅ DONE

**Was:** 1,361-line god object with ~350 lines of delegation methods.

**Result:** Split into module directory `tui/state/` with 8 focused files:

| File | Code | Responsibility |
|------|------|---------------|
| `mod.rs` | 360 | Core AppState struct, TreeState, FileListState, Focus |
| `navigation.rs` | 206 | Directory tree navigation (enter/leave/move) |
| `tags.rs` | 179 | Tag popup state and operations |
| `files.rs` | 156 | File list state and operations |
| `preview.rs` | 75 | Preview loading state |
| `rename.rs` | 81 | Rename dialog state |
| `filter.rs` | 64 | Filter dialog state |
| `search.rs` | 53 | Search state |

Key improvements:
- Removed delegation bloat — dialog methods called directly
- Extracted `Directory::file_path()` and `Directory::full_path()` helpers
- Each file has a single, clear responsibility

## Modules Needing Refactoring

### 2. `cli/sync.rs` — NEXT UP

**Problem:** 940 code lines. The file has three distinct concerns mixed together:

1. **Sync orchestration** (~50 lines) — `run_sync`, `run_sync_incremental`, `run_sync_impl`
2. **Sync strategies** (~570 lines) — `sync_database` (full, 4 phases) and `sync_database_incremental` (9 phases)
3. **Post-processing operations** (~130 lines) — `backfill_dimensions`, `tag_orientation`, `hash_files`
4. **Tests** (~190 code lines)

**Issues found:**

- **Post-processing doesn't belong here.** `backfill_dimensions`, `tag_orientation`, and `hash_files` are not sync-specific — they fill in NULL columns for any files, regardless of how they were added. They could be standalone CLI commands or shared utilities.

- **Duplicated patterns between full and incremental sync:**
  - Parent directory lookup (appears 4+ times)
  - Root-level files handling (`""` directory edge case, 2 times)
  - New directory insertion logic (2 times)
  - File add/update logic (nearly identical in both strategies)
  - File/directory deletion logic (2 times)

- **Batch processing pattern** duplicated across `hash_files` and `tag_orientation` (both do rayon parallel + chunked DB transactions)

**Plan:**

**Step A — Extract post-processing to `cli/post_process.rs`:**
- Move `backfill_dimensions`, `tag_orientation`, `hash_files` to new module
- These are optionally chained after sync but are independent operations
- Extract shared batch-processing helper if the pattern is clean
- Expected: sync.rs loses ~130 lines of code + their tests

**Step B — Extract shared sync helpers:**
- `get_or_insert_directory(db, path, parent_id, mtime, cache)` — the repeated parent lookup + insert pattern
- `get_or_insert_root_dir(db, cache)` — the `""` directory edge case
- `upsert_file(db, dir_id, file, dimensions_fn)` — the file add/update logic
- `delete_missing_dirs(db, ids)` / `delete_missing_files(db, ids)` — bulk deletion
- These can live as private helpers in sync.rs or in a `cli/sync_helpers.rs`
- Expected: removes ~100-150 lines of duplication between the two strategies

**Step C — Consider merging strategies (investigate):**
- Incremental sync is fundamentally different (mtime-based skip), so full merge may not make sense
- But the shared helpers from Step B should make both strategies shorter and more readable
- After Steps A+B, reassess whether further extraction is needed

**Expected result:** sync.rs drops from ~940 to ~650-700 code lines, with post-processing in its own module.

### 3. `tui/dialogs.rs` — ✅ INVESTIGATED (no action needed)

**Was:** 1,186 code lines — largest file in the codebase.

**Finding:** ~660 lines of code + ~800 lines of thorough tests. Five dialog types (FilterDialogState, TagInputState, RenameDialogState, SearchState, OperationsMenuState) are genuinely distinct with no meaningful duplication. The shared `sort_prefix_first()` helper is already extracted. A trait would be a forced abstraction — each dialog has unique semantics (multi-section focus, edit/browse modes, UTF-8 cursor positioning). Splitting into per-dialog modules would add boilerplate without improving readability.

### 4. `tui/operations.rs` — ✅ DONE

**Was:** 476 code lines with delegation antipatterns and duplicated parallel processing structure.

**Result:** Three focused improvements:

- **Step A:** Moved `move_up()` / `move_down()` to `OperationsMenuState` in `dialogs.rs` with tests, consistent with how all other dialog types own their navigation. Removed delegation methods from AppState.
- **Step B:** Extracted `parallel_compute_serial_write()` helper — deduplicates the "rayon parallel compute → serial SQLite write" pattern shared by Orientation and Hash operations.
- **Step C:** Extracted `collect_files_for_operation()` from the 176-line `run_operation()`. The main function now reads as a clear pipeline: queue check → dispatch dir preview → collect files → setup progress → spawn thread.

### 5. `scanner.rs` — TODO

**Problem:** 456 code lines. May have overlap with sync logic now that sync does its own filesystem/DB reconciliation.

## Modules That Look Reasonable

- **Database layer** (1,413 code lines across 6 files) — well-split by concern
- **TUI widgets** (1,529 code lines across 9 files) — ~170 lines/file average, reasonable
- **Web server** (849 code lines across 3 files) — appropriate for REST API + router
- **Core** (317 code lines) — fine
- **thumbnails.rs** (446 code lines) — image + video processing justifies the size
- **preview_loader.rs** (415 code lines) — channel-based background loading, reasonable
