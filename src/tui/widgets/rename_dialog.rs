use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::tui::state::RenameDialogState;

pub fn render_rename_dialog(frame: &mut Frame, area: Rect, dialog: &RenameDialogState) {
    let popup_width = 60;
    let popup_height = 20;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(x, y, popup_width, popup_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rename Directory ")
        .padding(ratatui::widgets::Padding::horizontal(1));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Split into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Top padding
            Constraint::Length(1),  // Original path label
            Constraint::Length(1),  // Spacing
            Constraint::Length(1),  // New name label
            Constraint::Length(1),  // New name input
            Constraint::Length(1),  // Spacing
            Constraint::Length(1),  // Suggestions label
            Constraint::Min(3),     // Suggestions list
            Constraint::Length(1),  // Help text
        ])
        .split(inner);

    // Original path
    let dir_name = dialog.original_path.rsplit('/').next().unwrap_or(&dialog.original_path);
    let original = Paragraph::new(format!("Current: {}", dir_name))
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(original, chunks[1]);

    // New name label
    let new_label = Paragraph::new("New name:");
    frame.render_widget(new_label, chunks[3]);

    // New name input with cursor
    render_input_with_cursor(frame, chunks[4], dialog);

    // Suggestions label
    let suggestions_label = Paragraph::new("Suggestions (Tab to use, Shift+Tab to append):")
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(suggestions_label, chunks[6]);

    // Suggestions list
    render_suggestions(frame, chunks[7], dialog);

    // Help text
    let help = Paragraph::new("Enter:Rename  Esc:Cancel  Up/Down:Select")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(help, chunks[8]);
}

fn render_input_with_cursor(frame: &mut Frame, area: Rect, dialog: &RenameDialogState) {
    // Build the input text with visual cursor
    let before_cursor = &dialog.new_name[..dialog.cursor_pos];
    let at_cursor = if dialog.cursor_pos < dialog.new_name.len() {
        // Get the character at cursor position
        let mut end = dialog.cursor_pos + 1;
        while end < dialog.new_name.len() && !dialog.new_name.is_char_boundary(end) {
            end += 1;
        }
        &dialog.new_name[dialog.cursor_pos..end]
    } else {
        " "
    };
    let after_cursor = if dialog.cursor_pos < dialog.new_name.len() {
        let mut end = dialog.cursor_pos + 1;
        while end < dialog.new_name.len() && !dialog.new_name.is_char_boundary(end) {
            end += 1;
        }
        &dialog.new_name[end..]
    } else {
        ""
    };

    let line = Line::from(vec![
        Span::raw("> "),
        Span::raw(before_cursor),
        Span::styled(at_cursor, Style::default().bg(Color::White).fg(Color::Black)),
        Span::raw(after_cursor),
    ]);

    let paragraph = Paragraph::new(line).style(Style::default().fg(Color::Yellow));
    frame.render_widget(paragraph, area);
}

fn render_suggestions(frame: &mut Frame, area: Rect, dialog: &RenameDialogState) {
    let visible_count = area.height as usize;
    let total = dialog.suggested_words.len();

    let items: Vec<ListItem> = dialog
        .suggested_words
        .iter()
        .enumerate()
        .skip(dialog.scroll_offset)
        .take(visible_count)
        .map(|(idx, word)| {
            let style = if idx == dialog.selected_suggestion {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else {
                Style::default()
            };
            ListItem::new(format!("  {}", word)).style(style)
        })
        .collect();

    // Show scroll indicator if there are more items
    let title = if total > visible_count {
        format!(" {}/{} ", dialog.scroll_offset + 1, total)
    } else {
        String::new()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::TOP).title(title));
    frame.render_widget(list, area);
}
