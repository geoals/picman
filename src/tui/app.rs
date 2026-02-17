use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use super::mouse::{self, MouseState};
use ratatui::prelude::*;
use std::io::stdout;
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info, instrument};

use crate::cli::{run_init, run_sync_incremental};
use crate::db::Database;

use super::state::AppState;
use super::ui::render;

/// Run the TUI application
#[instrument(skip_all, fields(library = %library_path.display(), skip_sync))]
pub fn run_tui(library_path: &Path, skip_sync: bool) -> Result<()> {
    let db_path = library_path.join(".picman.db");
    let mut status_parts = Vec::new();

    info!("starting TUI");

    // Auto-init if no database exists
    if !db_path.exists() {
        info!("no database found, initializing");
        let stats = run_init(library_path)?;
        info!(dirs = stats.directories, files = stats.files, "init complete");
        status_parts.push(format!(
            "Init: {} dirs, {} files",
            stats.directories, stats.files
        ));
    }

    // Incremental sync on startup (unless --skip-sync)
    if skip_sync {
        info!("skipping sync (--skip-sync)");
    } else {
        info!("syncing database with filesystem (incremental)");
        let sync_stats = run_sync_incremental(library_path)?;
        let sync_changes = sync_stats.directories_added
            + sync_stats.directories_removed
            + sync_stats.files_added
            + sync_stats.files_removed
            + sync_stats.files_modified;
        info!(
            dirs_added = sync_stats.directories_added,
            dirs_removed = sync_stats.directories_removed,
            files_added = sync_stats.files_added,
            files_removed = sync_stats.files_removed,
            files_modified = sync_stats.files_modified,
            "sync complete"
        );
        if sync_changes > 0 {
            status_parts.push(format!(
                "Sync: +{} -{} files",
                sync_stats.files_added,
                sync_stats.files_removed
            ));
        }
    }

    debug!("opening database");
    let db = Database::open(&db_path)?;

    // Initialize state
    debug!("loading directory tree");
    let mut state = AppState::new(library_path.to_path_buf(), db)?;
    info!(dirs = state.tree.directories.len(), "loaded directory tree");

    // Show startup status
    if !status_parts.is_empty() {
        state.status_message = Some(status_parts.join(" | "));
    }

    // Setup terminal
    debug!("setting up terminal");
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
    let mut mouse_state = MouseState::new();

    loop {
        // Check for background operation progress
        state.update_background_progress();

        // If we were cancelling and operation is done, quit
        if cancelling && !state.has_background_operation() {
            return Ok(());
        }

        // Poll for completed preview loads and insert into cache
        state.poll_preview_results();

        // Force full terminal repaint after closing overlays — image protocol
        // content (kitty/sixel) gets destroyed by overlays and ratatui's diff
        // alone can't restore it.
        if state.force_redraw {
            state.force_redraw = false;
            terminal.clear()?;
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
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match handle_key(key.code, state)? {
                        KeyAction::Quit => return Ok(()),
                        KeyAction::Cancelling => cancelling = true,
                        KeyAction::Continue => {}
                    }
                }
                Event::Mouse(mouse_event) => {
                    mouse::handle_mouse(mouse_event, state, &mut mouse_state)?;
                }
                _ => {}
            }
        }

        // Drain all pending events to avoid lag during rapid navigation
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match handle_key(key.code, state)? {
                        KeyAction::Quit => return Ok(()),
                        KeyAction::Cancelling => cancelling = true,
                        KeyAction::Continue => {}
                    }
                }
                Event::Mouse(mouse_event) => {
                    mouse::handle_mouse(mouse_event, state, &mut mouse_state)?;
                }
                _ => {}
            }
        }

        // After draining all keypresses, process deferred updates
        state.load_files_if_dirty()?;
        state.clear_skip_preview();
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
            KeyCode::Esc => {
                let tag_editing = focus == Some(FilterDialogFocus::Tag)
                    && state.filter_dialog.as_ref().is_some_and(|d| d.tag_editing);
                if tag_editing {
                    // Exit tag editing mode — keep input text so user can
                    // navigate the filtered list with j/k
                    if let Some(ref mut dialog) = state.filter_dialog {
                        dialog.tag_editing = false;
                    }
                } else {
                    state.apply_filter()?;
                }
            }
            KeyCode::Char('m') => {
                let tag_editing = focus == Some(FilterDialogFocus::Tag)
                    && state.filter_dialog.as_ref().is_some_and(|d| d.tag_editing);
                if tag_editing {
                    state.filter_dialog_char('m');
                } else {
                    state.apply_filter()?;
                }
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
            KeyCode::Left => {
                state.filter_dialog_left();
                state.auto_apply_filter()?;
            }
            KeyCode::Right => {
                state.filter_dialog_right();
                state.auto_apply_filter()?;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                match focus {
                    Some(FilterDialogFocus::VideoOnly) => {
                        state.filter_dialog_toggle_video();
                        state.auto_apply_filter()?;
                    }
                    Some(FilterDialogFocus::Tag) => {
                        let editing = state.filter_dialog.as_ref().is_some_and(|d| d.tag_editing);
                        let on_input = state.filter_dialog.as_ref().is_some_and(|d| d.tag_input_selected);
                        if editing {
                            let has_match = state.filter_dialog.as_ref()
                                .is_some_and(|d| !d.filtered_tags.is_empty());
                            if has_match {
                                // Add the highlighted autocomplete match
                                state.filter_dialog_add_tag();
                                state.auto_apply_filter()?;
                            } else {
                                // No matches (or empty input): exit editing mode
                                if let Some(ref mut dialog) = state.filter_dialog {
                                    dialog.tag_editing = false;
                                }
                            }
                        } else if on_input {
                            // On input line: Enter starts editing mode
                            if let Some(ref mut dialog) = state.filter_dialog {
                                dialog.tag_editing = true;
                            }
                        } else {
                            // On a tag in the list: Enter adds that tag
                            state.filter_dialog_add_tag();
                            state.auto_apply_filter()?;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                let tag_editing = focus == Some(FilterDialogFocus::Tag)
                    && state.filter_dialog.as_ref().is_some_and(|d| d.tag_editing);
                if tag_editing {
                    // Check if input is empty — if so, exit editing mode
                    let input_empty = state.filter_dialog.as_ref().is_some_and(|d| d.tag_input.is_empty());
                    if input_empty {
                        if let Some(ref mut dialog) = state.filter_dialog {
                            dialog.tag_editing = false;
                        }
                    } else {
                        state.filter_dialog_backspace();
                        state.auto_apply_filter()?;
                    }
                } else {
                    // Not editing tags: backspace removes last selected tag
                    state.filter_dialog_backspace();
                    state.auto_apply_filter()?;
                }
            }
            KeyCode::Char(c) => {
                // When editing tag input, all chars go to tag input
                let tag_editing = focus == Some(FilterDialogFocus::Tag)
                    && state.filter_dialog.as_ref().is_some_and(|d| d.tag_editing);
                if tag_editing {
                    state.filter_dialog_char(c);
                } else {
                    // Navigation and shortcuts when not editing tags
                    match c {
                        'i' => {
                            // Enter editing mode on the tag input line
                            let on_input = focus == Some(FilterDialogFocus::Tag)
                                && state.filter_dialog.as_ref().is_some_and(|d| d.tag_input_selected);
                            if on_input {
                                if let Some(ref mut dialog) = state.filter_dialog {
                                    dialog.tag_editing = true;
                                }
                            }
                        }
                        'j' => {
                            if focus == Some(FilterDialogFocus::Tag) {
                                state.filter_dialog_down();
                            } else {
                                state.filter_dialog_focus_down();
                            }
                        }
                        'k' => {
                            if focus == Some(FilterDialogFocus::Tag) {
                                state.filter_dialog_up();
                            } else {
                                state.filter_dialog_focus_up();
                            }
                        }
                        'h' => {
                            state.filter_dialog_left();
                            state.auto_apply_filter()?;
                        }
                        'l' => {
                            state.filter_dialog_right();
                            state.auto_apply_filter()?;
                        }
                        '0' => state.clear_filter()?,
                        '1' | 'a' => {
                            state.filter_dialog_set_rating(1);
                            state.auto_apply_filter()?;
                        }
                        '2' | 's' => {
                            state.filter_dialog_set_rating(2);
                            state.auto_apply_filter()?;
                        }
                        '3' | 'd' => {
                            state.filter_dialog_set_rating(3);
                            state.auto_apply_filter()?;
                        }
                        '4' | 'f' => {
                            state.filter_dialog_set_rating(4);
                            state.auto_apply_filter()?;
                        }
                        '5' | 'g' => {
                            state.filter_dialog_set_rating(5);
                            state.auto_apply_filter()?;
                        }
                        'v' => {
                            state.filter_dialog_toggle_video();
                            state.auto_apply_filter()?;
                        }
                        'u' => {
                            state.filter_dialog_set_unrated();
                            state.auto_apply_filter()?;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        return Ok(KeyAction::Continue);
    }

    // Handle tag input popup if active
    if state.tag_input.is_some() {
        let editing = state.tag_input.as_ref().is_some_and(|t| t.editing);
        let input_selected = state.tag_input.as_ref().is_some_and(|t| t.input_selected);

        if editing {
            match code {
                KeyCode::Esc => {
                    if let Some(ref mut input) = state.tag_input {
                        input.editing = false;
                    }
                }
                KeyCode::Enter => {
                    let input_empty = state
                        .tag_input
                        .as_ref()
                        .is_some_and(|t| t.input.is_empty());
                    if input_empty {
                        // Empty input: exit editing mode
                        if let Some(ref mut input) = state.tag_input {
                            input.editing = false;
                        }
                    } else {
                        // Non-empty: apply selected match or create new tag
                        state.toggle_tag()?;
                    }
                }
                KeyCode::Backspace => {
                    let input_empty = state
                        .tag_input
                        .as_ref()
                        .is_some_and(|t| t.input.is_empty());
                    if input_empty {
                        if let Some(ref mut input) = state.tag_input {
                            input.editing = false;
                        }
                    } else {
                        state.tag_input_backspace();
                    }
                }
                KeyCode::Up => state.tag_input_up(),
                KeyCode::Down => state.tag_input_down(),
                KeyCode::Char(c) => state.tag_input_char(c),
                _ => {}
            }
        } else {
            // Browse mode: vim-style navigation
            match code {
                KeyCode::Esc => state.close_tag_input(),
                KeyCode::Char('j') | KeyCode::Down => state.tag_input_down(),
                KeyCode::Char('k') | KeyCode::Up => state.tag_input_up(),
                KeyCode::Enter => {
                    if input_selected {
                        if let Some(ref mut input) = state.tag_input {
                            input.editing = true;
                        }
                    } else {
                        state.toggle_tag()?;
                    }
                }
                KeyCode::Char('i') if input_selected => {
                    if let Some(ref mut input) = state.tag_input {
                        input.editing = true;
                    }
                }
                _ => {}
            }
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

    // Handle help overlay — eat all keys except ? and Esc which close it
    if state.show_help {
        match code {
            KeyCode::Char('?') | KeyCode::Esc => {
                state.show_help = false;
                state.force_redraw = true;
            }
            _ => {}
        }
        return Ok(KeyAction::Continue);
    }

    // Handle operations menu
    if state.operations_menu.is_some() {
        match code {
            KeyCode::Esc | KeyCode::Char('o') => state.close_operations_menu(),
            KeyCode::Up | KeyCode::Char('k') => state.operations_menu_up(),
            KeyCode::Down | KeyCode::Char('j') => state.operations_menu_down(),
            KeyCode::Enter => state.operations_menu_select(),
            KeyCode::Char('1') => {
                state.close_operations_menu();
                state.run_operation(crate::tui::state::OperationType::Thumbnails);
            }
            KeyCode::Char('2') => {
                state.close_operations_menu();
                state.run_operation(crate::tui::state::OperationType::Orientation);
            }
            KeyCode::Char('3') => {
                state.close_operations_menu();
                state.run_operation(crate::tui::state::OperationType::Hash);
            }
            KeyCode::Char('4') => {
                state.close_operations_menu();
                state.run_operation(crate::tui::state::OperationType::DirPreview);
            }
            KeyCode::Char('5') => {
                state.close_operations_menu();
                state.run_operation(crate::tui::state::OperationType::DirPreviewRecursive);
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
        KeyCode::Char('?') => state.toggle_help(),
        _ => {}
    }
    Ok(KeyAction::Continue)
}

