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

use crate::db::Database;

use super::state::AppState;
use super::ui::render;

/// Run the TUI application
pub fn run_tui(library_path: &Path) -> Result<()> {
    // Open database
    let db_path = library_path.join(".picman.db");
    if !db_path.exists() {
        anyhow::bail!(
            "No database found at {}. Run 'picman init' first.",
            db_path.display()
        );
    }
    let db = Database::open(&db_path)?;

    // Initialize state
    let mut state = AppState::new(library_path.to_path_buf(), db)?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

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
    loop {
        terminal.draw(|frame| render(frame, state))?;

        // Wait for first event
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                if handle_key(key.code, state)? {
                    return Ok(());
                }
            }
        }

        // Drain all pending events to avoid lag during rapid navigation
        while event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if handle_key(key.code, state)? {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Handle a key press. Returns true if the app should quit.
fn handle_key(code: KeyCode, state: &mut AppState) -> Result<bool> {
    match code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('j') | KeyCode::Down => state.move_down()?,
        KeyCode::Char('k') | KeyCode::Up => state.move_up()?,
        KeyCode::Char('h') | KeyCode::Left => state.move_left(),
        KeyCode::Char('l') | KeyCode::Right => state.move_right(),
        KeyCode::Tab => state.toggle_focus(),
        KeyCode::Enter => state.select()?,
        KeyCode::Char('1') => state.set_rating(Some(1))?,
        KeyCode::Char('2') => state.set_rating(Some(2))?,
        KeyCode::Char('3') => state.set_rating(Some(3))?,
        KeyCode::Char('4') => state.set_rating(Some(4))?,
        KeyCode::Char('5') => state.set_rating(Some(5))?,
        KeyCode::Char('0') => state.set_rating(None)?,
        KeyCode::Char('?') => state.toggle_help(),
        _ => {}
    }
    Ok(false)
}
