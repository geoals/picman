# Picman

A fast, terminal-based photo library manager written in Rust. Designed for photographers who want to organize large media collections with ratings, tags, and filtering—all without leaving the terminal.

## Features

- **Interactive TUI** — Browse directories, preview images/videos, and manage metadata with vim-style navigation
- **Rating system** — 1-5 star ratings on files and directories
- **Tagging** — Add custom tags with autocomplete; filter by multiple tags (AND logic)
- **Fast scanning** — Parallel filesystem traversal with SQLite-backed metadata
- **Thumbnail caching** — Cached at 1440p for fast browsing; video thumbnails via ffmpeg
- **Directory previews** — Composite thumbnails showing directory contents at a glance
- **Orientation detection** — Auto-tag images as landscape/portrait using EXIF data
- **Duplicate detection** — xxHash3-64 file hashing for identifying duplicates
- **Batch operations** — Background processing with progress display and cancellation

### Supported Media

- **Images**: jpg, jpeg, png, gif, bmp, tiff, webp, heic, heif, raw (cr2, cr3, nef, arw, orf, rw2, dng, raf)
- **Videos**: mp4, mov, avi, mkv, wmv, flv, webm, m4v, 3gp, mts, m2ts

## Installation

### Prerequisites

- Rust 1.70+ and Cargo
- ffmpeg (optional, for video thumbnails)
- A terminal with image support (Kitty, iTerm2, or compatible)

### Build from source

```bash
git clone https://github.com/your-username/picman.git
cd picman
cargo build --release
```

The binary will be at `target/release/picman`.

## Quick Start

```bash
# Initialize a photo library
picman init /path/to/photos

# Launch the TUI browser
picman /path/to/photos

# Generate thumbnails for faster browsing
picman thumbnails /path/to/photos
```

### Basic TUI Navigation

| Key | Action |
|-----|--------|
| `j/k` or `↓/↑` | Move down/up |
| `h/l` or `←/→` | Collapse/expand or switch panes |
| `1-5` | Set rating |
| `t` | Add tag |
| `m` | Filter by rating/tags |
| `o` | Operations menu |
| `?` | Help |
| `q` | Quit |

## Documentation

- **[USAGE.md](USAGE.md)** — Complete guide to TUI keybindings and CLI commands
- **[ROADMAP.md](ROADMAP.md)** — Feature roadmap and planned enhancements

## Architecture

```
picman/
├── src/
│   ├── cli/        # CLI command implementations
│   ├── db/         # SQLite schema and queries
│   ├── tui/        # Terminal UI (ratatui)
│   │   └── widgets/  # Reusable UI components
│   ├── scanner.rs  # Filesystem traversal
│   └── hash.rs     # xxHash3-64 implementation
└── .picman.db      # SQLite database (created per library)
```

## Technology

- **Rust** with async-free design for simplicity
- **SQLite** via rusqlite for metadata storage
- **ratatui** + crossterm for the terminal UI
- **rayon** for parallel batch operations
- **xxHash3-64** for fast file hashing

## License

MIT
