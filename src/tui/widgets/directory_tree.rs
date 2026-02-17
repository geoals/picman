use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, List, ListItem},
};

use crate::tui::colors::{FOCUS_COLOR, HELP_TEXT, UNFOCUS_COLOR};
use crate::tui::state::{AppState, Focus};

use crate::db::Directory;
use crate::tui::state::TreeState;

/// Compute tree-drawing prefix strings for each visible directory.
///
/// Uses box-drawing characters (`├─`, `└─`, `│`) to show parent-child
/// relationships. Each depth level contributes a 3-character segment.
///
/// This is a pure function: it only reads directory metadata (depth, parent_id)
/// and produces strings, making it easy to test without a full AppState.
fn compute_tree_prefixes(visible_dirs: &[&Directory], tree: &TreeState) -> Vec<String> {
    use std::collections::HashSet;

    let n = visible_dirs.len();
    if n == 0 {
        return vec![];
    }

    // Step 1: Pre-compute whether each entry is the last visible child of its parent.
    // Reverse pass: the first time we see a parent_id, that entry is the last child.
    let mut seen_parents: HashSet<Option<i64>> = HashSet::new();
    let mut is_last = vec![false; n];
    for i in (0..n).rev() {
        let parent = visible_dirs[i].parent_id;
        if seen_parents.insert(parent) {
            is_last[i] = true;
        }
    }

    // Step 2: Forward pass — build prefix strings.
    // Track is_last status at each depth level so deeper entries know whether
    // their ancestors need continuation lines (│) or empty space.
    let mut is_last_at_depth: Vec<bool> = Vec::new();
    let mut prefixes = Vec::with_capacity(n);

    for i in 0..n {
        let depth = tree.depth(visible_dirs[i]);
        let mut prefix = String::new();

        // Ancestor continuation lines (depths 0..depth)
        for level in 0..depth {
            if level < is_last_at_depth.len() && is_last_at_depth[level] {
                prefix.push_str("   ");
            } else {
                prefix.push_str("│  ");
            }
        }

        // Own connector
        if is_last[i] {
            prefix.push_str("└─ ");
        } else {
            prefix.push_str("├─ ");
        }

        // Update tracking for this depth
        if depth >= is_last_at_depth.len() {
            is_last_at_depth.resize(depth + 1, false);
        }
        is_last_at_depth[depth] = is_last[i];

        prefixes.push(prefix);
    }

    prefixes
}

