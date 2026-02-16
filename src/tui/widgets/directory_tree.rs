use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, List, ListItem},
};

use crate::tui::colors::{FOCUS_COLOR, RATING_COLOR, UNFOCUS_COLOR};
use crate::tui::state::{AppState, Focus};

use super::has_dir_preview;

pub fn render_directory_tree(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let is_focused = state.focus == Focus::DirectoryTree;

    let visible_dirs = state.get_visible_directories();

    let items: Vec<ListItem> = visible_dirs
        .iter()
        .map(|dir| {
            let depth = state.tree.depth(dir);
            let indent = "  ".repeat(depth);

            let icon = if state.tree.has_visible_children(dir.id, &state.matching_dir_ids) {
                if state.tree.expanded.contains(&dir.id) {
                    "▼ "
                } else {
                    "▶ "
                }
            } else {
                "  "
            };

            // Get directory name (last component of path)
            let name = dir.path.rsplit('/').next().unwrap_or(&dir.path);

            let display_name = if name.is_empty() { "." } else { name };

            // Build line with styled spans for colored rating
            let mut spans = Vec::new();
            spans.push(Span::raw(indent));

            // Show rating as compact "★N" on left, colored yellow
            if let Some(r) = dir.rating {
                spans.push(Span::styled(
                    format!("★{} ", r),
                    Style::default().fg(RATING_COLOR),
                ));
            } else {
                spans.push(Span::raw("   "));
            }

            spans.push(Span::raw(icon));
            spans.push(Span::raw(display_name.to_string()));

            // Show missing preview indicator
            if !has_dir_preview(dir.id) {
                spans.push(Span::styled(" 󰋩", Style::default().fg(Color::Red)));
            }

            // Show missing file thumbnail indicator
            if state.dir_missing_file_thumbnails(dir.id) {
                spans.push(Span::styled(" 󰐊", Style::default().fg(Color::Yellow)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let border_style = if is_focused {
        Style::default().fg(FOCUS_COLOR)
    } else {
        Style::default().fg(UNFOCUS_COLOR)
    };

    let highlight_style = if is_focused {
        Style::default().bg(Color::Cyan).fg(Color::Black)
    } else {
        Style::default().bg(Color::Gray).fg(Color::Black)
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(" Directories "),
        )
        .highlight_style(highlight_style);

    frame.render_stateful_widget(list, area, &mut state.tree.list_state);
}
