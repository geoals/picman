# Picman Development Guidelines

## Performance is Key

This application must remain responsive even on slow HDDs with large photo libraries (100k+ files). Every feature should be designed with performance in mind.

### UI Responsiveness

- **Never block the UI thread** - all IO and CPU-intensive work must run in background threads
- Use channels (`mpsc`) to communicate between background workers and the UI
- Show progress indicators (spinners, progress bars) for any operation that might take time
- The UI should always respond to input, even during heavy operations

### Database Operations

- **Batch operations** - never insert/update rows one at a time in a loop; use transactions and batch inserts
- **Minimize queries** - fetch data in bulk rather than making many small queries
- **Index appropriately** - ensure queries used in hot paths have supporting indexes
- Keep the database on the same drive as the library to avoid cross-drive IO

### Parallel Processing

- Use `rayon` for CPU-bound batch operations (thumbnail generation, hashing, image processing)
- Parallel IO helps on SSDs but may not help (or hurt) on HDDs due to seek times
- Use `AtomicUsize` with `Ordering::Relaxed` for progress counters in parallel loops
- Consider IO vs CPU boundedness before adding parallelism

### HDD-Specific Considerations

- **Sequential access patterns** - when possible, process files in directory order to minimize seeks
- **Caching** - cache metadata and small files in memory; avoid re-reading
- **Lazy loading** - don't load data until it's needed (thumbnails, previews, metadata)
- **Incremental operations** - support resuming interrupted operations; don't redo completed work

### TUI Progress Indicators

Background operations in the TUI must provide rich feedback:

- **Visual progress bar** - graphical bar using block characters (`█░`)
- **Spinner animation** - animated spinner for visual activity indication
- **Elapsed time** - show how long the operation has been running
- **ETA** - calculate and display estimated time remaining based on current rate
- **Queue count** - show number of pending operations if any
- **Cancel hint** - remind users they can cancel with Esc

Use `BackgroundProgress` struct with `AtomicUsize` counters for thread-safe progress updates. The status bar renders at ~60fps so the spinner animates smoothly.

### Operation Queue

Operations in the TUI run sequentially via `operation_queue`:

- Only one background operation runs at a time
- Additional operations are queued and run in order
- Status bar shows "+N queued" when operations are waiting
- Cancelling (Esc) clears the entire queue
- Queue ensures predictable resource usage and avoids thrashing

### CLI Commands

- All batch CLI commands (sync, thumbnails, previews) should:
  - Show progress with `indicatif` progress bars
  - Use parallel processing where beneficial
  - Support incremental execution (skip already-processed items)
  - Report statistics on completion
