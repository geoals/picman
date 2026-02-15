use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::state::AppState;
use super::widgets::{
    render_details_panel, render_directory_tree, render_file_list, render_filter_dialog,
    render_preview, render_status_bar, render_tag_popup,
};

/// Main render function
pub fn render(frame: &mut Frame, state: &mut AppState) {
    let size = frame.area();

    // Main layout: status bar at bottom, content above
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(size);

    let content_area = main_chunks[0];
    let status_area = main_chunks[1];

    // Split content: left section (tree + files + details) | preview
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(content_area);

    let left_section = content_chunks[0];
    let preview_area = content_chunks[1];

    // Split left section: tree+files area | details panel (4 lines)
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(5)])
        .split(left_section);

    let tree_files_area = left_chunks[0];
    let details_area = left_chunks[1];

    // Split tree+files area: tree | file list (keep ~43%/57% ratio)
    let tree_files_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(43), Constraint::Percentage(57)])
        .split(tree_files_area);

    let tree_area = tree_files_chunks[0];
    let file_list_area = tree_files_chunks[1];

    // Render widgets
    render_directory_tree(frame, tree_area, state);
    render_file_list(frame, file_list_area, state);
    render_details_panel(frame, details_area, state);
    render_preview(frame, preview_area, state);
    render_status_bar(frame, status_area, state);

    // Render help overlay if shown
    if state.show_help {
        render_help_overlay(frame, size);
    }

    // Render tag input popup if active
    if let Some(ref tag_input) = state.tag_input {
        render_tag_popup(frame, size, tag_input);
    }

    // Render filter dialog if active
    if let Some(ref filter_dialog) = state.filter_dialog {
        render_filter_dialog(frame, size, filter_dialog);
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
    1-5/asdfg Set rating
    0        Clear rating
    t        Add tag
    m        Filter
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
