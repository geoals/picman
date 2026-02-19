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
| `o` | Operations menu (thumbnails, orientation, hash, dir previews) |
| `m` | Filter by rating/tags |
| `i` | Toggle expanded details panel (EXIF, hash, timestamps) |
| `/` | Search/filter items in focused panel |
| `?` | Toggle help overlay |
| `q` | Quit (or cancel background operation) |

### Mouse Support

| Action | Effect |
|--------|--------|
| Click | Select item / focus pane |
| Double-click | Open/expand (same as Enter) |
| Scroll wheel | Move selection up/down |

### File Actions

- **Enter on file**: Opens file with default system viewer (`xdg-open` on Linux, `open` on macOS)
- **Enter on directory**: Expands directory or moves to file list (empty directories stay on tree with a status message)

### Preview & Thumbnails

Images and videos show preview thumbnails. Thumbnails are cached to `~/.cache/picman/thumbnails/` at 1440p resolution for fast subsequent access.

- **Images**: Shows cached thumbnail if available, otherwise loads in background
- **Videos**: Shows thumbnail extracted via ffmpeg (requires ffmpeg installed)
- **Directories**: Shows composite preview from child files/subdirectories

Previews are loaded in a background thread with an LRU cache (200 items). Adjacent files are preloaded for instant display when scrolling.

### Operations Menu

Press `o` to open the operations menu for batch processing on the selected directory. Navigate with `j`/`k` or arrow keys, select with `Enter` or the number key:

| Key | Operation | Description |
|-----|-----------|-------------|
| `1` | Thumbnails | Generate preview thumbnails |
| `2` | Orientation | Tag images as landscape/portrait (EXIF-aware) |
| `3` | Hash | Compute file hashes |
| `4` | Dir preview | Generate directory preview (current only) |
| `5` | Dir preview (recursive) | Generate directory previews with subdirectories |

- Operations 1-3 run in parallel in the background with progress shown in status bar
- Already-processed files are skipped (existing thumbnails/tags/hashes)
- Press `q` during an operation to cancel gracefully
- Only one operation runs at a time; additional operations are queued

### Tag Popup

Press `t` to open the tag popup. Tags already applied to the selected item are shown with a `✓` checkmark. The popup stays open after toggling, so you can add/remove multiple tags in one session.

**Browse mode** (default):
- `j` / `k` or `↑` / `↓` to navigate tags
- `Enter` on a tag to toggle it (add if missing, remove if applied)
- `i` or `Enter` on the input line to switch to edit mode
- `Esc` to close

**Edit mode** (type to filter/create):
- Type to filter existing tags or create a new one
- `↑` / `↓` to navigate filtered suggestions
- `Enter` to toggle selected tag, or create and apply typed tag
- `Backspace` on empty input exits edit mode
- `Esc` to exit edit mode

### Filter Popup

Press `m` to open the filter dialog. It has sections for Rating, Video Only, and Tags. All changes auto-apply immediately.

**Navigation (browse mode):**
- `j` / `k` or `↑` / `↓` to move between sections (or within tag list)
- `Tab` / `Shift+Tab` to move between sections
- `h` / `l` or `←` / `→` to adjust rating
- `1-5` / `a-g` to set rating directly
- `u` to set unrated filter
- `v` to toggle video-only filter
- `Space` / `Enter` to toggle video or select tag
- `0` to clear entire filter
- `Backspace` to remove last added tag
- `m` or `Esc` to close

**Tag editing (press `i` on tag input line):**
- Type to filter available tags
- `↑` / `↓` to navigate filtered list
- `Enter` / `Space` to add selected tag (multiple tags use AND logic)
- `Backspace` on empty input exits editing mode
- `Esc` to exit editing mode

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

### Search

Press `/` to start an incremental search. The search filters items in the currently focused panel:

- **Directory tree focused**: Filters directories by name, keeping ancestor directories for tree structure
- **File list focused**: Filters files by filename

**While searching:**
- Type characters to narrow the search
- `Backspace` to delete last character (or cancel if empty)
- `Enter` to accept — exits search input but keeps the filter active
- `Esc` to cancel — clears the search and shows all items

The search query appears in the panel title (e.g., `Files /query_`). When a filter is accepted, the title shows filtered/total counts (e.g., `Files (12/42)`).

### Details Panel

The details panel shows metadata for the selected file or directory.

**Compact mode** (default): Shows path, size with dimensions, rating, timestamps, and tags. Files with cached thumbnails show a `*` indicator in the file list size column.

**Expanded mode** (press `i`): Takes 50% of the left section and shows additional information:
- Full file path, dimensions, size (formatted + exact bytes)
- Rating, modification/creation timestamps
- File hash (if computed) and thumbnail status
- Tags
- EXIF data: camera make/model, lens, aperture, shutter speed, ISO, focal length, GPS coordinates

EXIF data is read from the file header on demand and cached — it only re-reads when the selection changes.

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
picman sync /path/to/library --full         # full rescan (default is incremental)
```

By default, sync is **incremental**: only directories with changed mtime are scanned for file changes. Use `--full` to force a complete rescan of all files.

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
picman thumbnails /path/to/library --check  # show which dirs are missing thumbnails
```
- Skips files that already have thumbnails
- Shows progress with progress bar
- Video thumbnails require ffmpeg

### previews
Generate directory preview images (composite thumbnails shown when browsing directories).
```bash
picman previews /path/to/library
picman previews /path/to/library --check  # show which dirs are missing previews
```
- Skips directories that already have previews
- Shows progress with progress bar
- Runs faster if thumbnails are generated first (`picman thumbnails` before `picman previews`)

### status
Show library health information.
```bash
picman status /path/to/library
```
Reports directory/file counts, missing thumbnails, missing previews, and files without hashes.

### repair
Fix directory parent relationships based on paths.
```bash
picman repair /path/to/library
```
Useful after database corruption or manual edits.

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
