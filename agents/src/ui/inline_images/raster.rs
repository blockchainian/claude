use std::{collections::VecDeque, io::Cursor, sync::Arc};

use image::{DynamicImage, ImageFormat, ImageReader, Limits, imageops::FilterType};

use crate::images::ImageSource;

const CACHE_BYTES: usize = 64 << 20;

type Png = ((u32, u32), Arc<Vec<u8>>);

struct Entry {
    source: ImageSource,
    image: Arc<DynamicImage>,
    png: Option<Png>,
}

impl Entry {
    fn bytes(&self) -> usize {
        self.image.as_bytes().len() + self.png.as_ref().map_or(0, |(_, png)| png.len())
    }
}

pub(super) struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
    limit: usize,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            limit: CACHE_BYTES,
        }
    }

    pub fn image(&mut self, source: &ImageSource) -> Option<Arc<DynamicImage>> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.source == *source)
        {
            let entry = self.entries.remove(index)?;
            let image = Arc::clone(&entry.image);
            self.entries.push_back(entry);
            return Some(image);
        }
        let image = Arc::new(read(source)?);
        let entry = Entry {
            source: source.clone(),
            image: Arc::clone(&image),
            png: None,
        };
        if entry.bytes() <= self.limit {
            self.bytes += entry.bytes();
            self.entries.push_back(entry);
            self.trim();
        }
        Some(image)
    }

    pub fn png(&mut self, source: &ImageSource, pixels: (u32, u32)) -> Option<Arc<Vec<u8>>> {
        let image = self.image(source)?;
        if let Some((size, png)) = self
            .entries
            .back()
            .filter(|entry| entry.source == *source)
            .and_then(|entry| entry.png.as_ref())
            && *size == pixels
        {
            return Some(Arc::clone(png));
        }
        let resized = image.resize(pixels.0, pixels.1, FilterType::Triangle);
        let mut bytes = Cursor::new(Vec::new());
        resized.write_to(&mut bytes, ImageFormat::Png).ok()?;
        let png = Arc::new(bytes.into_inner());
        if let Some(entry) = self
            .entries
            .back_mut()
            .filter(|entry| entry.source == *source)
        {
            self.bytes -= entry.bytes();
            entry.png = Some((pixels, Arc::clone(&png)));
            self.bytes += entry.bytes();
            self.trim();
        }
        Some(png)
    }

    fn trim(&mut self) {
        while self.bytes > self.limit {
            let entry = self
                .entries
                .pop_front()
                .expect("cache bytes belong to entries");
            self.bytes -= entry.bytes();
        }
    }
}

fn read(source: &ImageSource) -> Option<DynamicImage> {
    let bytes = source.bytes()?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(256 << 20);
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    reader.limits(limits);
    reader.decode().ok()
}

#[cfg(test)]
mod tests {
    use super::super::tests::fixture;
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};

    #[test]
    fn inline_images_raster_reads_offset_scales_png_and_reuses_cache() {
        let source = fixture();
        let mut cache = Cache::new();
        let original = cache.image(&source).unwrap();
        assert_eq!((original.width(), original.height()), (960, 539));
        let png = cache.png(&source, (80, 80)).unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (80, 45));
        assert!(Arc::ptr_eq(&png, &cache.png(&source, (80, 80)).unwrap()));
        let different = cache.png(&source, (40, 40)).unwrap();
        let decoded = image::load_from_memory(&different).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (40, 22));
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.bytes <= cache.limit);
    }

    #[test]
    fn inline_images_raster_cache_evicts_by_bytes_and_recency() {
        let first = fixture();
        let mut second = first.clone();
        if let ImageSource::Record { pointer, .. } = &mut second {
            *pointer = "/toolUseResult/file/base64".into();
        }
        let temp = tempfile::NamedTempFile::new().unwrap();
        let ImageSource::Record { record, .. } = &first else {
            unreachable!()
        };
        std::fs::copy(&record.path, temp.path()).unwrap();
        let mut third = first.clone();
        if let ImageSource::Record { record, .. } = &mut third {
            record.path = temp.path().to_owned();
        }
        let mut cache = Cache::new();
        let original = cache.image(&first).unwrap();
        cache.limit = original.as_bytes().len() * 2;
        cache.image(&second).unwrap();
        cache.image(&first).unwrap();
        cache.image(&third).unwrap();
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.entries[0].source, first);
        assert_eq!(cache.entries[1].source, third);
        assert!(cache.bytes <= cache.limit);
        cache.limit = 1;
        cache.png(&first, (80, 80)).unwrap();
        assert_eq!(cache.bytes, 0);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn inline_images_raster_rejects_wrong_offsets_base64_and_image_bytes() {
        let mut source = fixture();
        let mut cache = Cache::new();
        let ImageSource::Record { record, .. } = &mut source else {
            unreachable!()
        };
        record.offset = 0;
        assert!(cache.image(&source).is_none());
        source = fixture();
        let ImageSource::Record { pointer, .. } = &mut source else {
            unreachable!()
        };
        *pointer = "/message/content/0/content/0/source/media_type".into();
        assert!(cache.image(&source).is_none());
        source = fixture();
        let (path, offset, expected_pointer) = match &source {
            ImageSource::Record { record, pointer } => {
                (record.path.clone(), record.offset, pointer.clone())
            }
            ImageSource::File { .. } => unreachable!(),
        };
        let bytes = std::fs::read(path).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes[offset as usize..]).unwrap();
        let ImageSource::Record { pointer, .. } = &mut source else {
            unreachable!()
        };
        *pointer = expected_pointer;
        *value.pointer_mut(pointer).unwrap() =
            serde_json::Value::String(STANDARD.encode(b"invalid image"));
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), serde_json::to_vec(&value).unwrap()).unwrap();
        if let ImageSource::Record { record, .. } = &mut source {
            record.path = temp.path().to_owned();
            record.offset = 0;
        }
        assert!(cache.image(&source).is_none());
    }

    #[test]
    fn inline_images_raster_reads_codex_data_url_records() {
        let mut source = fixture();
        let ImageSource::Record {
            record: source_record,
            pointer,
        } = &source
        else {
            unreachable!()
        };
        let bytes = std::fs::read(&source_record.path).unwrap();
        let line = &bytes[source_record.offset as usize..];
        let mut record: serde_json::Value = serde_json::from_slice(line).unwrap();
        let encoded = record.pointer(pointer).unwrap().as_str().unwrap();
        *record.pointer_mut(pointer).unwrap() =
            serde_json::Value::String(format!("data:image/png;base64,{encoded}"));

        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), serde_json::to_vec(&record).unwrap()).unwrap();
        if let ImageSource::Record { record, .. } = &mut source {
            record.path = file.path().to_owned();
            record.offset = 0;
        }

        let image = Cache::new().image(&source);
        assert_eq!(
            image.map(|image| (image.width(), image.height())),
            Some((960, 539))
        );
    }
}
