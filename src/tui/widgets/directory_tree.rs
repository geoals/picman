use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, List, ListItem},
};

use crate::tui::state::{AppState, Focus};

pub fn render_directory_tree(frame: &mut Frame, area: Rect, state: &AppState) {
    let is_focused = state.focus == Focus::DirectoryTree;

    let visible_dirs = state.tree.visible_directories();

    let items: Vec<ListItem> = visible_dirs
        .iter()
        .enumerate()
        .map(|(idx, dir)| {
            let depth = state.tree.depth(dir);
            let indent = "  ".repeat(depth);

            let icon = if state.tree.has_children(dir.id) {
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

            let line = format!("{}{}{}", indent, icon, display_name);

            let style = if idx == state.tree.selected_index {
                if is_focused {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                } else {
                    Style::default().bg(Color::Gray).fg(Color::Black)
                }
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Directories "),
    );

    frame.render_widget(list, area);
}
