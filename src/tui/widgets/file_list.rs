use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::thumbnails::has_thumbnail;
use crate::tui::colors::{FOCUS_COLOR, HEADER_COLOR, HELP_TEXT, UNFOCUS_COLOR, VIDEO_INDICATOR};
use crate::tui::state::{AppState, Focus};

pub fn render_file_list(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let is_focused = state.focus == Focus::FileList;

    // Build directory path for thumbnail checks
    let dir_path = state.get_selected_directory().map(|d| {
        if d.path.is_empty() {
            state.library_path.clone()
        } else {
            state.library_path.join(&d.path)
        }
    });

    // Get search-filtered file indices
    let visible_indices = state.visible_file_indices();

    let rows: Vec<Row> = visible_indices
        .iter()
        .map(|&idx| {
            let file_with_tags = &state.file_list.files[idx];
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

            // Format file size with thumbnail indicator
            let size = format_size(file.size);
            let has_thumb = dir_path
                .as_ref()
                .map(|dp| has_thumbnail(&dp.join(&file.filename)))
                .unwrap_or(false);
            let size_cell = if has_thumb {
                Cell::from(Line::from(vec![
                    Span::raw(size),
                    Span::styled(" *", Style::default().fg(HELP_TEXT)),
                ]))
            } else {
                Cell::from(size)
            };

            Row::new(vec![name_cell, size_cell])
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

    // Title: show search query when active, otherwise file count
    let visible_count = visible_indices.len();
    let title: Line = if state.search.active && is_focused {
        Line::from(vec![
            Span::raw(" Files /"),
            Span::styled(&state.search.query, Style::default().fg(FOCUS_COLOR)),
            Span::styled("_", Style::default().fg(FOCUS_COLOR).add_modifier(Modifier::SLOW_BLINK)),
            Span::raw(" "),
        ])
    } else if !state.search.query.is_empty() && is_focused {
        Line::from(format!(" Files ({}/{}) ", visible_count, state.file_list.files.len()))
    } else {
        Line::from(format!(" Files ({}) ", visible_count))
    };

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
