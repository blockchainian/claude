use super::*;
use crate::images::ImageAttachment;
use std::{ops::Range, path::PathBuf};

#[derive(Clone, Debug)]
pub(super) struct ImageSpan {
    range: Range<usize>,
    image: ImageAttachment,
}

impl LineEdit {
    pub(crate) fn stored_input(&self) -> crate::state::StoredDraftInput {
        self.preserve_images();
        crate::state::StoredDraftInput {
            text: self.text.clone(),
            images: self.image_paths(),
        }
    }

    pub(crate) fn restore_input(input: &crate::state::StoredDraftInput) -> Self {
        let mut edit = Self::default();
        edit.set_text(&input.text);
        let mut offset = 0;
        for (index, path) in input.images.iter().enumerate() {
            let label = format!("[Image #{}]", index + 1);
            let Some(position) = edit.text[offset..].find(&label) else {
                continue;
            };
            let start = offset + position;
            let end = start + label.len();
            match ImageAttachment::from_path(path) {
                Ok(image) => {
                    edit.images.push(ImageSpan {
                        range: start..end,
                        image,
                    });
                    offset = end;
                }
                Err(_) => {
                    edit.text.replace_range(start..end, "");
                    offset = start;
                }
            }
        }
        edit.end();
        edit.relabel_images();
        edit
    }

    pub(crate) fn insert_image(&mut self, image: ImageAttachment) {
        self.delete_selection();
        let number = self.images.len() + 1;
        let label = format!("[Image #{number}]");
        let start = self.cursor;
        self.insert(&label);
        self.images.push(ImageSpan {
            range: start..start + label.len(),
            image,
        });
        self.images.sort_by_key(|span| span.range.start);
        self.align_cursor();
    }

    pub(crate) fn image_paths(&self) -> Vec<PathBuf> {
        self.images
            .iter()
            .map(|span| span.image.path().to_owned())
            .collect()
    }

    pub(crate) fn preserve_images(&self) {
        for span in &self.images {
            span.image.preserve();
        }
    }

    pub(crate) fn prompt_text(&self) -> String {
        let mut text = self.text.clone();
        for span in self.images.iter().rev() {
            text.replace_range(span.range.clone(), "");
        }
        text
    }

    pub(crate) fn append_editor(&mut self, editor: Self) {
        self.clear_selection();
        self.end();
        let offset = self.cursor;
        self.insert(&editor.text);
        self.images
            .extend(editor.images.into_iter().map(|mut span| {
                span.range.start += offset;
                span.range.end += offset;
                span
            }));
        self.align_cursor();
    }

    pub(super) fn relabel_images(&mut self) {
        for index in 0..self.images.len() {
            let range = self.images[index].range.clone();
            let label = format!("[Image #{}]", index + 1);
            if self.text[range.clone()] == label {
                continue;
            }
            self.text.replace_range(range.clone(), &label);
            let adjust = |offset: usize| {
                if offset <= range.start {
                    offset
                } else if offset >= range.end {
                    offset - range.len() + label.len()
                } else {
                    range.start + label.len()
                }
            };
            self.cursor = adjust(self.cursor);
            self.selection = self
                .selection
                .map(|(anchor, end)| (adjust(anchor), adjust(end)));
            self.images[index].range.end = range.start + label.len();
            for span in &mut self.images[index + 1..] {
                span.range.start = adjust(span.range.start);
                span.range.end = adjust(span.range.end);
            }
        }
    }

    pub(super) fn image_caret(&self, offset: usize) -> usize {
        let start = self.image_boundary(offset, false);
        let end = self.image_boundary(offset, true);
        let boundary = if offset - start < end - offset {
            start
        } else {
            end
        };
        self.caret_boundary(boundary, boundary >= offset)
    }

    pub(super) fn caret_boundary(&self, offset: usize, forward: bool) -> usize {
        let mut positions = self
            .text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(self.text.len()))
            .filter(|&index| {
                self.images
                    .iter()
                    .all(|span| index <= span.range.start || index >= span.range.end)
            });
        if forward {
            positions
                .find(|&index| index >= offset)
                .unwrap_or(self.text.len())
        } else {
            positions.rfind(|&index| index <= offset).unwrap_or(0)
        }
    }

    pub(super) fn image_boundary(&self, cursor: usize, forward: bool) -> usize {
        self.images
            .iter()
            .find(|span| span.range.start < cursor && cursor < span.range.end)
            .map_or(cursor, |span| {
                if forward {
                    span.range.end
                } else {
                    span.range.start
                }
            })
    }

    pub(super) fn replace_images(&mut self, range: Range<usize>, text: &str) -> Range<usize> {
        let range = self.image_boundary(range.start, false)..self.image_boundary(range.end, true);
        self.images.retain_mut(|span| {
            if range.start < span.range.end && span.range.start < range.end {
                return false;
            }
            if span.range.start >= range.end {
                span.range.start = span.range.start - range.len() + text.len();
                span.range.end = span.range.end - range.len() + text.len();
            }
            true
        });
        self.text.replace_range(range.clone(), text);
        range
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_draft_images_keep_paths_and_restore_text_positions() {
        let mut edit = LineEdit::default();
        edit.insert("before ");
        edit.insert_image(
            ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap(),
        );
        edit.insert(" middle ");
        edit.insert_image(
            ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap(),
        );
        edit.insert(" after");
        let stored = edit.stored_input();
        let paths = stored.images.clone();
        drop(edit);
        assert!(paths.iter().all(|path| path.is_file()));
        let restored = LineEdit::restore_input(&stored);
        assert_eq!(restored.text(), stored.text);
        assert_eq!(restored.image_paths(), paths);
        assert_eq!(restored.prompt_text(), "before  middle  after");
        drop(restored);
        std::fs::remove_file(&paths[0]).unwrap();
        let restored = LineEdit::restore_input(&stored);
        assert_eq!(restored.text(), "before  middle [Image #1] after");
        assert_eq!(restored.image_paths(), vec![paths[1].clone()]);
        drop(restored);
        std::fs::remove_file(&paths[1]).unwrap();
    }
}

#[cfg(test)]
mod ordering_tests {
    use super::*;
    #[test]
    fn stored_images_restore_after_inserting_an_attachment_before_another() {
        let attach = || {
            ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap()
        };
        let mut editor = LineEdit::default();
        editor.insert("suffix");
        editor.insert_image(attach());
        editor.home();
        editor.insert_image(attach());
        let stored = editor.stored_input();
        let restored = LineEdit::restore_input(&stored);
        assert_eq!(restored.image_paths(), editor.image_paths());
        assert_eq!(restored.text(), editor.text());
        for path in stored.images {
            std::fs::remove_file(path).unwrap();
        }
    }
}
