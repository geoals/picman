# Picman Refactoring Plan

## Code Size Audit (updated 2026-02-19)

All line counts use `cloc` (code lines only — excludes blanks and comments).

Total Rust: **11,759 lines** (54 files)

### Largest Files

| File | Code | Status |
|------|------|--------|
| `tui/dialogs.rs` | 1,245 | ✅ Investigated — well-organized, no action needed |
| `cli/sync.rs` | 717 | ✅ Steps A+B done (was 940). Step C remaining |
| `serve/handlers.rs` | 534 | Reasonable — REST handlers |
| `tui/app.rs` | 500 | Reasonable — key dispatch |
| `scanner.rs` | 493 | ✅ Investigated — no overlap with sync |
| `thumbnails.rs` | 446 | Reasonable — image + video processing |
| `db/files.rs` | 445 | Reasonable — file queries |
| `tui/preview_loader.rs` | 415 | Reasonable — background image loading |
| `tui/state/mod.rs` | 360 | ✅ Done (was 1,361) |
| `tui/operations.rs` | 354 | ✅ Done (was 476) |
| `tui/widgets/details_panel.rs` | 330 | Reasonable |

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

### 2. `cli/sync.rs` — ✅ Steps A+B DONE

**Was:** 940 code lines with post-processing mixed in and duplicated patterns.

**Result (Steps A+B, commit cdbe23b):**
- Extracted post-processing to `cli/post_process.rs` (250 code lines)
- Extracted shared helpers: `resolve_parent_id`, `get_or_create_root_dir`, `insert_new_directory`, `upsert_file`
- sync.rs now 717 code lines

### 3. `tui/dialogs.rs` — ✅ INVESTIGATED (no action needed)

**Finding:** ~660 code + ~800 tests. Five distinct dialog types with no meaningful duplication. A trait would be forced abstraction.

### 4. `tui/operations.rs` — ✅ DONE

**Was:** 476 code lines with delegation antipatterns and duplicated parallel processing.

**Result:**
- Moved menu navigation onto `OperationsMenuState` (consistent with other dialog types)
- Extracted `parallel_compute_serial_write()` helper (deduplicates Orientation/Hash pattern)
- Extracted `collect_files_for_operation()` from 176-line `run_operation()`

### 5. `scanner.rs` — ✅ INVESTIGATED (no action needed)

**Finding:** No overlap with sync. Scanner provides pure FS traversal (`walkdir` + media classification); sync delegates to Scanner then does DB reconciliation. They are complementary, not redundant. Only `init.rs` and `sync.rs` use the `Scanner` struct; other callers (`post_process.rs`, `tui/operations.rs`) use standalone image metadata functions only (`detect_orientation`, `read_dimensions`).

## Remaining Work

### 6. `cli/sync.rs` Step C — Investigate merging sync strategies

**Current state:** sync.rs has 717 code lines with two separate strategies:
- `sync_database_incremental` (~190 lines, line 199) — mtime-based change detection, selective file scanning
- `sync_database` (~320 lines, line 393) — full scan, compares all FS entries against DB

Both now use the shared helpers from Step B (`resolve_parent_id`, `get_or_create_root_dir`, `insert_new_directory`, `upsert_file`).

**Investigation needed:**
1. Read both strategies end-to-end now that helpers are extracted
2. Identify remaining duplication between the two
3. Determine if they can share a common structure (e.g., a single function parameterized by "which dirs/files to scan") or if the strategies are fundamentally different enough to stay separate
4. Check if `sync_database` (full) is still needed at all — incremental is now the default (commit cbbd80c), full sync is only available via `--full` flag

**Decision criteria:**
- If there's substantial remaining duplication → merge into parameterized function
- If the strategies are mostly different after helpers extraction → leave separate (they're already manageable at ~190 and ~320 lines)
- If full sync is rarely used → consider simplifying or removing it

**How to start:** Read `src/cli/sync.rs` lines 199-end, compare the two strategy functions structurally.
