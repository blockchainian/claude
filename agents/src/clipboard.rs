use std::io;

/// Holds clipboard ownership for as long as the dashboard is running.
#[derive(Default)]
pub struct Clipboard {
    clipboard: Option<arboard::Clipboard>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn copy(&mut self, text: &str) -> io::Result<()> {
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new().map_err(io::Error::other)?);
        }
        self.clipboard
            .as_mut()
            .expect("clipboard was initialized")
            .set_text(text)
            .map_err(io::Error::other)?;
        Ok(())
    }
}
