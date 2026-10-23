mod raster;

use std::collections::VecDeque;

use base64::{Engine, engine::general_purpose::STANDARD};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

use crate::event::ImageBlock;

const UPLOAD_BYTES: usize = 64 << 20;
const CHUNK: usize = 4096;
const GRID_LIMIT: u16 = 297;

struct Uploaded {
    source: ImageBlock,
    id: u8,
    placements: Vec<((u16, u16), u32)>,
    bytes: usize,
}

/// Images uploaded to the terminal and the commands that keep them current.
/// Exists only while the terminal supports images.
pub struct InlineImages {
    cell_pixels: Option<(u16, u16)>,
    raster: raster::Cache,
    uploaded: VecDeque<Uploaded>,
    bytes: usize,
    limit: usize,
    commands: Vec<u8>,
    placement_id: u32,
}

impl InlineImages {
    /// Images for a terminal whose cells are `cell_pixels` in size, or 1:2 when unknown.
    pub fn new(cell_pixels: Option<(u16, u16)>) -> Self {
        Self {
            cell_pixels,
            raster: raster::Cache::new(),
            uploaded: VecDeque::new(),
            bytes: 0,
            limit: UPLOAD_BYTES,
            commands: Vec::new(),
            placement_id: 0,
        }
    }

    /// The cells `image` covers in a panel `width` cells wide at `zoom`;
    /// `None` when its data cannot be read or decoded.
    pub fn size(&mut self, image: &ImageBlock, width: u16, zoom: Zoom) -> Option<(u16, u16)> {
        if width == 0 {
            return None;
        }
        let raster = self.raster.image(image)?;
        let (cw, ch) = self.cell_size();
        let (w, h) = (f64::from(raster.width()), f64::from(raster.height()));
        let fit = f64::from(width).min(w / cw);
        // Explicit row/column diacritics can address at most 297 cells per axis.
        let columns = (fit * 1.25_f64.powi(i32::from(zoom.level)))
            .min(8.0 * w / cw)
            .min(f64::from(GRID_LIMIT))
            .min(f64::from(GRID_LIMIT) * ch * w / (h * cw));
        Some((
            columns.ceil().clamp(1.0, f64::from(GRID_LIMIT)) as u16,
            (columns * cw * h / (w * ch))
                .ceil()
                .clamp(1.0, f64::from(GRID_LIMIT)) as u16,
        ))
    }

    /// Draws the part of `image` that falls in `area`, starting at image row `first_row`,
    /// with `zoom` choosing the visible columns. Queues the upload and placement the first
    /// time `image` is drawn at this size.
    pub fn draw(
        &mut self,
        image: &ImageBlock,
        zoom: Zoom,
        first_row: u16,
        area: Rect,
        buf: &mut Buffer,
    ) {
        if area.is_empty() {
            return;
        }
        let Some(size) = self.size(image, area.width, zoom) else {
            return;
        };
        if first_row >= size.1 || area.intersection(buf.area).is_empty() {
            return;
        }
        let Some((id, placement)) = self.prepare(image, size) else {
            return;
        };
        let max_left = f32::from(size.0.saturating_sub(area.width)) / f32::from(size.0);
        let left = if zoom.left.is_nan() {
            0.0
        } else {
            zoom.left.clamp(0.0, max_left)
        };
        let first_column = (left * f32::from(size.0)).round() as u16;
        let visible = area.intersection(buf.area);
        for y in visible.top()..visible.bottom() {
            let row = first_row.saturating_add(y - area.y);
            if row >= size.1 {
                break;
            }
            for x in visible.left()..visible.right() {
                let column = first_column.saturating_add(x - area.x);
                if column >= size.0 {
                    break;
                }
                let symbol = format!(
                    "\u{10eeee}{}{}",
                    DIACRITICS[usize::from(row)],
                    DIACRITICS[usize::from(column)]
                );
                buf[(x, y)].reset();
                buf[(x, y)].set_symbol(&symbol).set_style(
                    Style::default()
                        .fg(Color::Indexed(id))
                        .underline_color(Color::Rgb(
                            (placement >> 16) as u8,
                            (placement >> 8) as u8,
                            placement as u8,
                        )),
                );
            }
        }
    }

