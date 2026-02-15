use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::stdout;
use std::path::Path;
use std::time::Duration;

use crate::cli::{run_init, run_sync};
use crate::db::Database;

use super::state::AppState;
use super::ui::render;

/// Run the TUI application
pub fn run_tui(library_path: &Path) -> Result<()> {
    let db_path = library_path.join(".picman.db");
    let mut status_parts = Vec::new();

    // Auto-init if no database exists
    if !db_path.exists() {
        let stats = run_init(library_path)?;
        status_parts.push(format!(
            "Init: {} dirs, {} files",
            stats.directories, stats.files
        ));
    }

    // Always sync on startup (fast mtime check)
    let sync_stats = run_sync(library_path, false, false)?;
    let sync_changes = sync_stats.directories_added
        + sync_stats.directories_removed
        + sync_stats.files_added
        + sync_stats.files_removed
        + sync_stats.files_modified;
    if sync_changes > 0 {
        status_parts.push(format!(
            "Sync: +{} -{} files",
            sync_stats.files_added,
            sync_stats.files_removed
        ));
    }

    let db = Database::open(&db_path)?;

    // Initialize state
    let mut state = AppState::new(library_path.to_path_buf(), db)?;

    // Show startup status
    if !status_parts.is_empty() {
        state.status_message = Some(status_parts.join(" | "));
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Main loop
    let result = run_app(&mut terminal, &mut state);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut AppState,
) -> Result<()> {
    let mut cancelling = false;

    loop {
        // Check for background operation progress
        state.update_background_progress();

        // If we were cancelling and operation is done, quit
        if cancelling && !state.has_background_operation() {
            return Ok(());
        }

        terminal.draw(|frame| render(frame, state))?;

        // Use shorter timeout when background work is happening to update progress
        let timeout = if state.background_progress.is_some() {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(1)
        };

        // Wait for event with timeout
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match handle_key(key.code, state)? {
                        KeyAction::Quit => return Ok(()),
                        KeyAction::Cancelling => cancelling = true,
                        KeyAction::Continue => {}
                    }
                }
            }
        }

        // Drain all pending events to avoid lag during rapid navigation
        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match handle_key(key.code, state)? {
                        KeyAction::Quit => return Ok(()),
                        KeyAction::Cancelling => cancelling = true,
                        KeyAction::Continue => {}
                    }
                }
            }
        }
    }
}

enum KeyAction {
    Quit,
    Continue,
    Cancelling, // Waiting for background operation to cancel
}

