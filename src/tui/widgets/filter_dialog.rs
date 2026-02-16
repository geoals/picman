use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::tui::colors::{HEADER_COLOR, HELP_TEXT, RATING_COLOR, TAG_COLOR};
use crate::tui::state::{FilterDialogFocus, FilterDialogState, RatingFilter};

pub fn render_filter_dialog(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let popup_width = 55;
    let popup_height = 20;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(x, y, popup_width, popup_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Filter ")
        .title_style(
            Style::default()
                .fg(HEADER_COLOR)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Split into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Rating section
            Constraint::Length(3), // Media section
            Constraint::Min(8),    // Tags section
            Constraint::Length(1), // Help text
        ])
        .split(inner);

    // Rating section
    render_rating_section(frame, chunks[0], dialog);

    // Media section
    render_media_section(frame, chunks[1], dialog);

    // Tags section
    render_tags_section(frame, chunks[2], dialog);

    // Help text
    render_help_text(frame, chunks[3]);
}

fn render_rating_section(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_focused = dialog.focus == FilterDialogFocus::Rating;

    let border_style = if is_focused {
        Style::default().fg(RATING_COLOR)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Rating ")
        .title_style(Style::default().fg(HEADER_COLOR));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build rating line with star icons
    let sel = |selected: bool| {
        if selected {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else {
            Style::default()
        }
    };

    let mut spans = vec![
        Span::styled(" [Any]", sel(dialog.rating_filter == RatingFilter::Any)),
        Span::raw("  "),
        Span::styled(
            "[Unrated]",
            sel(dialog.rating_filter == RatingFilter::Unrated),
        ),
        Span::raw("  "),
    ];

    // Star ratings 1-5
    for i in 1..=5 {
        let stars = "★".repeat(i);
        let style = if dialog.rating_filter == RatingFilter::MinRating(i as i32) {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else {
            Style::default().fg(RATING_COLOR)
        };
        spans.push(Span::styled(stars, style));
        if i < 5 {
            spans.push(Span::raw("  "));
        }
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, inner);
}

fn render_media_section(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_focused = dialog.focus == FilterDialogFocus::VideoOnly;

    let border_style = if is_focused {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Media ")
        .title_style(Style::default().fg(HEADER_COLOR));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let checkbox = if dialog.video_only { "[x]" } else { "[ ]" };
    let style = if is_focused {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default()
    };

    let text = format!(" {} Video only", checkbox);
    let paragraph = Paragraph::new(text).style(style);
    frame.render_widget(paragraph, inner);
}

fn render_tags_section(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_focused = dialog.focus == FilterDialogFocus::Tag;

    let border_style = if is_focused {
        Style::default().fg(TAG_COLOR)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Tags ")
        .title_style(Style::default().fg(HEADER_COLOR));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split tags section into parts
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Selected tags
            Constraint::Length(1), // Tag input
            Constraint::Min(2),    // Autocomplete list
        ])
        .split(inner);

    // Selected tags row
    render_selected_tags(frame, chunks[0], dialog);

    // Tag input row
    render_tag_input(frame, chunks[1], dialog);

    // Autocomplete list
    render_autocomplete_list(frame, chunks[2], dialog);
}

fn render_selected_tags(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let mut spans: Vec<Span> = vec![Span::raw(" Active: ")];

    if dialog.selected_tags.is_empty() {
        spans.push(Span::styled("(none)", Style::default().fg(Color::DarkGray)));
    } else {
        for (i, tag) in dialog.selected_tags.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                format!("#{}", tag),
                Style::default().fg(TAG_COLOR),
            ));
        }
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn render_tag_input(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_focused = dialog.focus == FilterDialogFocus::Tag;
    let style = if is_focused {
        Style::default().fg(TAG_COLOR)
    } else {
        Style::default().fg(Color::Gray)
    };

    let input_text = format!(" Add: > {}", dialog.tag_input);
    let paragraph = Paragraph::new(input_text).style(style);
    frame.render_widget(paragraph, area);
}

fn render_autocomplete_list(frame: &mut Frame, area: Rect, dialog: &FilterDialogState) {
    let is_tag_focused = dialog.focus == FilterDialogFocus::Tag;
    let visible_height = area.height as usize;

    let total_tags = dialog.filtered_tags.len();
    let scroll_offset = dialog.tag_scroll_offset;

    let items: Vec<ListItem> = dialog
        .filtered_tags
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(idx, tag)| {
            let style = if is_tag_focused && idx == dialog.tag_list_index {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else {
                Style::default().fg(TAG_COLOR)
            };
            ListItem::new(format!("   {}", tag)).style(style)
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

    let list = List::new(items).block(Block::default().borders(Borders::TOP).title(title));
    frame.render_widget(list, area);
}

fn render_help_text(frame: &mut Frame, area: Rect) {
    let help = " Arrows:Navigate  0:Clear  m/Esc:Close";
    let paragraph = Paragraph::new(help).style(Style::default().fg(HELP_TEXT));
    frame.render_widget(paragraph, area);
}