    /// Graphics commands queued since the last call.
    pub fn take_commands(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.commands)
    }

    fn cell_size(&self) -> (f64, f64) {
        match self.cell_pixels {
            Some((w, h)) if w != 0 && h != 0 => (f64::from(w), f64::from(h)),
            _ => (1.0, 2.0),
        }
    }

    fn prepare(&mut self, image: &ImageBlock, size: (u16, u16)) -> Option<(u8, u32)> {
        if let Some(index) = self
            .uploaded
            .iter()
            .position(|entry| entry.source == *image)
        {
            let placement = if let Some((_, id)) = self.uploaded[index]
                .placements
                .iter()
                .find(|(s, _)| *s == size)
            {
                *id
            } else {
                let id = self.place(self.uploaded[index].id, size)?;
                self.uploaded[index].placements.push((size, id));
                id
            };
            let entry = self.uploaded.remove(index)?;
            let id = entry.id;
            self.uploaded.push_back(entry);
            return Some((id, placement));
        }
        let raster = self.raster.image(image)?;
        let (cw, ch) = self.cell_size();
        let scale = ((f64::from(size.0) * cw / f64::from(raster.width()))
            .min(f64::from(size.1) * ch / f64::from(raster.height())))
        .min(1.0);
        let pixels = (
            (f64::from(raster.width()) * scale).round().max(1.0) as u32,
            (f64::from(raster.height()) * scale).round().max(1.0) as u32,
        );
        let png = self.raster.png(image, pixels)?;
        let bytes = png.len().max(pixels.0 as usize * pixels.1 as usize * 4);
        if bytes > self.limit {
            return None;
        }
        while self.uploaded.len() == 255 || self.bytes + bytes > self.limit {
            let entry = self
                .uploaded
                .pop_front()
                .expect("upload quota belongs to entries");
            self.bytes -= entry.bytes;
            self.command(&format!("a=d,d=I,i={},q=2", entry.id), &[]);
        }
        let id = (1..=255).find(|id| self.uploaded.iter().all(|entry| entry.id != *id))?;
        let encoded = STANDARD.encode(png.as_slice());
        let chunks = encoded.as_bytes().chunks(CHUNK);
        let count = chunks.len();
        for (index, chunk) in chunks.enumerate() {
            let more = u8::from(index + 1 < count);
            let control = if index == 0 {
                format!("a=t,f=100,t=d,i={id},q=2,m={more}")
            } else {
                format!("m={more}")
            };
            self.command(&control, chunk);
        }
        let placement = self.place(id, size)?;
        self.uploaded.push_back(Uploaded {
            source: image.clone(),
            id,
            placements: vec![(size, placement)],
            bytes,
        });
        self.bytes += bytes;
        Some((id, placement))
    }

    fn place(&mut self, id: u8, size: (u16, u16)) -> Option<u32> {
        // Placeholders select a placement through a 24-bit underline color. Keep dimensions
        // immutable so preparing a frame cannot resize the preceding frame's visible cells.
        let placement = (1..=0x00ff_ffff)
            .map(|step| (self.placement_id + step - 1) % 0x00ff_ffff + 1)
            .find(|candidate| {
                self.uploaded
                    .iter()
                    .all(|image| image.placements.iter().all(|(_, id)| id != candidate))
            })?;
        self.placement_id = placement;
        self.command(
            &format!("a=p,U=1,i={id},p={placement},c={},r={},q=2", size.0, size.1),
            &[],
        );
        Some(placement)
    }

    fn command(&mut self, control: &str, payload: &[u8]) {
        self.commands.extend_from_slice(b"\x1b_G");
        self.commands.extend_from_slice(control.as_bytes());
        if !payload.is_empty() {
            self.commands.push(b';');
            self.commands.extend_from_slice(payload);
        }
        self.commands.extend_from_slice(b"\x1b\\");
    }
}