pub fn render_directory_tree(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let is_focused = state.focus == Focus::DirectoryTree;

    let visible_dirs = state.get_visible_directories();
    let tree_prefixes = compute_tree_prefixes(&visible_dirs, &state.tree);

    let items: Vec<ListItem> = visible_dirs
        .iter()
        .enumerate()
        .map(|(i, dir)| {
            let icon = if state.tree.has_visible_children(dir.id, &state.matching_dir_ids) {
                if state.tree.expanded.contains(&dir.id) {
                    "- "
                } else {
                    "+ "
                }
            } else {
                "  "
            };

            // Get directory name (last component of path)
            let name = dir.path.rsplit('/').next().unwrap_or(&dir.path);

            let display_name = if name.is_empty() { "." } else { name };

            // Build line with styled spans
            let mut spans = Vec::new();

            // Tree prefix and expand/collapse icon (muted color)
            spans.push(Span::styled(
                format!("{}{}", tree_prefixes[i], icon),
                Style::default().fg(HELP_TEXT),
            ));

            spans.push(Span::raw(display_name.to_string()));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let border_style = if is_focused {
        Style::default().fg(FOCUS_COLOR)
    } else {
        Style::default().fg(UNFOCUS_COLOR)
    };

    let highlight_style = if is_focused {
        Style::default().bg(FOCUS_COLOR).fg(Color::Black)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(id: i64, path: &str, parent_id: Option<i64>) -> Directory {
        Directory {
            id,
            path: path.to_string(),
            parent_id,
            rating: None,
            mtime: Some(0),
        }
    }

    /// Two root-level siblings: both should get connectors, last gets └─
    #[test]
    fn test_flat_siblings() {
        let dirs = vec![
            dir(1, "photos", None),
            dir(2, "videos", None),
        ];
        let tree = TreeState::new(dirs.clone());
        let visible: Vec<&Directory> = dirs.iter().collect();

        let prefixes = compute_tree_prefixes(&visible, &tree);

        assert_eq!(prefixes[0], "├─ ");
        assert_eq!(prefixes[1], "└─ ");
    }

    /// Single root: should be └─ (last and only child)
    #[test]
    fn test_single_root() {
        let dirs = vec![dir(1, "photos", None)];
        let tree = TreeState::new(dirs.clone());
        let visible: Vec<&Directory> = dirs.iter().collect();

        let prefixes = compute_tree_prefixes(&visible, &tree);

        assert_eq!(prefixes[0], "└─ ");
    }

    /// Nested: parent expanded with one child visible
    ///
    /// ```text
    /// └─ photos       (only root, so last)
    ///    └─ vacation   (only child, so last; ancestor was last → space)
    /// ```
    #[test]
    fn test_nested_single_child() {
        let dirs = vec![
            dir(1, "photos", None),
            dir(2, "photos/vacation", Some(1)),
        ];
        let mut tree = TreeState::new(dirs.clone());
        tree.expanded.insert(1);
        let visible = tree.visible_directories();

        let prefixes = compute_tree_prefixes(&visible, &tree);

        assert_eq!(prefixes[0], "└─ ");
        assert_eq!(prefixes[1], "   └─ ");
    }

    /// Two roots, first expanded with two children:
    ///
    /// ```text
    /// ├─ photos
    /// │  ├─ vacation
    /// │  └─ archive
    /// └─ videos
    /// ```
    #[test]
    fn test_expanded_with_siblings_below() {
        let dirs = vec![
            dir(1, "photos", None),
            dir(2, "photos/vacation", Some(1)),
            dir(3, "photos/archive", Some(1)),
            dir(4, "videos", None),
        ];
        let mut tree = TreeState::new(dirs.clone());
        tree.expanded.insert(1);
        let visible = tree.visible_directories();

        let prefixes = compute_tree_prefixes(&visible, &tree);

        assert_eq!(prefixes[0], "├─ ");       // photos (has sibling videos)
        assert_eq!(prefixes[1], "│  ├─ ");    // vacation (has sibling archive)
        assert_eq!(prefixes[2], "│  └─ ");    // archive (last child of photos)
        assert_eq!(prefixes[3], "└─ ");       // videos (last root)
    }

    /// Deep nesting (3 levels):
    ///
    /// ```text
    /// ├─ photos
    /// │  └─ vacation
    /// │     └─ beach
    /// └─ videos
    /// ```
    #[test]
    fn test_deep_nesting() {
        let dirs = vec![
            dir(1, "photos", None),
            dir(2, "photos/vacation", Some(1)),
            dir(3, "photos/vacation/beach", Some(2)),
            dir(4, "videos", None),
        ];
        let mut tree = TreeState::new(dirs.clone());
        tree.expanded.insert(1);
        tree.expanded.insert(2);
        let visible = tree.visible_directories();

        let prefixes = compute_tree_prefixes(&visible, &tree);

        assert_eq!(prefixes[0], "├─ ");          // photos
        assert_eq!(prefixes[1], "│  └─ ");       // vacation (only child)
        assert_eq!(prefixes[2], "│     └─ ");    // beach (only child; grandparent not last → │, parent last → space)
        assert_eq!(prefixes[3], "└─ ");          // videos
    }

    /// Collapsed directory should not show children, but tree connectors
    /// should still be correct for the visible entries.
    #[test]
    fn test_collapsed_hides_children() {
        let dirs = vec![
            dir(1, "photos", None),
            dir(2, "photos/vacation", Some(1)),
            dir(3, "videos", None),
        ];
        // photos is NOT expanded
        let tree = TreeState::new(dirs.clone());
        let visible = tree.visible_directories();

        // Only roots visible
        assert_eq!(visible.len(), 2);

        let prefixes = compute_tree_prefixes(&visible, &tree);

        assert_eq!(prefixes[0], "├─ ");
        assert_eq!(prefixes[1], "└─ ");
    }

    /// Empty directory list produces empty prefixes
    #[test]
    fn test_empty() {
        let dirs: Vec<Directory> = vec![];
        let tree = TreeState::new(dirs);
        let visible: Vec<&Directory> = vec![];

        let prefixes = compute_tree_prefixes(&visible, &tree);

        assert!(prefixes.is_empty());
    }
}
