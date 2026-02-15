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
| `r` | Rename directory (with word suggestions from subdirs) |
| `o` | Operations menu (thumbnails, orientation, hash) |
| `m` | Filter by rating/tags |
| `p` | Generate directory preview (selected dir only) |
| `P` | Generate directory previews (recursive) |
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
- `↑` / `↓` or `Tab` to navigate between Rating, Video Only, and Tag sections
- `←` / `→` or `1-5` / `a-g` to select rating filter: Any, Unrated, or minimum 1-5
- `v` or `Space`/`Enter` (when on Video Only) to toggle video-only filter
- Type to filter available tags, `↑` / `↓` to navigate tag list
- `Enter` or `Space` to add selected tag to filter (multiple tags use AND logic)
- `Backspace` to remove last added tag (when input is empty)
- `0` to clear entire filter
- `m` or `Esc` to close (filter auto-applies on every change)

When a filter is active, the status bar shows: `[Filter: video 3+ #tag1 #tag2]`

### Rename Directory

When renaming (`r`, only works when directory is selected):
- Shows suggested words extracted from subdirectory names and file tags
- Type to edit the new name
- `←` / `→` to move cursor
- `↑` / `↓` to select from suggestions
- `Tab` to replace name with selected suggestion
- `Shift+Tab` to append selected suggestion to current name
- `Enter` to confirm rename
- `Esc` to cancel

The rename updates both the filesystem and database, preserving the directory ID so cached previews continue to work.

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

### thumbnails
Generate thumbnails for all media files (images and videos).
```bash
picman thumbnails /path/to/library
```
- Skips files that already have thumbnails
- Shows progress for each file
- Video thumbnails require ffmpeg

### previews
Generate directory preview images (composite thumbnails shown when browsing directories).
```bash
picman previews /path/to/library
```
- Skips directories that already have previews
- Shows progress for each directory
- Runs faster if thumbnails are generated first (`picman thumbnails` before `picman previews`)

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
