# picman Usage

## TUI (Terminal User Interface)

Launch the TUI by running picman with a library path:
```bash
picman /path/to/library
```

### Key Bindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `h` / `←` | Collapse directory / Move to left pane |
| `l` / `→` | Expand directory / Move to right pane |
| `Tab` | Switch focus between panes |
| `Enter` | Select directory (expands and enters first child) |
| `1-5` / `a-g` | Set rating (works on files and directories) |
| `0` | Clear rating |
| `t` | Add tag (opens popup with autocomplete) |
| `o` | Operations menu (thumbnails, orientation, hash) |
| `m` | Filter by rating/tags |
| `?` | Toggle help overlay |
| `q` | Quit |

### File Actions

- **Enter on file**: Opens file with default system viewer (`xdg-open` on Linux, `open` on macOS)
- **Enter on directory**: Expands directory or moves to file list

### Preview & Thumbnails

Images and videos show preview thumbnails. Thumbnails are cached to `~/.cache/picman/thumbnails/` at 1440p resolution for fast subsequent access.

- **Images**: Shows cached thumbnail if available, otherwise falls back to original file
- **Videos**: Shows thumbnail extracted via ffmpeg (requires ffmpeg installed)

### Operations Menu

Press `o` to open the operations menu for batch processing on the selected directory and all subdirectories:

| Key | Operation | Description |
|-----|-----------|-------------|
| `1` / `t` | Thumbnails | Generate preview thumbnails |
| `2` / `o` | Orientation | Tag images as landscape/portrait (EXIF-aware) |
| `3` / `h` | Hash | Compute file hashes |

- Operations run in parallel in the background with progress shown in status bar
- Already-processed files are skipped (existing thumbnails/tags/hashes)
- Press `q` during an operation to cancel gracefully

### Tag Popup

When adding a tag (`t`):
- Type to filter existing tags or create a new one
- `↑` / `↓` to navigate suggestions
- `Enter` to apply selected/typed tag
- `Esc` to cancel

### Filter Popup

When filtering (`m`):
- `Tab` to switch between Rating and Tag sections
- `←` / `→` or `1-5` / `a-g` to select minimum rating (or "Any")
- `v` to toggle video-only filter
- Type to filter available tags, `↑` / `↓` to navigate
- `Enter` to add selected tag to filter (multiple tags use AND logic)
- `Backspace` to remove last added tag (when input is empty)
- `0` to clear entire filter
- `Enter` (with no tag selected) to apply filter and close
- `Esc` to cancel without applying

When a filter is active, the status bar shows: `[Filter: video 3+ #tag1 #tag2]`

## CLI Commands

### init
Initialize a library database.
```bash
picman init /path/to/library
```

### sync
Sync database with filesystem changes.
```bash
picman sync /path/to/library
picman sync /path/to/library --hash         # also compute file hashes
picman sync /path/to/library --orientation  # tag images as landscape/portrait
```

The `--orientation` flag tags images based on dimensions (EXIF-aware). Square images are not tagged. You can also use the TUI operations menu (`o`) to tag orientation interactively.

### list
List files with optional filters.
```bash
picman list /path/to/library
picman list /path/to/library --rating 4    # 4+ stars
picman list /path/to/library --tag portrait
```

### rate
Set rating (1-5 stars) on a file.
```bash
picman rate /path/to/library photos/image.jpg 5
picman rate /path/to/library photos/image.jpg    # clear rating
```

### tag
Add/remove/list tags on a file.
```bash
picman tag /path/to/library photos/image.jpg --add portrait --add outdoor
picman tag /path/to/library photos/image.jpg --remove outdoor
picman tag /path/to/library photos/image.jpg --list
```

## Known Limitations

### File paths must be relative without "./" prefix

When specifying file paths for `rate` and `tag` commands, use the path exactly as stored in the database:

```bash
# Correct
picman rate /lib photos/image.jpg 5

# Incorrect - will fail with "File not found"
picman rate /lib ./photos/image.jpg 5
```

To check how paths are stored:
```bash
sqlite3 .picman.db "SELECT d.path, f.filename FROM files f JOIN directories d ON f.directory_id = d.id;"
```
