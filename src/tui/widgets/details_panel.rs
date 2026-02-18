use std::fs;

use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::colors::{
    format_rating, HEADER_COLOR, HELP_TEXT, RATING_COLOR, SUCCESS_COLOR, TAG_COLOR, WARNING_COLOR,
};
use crate::tui::state::{AppState, Focus};

use crate::thumbnails::{has_dir_preview, has_thumbnail, is_image_file, is_video_file};

pub fn render_details_panel(frame: &mut Frame, area: Rect, state: &AppState) {
    let content = match (&state.focus, state.details_expanded) {
        (Focus::FileList, true) => render_file_details_expanded(state),
        (Focus::FileList, false) => render_file_details(state),
        (Focus::DirectoryTree, _) => render_directory_details(state),
    };

    let title = Line::from(vec![
        Span::raw(" Details "),
        Span::styled("[i]", Style::default().fg(HELP_TEXT)),
        Span::raw(" "),
    ]);

    let panel = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(title));

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

    // Format size with optional dimensions
    let size = format_size(file.size);
    let size_dims = match (file.width, file.height) {
        (Some(w), Some(h)) => format!("{}  {}×{}", size, w, h),
        _ => size,
    };

    // Get filesystem metadata for timestamps
    let (modified, created) = if let Some(fs_path) = state.selected_file_path() {
        get_file_timestamps(&fs_path)
    } else {
        ("N/A".to_string(), "N/A".to_string())
    };

    // Line 1: path
    let line1 = Line::from(full_path);

    // Line 2: size + dimensions
    let line2 = Line::from(size_dims);

    // Line 3: rating
    let line3 = Line::from(vec![
        Span::raw("Rating: "),
        Span::styled(format_rating(file.rating), Style::default().fg(RATING_COLOR)),
    ]);

    // Line 4: timestamps
    let line4 = Line::from(format!("Modified: {}  Created: {}", modified, created));

    // Line 5: tags with colors
    let mut line5_spans: Vec<Span> = vec![Span::raw("Tags: ")];
    if file_with_tags.tags.is_empty() {
        line5_spans.push(Span::styled("none", Style::default().fg(HELP_TEXT)));
    } else {
        for (i, tag) in file_with_tags.tags.iter().enumerate() {
            if i > 0 {
                line5_spans.push(Span::raw(" "));
            }
            line5_spans.push(Span::styled(
                format!("#{}", tag),
                Style::default().fg(TAG_COLOR),
            ));
        }
    }
    let line5 = Line::from(line5_spans);

    Text::from(vec![line1, line2, line3, line4, line5])
}

