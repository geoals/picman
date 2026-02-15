use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::Paragraph,
};

use crate::tui::state::{AppState, Focus};

pub fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    use std::sync::atomic::Ordering;

    // Show thumbnail generation progress if active
    if let Some(ref progress) = state.thumbnail_progress {
        let completed = progress.completed.load(Ordering::Relaxed);
        let total = progress.total;
        let pct = if total > 0 { completed * 100 / total } else { 0 };
        let msg = format!("Generating thumbnails: {}/{} ({}%)", completed, total, pct);
        let status = Paragraph::new(msg)
            .style(Style::default().bg(Color::Magenta).fg(Color::White));
        frame.render_widget(status, area);
        return;
    }

    // Show status message if present
    if let Some(ref msg) = state.status_message {
        let status = Paragraph::new(msg.as_str())
            .style(Style::default().bg(Color::Blue).fg(Color::White));
        frame.render_widget(status, area);
        return;
    }

    let mut parts = Vec::new();

    // Current focus indicator
    let focus_str = match state.focus {
        Focus::DirectoryTree => "Tree",
        Focus::FileList => "Files",
    };
    parts.push(format!("[{}]", focus_str));

    // Filter indicator
    if state.filter.is_active() {
        let mut filter_parts = Vec::new();
        if state.filter.video_only {
            filter_parts.push("video".to_string());
        }
        if let Some(r) = state.filter.min_rating {
            filter_parts.push(format!("{}+", r));
        }
        for t in &state.filter.tags {
            filter_parts.push(format!("#{}", t));
        }
        parts.push(format!("[Filter: {}]", filter_parts.join(" ")));
    }

    // Selected file info
    if let Some(file_with_tags) = state.file_list.selected_file() {
        let file = &file_with_tags.file;
        parts.push(file.filename.clone());

        // File size
        parts.push(format_size(file.size));

        // Media type
        if let Some(ref media_type) = file.media_type {
            parts.push(media_type.clone());
        }
    }

    // Keybinding hints
    let hints = "j/k:move  m:filter  t:tag  ?:help  q:quit";

    let left_part = parts.join(" | ");
    let status_text = format!("{:width$}{}", left_part, hints, width = area.width as usize - hints.len());

    let status = Paragraph::new(status_text)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

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
