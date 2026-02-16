use std::fs;

use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::colors::{
    format_rating, IMAGE_INDICATOR, RATING_COLOR, TAG_COLOR, VIDEO_INDICATOR,
};
use crate::tui::state::{AppState, Focus};

use super::has_dir_preview;

pub fn render_details_panel(frame: &mut Frame, area: Rect, state: &AppState) {
    let content = match state.focus {
        Focus::FileList => render_file_details(state),
        Focus::DirectoryTree => render_directory_details(state),
    };

    let panel = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(" Details "));

    frame.render_widget(panel, area);
}

fn render_file_details(state: &AppState) -> Text<'static> {
    let Some(file_with_tags) = state.file_list.selected_file() else {
        return Text::raw("No file selected");
    };

    let file = &file_with_tags.file;
    let dir = state.get_selected_directory();

    // Build full path
    let full_path = match dir {
        Some(d) if !d.path.is_empty() => format!("{}/{}", d.path, file.filename),
        _ => file.filename.clone(),
    };

    // Format size
    let size = format_size(file.size);

    // Format media type with color indicator
    let media_type_str = file.media_type.as_deref().unwrap_or("unknown");
    let is_video = media_type_str.starts_with("video/");
    let media_type_color = if is_video {
        VIDEO_INDICATOR
    } else {
        IMAGE_INDICATOR
    };

    // Get filesystem metadata for timestamps
    let (modified, created) = if let Some(fs_path) = state.selected_file_path() {
        get_file_timestamps(&fs_path)
    } else {
        ("N/A".to_string(), "N/A".to_string())
    };

    // Line 1: path
    let line1 = Line::from(full_path);

    // Line 2: size | type | rating
    let line2 = Line::from(vec![
        Span::raw(format!("{} | ", size)),
        Span::styled(media_type_str.to_string(), Style::default().fg(media_type_color)),
        Span::raw(" | Rating: "),
        Span::styled(format_rating(file.rating), Style::default().fg(RATING_COLOR)),
    ]);

    // Line 3: timestamps
    let line3 = Line::from(format!("Modified: {}  Created: {}", modified, created));

    // Line 4: tags with colors
    let mut line4_spans: Vec<Span> = vec![Span::raw("Tags: ")];
    if file_with_tags.tags.is_empty() {
        line4_spans.push(Span::styled("none", Style::default().fg(Color::DarkGray)));
    } else {
        for (i, tag) in file_with_tags.tags.iter().enumerate() {
            if i > 0 {
                line4_spans.push(Span::raw(" "));
            }
            line4_spans.push(Span::styled(
                format!("#{}", tag),
                Style::default().fg(TAG_COLOR),
            ));
        }
    }
    let line4 = Line::from(line4_spans);

    Text::from(vec![line1, line2, line3, line4])
}

fn render_directory_details(state: &AppState) -> Text<'static> {
    let Some(dir) = state.get_selected_directory() else {
        return Text::raw("No directory selected");
    };

    // Format path
    let path = if dir.path.is_empty() {
        "(root)".to_string()
    } else {
        dir.path.clone()
    };

    // Count subdirs from in-memory tree (recursive)
    let subdir_count = count_subdirs_recursive(&state.tree.directories, dir.id);

    // Get file count and size from DB (recursive)
    let (file_count, total_size) = state.db.get_directory_stats(dir.id).unwrap_or((0, 0));

    // Get directory tags (query on demand)
    let tags = match state.db.get_directory_tags(dir.id) {
        Ok(t) => t,
        Err(_) => vec![],
    };

    // Line 1: path
    let line1 = Line::from(path);

    // Line 2: rating
    let line2 = Line::from(vec![
        Span::raw("Rating: "),
        Span::styled(format_rating(dir.rating), Style::default().fg(RATING_COLOR)),
    ]);

    // Line 3: stats
    let line3 = Line::from(format!(
        "{} subdirs | {} files | {}",
        subdir_count,
        file_count,
        format_size(total_size)
    ));

    // Line 4: tags with colors
    let mut line4_spans: Vec<Span> = vec![Span::raw("Tags: ")];
    if tags.is_empty() {
        line4_spans.push(Span::styled("none", Style::default().fg(Color::DarkGray)));
    } else {
        for (i, tag) in tags.iter().enumerate() {
            if i > 0 {
                line4_spans.push(Span::raw(" "));
            }
            line4_spans.push(Span::styled(
                format!("#{}", tag),
                Style::default().fg(TAG_COLOR),
            ));
        }
    }
    let line4 = Line::from(line4_spans);

    let mut lines = vec![line1, line2, line3, line4];

    // Add warning for missing preview
    if !has_dir_preview(dir.id) {
        lines.push(Line::from(Span::styled(
            "󰋩 Missing preview image",
            Style::default().fg(Color::Red),
        )));
    }

    // Add warning for missing file thumbnails
    if state.dir_missing_file_thumbnails(dir.id) {
        lines.push(Line::from(Span::styled(
            "󰐊 Missing file thumbnails",
            Style::default().fg(Color::Yellow),
        )));
    }

    Text::from(lines)
}

/// Count all descendant subdirectories recursively
fn count_subdirs_recursive(directories: &[crate::db::Directory], parent_id: i64) -> usize {
    let mut count = 0;
    for dir in directories {
        if dir.parent_id == Some(parent_id) {
            count += 1;
            count += count_subdirs_recursive(directories, dir.id);
        }
    }
    count
}

fn format_size(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn get_file_timestamps(path: &std::path::Path) -> (String, String) {
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return ("N/A".to_string(), "N/A".to_string()),
    };

    let modified = metadata
        .modified()
        .ok()
        .map(format_system_time)
        .unwrap_or_else(|| "N/A".to_string());

    let created = metadata
        .created()
        .ok()
        .map(format_system_time)
        .unwrap_or_else(|| "N/A".to_string());

    (modified, created)
}

fn format_system_time(time: std::time::SystemTime) -> String {
    let duration = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs() as i64;

    // Convert to local time components (simplified, assumes UTC offset is handled by libc)
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;

    // Calculate year, month, day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days_since_epoch);

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        year, month, day, hours, minutes
    )
}

fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}
