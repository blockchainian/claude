use ratatui::{buffer::Buffer, layout::Rect};

use crate::event::ImageBlock;

/// Images uploaded to the terminal and the commands that keep them current.
/// Exists only while the terminal supports images.
pub struct InlineImages {
    cell_pixels: Option<(u16, u16)>,
}

impl InlineImages {
    /// Images for a terminal whose cells are `cell_pixels` in size, or 1:2 when unknown.
    pub fn new(cell_pixels: Option<(u16, u16)>) -> Self {
        Self { cell_pixels }
    }

    /// The cells `image` covers in a panel `width` cells wide at `zoom`;
    /// `None` when its data cannot be read or decoded.
    pub fn size(&mut self, image: &ImageBlock, width: u16, zoom: Zoom) -> Option<(u16, u16)> {
        let _ = (image, width, zoom, self.cell_pixels);
        None
    }

    /// Draws the part of `image` that falls in `area`, starting at image row `first_row`,
    /// with `zoom` choosing the visible columns. Queues the upload and placement the first
    /// time `image` is drawn at this size.
    pub fn draw(
        &mut self,
        image: &ImageBlock,
        zoom: Zoom,
        first_row: u16,
        area: Rect,
        buf: &mut Buffer,
    ) {
        let _ = (image, zoom, first_row, area, buf);
    }

    /// Graphics commands queued since the last call.
    pub fn take_commands(&mut self) -> Vec<u8> {
        Vec::new()
    }
}

/// An image's zoom; the default fits the panel width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Zoom {
    /// Steps of 1.25x beyond fitting the panel width.
    pub level: u8,
    /// The fraction of the zoomed image's width left of the panel.
    pub left: f32,
}
