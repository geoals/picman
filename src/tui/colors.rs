//! Semantic color constants for consistent TUI styling.

use ratatui::prelude::*;

// Semantic colors
pub const RATING_COLOR: Color = Color::Yellow;
pub const TAG_COLOR: Color = Color::Blue;
pub const VIDEO_INDICATOR: Color = Color::Magenta;
pub const IMAGE_INDICATOR: Color = Color::Green;
pub const FOCUS_COLOR: Color = Color::Cyan;
pub const UNFOCUS_COLOR: Color = Color::DarkGray;
pub const HEADER_COLOR: Color = Color::White;
pub const HELP_TEXT: Color = Color::DarkGray;
pub const SUCCESS_COLOR: Color = Color::Green;
pub const WARNING_COLOR: Color = Color::Yellow;
pub const STATUS_BAR_BG: Color = Color::DarkGray;
pub const STATUS_BAR_FG: Color = Color::White;

/// Format a rating as filled stars only (e.g., "★★★").
pub fn format_rating(rating: Option<i32>) -> String {
    match rating {
        Some(r) => "★".repeat(r as usize),
        None => "unrated".to_string(),
    }
}

/// Format a rating compactly (e.g., "★4" or empty for unrated).
pub fn format_rating_compact(rating: Option<i32>) -> String {
    rating.map(|r| format!("★{}", r)).unwrap_or_default()
}

/// Create a styled span for a rating (yellow stars).
pub fn styled_rating(rating: Option<i32>) -> Span<'static> {
    let text = format_rating(rating);
    Span::styled(text, Style::default().fg(RATING_COLOR))
}

/// Create a styled span for a tag (blue with # prefix).
pub fn styled_tag(tag: &str) -> Span<'_> {
    Span::styled(format!("#{}", tag), Style::default().fg(TAG_COLOR))
}

/// Create styled spans for multiple tags.
pub fn styled_tags(tags: &[String]) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    for (i, tag) in tags.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(styled_tag(tag));
    }
    spans
}
