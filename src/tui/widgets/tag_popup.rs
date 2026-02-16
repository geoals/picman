use ratatui::{
    layout::{Constraint, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::tui::colors::{HEADER_COLOR, TAG_COLOR};
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
        .title_style(Style::default().fg(HEADER_COLOR).add_modifier(Modifier::BOLD));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Split into input area and suggestions area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(inner);

    // Render input field with tag color
    let input_text = format!("> {}", tag_input.input);
    let input = Paragraph::new(input_text)
        .style(Style::default().fg(TAG_COLOR))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(TAG_COLOR))
                .title(" Tag "),
        );
    frame.render_widget(input, chunks[0]);

    // Render suggestions with consistent colors
    let items: Vec<ListItem> = tag_input
        .filtered_tags
        .iter()
        .enumerate()
        .map(|(idx, tag)| {
            let style = if idx == tag_input.selected_index {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else {
                Style::default().fg(TAG_COLOR)
            };
            ListItem::new(tag.as_str()).style(style)
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, chunks[1]);
}
