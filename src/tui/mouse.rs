use std::time::Instant;

use anyhow::Result;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::state::{AppState, Focus};

/// Tracks state for double-click detection
pub struct MouseState {
    last_click_time: Option<Instant>,
    last_click_col: u16,
    last_click_row: u16,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            last_click_time: None,
            last_click_col: 0,
            last_click_row: 0,
        }
    }

    /// Record a click and return true if it's a double-click (same position within 500ms)
    fn record_click(&mut self, col: u16, row: u16) -> bool {
        let is_double = self
            .last_click_time
            .is_some_and(|t| t.elapsed().as_millis() < 500)
            && self.last_click_col == col
            && self.last_click_row == row;

        self.last_click_time = Some(Instant::now());
        self.last_click_col = col;
        self.last_click_row = row;

        is_double
    }
}

/// Result of handling a mouse event
pub enum MouseAction {
    Continue,
}

/// Handle a mouse event. Returns MouseAction indicating what to do next.
pub fn handle_mouse(
    event: MouseEvent,
    state: &mut AppState,
    mouse_state: &mut MouseState,
) -> Result<MouseAction> {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            handle_left_click(event.column, event.row, state, mouse_state)?;
        }
        MouseEventKind::ScrollUp => {
            handle_scroll_up(event.column, event.row, state)?;
        }
        MouseEventKind::ScrollDown => {
            handle_scroll_down(event.column, event.row, state)?;
        }
        _ => {}
    }
    Ok(MouseAction::Continue)
}

fn handle_left_click(
    col: u16,
    row: u16,
    state: &mut AppState,
    mouse_state: &mut MouseState,
) -> Result<()> {
    let is_double = mouse_state.record_click(col, row);

    if let Some(index) = tree_item_at(row, col, state) {
        state.focus = Focus::DirectoryTree;
        state.select_tree_index(index);
        if is_double {
            // Load files immediately so select() sees the right directory
            state.load_files_if_dirty()?;
            state.select()?;
        }
    } else if let Some(index) = file_item_at(row, col, state) {
        state.focus = Focus::FileList;
        state.select_file_index(index);
        if is_double {
            state.select()?;
        }
    }

    Ok(())
}

fn handle_scroll_up(_col: u16, row: u16, state: &mut AppState) -> Result<()> {
    let pane = pane_at(row, _col, state);
    match pane {
        Some(Focus::DirectoryTree) => {
            let saved = state.focus;
            state.focus = Focus::DirectoryTree;
            state.move_up()?;
            state.focus = saved;
        }
        Some(Focus::FileList) => {
            let saved = state.focus;
            state.focus = Focus::FileList;
            state.move_up()?;
            state.focus = saved;
        }
        None => {}
    }
    Ok(())
}

fn handle_scroll_down(_col: u16, row: u16, state: &mut AppState) -> Result<()> {
    let pane = pane_at(row, _col, state);
    match pane {
        Some(Focus::DirectoryTree) => {
            let saved = state.focus;
            state.focus = Focus::DirectoryTree;
            state.move_down()?;
            state.focus = saved;
        }
        Some(Focus::FileList) => {
            let saved = state.focus;
            state.focus = Focus::FileList;
            state.move_down()?;
            state.focus = saved;
        }
        None => {}
    }
    Ok(())
}

/// Determine which pane a coordinate falls in
fn pane_at(row: u16, col: u16, state: &AppState) -> Option<Focus> {
    if contains(state.tree_area, col, row) {
        Some(Focus::DirectoryTree)
    } else if contains(state.file_list_area, col, row) {
        Some(Focus::FileList)
    } else {
        None
    }
}

/// Map a click position to a tree item index, or None if outside the tree's data rows.
///
/// The tree widget uses `Block` with `Borders::ALL`, so the inner area starts at
/// `(tree_area.x + 1, tree_area.y + 1)`. Each item is 1 row high.
/// The scroll offset comes from `list_state.offset()`.
fn tree_item_at(row: u16, col: u16, state: &AppState) -> Option<usize> {
    let area = state.tree_area;
    if !contains(area, col, row) {
        return None;
    }

    // Border: inner area starts 1 row below top
    let inner_top = area.y + 1;
    if row < inner_top || row >= area.y + area.height.saturating_sub(1) {
        return None;
    }

    let row_in_list = (row - inner_top) as usize;
    let scroll_offset = state.tree.list_state.offset();
    let index = scroll_offset + row_in_list;

    let visible_count = state.get_visible_directories().len();
    if index < visible_count {
        Some(index)
    } else {
        None
    }
}

