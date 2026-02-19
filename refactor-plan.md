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

## Completed Investigations

### 6. `cli/sync.rs` Step C — ✅ INVESTIGATED (no merge needed)

**Finding:** After the helpers extraction in Step B, the two strategies are fundamentally different in purpose and structure:

- `sync_database_incremental` (~190 lines) — optimized for speed: scans dirs first, only stats files in changed dirs, tracks mtime, has quick-exit path. No move detection.
- `sync_database` (~280 lines) — optimized for correctness: full FS scan, ~150 lines of move-detection logic (basename matching, metadata/tag/thumbnail preservation). Essential for users who reorganize their library.

**Shared structure** (already extracted): `insert_new_directory`, `upsert_file`, `get_or_create_root_dir`, `resolve_parent_id`. The remaining bodies are dominated by strategy-specific logic.

**Decision:** Leave separate. Merging would require many conditional branches and make the code harder to follow — the wrong abstraction. Both functions are well within manageable size.

**Note:** `--full` sync remains valuable because incremental sync does not detect directory moves (metadata/tags/thumbnails are lost when dirs are renamed). This is an acceptable speed-vs-completeness tradeoff.

## Remaining Work

None — all items investigated or completed. The codebase is in good shape at ~11,759 lines across 54 files, with the largest files either well-organized or already refactored.
