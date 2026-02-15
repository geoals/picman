use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::tui::state::{FilterDialogFocus, FilterDialogState};

pub fn render_filter_dialog(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let popup_width = 40;
    let popup_height = 16;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(x, y, popup_width, popup_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Filter ");

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Split into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // Rating row
            Constraint::Length(2),  // Selected tags
            Constraint::Length(2),  // Tag input
            Constraint::Min(4),     // Autocomplete list
            Constraint::Length(2),  // Help text
        ])
        .split(inner);

    // Rating row
    render_rating_row(frame, chunks[0], dialog);

    // Selected tags row
    render_selected_tags(frame, chunks[1], dialog);

    // Tag input row
    render_tag_input(frame, chunks[2], dialog);

    // Autocomplete list
    render_autocomplete_list(frame, chunks[3], dialog);

    // Help text
    render_help_text(frame, chunks[4]);
}

fn render_rating_row(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_focused = dialog.focus == FilterDialogFocus::Rating;
    let focus_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    // Build rating display: "Rating: [Any] 1 2 3 4 5"
    let mut spans = vec![
        Span::styled("Rating: ", focus_style),
    ];

    // "Any" option
    let any_style = if dialog.rating_selected.is_none() {
        Style::default().bg(Color::Cyan).fg(Color::Black)
    } else {
        Style::default()
    };
    spans.push(Span::styled("[Any]", any_style));
    spans.push(Span::raw(" "));

    // Numbers 1-5
    for i in 1..=5 {
        let num_style = if dialog.rating_selected == Some(i) {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!("{}", i), num_style));
        if i < 5 {
            spans.push(Span::raw(" "));
        }
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn render_selected_tags(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let tags_str = if dialog.selected_tags.is_empty() {
        "(none)".to_string()
    } else {
        dialog.selected_tags.iter()
            .map(|t| format!("#{}", t))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let text = format!("Tags: {}", tags_str);
    let paragraph = Paragraph::new(text);
    frame.render_widget(paragraph, area);
}

fn render_tag_input(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_focused = dialog.focus == FilterDialogFocus::Tag;
    let style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let input_text = format!("Add: > {}", dialog.tag_input);
    let paragraph = Paragraph::new(input_text).style(style);
    frame.render_widget(paragraph, area);
}

fn render_autocomplete_list(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let items: Vec<ListItem> = dialog
        .filtered_tags
        .iter()
        .enumerate()
        .take(area.height as usize)
        .map(|(idx, tag)| {
            let style = if idx == dialog.tag_list_index {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else {
                Style::default()
            };
            ListItem::new(format!("  {}", tag)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(list, area);
}

fn render_help_text(frame: &mut Frame, area: Rect) {
    let help = "Tab:Switch  0:Clear  Enter:Apply  Esc:Cancel";
    let paragraph = Paragraph::new(help)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}
