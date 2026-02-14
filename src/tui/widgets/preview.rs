use ratatui::{
    layout::Rect,
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{picker::Picker, StatefulImage};
use std::cell::RefCell;

use crate::tui::state::AppState;

// Thread-local picker for image rendering
thread_local! {
    static PICKER: RefCell<Option<Picker>> = RefCell::new(Picker::from_termios().ok());
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

    // Check if it's an image
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let is_image = matches!(
        extension.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" | "tif"
    );

    if !is_image {
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

    // Try to render image with picker
    PICKER.with_borrow_mut(|picker_opt| {
        let picker = match picker_opt.as_mut() {
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
                let error = Paragraph::new("Failed to load image");
                frame.render_widget(error, inner);
                return;
            }
        };

        // Create protocol for the image
        let mut protocol = picker.new_resize_protocol(image);

        // Render the image
        let image_widget = StatefulImage::new(None);
        frame.render_stateful_widget(image_widget, inner, &mut protocol);
    });
}
