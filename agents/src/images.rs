//! Temporary PNG attachments shared by the composer and providers.

use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Keeps a PNG alive while a draft or pending send references it.
#[derive(Clone, Debug)]
pub(crate) struct ImageAttachment(Arc<ImageFile>);

#[derive(Debug)]
struct ImageFile {
    path: tempfile::TempPath,
    submitted: AtomicBool,
}

impl Drop for ImageFile {
    fn drop(&mut self) {
        self.path
            .disable_cleanup(self.submitted.load(Ordering::Relaxed));
    }
}

impl ImageAttachment {
    pub(crate) fn path(&self) -> &Path {
        self.0.path.as_ref()
    }

    /// Codex can read LocalImage after acknowledging the request, just as the CLI does.
    pub(crate) fn preserve(&self) {
        self.0.submitted.store(true, Ordering::Relaxed);
    }

    /// Restores a persisted attachment without copying or taking ownership of its file.
    pub(crate) fn from_path(path: &Path) -> io::Result<Self> {
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "attachment file is missing",
            ));
        }
        Ok(Self(Arc::new(ImageFile {
            path: tempfile::TempPath::try_from_path(path)?,
            submitted: AtomicBool::new(true),
        })))
    }

    pub(crate) fn from_image(image: image::DynamicImage) -> io::Result<Self> {
        let file = tempfile::Builder::new()
            .prefix("agents-image-")
            .suffix(".png")
            .tempfile()?;
        image
            .save_with_format(file.path(), image::ImageFormat::Png)
            .map_err(io::Error::other)?;
        Ok(Self(Arc::new(ImageFile {
            path: file.into_temp_path(),
            submitted: AtomicBool::new(false),
        })))
    }

    pub(crate) fn from_paste(text: &str) -> io::Result<Option<Self>> {
        let text = text.trim();
        let parts = shlex::split(text);
        let path = if text.starts_with("file://") {
            url::Url::parse(text)
                .ok()
                .and_then(|url| url.to_file_path().ok())
        } else if Path::new(text).is_file() {
            Some(PathBuf::from(text))
        } else {
            parts
                .filter(|parts| parts.len() == 1)
                .map(|parts| PathBuf::from(&parts[0]))
        };
        let Some(path) = path else {
            return Ok(None);
        };
        if image::ImageFormat::from_path(&path).is_err() {
            return Ok(None);
        }
        let image = image::open(&path).map_err(io::Error::other)?;
        Self::from_image(image).map(Some)
    }
}

#[cfg(test)]
pub(crate) fn fixture_png() -> Vec<u8> {
    let event: serde_json::Value = serde_json::from_str(
        include_str!("../tests/fixtures/claude-queued-image.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let data = event["attachment"]["prompt"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["type"] == "image")
        .unwrap()["source"]["data"]
        .as_str()
        .unwrap();
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submitted_images_outlive_the_dashboard_request() {
        let image =
            ImageAttachment::from_image(image::load_from_memory(&fixture_png()).unwrap()).unwrap();
        let path = image.path().to_owned();
        image.preserve();
        drop(image);
        assert!(
            path.is_file(),
            "Codex may read LocalImage after the RPC acknowledgement"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pasted_paths_accept_quotes_shell_escapes_and_file_urls() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("a picture.png");
        std::fs::write(&source, fixture_png()).unwrap();
        for pasted in [
            format!("'{}'", source.display()),
            source.to_string_lossy().replace(' ', "\\ "),
            format!("file://{}", source.to_string_lossy().replace(' ', "%20")),
        ] {
            let image = ImageAttachment::from_paste(&pasted)
                .unwrap()
                .expect("image path");
            assert_ne!(image.path(), source);
            assert_eq!(
                std::fs::read(image.path()).unwrap()[..8],
                [137, 80, 78, 71, 13, 10, 26, 10]
            );
            let path = image.path().to_owned();
            drop(image);
            assert!(!path.exists(), "unused image must be removed");
        }
        assert!(ImageAttachment::from_paste("plain text").unwrap().is_none());
    }
}