/// Handle a key press. Returns KeyAction indicating what to do next.
fn handle_key(code: KeyCode, state: &mut AppState) -> Result<KeyAction> {
    // Handle filter dialog if active
    if state.filter_dialog.is_some() {
        use super::state::FilterDialogFocus;
        let focus = state.filter_dialog_focus();

        match code {
            KeyCode::Esc | KeyCode::Char('m') => {
                // Apply filter when closing
                state.apply_filter()?;
            }
            KeyCode::Tab => state.filter_dialog_focus_down(),
            KeyCode::BackTab => state.filter_dialog_focus_up(),
            KeyCode::Up => {
                if focus == Some(FilterDialogFocus::Tag) {
                    state.filter_dialog_up();
                } else {
                    state.filter_dialog_focus_up();
                }
            }
            KeyCode::Down => {
                if focus == Some(FilterDialogFocus::Tag) {
                    state.filter_dialog_down();
                } else {
                    state.filter_dialog_focus_down();
                }
            }
            KeyCode::Left => state.filter_dialog_left(),
            KeyCode::Right => state.filter_dialog_right(),
            KeyCode::Enter | KeyCode::Char(' ') => {
                match focus {
                    Some(FilterDialogFocus::VideoOnly) => {
                        state.filter_dialog_toggle_video();
                        state.auto_apply_filter()?;
                    }
                    Some(FilterDialogFocus::Tag) => {
                        state.filter_dialog_add_tag();
                        state.auto_apply_filter()?;
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                state.filter_dialog_backspace();
                state.auto_apply_filter()?;
            }
            KeyCode::Char('0') => {
                state.clear_filter()?;
            }
            KeyCode::Char('1') | KeyCode::Char('a') => {
                state.filter_dialog_set_rating(1);
                state.auto_apply_filter()?;
            }
            KeyCode::Char('2') | KeyCode::Char('s') => {
                state.filter_dialog_set_rating(2);
                state.auto_apply_filter()?;
            }
            KeyCode::Char('3') | KeyCode::Char('d') => {
                state.filter_dialog_set_rating(3);
                state.auto_apply_filter()?;
            }
            KeyCode::Char('4') | KeyCode::Char('f') => {
                state.filter_dialog_set_rating(4);
                state.auto_apply_filter()?;
            }
            KeyCode::Char('5') | KeyCode::Char('g') => {
                state.filter_dialog_set_rating(5);
                state.auto_apply_filter()?;
            }
            KeyCode::Char('v') => {
                state.filter_dialog_toggle_video();
                state.auto_apply_filter()?;
            }
            KeyCode::Char('u') => {
                state.filter_dialog_set_unrated();
                state.auto_apply_filter()?;
            }
            KeyCode::Char(c) => {
                state.filter_dialog_char(c);
            }
            _ => {}
        }
        return Ok(KeyAction::Continue);
    }

    // Handle tag input popup if active
    if state.tag_input.is_some() {
        match code {
            KeyCode::Esc => state.close_tag_input(),
            KeyCode::Enter => state.apply_tag()?,
            KeyCode::Backspace => state.tag_input_backspace(),
            KeyCode::Up => state.tag_input_up(),
            KeyCode::Down => state.tag_input_down(),
            KeyCode::Char(c) => state.tag_input_char(c),
            _ => {}
        }
        return Ok(KeyAction::Continue);
    }

    // Handle rename dialog if active
    if let Some(ref mut dialog) = state.rename_dialog {
        const VISIBLE_SUGGESTIONS: usize = 8;
        match code {
            KeyCode::Esc => state.close_rename_dialog(),
            KeyCode::Enter => state.apply_rename()?,
            KeyCode::Backspace => dialog.backspace(),
            KeyCode::Delete => dialog.delete(),
            KeyCode::Left => dialog.move_cursor_left(),
            KeyCode::Right => dialog.move_cursor_right(),
            KeyCode::Home => dialog.move_cursor_home(),
            KeyCode::End => dialog.move_cursor_end(),
            KeyCode::Up => dialog.select_prev_suggestion(VISIBLE_SUGGESTIONS),
            KeyCode::Down => dialog.select_next_suggestion(VISIBLE_SUGGESTIONS),
            KeyCode::Tab => dialog.use_suggestion(),
            KeyCode::BackTab => dialog.append_suggestion(),
            KeyCode::Char(c) => dialog.insert_char(c),
            _ => {}
        }
        return Ok(KeyAction::Continue);
    }

    // Handle operations menu
    if state.operations_menu.is_some() {
        match code {
            KeyCode::Esc => state.close_operations_menu(),
            KeyCode::Up | KeyCode::Char('k') => state.operations_menu_up(),
            KeyCode::Down | KeyCode::Char('j') => state.operations_menu_down(),
            KeyCode::Enter => state.operations_menu_select(),
            KeyCode::Char('1') | KeyCode::Char('t') => {
                state.close_operations_menu();
                state.run_operation(crate::tui::state::OperationType::Thumbnails);
            }
            KeyCode::Char('2') | KeyCode::Char('o') => {
                state.close_operations_menu();
                state.run_operation(crate::tui::state::OperationType::Orientation);
            }
            KeyCode::Char('3') | KeyCode::Char('h') => {
                state.close_operations_menu();
                state.run_operation(crate::tui::state::OperationType::Hash);
            }
            _ => {}
        }
        return Ok(KeyAction::Continue);
    }

    // Clear status message on any key
    state.clear_status_message();

    // Normal key handling
    match code {
        KeyCode::Char('q') => {
            if state.has_background_operation() {
                state.cancel_background_operation();
                return Ok(KeyAction::Cancelling);
            }
            return Ok(KeyAction::Quit);
        }
        KeyCode::Char('j') | KeyCode::Down => state.move_down()?,
        KeyCode::Char('k') | KeyCode::Up => state.move_up()?,
        KeyCode::Char('h') | KeyCode::Left => state.move_left(),
        KeyCode::Char('l') | KeyCode::Right => state.move_right(),
        KeyCode::Tab => state.toggle_focus(),
        KeyCode::Enter => state.select()?,
        KeyCode::Char('1') | KeyCode::Char('a') => state.set_rating(Some(1))?,
        KeyCode::Char('2') | KeyCode::Char('s') => state.set_rating(Some(2))?,
        KeyCode::Char('3') | KeyCode::Char('d') => state.set_rating(Some(3))?,
        KeyCode::Char('4') | KeyCode::Char('f') => state.set_rating(Some(4))?,
        KeyCode::Char('5') | KeyCode::Char('g') => state.set_rating(Some(5))?,
        KeyCode::Char('0') => state.set_rating(None)?,
        KeyCode::Char('t') => state.open_tag_input()?,
        KeyCode::Char('r') => state.open_rename_dialog()?,
        KeyCode::Char('o') => state.open_operations_menu(),
        KeyCode::Char('m') => state.open_filter_dialog()?,
        KeyCode::Char('p') => state.run_operation(crate::tui::state::OperationType::DirPreview),
        KeyCode::Char('P') => state.run_operation(crate::tui::state::OperationType::DirPreviewRecursive),
        KeyCode::Char('?') => state.toggle_help(),
        _ => {}
    }
    Ok(KeyAction::Continue)
}

