# Picman Refactoring Plan

## Code Size Audit (2026-02-18)

Total Rust: **11,242 lines** (excluding blanks and comments)

### Module Distribution

| Module | Files | Lines | % of total |
|--------|-------|-------|------------|
| TUI (non-widgets) | 9 | 3,756 | 33% |
| CLI | 10 | 2,030 | 18% |
| TUI widgets | 9 | 1,537 | 14% |
| Database | 6 | 1,344 | 12% |
| Standalone (scanner, thumbnails, hash, suggestions) | 4 | 1,089 | 10% |
| Web server | 3 | 849 | 8% |
| Core (main, lib, logging) | 3 | 317 | 3% |

### Largest Files

| File | Lines | Concern |
|------|-------|---------|
| `tui/state.rs` | 1,361 | God object, delegation bloat |
| `cli/sync.rs` | 940 | Single command doing too many things |
| `tui/dialogs.rs` | 638 | May have duplication across dialog types |
| `tui/app.rs` | 522 | Key dispatch — will change with state.rs refactor |
| `scanner.rs` | 456 | Large for walkdir traversal |
| `thumbnails.rs` | 450 | Image + ffmpeg processing |
| `db/files.rs` | 445 | File queries |
| `tui/preview_loader.rs` | 415 | Background image loading |
| `tui/operations.rs` | 371 | Also extends AppState with delegation methods |

## Modules Needing Refactoring

### 1. `tui/state.rs` — IN PROGRESS

**Problem:** 1,361 lines. AppState is a god object with ~350 lines of delegation methods that forward calls to dialog sub-states. Also has duplicated patterns (path construction x3, ancestor traversal x3, mirrored move_up/move_down).

**Plan:** See `.claude/plans/abstract-bouncing-feather.md` for detailed steps. Summary:
- Move dialog-specific logic into dialog types in `dialogs.rs`
- Call dialog methods directly from `app.rs` (rename dialog already does this)
- Extract `Directory::file_path()`, `TreeState::ancestor_ids()` helpers
- Consolidate `move_up`/`move_down`
- Expected reduction: ~1,361 → ~970 lines

### 2. `cli/sync.rs` — TODO

**Problem:** 940 lines in a single CLI command file. Likely combines scanning, diffing, database updating, and progress reporting in one function. Needs investigation.

**Suspected issues:**
- Multiple responsibilities in one file
- Possible overlap with `scanner.rs` (456 lines)
- Progress reporting boilerplate that could share patterns with other CLI commands

### 3. `tui/operations.rs` — TODO

**Problem:** 371 lines, extends AppState with more delegation methods (`operations_menu_up/down/select`, `run_operation`, etc.). Same god-object pattern as state.rs.

**Suspected issues:**
- `operations_menu_up/down` are the same trivial delegation pattern
- `run_operation` spawns threads directly — might benefit from the same two-pass pattern

### 4. `tui/dialogs.rs` — TODO (after state.rs refactor)

**Problem:** 638 lines. Multiple dialog types may share patterns (input handling, navigation) that could be consolidated. Will grow when filter dialog methods move here from state.rs, but the new methods should be well-scoped.

**Needs investigation after state.rs refactor** to see the new shape.

### 5. `scanner.rs` — TODO

**Problem:** 456 lines for filesystem traversal seems high. May have overlap with `cli/sync.rs`.

## Modules That Look Reasonable

- **Database layer** (1,344 lines across 6 files) — well-split by concern
- **TUI widgets** (1,537 lines across 9 files) — ~170 lines/file average, reasonable
- **Web server** (849 lines across 3 files) — appropriate for REST API + router
- **Core** (317 lines) — fine
- **thumbnails.rs** (450 lines) — image + video processing justifies the size
- **preview_loader.rs** (415 lines) — channel-based background loading, reasonable