/// An image's zoom; the default fits the panel width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Zoom {
    /// Steps of 1.25x beyond fitting the panel width.
    pub level: u8,
    /// The fraction of the zoomed image's width left of the panel.
    pub left: f32,
}

// https://sw.kovidgoyal.net/kitty/graphics-protocol/#unicode-placeholders
const DIACRITICS: [char; 297] = [
    '\u{305}',
    '\u{30d}',
    '\u{30e}',
    '\u{310}',
    '\u{312}',
    '\u{33d}',
    '\u{33e}',
    '\u{33f}',
    '\u{346}',
    '\u{34a}',
    '\u{34b}',
    '\u{34c}',
    '\u{350}',
    '\u{351}',
    '\u{352}',
    '\u{357}',
    '\u{35b}',
    '\u{363}',
    '\u{364}',
    '\u{365}',
    '\u{366}',
    '\u{367}',
    '\u{368}',
    '\u{369}',
    '\u{36a}',
    '\u{36b}',
    '\u{36c}',
    '\u{36d}',
    '\u{36e}',
    '\u{36f}',
    '\u{483}',
    '\u{484}',
    '\u{485}',
    '\u{486}',
    '\u{487}',
    '\u{592}',
    '\u{593}',
    '\u{594}',
    '\u{595}',
    '\u{597}',
    '\u{598}',
    '\u{599}',
    '\u{59c}',
    '\u{59d}',
    '\u{59e}',
    '\u{59f}',
    '\u{5a0}',
    '\u{5a1}',
    '\u{5a8}',
    '\u{5a9}',
    '\u{5ab}',
    '\u{5ac}',
    '\u{5af}',
    '\u{5c4}',
    '\u{610}',
    '\u{611}',
    '\u{612}',
    '\u{613}',
    '\u{614}',
    '\u{615}',
    '\u{616}',
    '\u{617}',
    '\u{657}',
    '\u{658}',
    '\u{659}',
    '\u{65a}',
    '\u{65b}',
    '\u{65d}',
    '\u{65e}',
    '\u{6d6}',
    '\u{6d7}',
    '\u{6d8}',
    '\u{6d9}',
    '\u{6da}',
    '\u{6db}',
    '\u{6dc}',
    '\u{6df}',
    '\u{6e0}',
    '\u{6e1}',
    '\u{6e2}',
    '\u{6e4}',
    '\u{6e7}',
    '\u{6e8}',
    '\u{6eb}',
    '\u{6ec}',
    '\u{730}',
    '\u{732}',
    '\u{733}',
    '\u{735}',
    '\u{736}',
    '\u{73a}',
    '\u{73d}',
    '\u{73f}',
    '\u{740}',
    '\u{741}',
    '\u{743}',
    '\u{745}',
    '\u{747}',
    '\u{749}',
    '\u{74a}',
    '\u{7eb}',
    '\u{7ec}',
    '\u{7ed}',
    '\u{7ee}',
    '\u{7ef}',
    '\u{7f0}',
    '\u{7f1}',
    '\u{7f3}',
    '\u{816}',
    '\u{817}',
    '\u{818}',
    '\u{819}',
    '\u{81b}',
    '\u{81c}',
    '\u{81d}',
    '\u{81e}',
    '\u{81f}',
    '\u{820}',
    '\u{821}',
    '\u{822}',
    '\u{823}',
    '\u{825}',
    '\u{826}',
    '\u{827}',
    '\u{829}',
    '\u{82a}',
    '\u{82b}',
    '\u{82c}',
    '\u{82d}',
    '\u{951}',
    '\u{953}',
    '\u{954}',
    '\u{f82}',
    '\u{f83}',
    '\u{f86}',
    '\u{f87}',
    '\u{135d}',
    '\u{135e}',
    '\u{135f}',
    '\u{17dd}',
    '\u{193a}',
    '\u{1a17}',
    '\u{1a75}',
    '\u{1a76}',
    '\u{1a77}',
    '\u{1a78}',
    '\u{1a79}',
    '\u{1a7a}',
    '\u{1a7b}',
    '\u{1a7c}',
    '\u{1b6b}',
    '\u{1b6d}',
    '\u{1b6e}',
    '\u{1b6f}',
    '\u{1b70}',
    '\u{1b71}',
    '\u{1b72}',
    '\u{1b73}',
    '\u{1cd0}',
    '\u{1cd1}',
    '\u{1cd2}',
    '\u{1cda}',
    '\u{1cdb}',
    '\u{1ce0}',
    '\u{1dc0}',
    '\u{1dc1}',
    '\u{1dc3}',
    '\u{1dc4}',
    '\u{1dc5}',
    '\u{1dc6}',
    '\u{1dc7}',
    '\u{1dc8}',
    '\u{1dc9}',
    '\u{1dcb}',
    '\u{1dcc}',
    '\u{1dd1}',
    '\u{1dd2}',
    '\u{1dd3}',
    '\u{1dd4}',
    '\u{1dd5}',
    '\u{1dd6}',
    '\u{1dd7}',
    '\u{1dd8}',
    '\u{1dd9}',
    '\u{1dda}',
    '\u{1ddb}',
    '\u{1ddc}',
    '\u{1ddd}',
    '\u{1dde}',
    '\u{1ddf}',
    '\u{1de0}',
    '\u{1de1}',
    '\u{1de2}',
    '\u{1de3}',
    '\u{1de4}',
    '\u{1de5}',
    '\u{1de6}',
    '\u{1dfe}',
    '\u{20d0}',
    '\u{20d1}',
    '\u{20d4}',
    '\u{20d5}',
    '\u{20d6}',
    '\u{20d7}',
    '\u{20db}',
    '\u{20dc}',
    '\u{20e1}',
    '\u{20e7}',
    '\u{20e9}',
    '\u{20f0}',
    '\u{2cef}',
    '\u{2cf0}',
    '\u{2cf1}',
    '\u{2de0}',
    '\u{2de1}',
    '\u{2de2}',
    '\u{2de3}',
    '\u{2de4}',
    '\u{2de5}',
    '\u{2de6}',
    '\u{2de7}',
    '\u{2de8}',
    '\u{2de9}',
    '\u{2dea}',
    '\u{2deb}',
    '\u{2dec}',
    '\u{2ded}',
    '\u{2dee}',
    '\u{2def}',
    '\u{2df0}',
    '\u{2df1}',
    '\u{2df2}',
    '\u{2df3}',
    '\u{2df4}',
    '\u{2df5}',
    '\u{2df6}',
    '\u{2df7}',
    '\u{2df8}',
    '\u{2df9}',
    '\u{2dfa}',
    '\u{2dfb}',
    '\u{2dfc}',
    '\u{2dfd}',
    '\u{2dfe}',
    '\u{2dff}',
    '\u{a66f}',
    '\u{a67c}',
    '\u{a67d}',
    '\u{a6f0}',
    '\u{a6f1}',
    '\u{a8e0}',
    '\u{a8e1}',
    '\u{a8e2}',
    '\u{a8e3}',
    '\u{a8e4}',
    '\u{a8e5}',
    '\u{a8e6}',
    '\u{a8e7}',
    '\u{a8e8}',
    '\u{a8e9}',
    '\u{a8ea}',
    '\u{a8eb}',
    '\u{a8ec}',
    '\u{a8ed}',
    '\u{a8ee}',
    '\u{a8ef}',
    '\u{a8f0}',
    '\u{a8f1}',
    '\u{aab0}',
    '\u{aab2}',
    '\u{aab3}',
    '\u{aab7}',
    '\u{aab8}',
    '\u{aabe}',
    '\u{aabf}',
    '\u{aac1}',
    '\u{fe20}',
    '\u{fe21}',
    '\u{fe22}',
    '\u{fe23}',
    '\u{fe24}',
    '\u{fe25}',
    '\u{fe26}',
    '\u{10a0f}',
    '\u{10a38}',
    '\u{1d185}',
    '\u{1d186}',
    '\u{1d187}',
    '\u{1d188}',
    '\u{1d189}',
    '\u{1d1aa}',
    '\u{1d1ab}',
    '\u{1d1ac}',
    '\u{1d1ad}',
    '\u{1d242}',
    '\u{1d243}',
    '\u{1d244}',
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::SourceRecord;
    use ratatui::style::Color;

    pub(super) fn fixture() -> ImageBlock {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-read-image.jsonl");
        let bytes = std::fs::read(&path).unwrap();
        ImageBlock {
            media_type: "image/png".into(),
            record: SourceRecord {
                path,
                offset: (bytes.iter().position(|b| *b == b'\n').unwrap() + 1) as u64,
            },
            pointer: "/message/content/0/content/0/source/data".into(),
        }
    }

    #[test]
    fn inline_images_size_preserves_aspect_and_caps_zoom() {
        let image = fixture();
        let mut images = InlineImages::new(Some((10, 20)));
        assert_eq!(images.size(&image, 80, Zoom::default()), Some((80, 23)));
        assert_eq!(images.size(&image, 200, Zoom::default()), Some((96, 27)));
        assert_eq!(
            images.size(
                &image,
                80,
                Zoom {
                    level: 1,
                    left: 0.0
                }
            ),
            Some((100, 29))
        );
        assert_eq!(
            images.size(
                &image,
                80,
                Zoom {
                    level: 255,
                    left: 0.0
                }
            ),
            Some((297, 84))
        );
        assert_eq!(images.size(&image, 0, Zoom::default()), None);
        let mut unknown = InlineImages::new(None);
        assert_eq!(unknown.size(&image, 80, Zoom::default()), Some((80, 23)));
        let mut large_cells = InlineImages::new(Some((100, 200)));
        assert_eq!(
            large_cells.size(
                &image,
                80,
                Zoom {
                    level: 255,
                    left: 0.0
                }
            ),
            Some((77, 22))
        );
    }

    #[test]
    fn inline_images_draws_clipped_placeholders_and_replaces_only_placement() {
        let image = fixture();
        let mut images = InlineImages::new(Some((10, 20)));
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 30));
        let area = Rect::new(2, 3, 80, 2);
        images.draw(&image, Zoom::default(), 1, area, &mut buf);
        assert_eq!(buf[(2, 3)].symbol(), "\u{10eeee}\u{30d}\u{305}");
        assert_eq!(buf[(3, 3)].symbol(), "\u{10eeee}\u{30d}\u{30d}");
        assert_eq!(buf[(2, 3)].fg, Color::Indexed(1));
        assert_eq!(buf[(1, 3)].symbol(), " ");
        let commands = String::from_utf8(images.take_commands()).unwrap();
        assert!(commands.starts_with("\x1b_Ga=t,f=100,t=d,i=1,q=2,m=1;"));
        assert!(commands.contains("\x1b_Gm=0;"));
        assert!(commands.ends_with("\x1b_Ga=p,U=1,i=1,p=1,c=80,r=23,q=2\x1b\\"));
        let mut encoded = String::new();
        for command in commands.split("\x1b_G").skip(1) {
            let command = command.strip_suffix("\x1b\\").unwrap();
            if let Some((control, payload)) = command.split_once(';') {
                assert!(payload.len() <= CHUNK);
                assert_eq!(payload.len() % 4, 0);
                assert!(control.ends_with("m=1") || control.ends_with("m=0"));
                encoded.push_str(payload);
            }
        }
        let png = STANDARD.decode(encoded).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let raster = image::load_from_memory(&png).unwrap();
        assert_eq!(raster.width(), 800);
        assert!((448..=450).contains(&raster.height()));
        images.draw(&image, Zoom::default(), 2, area, &mut buf);
        assert!(images.take_commands().is_empty());
        images.draw(
            &image,
            Zoom {
                level: 1,
                left: 0.1,
            },
            0,
            area,
            &mut buf,
        );
        assert_eq!(
            images.take_commands(),
            b"\x1b_Ga=p,U=1,i=1,p=2,c=100,r=29,q=2\x1b\\"
        );
        assert_eq!(buf[(2, 3)].fg, Color::Indexed(1));
        assert_eq!(
            buf[(2, 3)].symbol(),
            format!("\u{10eeee}{}{}", DIACRITICS[0], DIACRITICS[10])
        );
    }

    #[test]
    fn zoom_placement_cannot_resize_cells_from_the_previous_frame() {
        let image = fixture();
        let mut images = InlineImages::new(Some((10, 20)));
        let area = Rect::new(0, 0, 80, 3);
        let mut before = Buffer::empty(area);
        images.draw(&image, Zoom::default(), 0, area, &mut before);
        let initial = String::from_utf8(images.take_commands()).unwrap();
        let mut after = Buffer::empty(area);
        images.draw(
            &image,
            Zoom {
                level: 1,
                left: 0.1,
            },
            1,
            area,
            &mut after,
        );
        let zoom = String::from_utf8(images.take_commands()).unwrap();
        // Commands reach the terminal while the previous frame's cells are still visible.
        // Both frames must select explicit placements with independent dimensions.
        assert_ne!(before[(0, 0)].underline_color, Color::Reset);
        assert_ne!(
            before[(0, 0)].underline_color,
            after[(0, 0)].underline_color
        );
        let placement = |commands: &str, cells: &Buffer, size: (u16, u16)| {
            let command = commands.rsplit("a=p,").next().unwrap();
            let field = |key: &str| {
                command
                    .split(',')
                    .find_map(|field| field.strip_prefix(key))
                    .unwrap()
                    .parse::<u32>()
                    .unwrap()
            };
            let id = field("p=");
            assert_eq!(
                (field("c="), field("r=")),
                (u32::from(size.0), u32::from(size.1))
            );
            for cell in &cells.content {
                assert_eq!(
                    cell.underline_color,
                    Color::Rgb((id >> 16) as u8, (id >> 8) as u8, id as u8)
                );
                let mut chars = cell.symbol().chars();
                assert_eq!(chars.next(), Some('\u{10eeee}'));
                let row_char = chars.next().unwrap();
                let column_char = chars.next().unwrap();
                let row = DIACRITICS.iter().position(|&c| c == row_char).unwrap();
                let column = DIACRITICS.iter().position(|&c| c == column_char).unwrap();
                assert!(row < usize::from(size.1) && column < usize::from(size.0));
            }
            id
        };
        assert_ne!(
            placement(&initial, &before, (80, 23)),
            placement(&zoom, &after, (100, 29))
        );
        images.draw(&image, Zoom::default(), 0, area, &mut before);
        assert!(images.take_commands().is_empty());
    }

    #[test]
    fn inline_images_clamps_pan_and_clips_to_buffer_and_image() {
        let image = fixture();
        let mut images = InlineImages::new(Some((10, 20)));
        let area = Rect::new(0, 0, 80, 3);
        let zoom = Zoom {
            level: 1,
            left: 5.0,
        };
        let mut buf = Buffer::empty(Rect::new(2, 1, 78, 3));
        images.draw(&image, zoom, 27, area, &mut buf);
        assert_eq!(
            buf[(2, 1)].symbol(),
            format!("\u{10eeee}{}{}", DIACRITICS[28], DIACRITICS[22])
        );
        assert_eq!(
            buf[(79, 1)].symbol(),
            format!("\u{10eeee}{}{}", DIACRITICS[28], DIACRITICS[99])
        );
        assert_eq!(buf[(2, 2)].symbol(), " ");
        let mut buf = Buffer::empty(area);
        for left in [-1.0, f32::NAN, f32::NEG_INFINITY] {
            images.draw(&image, Zoom { level: 1, left }, 0, area, &mut buf);
            assert_eq!(buf[(0, 0)].symbol(), "\u{10eeee}\u{305}\u{305}");
        }
        let mut small = InlineImages::new(Some((100, 200)));
        small.draw(&image, Zoom::default(), 0, area, &mut buf);
        assert_eq!(buf[(10, 0)].fg, Color::Indexed(1));
        assert_eq!(small.uploaded[0].placements[0].0, (10, 3));
    }

    #[test]
    fn inline_images_evicts_least_recently_drawn_image_at_byte_limit() {
        let first = fixture();
        let mut second = first.clone();
        second.pointer = "/toolUseResult/file/base64".into();
        let mut images = InlineImages::new(Some((10, 20)));
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        images.draw(&first, Zoom::default(), 0, area, &mut buf);
        images.limit = images.bytes * 2;
        images.draw(&second, Zoom::default(), 0, area, &mut buf);
        images.draw(&first, Zoom::default(), 0, area, &mut buf);
        images.take_commands();
        images.limit /= 2;
        // A third source uses another real record, without changing its payload.
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::copy(&first.record.path, temp.path()).unwrap();
        let mut third = first.clone();
        third.record.path = temp.path().to_owned();
        images.draw(&third, Zoom::default(), 0, area, &mut buf);
        let commands = String::from_utf8(images.take_commands()).unwrap();
        assert!(commands.starts_with("\x1b_Ga=d,d=I,i=2,q=2\x1b\\\x1b_Ga=d,d=I,i=1,q=2\x1b\\"));
        assert_eq!(images.uploaded.len(), 1);
        assert!(images.bytes <= images.limit);
    }

    #[test]
    fn inline_images_reuses_lru_id_when_all_255_ids_are_taken() {
        use std::io::Write;

        let mut source = fixture();
        let fixture = std::fs::read(&source.record.path).unwrap();
        let record = &fixture[source.record.offset as usize..];
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        for _ in 0..256 {
            temp.write_all(record).unwrap();
        }
        source.record.path = temp.path().to_owned();
        let mut images = InlineImages::new(None);
        let area = Rect::new(0, 0, 1, 1);
        let mut buf = Buffer::empty(area);
        for index in 0..255 {
            source.record.offset = index * record.len() as u64;
            images.draw(&source, Zoom::default(), 0, area, &mut buf);
            assert_eq!(buf[(0, 0)].fg, Color::Indexed((index + 1) as u8));
            images.take_commands();
        }
        source.record.offset = 0;
        images.draw(&source, Zoom::default(), 0, area, &mut buf);
        assert!(images.take_commands().is_empty());
        source.record.offset = 255 * record.len() as u64;
        images.draw(&source, Zoom::default(), 0, area, &mut buf);
        let commands = String::from_utf8(images.take_commands()).unwrap();
        assert!(commands.starts_with("\x1b_Ga=d,d=I,i=2,q=2\x1b\\\x1b_Ga=t,f=100,t=d,i=2,"));
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(2));
        assert_eq!(images.uploaded.len(), 255);
    }

    #[test]
    fn inline_images_unreadable_or_invalid_data_has_no_size_or_commands() {
        let mut image = fixture();
        let mut images = InlineImages::new(None);
        image.pointer = "/missing".into();
        assert_eq!(images.size(&image, 80, Zoom::default()), None);
        let area = Rect::new(0, 0, 80, 2);
        images.draw(&image, Zoom::default(), 0, area, &mut Buffer::empty(area));
        assert!(images.take_commands().is_empty());
        image.record.path = image.record.path.with_extension("absent");
        assert_eq!(images.size(&image, 80, Zoom::default()), None);
    }
}
