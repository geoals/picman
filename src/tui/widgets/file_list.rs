use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::tui::colors::{FOCUS_COLOR, HEADER_COLOR, UNFOCUS_COLOR, VIDEO_INDICATOR};
use crate::tui::state::{AppState, Focus};

pub fn render_file_list(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let is_focused = state.focus == Focus::FileList;

    let rows: Vec<Row> = state
        .file_list
        .files
        .iter()
        .map(|file_with_tags| {
            let file = &file_with_tags.file;

            // Format filename with video indicator
            let is_video = file
                .media_type
                .as_deref()
                .map(|t| t.starts_with("video/"))
                .unwrap_or(false);
            let name_cell = if is_video {
                Cell::from(Line::from(vec![
                    Span::styled("[V] ", Style::default().fg(VIDEO_INDICATOR)),
                    Span::raw(&file.filename),
                ]))
            } else {
                Cell::from(file.filename.as_str())
            };

            // Format file size
            let size = format_size(file.size);

            Row::new(vec![name_cell, Cell::from(size)])
        })
        .collect();

    let header = Row::new(vec!["Name", "Size"])
        .style(
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);

    let widths = [
        Constraint::Min(0),
        Constraint::Length(10),
    ];

    let border_style = if is_focused {
        Style::default().fg(FOCUS_COLOR)
    } else {
        Style::default().fg(UNFOCUS_COLOR)
    };

    let file_count = state.file_list.files.len();
    let title = format!(" Files ({}) ", file_count);

    let highlight_style = if is_focused {
        Style::default().bg(FOCUS_COLOR).fg(Color::Black)
    } else {
        Style::default().bg(Color::Gray).fg(Color::Black)
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        )
        .row_highlight_style(highlight_style);

    frame.render_stateful_widget(table, area, &mut state.file_list.table_state);
}

fn format_size(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
