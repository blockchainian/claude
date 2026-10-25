use std::{io, path::PathBuf};

/// Holds clipboard ownership for as long as the dashboard is running.
#[derive(Default)]
pub struct Clipboard {
    clipboard: Option<arboard::Clipboard>,
    directory: Option<PathBuf>,
}

impl Clipboard {
    pub fn new() -> Self {
        match std::env::var_os("AGENTS_CLIPBOARD_DIR") {
            Some(directory) => Self::from_directory(directory.into()),
            None => Self::default(),
        }
    }

    fn from_directory(directory: PathBuf) -> Self {
        Self {
            clipboard: None,
            directory: Some(directory),
        }
    }

    pub fn text(&mut self) -> io::Result<String> {
        if let Some(directory) = &self.directory {
            return std::fs::read_to_string(directory.join("text"));
        }
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new().map_err(io::Error::other)?);
        }
        self.clipboard
            .as_mut()
            .expect("clipboard was initialized")
            .get_text()
            .map_err(io::Error::other)
    }

    pub fn copy(&mut self, text: &str) -> io::Result<()> {
        if let Some(directory) = &self.directory {
            return std::fs::write(directory.join("text"), text);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_clipboard_reads_text_verbatim() {
        let directory = tempfile::tempdir().unwrap();
        let mut clipboard = Clipboard::from_directory(directory.path().to_owned());
        for text in ["first 🦀\n第二行\r\nthird", ""] {
            std::fs::write(directory.path().join("text"), text).unwrap();
            assert_eq!(clipboard.text().unwrap(), text);
        }
    }

    #[test]
    fn directory_clipboard_reports_missing_and_invalid_text() {
        let directory = tempfile::tempdir().unwrap();
        let mut clipboard = Clipboard::from_directory(directory.path().to_owned());
        assert_eq!(
            clipboard.text().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::write(directory.path().join("text"), [0xff]).unwrap();
        assert_eq!(
            clipboard.text().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn directory_clipboard_copies_text_synchronously_and_isolates_instances() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let mut first_clipboard = Clipboard::from_directory(first.path().to_owned());
        let mut second_clipboard = Clipboard::from_directory(second.path().to_owned());
        first_clipboard.copy("first 🦀").unwrap();
        second_clipboard.copy("second").unwrap();
        assert_eq!(
            std::fs::read_to_string(first.path().join("text")).unwrap(),
            "first 🦀"
        );
        assert_eq!(
            std::fs::read_to_string(second.path().join("text")).unwrap(),
            "second"
        );
        first_clipboard.copy("replacement").unwrap();
        assert_eq!(
            std::fs::read_to_string(first.path().join("text")).unwrap(),
            "replacement"
        );
    }

    #[test]
    fn directory_clipboard_reports_missing_text_directory() {
        let directory = tempfile::tempdir().unwrap();
        let mut clipboard = Clipboard::from_directory(directory.path().join("missing"));
        assert_eq!(
            clipboard.copy("text").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
