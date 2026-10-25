mod attachment;

pub(crate) use attachment::ImageAttachment;
#[cfg(test)]
pub(crate) use attachment::fixture_png;

use std::{
    collections::HashMap,
    fs::File,
    hash::{Hash, Hasher},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{LazyLock, Mutex},
    time::SystemTime,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};

use crate::event::SourceRecord;

const IMAGE_BYTES: usize = 64 << 20;
const RECORD_BYTES: usize = 128 << 20;
const IMAGE_STRING_BYTES: usize = IMAGE_BYTES.div_ceil(3) * 4 + 1024;

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

impl Image {
    /// An image file on disk, labelled by its path; `None` when no file is there.
    pub(crate) fn file(path: PathBuf) -> Option<Self> {
        let metadata = std::fs::metadata(&path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            label: path.to_string_lossy().into_owned(),
            source: ImageSource::File {
                path,
                len: metadata.len(),
                modified: metadata.modified().ok()?,
            },
        })
    }
}

impl ImageSource {
    /// Returns the file itself, or persists decoded record bytes for opening.
    pub(crate) fn local_file(&self) -> io::Result<PathBuf> {
        if let Self::File { path, .. } = self {
            return Ok(path.clone());
        }
        static PATHS: LazyLock<Mutex<HashMap<ImageSource, PathBuf>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));
        let mut paths = PATHS
            .lock()
            .map_err(|_| io::Error::other("Image path cache lock poisoned"))?;
        if let Some(path) = paths.get(self) {
            if path.try_exists()? {
                return Ok(path.clone());
            }
            paths.remove(self);
        }
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid image data");
        let bytes = self.bytes().ok_or_else(invalid)?;
        let format = image::guess_format(&bytes).map_err(|_| invalid())?;
        let extension = format.extensions_str().first().ok_or_else(invalid)?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
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
        paths.insert(self.clone(), path.clone());
        Ok(path)
    }

    /// Reads the image bytes on demand.
    pub(crate) fn bytes(&self) -> Option<Vec<u8>> {
        self.bytes_with_limits(RECORD_BYTES, IMAGE_STRING_BYTES, IMAGE_BYTES)
    }

    fn bytes_with_limits(
        &self,
        record_limit: usize,
        string_limit: usize,
        decoded_limit: usize,
    ) -> Option<Vec<u8>> {
        match self {
            Self::Record { record, pointer } => {
                let mut file = File::open(&record.path).ok()?;
                file.seek(SeekFrom::Start(record.offset)).ok()?;
                let mut line = Vec::new();
                BufReader::new(file)
                    .take(record_limit as u64 + 1)
                    .read_until(b'\n', &mut line)
                    .ok()?;
                if line.len() > record_limit || (!pointer.is_empty() && !pointer.starts_with('/')) {
                    return None;
                }
                let tokens: Vec<_> = pointer
                    .strip_prefix('/')
                    .into_iter()
                    .flat_map(|pointer| pointer.split('/'))
                    .map(|token| token.replace("~1", "/").replace("~0", "~"))
                    .collect();
                let mut json = serde_json::Deserializer::from_slice(&line);
                let bytes = ImageBytes {
                    tokens: &tokens,
                    string_limit,
                    decoded_limit,
                }
                .deserialize(&mut json)
                .ok()??;
                json.end().ok()?;
                Some(bytes)
            }
            Self::File { path, len, .. } => {
                let limit = decoded_limit as u64;
                if *len > limit {
                    return None;
                }
                let mut bytes = Vec::new();
                File::open(path)
                    .ok()?
                    .take(limit + 1)
                    .read_to_end(&mut bytes)
                    .ok()?;
                (bytes.len() as u64 <= limit).then_some(bytes)
            }
        }
    }
}

/// Visits only the pointed-to string; unrelated JSON values are skipped without building a tree.
struct ImageBytes<'a> {
    tokens: &'a [String],
    string_limit: usize,
    decoded_limit: usize,
}

impl<'de> DeserializeSeed<'de> for ImageBytes<'_> {
    type Value = Option<Vec<u8>>;

    fn deserialize<D: serde::Deserializer<'de>>(self, json: D) -> Result<Self::Value, D::Error> {
        if self.tokens.is_empty() {
            json.deserialize_str(self)
        } else {
            json.deserialize_any(self)
        }
    }
}