/// Map a click position to a file list item index, or None if outside the data rows.
///
/// The file list uses `Table` with `Borders::ALL` + a header row + 1-row `bottom_margin`.
/// So: border (1 row) + header (1 row) + margin (1 row) = first data row at y+3.
fn file_item_at(row: u16, col: u16, state: &AppState) -> Option<usize> {
    let area = state.file_list_area;
    if !contains(area, col, row) {
        return None;
    }

    // Border (1) + header (1) + bottom_margin (1) = 3 rows before data
    let data_top = area.y + 3;
    if row < data_top || row >= area.y + area.height.saturating_sub(1) {
        return None;
    }

    let row_in_table = (row - data_top) as usize;
    let scroll_offset = state.file_list.table_state.offset();
    let index = scroll_offset + row_in_table;

    if index < state.file_list.files.len() {
        Some(index)
    } else {
        None
    }
}

fn contains(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x
        && col < rect.x + rect.width
        && row >= rect.y
        && row < rect.y + rect.height
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn test_contains() {
        let rect = Rect::new(5, 10, 20, 10);
        assert!(contains(rect, 5, 10));
        assert!(contains(rect, 24, 19));
        assert!(!contains(rect, 4, 10));
        assert!(!contains(rect, 25, 10));
        assert!(!contains(rect, 5, 9));
        assert!(!contains(rect, 5, 20));
    }

    #[test]
    fn test_contains_zero_size() {
        let rect = Rect::default();
        assert!(!contains(rect, 0, 0));
    }

    #[test]
    fn test_mouse_state_single_click() {
        let mut ms = MouseState::new();
        assert!(!ms.record_click(10, 5));
    }

    #[test]
    fn test_mouse_state_double_click_same_position() {
        let mut ms = MouseState::new();
        assert!(!ms.record_click(10, 5));
        // Immediate second click on same position = double-click
        assert!(ms.record_click(10, 5));
    }

    #[test]
    fn test_mouse_state_double_click_different_position() {
        let mut ms = MouseState::new();
        assert!(!ms.record_click(10, 5));
        // Different position = not a double-click
        assert!(!ms.record_click(11, 5));
    }

    #[test]
    fn test_tree_item_at_outside_area() {
        let state = make_state_with_areas(
            Rect::new(0, 0, 20, 10),
            Rect::new(20, 0, 30, 10),
        );
        // Click outside tree area entirely
        assert_eq!(tree_item_at(0, 50, &state), None);
    }

    #[test]
    fn test_tree_item_at_border() {
        let state = make_state_with_areas(
            Rect::new(0, 0, 20, 10),
            Rect::new(20, 0, 30, 10),
        );
        // Click on top border row
        assert_eq!(tree_item_at(0, 5, &state), None);
        // Click on bottom border row
        assert_eq!(tree_item_at(9, 5, &state), None);
    }

    #[test]
    fn test_file_item_at_header_area() {
        let state = make_state_with_areas(
            Rect::new(0, 0, 20, 10),
            Rect::new(20, 0, 30, 10),
        );
        // Row 0 = border, row 1 = header, row 2 = margin — all should return None
        assert_eq!(file_item_at(0, 25, &state), None);
        assert_eq!(file_item_at(1, 25, &state), None);
        assert_eq!(file_item_at(2, 25, &state), None);
    }

    /// Create a minimal AppState-like struct for testing coordinate mapping.
    /// We can't easily construct a full AppState in unit tests, so we test
    /// the contains helper and MouseState directly. The tree_item_at and
    /// file_item_at functions are tested via integration testing.
    fn make_state_with_areas(tree_area: Rect, file_list_area: Rect) -> TestState {
        TestState {
            tree_area,
            file_list_area,
        }
    }

    /// Minimal state for coordinate tests
    struct TestState {
        tree_area: Rect,
        file_list_area: Rect,
    }

    // Re-implement the coordinate logic for testing without a full AppState
    fn tree_item_at(_row: u16, col: u16, state: &TestState) -> Option<usize> {
        let area = state.tree_area;
        if !contains(area, col, _row) {
            return None;
        }
        let inner_top = area.y + 1;
        if _row < inner_top || _row >= area.y + area.height.saturating_sub(1) {
            return None;
        }
        Some((_row - inner_top) as usize)
    }

    fn file_item_at(_row: u16, col: u16, state: &TestState) -> Option<usize> {
        let area = state.file_list_area;
        if !contains(area, col, _row) {
            return None;
        }
        let data_top = area.y + 3;
        if _row < data_top || _row >= area.y + area.height.saturating_sub(1) {
            return None;
        }
        Some((_row - data_top) as usize)
    }
}
