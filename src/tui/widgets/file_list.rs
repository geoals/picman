use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Row, Table},
};

use crate::tui::state::{AppState, Focus};

pub fn render_file_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let is_focused = state.focus == Focus::FileList;

    let rows: Vec<Row> = state
        .file_list
        .files
        .iter()
        .enumerate()
        .map(|(idx, file_with_tags)| {
            let file = &file_with_tags.file;

            // Format rating as stars
            let rating = file
                .rating
                .map(|r| "*".repeat(r as usize))
                .unwrap_or_else(|| "-".to_string());

            // Format tags
            let tags = if file_with_tags.tags.is_empty() {
                String::new()
            } else {
                file_with_tags.tags.join(", ")
            };

            // Format file size
            let size = format_size(file.size);

            let style = if idx == state.file_list.selected_index {
                if is_focused {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                } else {
                    Style::default().bg(Color::Gray).fg(Color::Black)
                }
            } else {
                Style::default()
            };

            Row::new(vec![
                file.filename.clone(),
                rating,
                tags,
                size,
            ])
            .style(style)
        })
        .collect();

    let header = Row::new(vec!["Name", "Rating", "Tags", "Size"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let widths = [
        Constraint::Percentage(40),
        Constraint::Length(8),
        Constraint::Percentage(35),
        Constraint::Length(10),
    ];

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let file_count = state.file_list.files.len();
    let title = format!(" Files ({}) ", file_count);

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        );

    frame.render_widget(table, area);
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
