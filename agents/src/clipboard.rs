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

    pub(crate) fn image(&mut self) -> io::Result<crate::images::ImageAttachment> {
        if let Some(directory) = &self.directory {
            let image = image::open(directory.join("image.png")).map_err(|error| match error {
                image::ImageError::IoError(error) => error,
                error => io::Error::other(error),
            })?;
            return crate::images::ImageAttachment::from_image(image);
        }
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new().map_err(io::Error::other)?);
        }
        let clipboard = self.clipboard.as_mut().expect("clipboard was initialized");
        // Finder exposes copied images as files; screenshots expose RGBA pixels.
        if let Ok(files) = clipboard.get().file_list() {
            for path in files {
                if let Some(image) =
                    crate::images::ImageAttachment::from_paste(&path.to_string_lossy())?
                {
                    return Ok(image);
                }
            }
        }
        let image = clipboard.get_image().map_err(io::Error::other)?;
        let width = u32::try_from(image.width).map_err(io::Error::other)?;
        let height = u32::try_from(image.height).map_err(io::Error::other)?;
        let rgba = image::RgbaImage::from_raw(width, height, image.bytes.into_owned())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid clipboard image"))?;
        crate::images::ImageAttachment::from_image(image::DynamicImage::ImageRgba8(rgba))
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
    fn directory_clipboard_reads_the_real_image_fixture() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("image.png");
        std::fs::write(&source, crate::images::fixture_png()).unwrap();
        let mut clipboard = Clipboard::from_directory(directory.path().to_owned());
        let attachment = clipboard.image().unwrap();
        assert_ne!(attachment.path(), source);
        assert_eq!(
            image::open(attachment.path()).unwrap(),
            image::open(source).unwrap()
        );
    }

    #[test]
    fn directory_clipboard_reports_missing_or_invalid_data() {
        let directory = tempfile::tempdir().unwrap();
        let mut clipboard = Clipboard::from_directory(directory.path().to_owned());
        assert_eq!(
            clipboard.image().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::write(directory.path().join("image.png"), b"invalid image").unwrap();
        assert!(clipboard.image().is_err());
        let mut clipboard = Clipboard::from_directory(directory.path().join("missing"));
        assert_eq!(
            clipboard.copy("text").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