fn render_file_details_expanded(state: &AppState) -> Text<'static> {
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

    let (modified, created) = if let Some(fs_path) = state.selected_file_path() {
        get_file_timestamps(&fs_path)
    } else {
        ("N/A".to_string(), "N/A".to_string())
    };

    let section = Style::default()
        .fg(HEADER_COLOR)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();

    // === File info section ===
    lines.push(Line::from(Span::styled("File", section)));
    lines.push(Line::from(format!("  {}", full_path)));

    // Dimensions
    if let (Some(w), Some(h)) = (file.width, file.height) {
        lines.push(Line::from(format!("  {}×{}", w, h)));
    }

    // Size (formatted + exact bytes)
    let size_str = format_size(file.size);
    if file.size >= 1024 {
        lines.push(Line::from(format!("  {} ({} bytes)", size_str, file.size)));
    } else {
        lines.push(Line::from(format!("  {}", size_str)));
    }

    // Rating
    lines.push(Line::from(vec![
        Span::raw("  Rating: "),
        Span::styled(format_rating(file.rating), Style::default().fg(RATING_COLOR)),
    ]));

    // Timestamps
    lines.push(Line::from(vec![
        Span::raw("  Modified: "),
        Span::styled(modified, Style::default().fg(WARNING_COLOR)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  Created:  "),
        Span::styled(created, Style::default().fg(SUCCESS_COLOR)),
    ]));

    // Hash
    if let Some(ref hash) = file.hash {
        lines.push(Line::from(vec![
            Span::raw("  Hash: "),
            Span::styled(hash.clone(), Style::default().fg(HELP_TEXT)),
        ]));
    }

    // Thumbnail status
    let has_thumb = state
        .selected_file_path()
        .map(|p| has_thumbnail(&p))
        .unwrap_or(false);
    lines.push(Line::from(vec![
        Span::raw("  Thumbnail: "),
        if has_thumb {
            Span::styled("✓", Style::default().fg(SUCCESS_COLOR))
        } else {
            Span::styled("✗", Style::default().fg(HELP_TEXT))
        },
    ]));

    // Tags
    let mut tag_spans: Vec<Span> = vec![Span::raw("  Tags: ")];
    if file_with_tags.tags.is_empty() {
        tag_spans.push(Span::styled("none", Style::default().fg(HELP_TEXT)));
    } else {
        for (i, tag) in file_with_tags.tags.iter().enumerate() {
            if i > 0 {
                tag_spans.push(Span::raw(" "));
            }
            tag_spans.push(Span::styled(
                format!("#{}", tag),
                Style::default().fg(TAG_COLOR),
            ));
        }
    }
    lines.push(Line::from(tag_spans));

    // === EXIF section ===
    if let Some((ref cached_path, ref exif)) = state.cached_exif {
        let current_path = state.selected_file_path();
        let is_current = current_path
            .as_ref()
            .map(|p| p == cached_path)
            .unwrap_or(false);

        if is_current && exif.has_any() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("EXIF", section)));

            if let Some(ref make) = exif.camera_make {
                if let Some(ref model) = exif.camera_model {
                    lines.push(Line::from(format!("  {} {}", make, model)));
                } else {
                    lines.push(Line::from(format!("  {}", make)));
                }
            } else if let Some(ref model) = exif.camera_model {
                lines.push(Line::from(format!("  {}", model)));
            }

            if let Some(ref lens) = exif.lens {
                lines.push(Line::from(format!("  Lens: {}", lens)));
            }

            // Exposure line: aperture, shutter, ISO, focal length
            let exposure_parts: Vec<&str> = [
                exif.aperture.as_deref(),
                exif.shutter_speed.as_deref(),
                exif.iso.as_deref(),
                exif.focal_length.as_deref(),
            ]
            .iter()
            .filter_map(|s| *s)
            .collect();

            if !exposure_parts.is_empty() {
                lines.push(Line::from(format!("  {}", exposure_parts.join("  "))));
            }

            if let (Some(lat), Some(lon)) = (exif.gps_lat, exif.gps_lon) {
                lines.push(Line::from(format!("  GPS: {:.6}, {:.6}", lat, lon)));
            }
        }
    }

    Text::from(lines)
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
        line4_spans.push(Span::styled("none", Style::default().fg(HELP_TEXT)));
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

    // Check for missing preview (either dir preview or file thumbnails)
    let missing_dir_preview = !has_dir_preview(dir.id);
    let missing_file_thumbnail = check_dir_missing_thumbnails(state, dir);

    if missing_dir_preview || missing_file_thumbnail {
        lines.push(Line::from(Span::styled(
            "󰋩 Missing preview",
            Style::default().fg(Color::Red),
        )));
    }

    Text::from(lines)
}

/// Check if a directory has any media files missing thumbnails (cached)
fn check_dir_missing_thumbnails(state: &AppState, dir: &crate::db::Directory) -> bool {
    // Check cache first
    if let Some((cached_id, cached_result)) = *state.missing_preview_cache.borrow() {
        if cached_id == dir.id {
            return cached_result;
        }
    }

    // Compute and cache
    let result = compute_missing_thumbnails(state, dir);
    *state.missing_preview_cache.borrow_mut() = Some((dir.id, result));
    result
}

/// Actually check if directory has missing thumbnails
fn compute_missing_thumbnails(state: &AppState, dir: &crate::db::Directory) -> bool {
    let files = match state.db.get_files_in_directory(dir.id) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let dir_path = dir.full_path(&state.library_path);

    // Check first media file only (quick check)
    for file in &files {
        let path = dir_path.join(&file.filename);
        if is_image_file(&path) || is_video_file(&path) {
            return !has_thumbnail(&path);
        }
    }

    false
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
