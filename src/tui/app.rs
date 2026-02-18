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

use super::state::{AppState, Focus};
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

        // Clear skip_preview AFTER rendering so it takes effect this frame.
        // Event handling below may set it again for the next frame.
        state.clear_skip_preview();

        // Use shorter timeout when we're waiting for async work:
        // - Background operations (thumbnails, hashing): 100ms for progress updates
        // - Pending preview: 5ms so the worker result is picked up promptly
        // - Idle: 1 second to save CPU
        let preview_ready = match state.focus {
            Focus::FileList => match state.selected_file_path() {
                Some(sel) => state.preview_cache.borrow().has_protocol(&sel),
                None => true,
            },
            Focus::DirectoryTree => {
                state.preview_loader.borrow().pending_count() == 0
            }
        };

        let timeout = if state.background_progress.is_some() {
            Duration::from_millis(100)
        } else if !preview_ready {
            Duration::from_millis(5)
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
        state.refresh_exif_cache();
    }
}

enum KeyAction {
    Quit,
    Continue,
    Cancelling, // Waiting for background operation to cancel
}

/// Deferred action from filter dialog key handling (two-pass borrow-checker pattern)
enum FilterAction {
    None,
    AutoApply,
    Apply,
    Clear,
}

/// Handle a key press. Returns KeyAction indicating what to do next.
fn handle_key(code: KeyCode, state: &mut AppState) -> Result<KeyAction> {
    // Handle filter dialog if active
    if state.filter_dialog.is_some() {
        use super::dialogs::FilterDialogFocus;
        let mut action = FilterAction::None;

        // First pass: mutate the dialog directly
        if let Some(ref mut dialog) = state.filter_dialog {
            let focus = dialog.focus;
            let tag_editing = focus == FilterDialogFocus::Tag && dialog.tag_editing;

            match code {
                KeyCode::Esc => {
                    if tag_editing {
                        dialog.tag_editing = false;
                    } else {
                        action = FilterAction::Apply;
                    }
                }
                KeyCode::Char('m') => {
                    if tag_editing {
                        dialog.char_input('m');
                    } else {
                        action = FilterAction::Apply;
                    }
                }
                KeyCode::Tab => dialog.cycle_focus_down(),
                KeyCode::BackTab => dialog.cycle_focus_up(),
                KeyCode::Up => {
                    if focus == FilterDialogFocus::Tag {
                        dialog.navigate_up();
                    } else {
                        dialog.cycle_focus_up();
                    }
                }
                KeyCode::Down => {
                    if focus == FilterDialogFocus::Tag {
                        dialog.navigate_down();
                    } else {
                        dialog.cycle_focus_down();
                    }
                }
                KeyCode::Left => {
                    dialog.navigate_rating_left();
                    action = FilterAction::AutoApply;
                }
                KeyCode::Right => {
                    dialog.navigate_rating_right();
                    action = FilterAction::AutoApply;
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match focus {
                        FilterDialogFocus::VideoOnly => {
                            dialog.toggle_video();
                            action = FilterAction::AutoApply;
                        }
                        FilterDialogFocus::Tag => {
                            if tag_editing {
                                if !dialog.filtered_tags.is_empty() {
                                    dialog.add_tag();
                                    action = FilterAction::AutoApply;
                                } else {
                                    dialog.tag_editing = false;
                                }
                            } else if dialog.tag_input_selected {
                                dialog.tag_editing = true;
                            } else {
                                dialog.add_tag();
                                action = FilterAction::AutoApply;
                            }
                        }
                        FilterDialogFocus::Rating => {}
                    }
                }
                KeyCode::Backspace => {
                    if tag_editing {
                        if dialog.tag_input.is_empty() {
                            dialog.tag_editing = false;
                        } else {
                            dialog.backspace();
                            action = FilterAction::AutoApply;
                        }
                    } else {
                        dialog.backspace();
                        action = FilterAction::AutoApply;
                    }
                }
                KeyCode::Char(c) => {
                    if tag_editing {
                        dialog.char_input(c);
                    } else {
                        match c {
                            'i' => {
                                if focus == FilterDialogFocus::Tag && dialog.tag_input_selected {
                                    dialog.tag_editing = true;
                                }
                            }
                            'j' => {
                                if focus == FilterDialogFocus::Tag {
                                    dialog.navigate_down();
                                } else {
                                    dialog.cycle_focus_down();
                                }
                            }
                            'k' => {
                                if focus == FilterDialogFocus::Tag {
                                    dialog.navigate_up();
                                } else {
                                    dialog.cycle_focus_up();
                                }
                            }
                            'h' => {
                                dialog.navigate_rating_left();
                                action = FilterAction::AutoApply;
                            }
                            'l' => {
                                dialog.navigate_rating_right();
                                action = FilterAction::AutoApply;
                            }
                            '0' => action = FilterAction::Clear,
                            '1' | 'a' => {
                                dialog.set_rating(1);
                                action = FilterAction::AutoApply;
                            }
                            '2' | 's' => {
                                dialog.set_rating(2);
                                action = FilterAction::AutoApply;
                            }
                            '3' | 'd' => {
                                dialog.set_rating(3);
                                action = FilterAction::AutoApply;
                            }
                            '4' | 'f' => {
                                dialog.set_rating(4);
                                action = FilterAction::AutoApply;
                            }
                            '5' | 'g' => {
                                dialog.set_rating(5);
                                action = FilterAction::AutoApply;
                            }
                            'v' => {
                                dialog.toggle_video();
                                action = FilterAction::AutoApply;
                            }
                            'u' => {
                                dialog.set_unrated();
                                action = FilterAction::AutoApply;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Second pass: actions that need &mut AppState
        match action {
            FilterAction::AutoApply => state.auto_apply_filter()?,
            FilterAction::Apply => state.apply_filter()?,
            FilterAction::Clear => state.clear_filter()?,
            FilterAction::None => {}
        }
        return Ok(KeyAction::Continue);
    }

    // Handle tag input popup if active
    if state.tag_input.is_some() {
        let mut do_toggle = false;
        let mut do_close = false;

        // First pass: mutate the dialog directly
        if let Some(ref mut input) = state.tag_input {
            if input.editing {
                match code {
                    KeyCode::Esc => input.editing = false,
                    KeyCode::Enter => {
                        if input.input.is_empty() {
                            input.editing = false;
                        } else {
                            do_toggle = true;
                        }
                    }
                    KeyCode::Backspace => {
                        if input.input.is_empty() {
                            input.editing = false;
                        } else {
                            input.pop_char_and_filter();
                        }
                    }
                    KeyCode::Up => input.move_up(),
                    KeyCode::Down => input.move_down(),
                    KeyCode::Char(c) => input.push_char_and_filter(c),
                    _ => {}
                }
            } else {
                // Browse mode: vim-style navigation
                match code {
                    KeyCode::Esc => do_close = true,
                    KeyCode::Char('j') | KeyCode::Down => input.move_down(),
                    KeyCode::Char('k') | KeyCode::Up => input.move_up(),
                    KeyCode::Enter => {
                        if input.input_selected {
                            input.editing = true;
                        } else {
                            do_toggle = true;
                        }
                    }
                    KeyCode::Char('i') if input.input_selected => {
                        input.editing = true;
                    }
                    _ => {}
                }
            }
        }

        // Second pass: actions that need &mut AppState
        if do_toggle {
            state.toggle_tag()?;
        } else if do_close {
            state.close_tag_input();
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

    // Handle search mode
    if state.search.active {
        match code {
            KeyCode::Esc => {
                state.search.deactivate();
            }
            KeyCode::Enter => {
                state.search.accept();
            }
            KeyCode::Backspace => {
                if state.search.query.is_empty() {
                    state.search.deactivate();
                } else {
                    state.search.pop_char();
                }
            }
            KeyCode::Char(c) => {
                state.search.push_char(c);
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
        KeyCode::Char('/') => state.search.activate(),
        KeyCode::Char('i') => {
            state.details_expanded = !state.details_expanded;
            if state.details_expanded {
                // Read EXIF for current file if we don't have it cached
                if let Some(path) = state.selected_file_path() {
                    let needs_read = state
                        .cached_exif
                        .as_ref()
                        .map(|(p, _)| p != &path)
                        .unwrap_or(true);
                    if needs_read {
                        let info = crate::tui::exif::read_exif(&path);
                        state.cached_exif = Some((path, info));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(KeyAction::Continue)
}

