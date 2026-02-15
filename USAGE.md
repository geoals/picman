# picman CLI Usage

## Commands

### init
Initialize a library database.
```bash
picman init /path/to/library
```

### sync
Sync database with filesystem changes.
```bash
picman sync /path/to/library
picman sync /path/to/library --hash  # also compute file hashes
```

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
