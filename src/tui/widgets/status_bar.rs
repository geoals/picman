use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::Paragraph,
};

use crate::tui::state::{AppState, Focus};

pub fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState) {
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
