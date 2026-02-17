use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::colors::{HEADER_COLOR, HELP_TEXT};
use super::state::AppState;
use super::widgets::{
    render_details_panel, render_directory_tree, render_file_list, render_filter_dialog,
    render_preview, render_rename_dialog, render_status_bar, render_tag_popup,
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

    // Split left section: tree+files area | details panel
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(7)])
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

    // Save layout rects for mouse hit-testing
    state.tree_area = tree_area;
    state.file_list_area = file_list_area;

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

    // Render rename dialog if active
    if let Some(ref rename_dialog) = state.rename_dialog {
        render_rename_dialog(frame, size, rename_dialog);
    }

    // Render operations menu if active
    if let Some(ref menu) = state.operations_menu {
        render_operations_menu(frame, size, menu);
    }
}

fn render_operations_menu(frame: &mut Frame, area: Rect, menu: &super::state::OperationsMenuState) {
    let dir_name = if menu.directory_path.is_empty() { "." } else { &menu.directory_path };

    let options = [
        ("1", "Thumbnails",              "Generate preview thumbnails"),
        ("2", "Orientation",             "Tag landscape/portrait"),
        ("3", "Hash",                    "Compute file hashes"),
        ("4", "Dir preview",             "Current directory only"),
        ("5", "Dir preview (recursive)", "Include subdirectories"),
    ];

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(" Directory: ", Style::default().fg(Color::DarkGray)),
            Span::styled(dir_name, Style::default().fg(HEADER_COLOR)),
            Span::styled(format!(" ({} files)", menu.file_count), Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
    ];

    for (i, (key, name, desc)) in options.iter().enumerate() {
        let is_selected = i == menu.selected;
        if is_selected {
            lines.push(Line::from(vec![
                Span::styled(" ▸ ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("[{}] {}", key, name),
                    Style::default().bg(Color::Cyan).fg(Color::Black),
                ),
                Span::styled(format!("  {}", desc), Style::default().fg(Color::White)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(format!("[{}]", key), Style::default().fg(Color::Cyan)),
                Span::styled(format!(" {}", name), Style::default().fg(Color::White)),
                Span::styled(format!("  {}", desc), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Enter: select  o/Esc: cancel",
        Style::default().fg(HELP_TEXT),
    )));

    let width = 55;
    let height = 11;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;

    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Operations ")
        .title_style(Style::default().fg(HEADER_COLOR).add_modifier(Modifier::BOLD));

    let dialog = Paragraph::new(lines).block(block);

    frame.render_widget(dialog, dialog_area);
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
    Enter    Open file / Select dir
    1-5/asdfg Set rating
    0        Clear rating
    t        Add tag
    r        Rename directory
    o        Operations menu
    m        Filter
    ?        Toggle help
    q        Quit

  Mouse:
    Click        Select item / focus pane
    Double-click Open/expand (= Enter)
    Scroll wheel Move selection up/down
"#;

    let help_width = 42;
    let help_height = 22;
    let x = (area.width.saturating_sub(help_width)) / 2;
    let y = (area.height.saturating_sub(help_height)) / 2;

    let help_area = Rect::new(x, y, help_width, help_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, help_area);

    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title(" Help "));

    frame.render_widget(help, help_area);
}
