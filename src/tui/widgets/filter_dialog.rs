use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::tui::state::{FilterDialogFocus, FilterDialogState, RatingFilter};

pub fn render_filter_dialog(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let popup_width = 50;
    let popup_height = 18;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(x, y, popup_width, popup_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Filter ")
        .padding(ratatui::widgets::Padding::horizontal(1));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Split into sections with more spacing
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Top padding
            Constraint::Length(2),  // Rating row
            Constraint::Length(1),  // Spacing
            Constraint::Length(2),  // Video filter row
            Constraint::Length(1),  // Spacing
            Constraint::Length(2),  // Selected tags
            Constraint::Length(2),  // Tag input
            Constraint::Min(3),     // Autocomplete list
            Constraint::Length(2),  // Help text
        ])
        .split(inner);

    // Rating row
    render_rating_row(frame, chunks[1], dialog);

    // Video filter row
    render_video_row(frame, chunks[3], dialog);

    // Selected tags row
    render_selected_tags(frame, chunks[5], dialog);

    // Tag input row
    render_tag_input(frame, chunks[6], dialog);

    // Autocomplete list (only show highlight when tag section is focused)
    render_autocomplete_list(frame, chunks[7], dialog);

    // Help text
    render_help_text(frame, chunks[8]);
}

fn render_video_row(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_focused = dialog.focus == FilterDialogFocus::VideoOnly;
    let checkbox = if dialog.video_only { "[x]" } else { "[ ]" };

    let style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let text = format!("Video only: {}  (v or Space to toggle)", checkbox);
    let paragraph = Paragraph::new(text).style(style);
    frame.render_widget(paragraph, area);
}

fn render_rating_row(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_focused = dialog.focus == FilterDialogFocus::Rating;
    let focus_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    // Build rating display: "Rating: [Any] [Unrated] 1 2 3 4 5"
    let mut spans = vec![
        Span::styled("Rating: ", focus_style),
    ];

    // Helper to get style
    let sel = |selected: bool| {
        if selected {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else {
            Style::default()
        }
    };

    // "Any" option
    spans.push(Span::styled("[Any]", sel(dialog.rating_filter == RatingFilter::Any)));
    spans.push(Span::raw(" "));

    // "Unrated" option
    spans.push(Span::styled("[Unrated]", sel(dialog.rating_filter == RatingFilter::Unrated)));
    spans.push(Span::raw(" "));

    // Numbers 1-5
    for i in 1..=5 {
        spans.push(Span::styled(format!("{}", i), sel(dialog.rating_filter == RatingFilter::MinRating(i))));
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
    let is_tag_focused = dialog.focus == FilterDialogFocus::Tag;
    let visible_height = area.height.saturating_sub(1) as usize; // -1 for border

    let total_tags = dialog.filtered_tags.len();
    let scroll_offset = dialog.tag_scroll_offset;

    let items: Vec<ListItem> = dialog
        .filtered_tags
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(idx, tag)| {
            // Only highlight selection when tag section is focused
            let style = if is_tag_focused && idx == dialog.tag_list_index {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else {
                Style::default()
            };
            ListItem::new(format!("  {}", tag)).style(style)
        })
        .collect();

    // Show scroll indicators if needed
    let title = if total_tags > visible_height {
        let at_top = scroll_offset == 0;
        let at_bottom = scroll_offset + visible_height >= total_tags;
        match (at_top, at_bottom) {
            (true, false) => " ▼ ".to_string(),
            (false, true) => " ▲ ".to_string(),
            (false, false) => " ▲▼ ".to_string(),
            (true, true) => String::new(),
        }
    } else {
        String::new()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::TOP).title(title));
    frame.render_widget(list, area);
}

fn render_help_text(frame: &mut Frame, area: Rect) {
    let help = "Tab/Arrows:Navigate  0:Clear  m/Esc:Close";
    let paragraph = Paragraph::new(help)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}
