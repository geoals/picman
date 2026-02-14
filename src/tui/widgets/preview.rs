use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{picker::Picker, FilterType, Resize, StatefulImage};
use std::sync::{Mutex, OnceLock};

use crate::tui::state::{AppState, PreviewCache};

// Global picker (created once, thread-safe)
static PICKER: OnceLock<Mutex<Option<Picker>>> = OnceLock::new();

fn get_picker_mutex() -> &'static Mutex<Option<Picker>> {
    PICKER.get_or_init(|| {
        Mutex::new(Picker::from_termios().ok().map(|mut picker| {
            picker.guess_protocol();
            picker
        }))
    })
}

fn is_image_file(path: &std::path::Path) -> bool {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    matches!(
        extension.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" | "tif"
    )
}

pub fn render_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Preview ");

    // Get selected file path
    let file_path = match state.selected_file_path() {
        Some(path) => path,
        None => {
            let placeholder = Paragraph::new("Select a file to preview")
                .block(block)
                .alignment(Alignment::Center);
            frame.render_widget(placeholder, area);
            return;
        }
    };

    if !is_image_file(&file_path) {
        let info = format!("File: {}", file_path.display());
        let placeholder = Paragraph::new(info)
            .block(block)
            .alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    // Calculate inner area for image
    let inner = block.inner(area);
    frame.render_widget(block.clone(), area);

    // Check if we need to load a new image (cache miss)
    let mut cache = state.preview_cache.borrow_mut();
    let needs_load = match cache.as_ref() {
        Some(c) => c.path != file_path,
        None => true,
    };

    if needs_load {
        // Get mutable access to picker
        let mut picker_guard = match get_picker_mutex().lock() {
            Ok(g) => g,
            Err(_) => {
                let error = Paragraph::new("Preview unavailable");
                frame.render_widget(error, inner);
                return;
            }
        };

        let picker = match picker_guard.as_mut() {
            Some(p) => p,
            None => {
                let placeholder = Paragraph::new(
                    "Image preview not available\n(terminal doesn't support graphics)",
                );
                frame.render_widget(placeholder, inner);
                return;
            }
        };

        // Load the image
        let image = match image::open(&file_path) {
            Ok(img) => img,
            Err(_) => {
                *cache = None;
                let error = Paragraph::new("Failed to load image");
                frame.render_widget(error, inner);
                return;
            }
        };

        // Create protocol for the image
        let protocol = picker.new_resize_protocol(image);
        *cache = Some(PreviewCache::new(file_path, protocol));
    }

    // Render the cached image
    if let Some(ref mut preview) = *cache {
        let image_widget =
            StatefulImage::new(None).resize(Resize::Fit(Some(FilterType::Lanczos3)));
        frame.render_stateful_widget(image_widget, inner, &mut preview.protocol);
    }
}
