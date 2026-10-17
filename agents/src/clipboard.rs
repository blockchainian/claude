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

    pub(crate) fn image(&mut self) -> io::Result<crate::images::ImageAttachment> {
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
