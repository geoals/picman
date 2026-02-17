use ratatui::{layout::Rect, prelude::*, widgets::Paragraph};

use crate::tui::colors::{FOCUS_COLOR, HELP_TEXT, RATING_COLOR, SUCCESS_COLOR, TAG_COLOR, UNFOCUS_COLOR, VIDEO_INDICATOR, WARNING_COLOR};
use crate::tui::state::{AppState, Focus, RatingFilter};

/// Spinner frames for indeterminate progress
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Format duration as human-readable string
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    use std::sync::atomic::Ordering;

    // Show background operation progress if active
    if let Some(ref progress) = state.background_progress {
        let completed = progress.completed.load(Ordering::Relaxed);
        let total = progress.total;
        let elapsed = progress.start_time.elapsed();
        let elapsed_secs = elapsed.as_secs();

        let mut spans: Vec<Span> = Vec::new();

        // Spinner animation (based on elapsed time)
        let spinner_idx = (elapsed.as_millis() / 80) as usize % SPINNER_FRAMES.len();
        spans.push(Span::styled(
            format!("{} ", SPINNER_FRAMES[spinner_idx]),
            Style::default().fg(FOCUS_COLOR),
        ));

        // Operation label
        spans.push(Span::raw(format!("{} ", progress.operation.label())));

        // Visual progress bar
        let bar_width = 20usize;
        let filled = if total > 0 {
            (completed * bar_width / total).min(bar_width)
        } else {
            0
        };
        let empty = bar_width - filled;

        spans.push(Span::styled("[", Style::default().fg(UNFOCUS_COLOR)));
        spans.push(Span::styled(
            "█".repeat(filled),
            Style::default().fg(FOCUS_COLOR),
        ));
        spans.push(Span::styled(
            "░".repeat(empty),
            Style::default().fg(UNFOCUS_COLOR),
        ));
        spans.push(Span::styled("] ", Style::default().fg(UNFOCUS_COLOR)));

        // Count and percentage
        let pct = if total > 0 { completed * 100 / total } else { 0 };
        spans.push(Span::raw(format!("{}/{} ({}%) ", completed, total, pct)));

        // Elapsed time
        spans.push(Span::styled(
            format_duration(elapsed_secs),
            Style::default().fg(WARNING_COLOR),
        ));

        // ETA (only show if we have meaningful progress)
        if completed > 0 && completed < total {
            let rate = completed as f64 / elapsed_secs.max(1) as f64;
            let remaining = (total - completed) as f64 / rate;
            spans.push(Span::raw(" | ETA "));
            spans.push(Span::styled(
                format_duration(remaining as u64),
                Style::default().fg(SUCCESS_COLOR),
            ));
        }

        // Queue count (if any)
        let queue_len = state.operation_queue.len();
        if queue_len > 0 {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("+{} queued", queue_len),
                Style::default().fg(Color::Magenta),
            ));
        }

        // Cancel hint
        spans.push(Span::styled(
            " [q]",
            Style::default().fg(HELP_TEXT),
        ));

        let line = Line::from(spans);
        let status = Paragraph::new(line).style(Style::default().bg(WARNING_COLOR).fg(Color::Black));
        frame.render_widget(status, area);
        return;
    }

    // Show status message if present
    if let Some(ref msg) = state.status_message {
        let status = Paragraph::new(msg.as_str())
            .style(Style::default().bg(SUCCESS_COLOR).fg(Color::Black));
        frame.render_widget(status, area);
        return;
    }

    // Build status line with styled spans
    let mut spans: Vec<Span> = Vec::new();

    // Current focus indicator
    let focus_str = match state.focus {
        Focus::DirectoryTree => "Tree",
        Focus::FileList => "Files",
    };
    spans.push(Span::raw(format!("[{}]", focus_str)));

    // Filter indicator with colors
    if state.filter.is_active() {
        spans.push(Span::raw(" [Filter: "));

        let mut first = true;

        // Video filter (magenta)
        if state.filter.video_only {
            spans.push(Span::styled("video", Style::default().fg(VIDEO_INDICATOR)));
            first = false;
        }

        // Rating filter (yellow)
        match state.filter.rating {
            RatingFilter::Any => {}
            RatingFilter::Unrated => {
                if !first {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled("unrated", Style::default().fg(RATING_COLOR)));
                first = false;
            }
            RatingFilter::MinRating(r) => {
                if !first {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(
                    format!("{}+", r),
                    Style::default().fg(RATING_COLOR),
                ));
                first = false;
            }
        }

        // Tag filters (blue)
        for t in &state.filter.tags {
            if !first {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(format!("#{}", t), Style::default().fg(TAG_COLOR)));
            first = false;
        }

        spans.push(Span::raw("]"));
    }

    // Selected file info
    if let Some(file_with_tags) = state.file_list.selected_file() {
        let file = &file_with_tags.file;
        spans.push(Span::raw(format!(" | {}", file.filename)));
        spans.push(Span::raw(format!(" | {}", format_size(file.size))));

        if let Some(ref media_type) = file.media_type {
            spans.push(Span::raw(format!(" | {}", media_type)));
        }
    }

    // Calculate remaining width for hints
    let left_content: String = spans.iter().map(|s| s.content.as_ref()).collect();
    let hints = "j/k:move  m:filter  t:tag  o:operations  ?:help  q:quit";

    // Pad with spaces to right-align hints
    let padding_needed = area
        .width
        .saturating_sub(left_content.len() as u16)
        .saturating_sub(hints.len() as u16);
    if padding_needed > 0 {
        spans.push(Span::raw(" ".repeat(padding_needed as usize)));
    }
    spans.push(Span::raw(hints));

    let line = Line::from(spans);
    let status =
        Paragraph::new(line).style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(status, area);
}

fn format_size(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}
