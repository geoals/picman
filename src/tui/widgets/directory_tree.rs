use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, List, ListItem},
};

use crate::tui::state::{AppState, Focus};

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
            let name = dir
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&dir.path);

            let display_name = if name.is_empty() { "." } else { name };

            // Show rating as compact "★N" on left, or spaces for alignment
            let rating = dir
                .rating
                .map(|r| format!("★{} ", r))
                .unwrap_or_else(|| "   ".to_string());

            let line = format!("{}{}{}{}", indent, rating, icon, display_name);

            ListItem::new(line)
        })
        .collect();

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
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
