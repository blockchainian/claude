use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader, Cursor, Seek, SeekFrom},
    sync::Arc,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, imageops::FilterType};

use crate::event::ImageBlock;

const CACHE_BYTES: usize = 64 << 20;

type Png = ((u32, u32), Arc<Vec<u8>>);

struct Entry {
    source: ImageBlock,
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

    pub fn image(&mut self, source: &ImageBlock) -> Option<Arc<DynamicImage>> {
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

    pub fn png(&mut self, source: &ImageBlock, pixels: (u32, u32)) -> Option<Arc<Vec<u8>>> {
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

fn read(source: &ImageBlock) -> Option<DynamicImage> {
    let mut file = File::open(&source.record.path).ok()?;
    file.seek(SeekFrom::Start(source.record.offset)).ok()?;
    let mut line = Vec::new();
    BufReader::new(file).read_until(b'\n', &mut line).ok()?;
    let record: serde_json::Value = serde_json::from_slice(&line).ok()?;
    let data = record.pointer(&source.pointer)?.as_str()?;
    let bytes = STANDARD.decode(data).ok()?;
    image::load_from_memory(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::super::tests::fixture;
    use super::*;

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
        second.pointer = "/toolUseResult/file/base64".into();
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::copy(&first.record.path, temp.path()).unwrap();
        let mut third = first.clone();
        third.record.path = temp.path().to_owned();
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
        source.record.offset = 0;
        assert!(cache.image(&source).is_none());
        source = fixture();
        source.pointer = "/message/content/0/content/0/source/media_type".into();
        assert!(cache.image(&source).is_none());
        let bytes = std::fs::read(&source.record.path).unwrap();
        let mut record: serde_json::Value =
            serde_json::from_slice(&bytes[source.record.offset as usize..]).unwrap();
        source.pointer = fixture().pointer;
        *record.pointer_mut(&source.pointer).unwrap() =
            serde_json::Value::String(STANDARD.encode(b"invalid image"));
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), serde_json::to_vec(&record).unwrap()).unwrap();
        source.record.path = temp.path().to_owned();
        source.record.offset = 0;
        assert!(cache.image(&source).is_none());
    }
}
