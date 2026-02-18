# Serve Module — Web UI

Axum web server serving a REST API + embedded SPA for browsing and managing a photo library.

## Module Structure

```
src/serve/
├── mod.rs          — Router setup, AppState, run_serve() entry point
├── handlers.rs     — All request handlers + AppError type + spawn_db helper
├── models.rs       — JSON request/response structs (serde)
└── assets/         — Embedded SPA (rust_embed, no build step)
    ├── index.html  — Shell HTML, loads app.js as ES module
    ├── app.js      — Entry point, wires modules together, runs init
    ├── state.js    — Shared application state object
    ├── api.js      — HTTP helpers, data fetching (returns data, never renders)
    ├── tree.js     — Directory tree rendering, navigation, breadcrumb
    ├── grid.js     — Photo grid, infinite scroll, zoom controls
    ├── lightbox.js — Full-screen photo viewer
    ├── tags.js     — Rating stars, directory tag editing, sidebar tag chips
    ├── filters.js  — Rating/tag filter coordination
    ├── style.css   — CSS entry point (@import manifest)
    ├── base.css    — Reset, design tokens, layout shell, loading states, scrollbars
    ├── sidebar.css — Sidebar container, title, filter controls
    ├── tree.css    — Directory tree items
    ├── toolbar.css — Toolbar, breadcrumb, zoom controls, file count
    ├── grid.css    — Photo grid, masonry columns, cells, overlays
    ├── tags.css    — Rating stars, tag chips, tag input/autocomplete
    └── lightbox.css — Full-screen viewer
```

## Architecture

- **Runtime**: Builds its own `tokio::runtime::Runtime` in `run_serve()` — called from sync context in `main.rs`
- **State**: `Arc<AppState>` shared across handlers, containing `Arc<Mutex<Database>>` and `library_path: PathBuf`
- **DB access**: All database work runs via `spawn_db()` — a helper that calls `tokio::task::spawn_blocking` to avoid blocking the async runtime. Takes a closure `FnOnce(&Database) -> anyhow::Result<T>`
- **Assets**: SPA files embedded at compile time via `#[derive(Embed)]` on the `Assets` struct. Fallback handler serves `index.html` for SPA routing. JS uses native ES modules (`import`/`export`), CSS uses `@import` — no build step needed. The API layer returns data without calling renderers; callers handle rendering after checking the result

## API Routes

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| GET | `/api/health` | `health` | Health check |
| GET | `/api/directories` | `get_directories` | All directories with tags and file counts |
| GET | `/api/directories/{id}/files` | `get_directory_files` | Paginated files in directory (`?page=&per_page=&recursive=`) |
| PUT | `/api/directories/{id}/rating` | `set_directory_rating` | Set/clear rating (body: `{"rating": 1-5 or null}`) |
| POST | `/api/directories/{id}/tags` | `add_directory_tag` | Add tag (body: `{"tag": "name"}`, lowercased) |
| DELETE | `/api/directories/{id}/tags/{tag_name}` | `remove_directory_tag` | Remove tag |
| GET | `/api/tags` | `get_tags` | All tags with file/directory counts |
| GET | `/api/files` | `get_filtered_files` | Filter files (`?rating=&tag=&page=&per_page=`) |
| GET | `/thumb/{file_id}` | `serve_web_thumbnail` | Cached thumbnail JPEG |
| GET | `/preview/{file_id}` | `serve_preview` | Larger preview JPEG |
| GET | `/dir-preview/{dir_id}` | `serve_dir_preview` | Directory preview JPEG |
| GET | `/original/{*path}` | `serve_original` | Original file (path-traversal protected) |
| GET | `/*` (fallback) | `serve_embedded_asset` | SPA static assets |

## Database Dependency (`crate::db`)

The module depends on `Database` from `src/db/`. Key types and methods used:

```rust
// src/db/schema.rs
pub struct Database { /* wraps rusqlite::Connection */ }
impl Database {
    pub fn open(path: &Path) -> Result<Self>
    pub fn open_in_memory() -> Result<Self>       // used in tests
    pub fn connection(&self) -> &Connection        // raw access for custom queries in handlers
}

// src/db/directories.rs
pub struct Directory {
    pub id: i64,
    pub path: String,
    pub parent_id: Option<i64>,
    pub rating: Option<i32>,
    pub mtime: Option<i64>,
}
impl Database {
    pub fn get_all_directories(&self) -> Result<Vec<Directory>>
    pub fn get_directory(&self, id: i64) -> Result<Option<Directory>>
    pub fn insert_directory(&self, path: &str, parent_id: Option<i64>, mtime: Option<i64>) -> Result<i64>
    pub fn set_directory_rating(&self, id: i64, rating: Option<i32>) -> Result<()>
}

// src/db/files.rs
pub struct File {
    pub id: i64,
    pub directory_id: i64,
    pub filename: String,
    pub size: i64,
    pub mtime: i64,
    pub hash: Option<String>,
    pub rating: Option<i32>,
    pub media_type: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}
impl Database {
    pub fn get_all_files(&self) -> Result<Vec<File>>
}

// src/db/tags.rs
impl Database {
    pub fn get_all_directory_tags(&self) -> Result<HashMap<i64, Vec<String>>>
    pub fn get_directory_tags(&self, directory_id: i64) -> Result<Vec<String>>
    pub fn add_directory_tag(&self, directory_id: i64, tag_name: &str) -> Result<()>
    pub fn remove_directory_tag(&self, directory_id: i64, tag_name: &str) -> Result<()>
}
```

Handlers also run raw SQL queries via `db.connection()` for pagination and filtering (see `get_directory_files`, `get_filtered_files`, `get_tags`, `batch_get_file_tags`).

## Thumbnails Dependency (`crate::thumbnails`)

```rust
pub fn get_preview_path_for_file(file_path: &Path) -> Option<(PathBuf, bool)>
pub fn get_cached_dir_preview(dir_id: i64) -> Option<PathBuf>
```

Thumbnails are pre-generated by CLI commands and cached to `~/.cache/picman/`. The serve module only reads them.

## Error Handling

`AppError` enum in `handlers.rs` with three variants:

- `NotFound` → 404
- `BadRequest(String)` → 400 with message body
- `Internal(String)` → 500 (logs to stderr)

## Testing

Tests live in `mod.rs` and use `tower::ServiceExt::oneshot` to send requests through the router without binding a port. Helper `test_state()` creates an in-memory database. Run with:

```sh
cargo test --lib serve
```

## Key Crates

- **axum** — HTTP framework (Router, handlers, extractors)
- **tokio** — Async runtime
- **rust_embed** — Compile-time asset embedding
- **serde / serde_json** — JSON serialization
- **rusqlite** — SQLite (via `db.connection()` for raw queries)
- **mime_guess** — Content-type detection for file serving
- **tower** — `ServiceExt::oneshot` in tests
