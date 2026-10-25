use super::*;
use std::ops::Range;

#[derive(Clone)]
pub(super) struct Placed<P> {
    range: Range<usize>,
    payload: P,
}

impl<P: Placeholder> Editor<P> {
    /// Iterates over placeholder payloads in text order.
    pub fn placeholders(&self) -> impl Iterator<Item = &P> {
        self.placed.iter().map(|span| &span.payload)
    }

    /// Restores numbered labels as placeholders in order; a None payload removes its label.
    pub fn from_labeled_text(text: &str, payloads: impl IntoIterator<Item = Option<P>>) -> Self {
        let mut edit = Self::default();
        edit.set_text(text);
        let mut offset = 0;
        for (index, payload) in payloads.into_iter().enumerate() {
            let label = P::label(index + 1);
            let Some(position) = edit.text[offset..].find(&label) else {
                continue;
            };
            let start = offset + position;
            let end = start + label.len();
            match payload {
                Some(payload) => {
                    edit.placed.push(Placed {
                        range: start..end,
                        payload,
                    });
                    offset = end;
                }
                None => {
                    edit.text.replace_range(start..end, "");
                    offset = start;
                }
            }
        }
        edit.end();
        edit.relabel_placeholders();
        edit
    }

    /// Replaces the selection with a placeholder and numbers labels in text order.
    pub fn insert_placeholder(&mut self, payload: P) {
        self.delete_selection();
        let number = self.placed.len() + 1;
        let label = P::label(number);
        let start = self.cursor;
        self.insert(&label);
        self.placed.push(Placed {
            range: start..start + label.len(),
            payload,
        });
        self.placed.sort_by_key(|span| span.range.start);
        self.align_cursor();
    }

    /// Returns the text with all placeholder labels removed.
    pub fn text_without_placeholders(&self) -> String {
        let mut text = self.text.clone();
        for span in self.placed.iter().rev() {
            text.replace_range(span.range.clone(), "");
        }
        text
    }

    /// Appends another editor at the end, carrying its placeholders and clearing selection.
    pub fn append(&mut self, editor: Self) {
        self.clear_selection();
        self.end();
        let offset = self.cursor;
        self.insert(&editor.text);
        self.placed
            .extend(editor.placed.into_iter().map(|mut span| {
                span.range.start += offset;
                span.range.end += offset;
                span
            }));
        self.align_cursor();
    }

    pub(super) fn relabel_placeholders(&mut self) {
        for index in 0..self.placed.len() {
            let range = self.placed[index].range.clone();
            let label = P::label(index + 1);
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
            self.placed[index].range.end = range.start + label.len();
            for span in &mut self.placed[index + 1..] {
                span.range.start = adjust(span.range.start);
                span.range.end = adjust(span.range.end);
            }
        }
    }

    pub(super) fn placeholder_caret(&self, offset: usize) -> usize {
        let start = self.placeholder_boundary(offset, false);
        let end = self.placeholder_boundary(offset, true);
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
                self.placed
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

    pub(super) fn placeholder_boundary(&self, cursor: usize, forward: bool) -> usize {
        self.placed
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

    pub(super) fn replace_placeholders(&mut self, range: Range<usize>, text: &str) -> Range<usize> {
        let range = self.placeholder_boundary(range.start, false)
            ..self.placeholder_boundary(range.end, true);
        self.placed.retain_mut(|span| {
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
