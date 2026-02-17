use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::tui::colors::{FOCUS_COLOR, HEADER_COLOR, HELP_TEXT, TAG_COLOR};
use crate::tui::state::TagInputState;

pub fn render_tag_popup(frame: &mut Frame, area: Rect, tag_input: &TagInputState) {
    let popup_width = 40;
    let popup_height = 12;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(x, y, popup_width, popup_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Add Tag ")
        .title_style(
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Split into input line, autocomplete list, and help text
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Input line
            Constraint::Min(0),   // Autocomplete list
            Constraint::Length(1), // Help text
        ])
        .split(inner);

    render_input_line(frame, chunks[0], tag_input);
    render_autocomplete(frame, chunks[1], tag_input);
    render_help(frame, chunks[2], tag_input);
}

fn render_input_line(frame: &mut Frame, area: Rect, tag_input: &TagInputState) {
    let has_input = !tag_input.input.is_empty();

    if tag_input.editing {
        // Actively editing: show input with cursor
        let input_text = format!(" > {}_", tag_input.input);
        let paragraph = Paragraph::new(input_text).style(Style::default().fg(FOCUS_COLOR));
        frame.render_widget(paragraph, area);
    } else if tag_input.input_selected && has_input {
        // Input line selected with preserved search text
        let line = Line::from(vec![
            Span::styled(
                format!(" > {}", tag_input.input),
                Style::default().bg(FOCUS_COLOR).fg(Color::Black),
            ),
            Span::styled("  i/Enter to edit", Style::default().fg(HELP_TEXT)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    } else if tag_input.input_selected {
        // Input line selected, empty: show hint
        let line = Line::from(vec![
            Span::styled(" i", Style::default().bg(FOCUS_COLOR).fg(Color::Black)),
            Span::styled("/", Style::default().fg(HELP_TEXT)),
            Span::styled("Enter", Style::default().bg(FOCUS_COLOR).fg(Color::Black)),
            Span::styled(" to type tag", Style::default().fg(HELP_TEXT)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    } else if has_input {
        // Tag in list is selected, search text preserved (dimmed)
        let line = Line::from(Span::styled(
            format!(" > {}", tag_input.input),
            Style::default().fg(HELP_TEXT),
        ));
        frame.render_widget(Paragraph::new(line), area);
    } else {
        // Tag in list is selected, no search text
        let line = Line::from(Span::styled(
            " Type to add tag...",
            Style::default().fg(HELP_TEXT),
        ));
        frame.render_widget(Paragraph::new(line), area);
    }
}

fn render_autocomplete(frame: &mut Frame, area: Rect, tag_input: &TagInputState) {
    let show_highlight = !tag_input.input_selected || tag_input.editing;

    let items: Vec<ListItem> = tag_input
        .filtered_tags
        .iter()
        .enumerate()
        .map(|(idx, tag)| {
            let style = if show_highlight && idx == tag_input.selected_index {
                Style::default().bg(FOCUS_COLOR).fg(Color::Black)
            } else {
                Style::default().fg(TAG_COLOR)
            };
            ListItem::new(format!("   {}", tag)).style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::TOP));
    frame.render_widget(list, area);
}

fn render_help(frame: &mut Frame, area: Rect, tag_input: &TagInputState) {
    let text = if tag_input.editing {
        " Esc:Cancel  Enter:Add  ↑↓:Select"
    } else {
        " i:Edit  j/k:Nav  Enter:Select  Esc:Close"
    };
    let paragraph = Paragraph::new(text).style(Style::default().fg(HELP_TEXT));
    frame.render_widget(paragraph, area);
}
