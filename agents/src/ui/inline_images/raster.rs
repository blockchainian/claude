use std::{collections::VecDeque, io::Cursor, sync::Arc};

use image::{DynamicImage, ImageFormat, ImageReader, Limits, imageops::FilterType};

use crate::images::ImageSource;

const CACHE_BYTES: usize = 64 << 20;

type Png = ((u32, u32), Arc<Vec<u8>>);

struct Entry {
    source: ImageSource,
    image: Option<Arc<DynamicImage>>,
    dimensions: Option<(u32, u32)>,
    png: Option<Png>,
}

impl Entry {
    fn bytes(&self) -> usize {
        self.image
            .as_ref()
            .map_or(1024, |image| image.as_bytes().len())
            + self.png.as_ref().map_or(0, |(_, png)| png.len())
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
            let image = entry.image.clone();
            self.entries.push_back(entry);
            return image;
        }
        let raster = read(source, self.limit);
        let dimensions = raster.as_ref().map(|(_, dimensions)| *dimensions);
        let image = raster.map(|(image, _)| Arc::new(image));
        let entry = Entry {
            source: source.clone(),
            image: image.clone(),
            dimensions,
            png: None,
        };
        if entry.bytes() <= self.limit {
            self.bytes += entry.bytes();
            self.entries.push_back(entry);
            self.trim();
        }
        image
    }

    pub fn dimensions(&mut self, source: &ImageSource) -> Option<(u32, u32)> {
        self.image(source)?;
        self.entries
            .back()
            .filter(|entry| entry.source == *source)?
            .dimensions
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
            // Keep the decoded raster even when this encoded size cannot share its budget.
            entry.png = None;
            if entry.bytes() + png.len() <= self.limit {
                entry.png = Some((pixels, Arc::clone(&png)));
            }
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

fn read(source: &ImageSource, budget: usize) -> Option<(DynamicImage, (u32, u32))> {
    let bytes = source.bytes()?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(256 << 20);
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let dimensions = (image.width(), image.height());
    let image = if image.as_bytes().len() > budget {
        // Leave room for the resized PNG beside the retained raster.
        let pixels = budget / 2 / usize::from(image.color().bytes_per_pixel());
        let scale = (pixels as f64 / (f64::from(dimensions.0) * f64::from(dimensions.1)))
            .sqrt()
            .min(1.0);
        let width = (f64::from(dimensions.0) * scale).floor().clamp(1.0, 4096.0) as u32;
        let height = (f64::from(dimensions.1) * scale).floor().clamp(1.0, 4096.0) as u32;
        image.resize(width, height, FilterType::Triangle)
    } else {
        image
    };
    Some((image, dimensions))
}

#[cfg(test)]
mod tests {
    use super::super::tests::fixture;
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};

    #[test]
    fn inline_images_raster_retains_images_larger_than_cache_budget() {
        let source = fixture();
        let mut cache = Cache::new();
        cache.limit = 256 << 10;
        let image = cache.image(&source).unwrap();
        assert!(image.as_bytes().len() <= cache.limit / 2);
        assert_eq!(cache.dimensions(&source), Some((960, 539)));
        assert!(Arc::ptr_eq(&image, &cache.image(&source).unwrap()));
        assert_eq!(cache.entries.len(), 1);
        let png = cache.png(&source, (80, 80)).unwrap();
        assert!(Arc::ptr_eq(&png, &cache.png(&source, (80, 80)).unwrap()));
        assert!(cache.bytes <= cache.limit);
    }

    #[test]
    fn inline_images_raster_png_over_budget_keeps_the_decoded_image() {
        let source = fixture();
        let mut cache = Cache::new();
        let image = cache.image(&source).unwrap();
        cache.limit = image.as_bytes().len();
        cache.png(&source, (80, 80)).unwrap();
        assert!(Arc::ptr_eq(&image, &cache.image(&source).unwrap()));
        assert!(cache.bytes <= cache.limit);
    }

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

    fn file_source(path: &std::path::Path) -> ImageSource {
        let metadata = std::fs::metadata(path).unwrap();
        ImageSource::File {
            path: path.to_owned(),
            len: metadata.len(),
            modified: metadata.modified().unwrap(),
        }
    }

    #[test]
    fn inline_images_raster_success_hits_need_no_source_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.data");
        std::fs::write(&path, crate::images::fixture_png()).unwrap();
        let source = file_source(&path);
        let mut cache = Cache::new();
        let image = cache.image(&source).unwrap();
        let png = cache.png(&source, (20, 20)).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(Arc::ptr_eq(&image, &cache.image(&source).unwrap()));
        assert!(Arc::ptr_eq(&png, &cache.png(&source, (20, 20)).unwrap()));
    }

    #[test]
    fn inline_images_raster_failed_records_stay_cached_until_evicted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("transcript.jsonl");
        let mut source = fixture();
        let ImageSource::Record { record, .. } = &mut source else {
            unreachable!()
        };
        let bytes = std::fs::read(&record.path).unwrap();
        std::fs::write(&path, &bytes[..record.offset as usize]).unwrap();
        record.path = path.clone();
        let mut cache = Cache::new();
        cache.limit = 1024;
        assert!(cache.image(&source).is_none());
        assert_eq!(cache.bytes, 1024);
        assert_eq!(cache.entries.len(), 1);
        std::fs::write(&path, bytes).unwrap();
        assert!(
            cache.image(&source).is_none(),
            "the same key must not read the repaired record"
        );
        assert!(cache.png(&source, (20, 20)).is_none());
        let mut other = source.clone();
        let ImageSource::Record { pointer, .. } = &mut other else {
            unreachable!()
        };
        *pointer = "/missing".into();
        assert!(cache.image(&other).is_none());
        assert_eq!(cache.bytes, 1024);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[0].source, other);
        assert!(
            cache.image(&source).is_some(),
            "eviction must permit reading the repaired record"
        );
        assert!(cache.bytes <= cache.limit);
        assert!(
            cache.entries[0].image.is_some(),
            "the repaired image is resized and retained"
        );
    }

    #[test]
    fn inline_images_raster_file_metadata_selects_the_current_image() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        let bytes = crate::images::fixture_png();
        std::fs::write(&path, &bytes).unwrap();
        let first = file_source(&path);
        let mut cache = Cache::new();
        let image = cache.image(&first).unwrap();
        let thumbnail = image.resize_exact(17, 11, FilterType::Triangle);
        thumbnail.save(&path).unwrap();
        let second = file_source(&path);
        assert_ne!(first, second);
        let current = cache.image(&second).unwrap();
        assert_eq!((current.width(), current.height()), (17, 11));
        assert!(Arc::ptr_eq(&image, &cache.image(&first).unwrap()));
    }

    #[test]
    fn inline_images_raster_limits_image_dimensions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wide.png");
        let image = image::load_from_memory(&crate::images::fixture_png()).unwrap();
        image
            .resize_exact(16_385, 1, FilterType::Nearest)
            .save(&path)
            .unwrap();
        assert_eq!(image::open(&path).unwrap().width(), 16_385);
        assert!(Cache::new().image(&file_source(&path)).is_none());
    }
}
