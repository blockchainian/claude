mod attachment;

pub(crate) use attachment::ImageAttachment;
#[cfg(test)]
pub(crate) use attachment::fixture_png;

use std::{
    fs::File,
    io::{self, BufRead, BufReader, Seek, SeekFrom, Write},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    time::SystemTime,
};

use base64::{Engine, engine::general_purpose::STANDARD};

use crate::event::SourceRecord;

/// An image a provider found: the text standing for it and where its bytes are.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Image {
    pub label: String,
    pub source: ImageSource,
}

/// Where an image's bytes come from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ImageSource {
    /// Base64 (bare or a data URL) at a JSON pointer in one transcript line.
    Record {
        record: SourceRecord,
        pointer: String,
    },
    /// A file on disk, keyed by the size and modification time seen when it was found.
    File {
        path: PathBuf,
        len: u64,
        modified: SystemTime,
    },
}

impl ImageSource {
    /// Reads the image bytes on demand.
    pub(crate) fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            Self::Record { record, pointer } => {
                let mut file = File::open(&record.path).ok()?;
                file.seek(SeekFrom::Start(record.offset)).ok()?;
                let mut line = Vec::new();
                BufReader::new(file).read_until(b'\n', &mut line).ok()?;
                let value: serde_json::Value = serde_json::from_slice(&line).ok()?;
                let encoded = value.pointer(pointer)?.as_str()?;
                let encoded = if let Some(data) = encoded.strip_prefix("data:") {
                    let (metadata, data) = data.split_once(',')?;
                    if !metadata.ends_with(";base64") {
                        return None;
                    }
                    data
                } else {
                    encoded
                };
                STANDARD.decode(encoded).ok()
            }
            Self::File { path, len, .. } => {
                if *len > (64 << 20) || std::fs::metadata(path).ok()?.len() > (64 << 20) {
                    return None;
                }
                std::fs::read(path).ok()
            }
        }
    }

    /// Returns a path `open` can take, materializing transcript data only when needed.
    pub(crate) fn local_file(&self) -> io::Result<PathBuf> {
        let bytes = match self {
            Self::File { path, .. } => return Ok(path.clone()),
            Self::Record { .. } => {
                let invalid = || {
                    io::Error::new(io::ErrorKind::InvalidData, "Invalid transcript image")
                };
                self.bytes().ok_or_else(invalid)?
            }
        };
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid transcript image");
        let format = image::guess_format(&bytes).map_err(|_| invalid())?;
        let extension = format.extensions_str().first().ok_or_else(invalid)?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        bytes.hash(&mut hash);
        let directory = std::env::temp_dir().join(format!("agents-images-{}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(&directory)?;
        let path = directory.join(format!("{:016x}.{extension}", hash.finish()));
        if !path.exists() {
            let mut file = tempfile::NamedTempFile::new_in(&directory)?;
            file.write_all(&bytes)?;
            match file.persist_noclobber(&path) {
                Ok(_) => {}
                Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.error),
            }
        }
        if std::fs::read(&path)? != bytes {
            return Err(invalid());
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_source() -> ImageSource {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-read-image.jsonl");
        let bytes = std::fs::read(&path).unwrap();
        ImageSource::Record {
            record: SourceRecord {
                path,
                offset: (bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1) as u64,
            },
            pointer: "/message/content/0/content/0/source/data".into(),
        }
    }

    #[test]
    fn record_images_decode_and_materialize_using_their_file_format() {
        let source = record_source();
        let bytes = source.bytes().unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let path = source.local_file().unwrap();
        assert_eq!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("png")
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn file_sources_read_real_image_bytes_and_reject_oversized_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        let bytes = fixture_png();
        std::fs::write(&path, &bytes).unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        let source = ImageSource::File {
            path: path.clone(),
            len: metadata.len(),
            modified: metadata.modified().unwrap(),
        };
        assert_eq!(source.bytes().unwrap(), bytes);

        let oversized = directory.path().join("oversized.png");
        File::create(&oversized)
            .unwrap()
            .set_len((64 << 20) + 1)
            .unwrap();
        let metadata = std::fs::metadata(&oversized).unwrap();
        let source = ImageSource::File {
            path: oversized,
            len: metadata.len(),
            modified: metadata.modified().unwrap(),
        };
        assert!(source.bytes().is_none());
    }
}