impl<'de> Visitor<'de> for ImageBytes<'_> {
    type Value = Option<Vec<u8>>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an image string or its JSON pointer parent")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let Some(token) = self.tokens.first() else {
            return Err(serde::de::Error::custom("image must be a string"));
        };
        let mut result = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == *token {
                result = map.next_value_seed(ImageBytes {
                    tokens: &self.tokens[1..],
                    ..self
                })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(result)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Self::Value, S::Error> {
        let index = self
            .tokens
            .first()
            .and_then(|token| {
                if token.starts_with('+') || (token.len() > 1 && token.starts_with('0')) {
                    None
                } else {
                    token.parse::<usize>().ok()
                }
            })
            .ok_or_else(|| serde::de::Error::custom("invalid image array index"))?;
        let mut result = None;
        let mut position = 0;
        loop {
            if position == index {
                let Some(bytes) = sequence.next_element_seed(ImageBytes {
                    tokens: &self.tokens[1..],
                    ..self
                })?
                else {
                    break;
                };
                result = bytes;
            } else if sequence.next_element::<IgnoredAny>()?.is_none() {
                break;
            }
            position += 1;
        }
        Ok(result)
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        if !self.tokens.is_empty() || value.len() > self.string_limit {
            return Err(E::custom("image string exceeds limit"));
        }
        let encoded = if value.starts_with("data:") {
            value
                .split_once(',')
                .ok_or_else(|| E::custom("invalid image data URL"))?
                .1
        } else {
            value
        };
        let padding = encoded
            .as_bytes()
            .iter()
            .rev()
            .take_while(|byte| **byte == b'=')
            .count();
        let decoded_len = (encoded.len() / 4 * 3)
            .checked_sub(padding)
            .filter(|length| encoded.len().is_multiple_of(4) && *length <= self.decoded_limit)
            .ok_or_else(|| E::custom("decoded image exceeds limit"))?;
        let mut bytes = vec![0; decoded_len];
        let length = STANDARD
            .decode_slice(encoded, &mut bytes)
            .map_err(E::custom)?;
        bytes.truncate(length);
        Ok(Some(bytes))
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
    fn record_image_limits_cover_line_string_and_decoded_bytes() {
        let original = record_source();
        let ImageSource::Record { record, pointer } = &original else {
            unreachable!()
        };
        let bytes = std::fs::read(&record.path).unwrap();
        let line = &bytes[record.offset as usize..];
        let value: serde_json::Value = serde_json::from_slice(line).unwrap();
        let encoded = value.pointer(pointer).unwrap().as_str().unwrap();
        let decoded = original.bytes().unwrap();
        assert_eq!(
            original
                .bytes_with_limits(line.len(), encoded.len(), decoded.len())
                .unwrap(),
            decoded
        );
        assert!(
            original
                .bytes_with_limits(line.len() - 1, encoded.len(), decoded.len())
                .is_none()
        );
        assert!(
            original
                .bytes_with_limits(line.len(), encoded.len() - 1, decoded.len())
                .is_none()
        );
        assert!(
            original
                .bytes_with_limits(line.len(), encoded.len(), decoded.len() - 1)
                .is_none()
        );
    }

    #[test]
    fn record_image_limits_handle_data_urls_escaped_strings_and_json_pointers() {
        let original = record_source();
        let image = original.bytes().unwrap();
        let encoded = STANDARD.encode(&image);
        let file = tempfile::NamedTempFile::new().unwrap();
        // Keep the upstream image bytes while exercising JSON escapes around the pointer and data.
        let line = format!(
            r#"{{"ignored":[{{"text":"not an image"}}],"a/b~c":["data:image/png;base64,{}"]}}"#,
            encoded
        )
        .replace("/", "\\u002f");
        std::fs::write(file.path(), &line).unwrap();
        let source = ImageSource::Record {
            record: SourceRecord {
                path: file.path().to_owned(),
                offset: 0,
            },
            pointer: "/a~1b~0c/0".into(),
        };
        let string_len = "data:image/png;base64,".len() + encoded.len();
        assert_eq!(
            source
                .bytes_with_limits(line.len(), string_len, image.len())
                .unwrap(),
            image
        );
        assert!(
            source
                .bytes_with_limits(line.len(), string_len - 1, image.len())
                .is_none()
        );
        assert!(
            source
                .bytes_with_limits(line.len(), string_len, image.len() - 1)
                .is_none()
        );
        std::fs::write(file.path(), format!("{line} invalid")).unwrap();
        assert!(source.bytes().is_none());
    }

    #[test]
    fn local_file_caches_record_paths_and_recreates_deleted_files() {
        let original = record_source();
        let ImageSource::Record { record, pointer } = &original else {
            unreachable!()
        };
        let transcript = std::fs::read(&record.path).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &transcript).unwrap();
        let source = ImageSource::Record {
            record: SourceRecord {
                path: file.path().to_owned(),
                offset: record.offset,
            },
            pointer: pointer.clone(),
        };
        let bytes = source.bytes().unwrap();
        let path = source.local_file().unwrap();
        assert_eq!(path.extension().unwrap(), "png");
        assert_eq!(
            path.parent().unwrap(),
            std::env::temp_dir().join(format!("agents-images-{}", std::process::id()))
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        std::fs::write(file.path(), b"unavailable transcript image").unwrap();
        assert_eq!(source.clone().local_file().unwrap(), path);
        let other_source = ImageSource::Record {
            record: SourceRecord {
                path: file.path().to_owned(),
                offset: record.offset,
            },
            pointer: "/missing/image".into(),
        };
        assert!(other_source.local_file().is_err());

        std::fs::remove_file(&path).unwrap();
        assert!(source.local_file().is_err());
        std::fs::write(file.path(), transcript).unwrap();
        assert_eq!(source.local_file().unwrap(), path);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
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
        std::fs::remove_file(&path).unwrap();
        assert_eq!(source.local_file().unwrap(), path);

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
