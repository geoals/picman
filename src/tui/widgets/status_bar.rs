use ratatui::{layout::Rect, prelude::*, widgets::Paragraph};

use crate::tui::colors::{RATING_COLOR, TAG_COLOR, VIDEO_INDICATOR, WARNING_COLOR};
use crate::tui::state::{AppState, Focus, RatingFilter};

pub fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    use std::sync::atomic::Ordering;

    // Show background operation progress if active
    if let Some(ref progress) = state.background_progress {
        let completed = progress.completed.load(Ordering::Relaxed);
        let total = progress.total;
        let pct = if total > 0 {
            completed * 100 / total
        } else {
            0
        };
        let msg = format!(
            "{}: {}/{} ({}%)",
            progress.operation.label(),
            completed,
            total,
            pct
        );
        let status = Paragraph::new(msg).style(Style::default().bg(WARNING_COLOR).fg(Color::Black));
        frame.render_widget(status, area);
        return;
    }

    // Show status message if present
    if let Some(ref msg) = state.status_message {
        let status = Paragraph::new(msg.as_str())
            .style(Style::default().bg(Color::Green).fg(Color::Black));
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
    let hints = "j/k:move  m:filter  t:tag  ?:help  q:quit";

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
