//! Logging infrastructure for picman.
//!
//! Uses the `tracing` crate with file-based output. Logs go to `~/.cache/picman/picman.log`.
//! Configure verbosity via `PICMAN_LOG` environment variable (default: `info`).
//!
//! # Example
//!
//! ```bash
//! # Normal operation (info level)
//! picman sync .
//!
//! # Debug performance
//! PICMAN_LOG=debug picman sync .
//!
//! # Trace everything
//! PICMAN_LOG=trace picman sync .
//!
//! # View logs during TUI
//! tail -f ~/.cache/picman/picman.log
//! ```

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt::{self, format::FmtSpan}, prelude::*, EnvFilter};

/// Initialize the logging system.
///
/// Creates the log directory if needed and sets up file-based logging.
/// When PICMAN_LOG is set, also logs to stderr for immediate feedback.
/// Returns a guard that must be held for the duration of the program.
/// When the guard is dropped, any remaining logs are flushed.
pub fn init_logging() -> Result<WorkerGuard> {
    let cache_dir = directories::ProjectDirs::from("", "", "picman")
        .context("Failed to determine cache directory")?
        .cache_dir()
        .to_path_buf();

    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("Failed to create cache directory: {}", cache_dir.display()))?;

    let log_file = cache_dir.join("picman.log");

    // Open file in append mode
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .with_context(|| format!("Failed to open log file: {}", log_file.display()))?;

    // Non-blocking writer to avoid blocking on I/O
    let (non_blocking, guard) = tracing_appender::non_blocking(file);

    // Check if PICMAN_LOG is explicitly set (for stderr output)
    let log_env = std::env::var("PICMAN_LOG").ok();
    let verbose = log_env.is_some();
    let filter_str = log_env.unwrap_or_else(|| "info".to_string());

    // File layer - always active, logs span close events with duration
    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .with_span_events(FmtSpan::CLOSE);

    // Stderr layer - only when PICMAN_LOG is explicitly set
    let stderr_layer = verbose.then(|| {
        fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .with_span_events(FmtSpan::CLOSE)
    });

    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new(&filter_str))
        .with(file_layer)
        .with(stderr_layer);

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global tracing subscriber")?;

    Ok(guard)
}

/// Get the path to the log file.
pub fn log_file_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("", "", "picman")
        .map(|dirs| dirs.cache_dir().join("picman.log"))
}
