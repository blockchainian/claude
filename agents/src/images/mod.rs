mod attachment;

pub(crate) use attachment::ImageAttachment;
#[cfg(test)]
pub(crate) use attachment::fixture_png;

use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
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
                let encoded = if encoded.starts_with("data:") {
                    encoded.split_once(',')?.1
                } else {
                    encoded
                };
                STANDARD.decode(encoded).ok()
            }
            Self::File { path, len, .. } => {
                const LIMIT: u64 = 64 << 20;
                if *len > LIMIT {
                    return None;
                }
                let mut bytes = Vec::new();
                File::open(path)
                    .ok()?
                    .take(LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .ok()?;
                (bytes.len() as u64 <= LIMIT).then_some(bytes)
            }
        }
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
    fn record_images_decode_using_their_transcript_position() {
        let source = record_source();
        let bytes = source.bytes().unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn record_images_read_codex_data_urls() {
        let source = ImageSource::Record {
            record: SourceRecord {
                path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/codex-images/wrapped.jsonl"),
                offset: 0,
            },
            pointer: "/payload/content/1/image_url".into(),
        };
        assert_eq!(source.bytes().unwrap(), b"\x89PNG");
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
