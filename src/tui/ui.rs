use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::state::AppState;
use super::widgets::{render_directory_tree, render_file_list, render_preview, render_status_bar};

/// Main render function
pub fn render(frame: &mut Frame, state: &AppState) {
    let size = frame.area();

    // Main layout: status bar at bottom, content above
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(size);

    let content_area = main_chunks[0];
    let status_area = main_chunks[1];

    // 3-column layout: tree | file list | preview (largest)
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(65),
        ])
        .split(content_area);

    let tree_area = content_chunks[0];
    let file_list_area = content_chunks[1];
    let preview_area = content_chunks[2];

    // Render widgets
    render_directory_tree(frame, tree_area, state);
    render_file_list(frame, file_list_area, state);
    render_preview(frame, preview_area, state);
    render_status_bar(frame, status_area, state);

    // Render help overlay if shown
    if state.show_help {
        render_help_overlay(frame, size);
    }
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let help_text = r#"
  Key Bindings

  Navigation:
    j/↓      Move down
    k/↑      Move up
    h/←      Collapse/Left pane
    l/→      Expand/Right pane
    Tab      Switch focus

  Actions:
    Enter    Select directory
    1-5      Set rating
    0        Clear rating
    ?        Toggle help
    q        Quit
"#;

    let help_width = 40;
    let help_height = 16;
    let x = (area.width.saturating_sub(help_width)) / 2;
    let y = (area.height.saturating_sub(help_height)) / 2;

    let help_area = Rect::new(x, y, help_width, help_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, help_area);

    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title(" Help "));

    frame.render_widget(help, help_area);
}
