use super::*;
use std::cell::Cell;

#[derive(Default)]
pub(super) struct PromptMouse {
    anchor: Option<usize>,
    /// Wheel position with the caret and content length it belongs to.
    scroll: Cell<Option<(usize, usize, usize)>>,
}

impl PromptBox {
    pub(in crate::ui) fn paste_mouse(&mut self, area: Rect, label: &str, mouse: MouseEvent) {
        let layout = self.layout(area.width, label);
        let start = self.mouse_scroll_start(&layout, usize::from(area.height));
        let (width, inset) = self.text_width.get();
        self.editor.place_caret(
            line_edit::Wrap {
                width,
                first_row_inset: inset,
            },
            usize::from(mouse.column.saturating_sub(area.x)),
            start + usize::from(mouse.row.saturating_sub(area.y)),
        );
        self.mouse.anchor = None;
    }

    pub(in crate::ui) fn mouse_selecting(&self) -> bool {
        self.mouse.anchor.is_some()
    }

    pub(in crate::ui) fn select_mouse(
        &mut self,
        area: Rect,
        label: &str,
        mouse: MouseEvent,
    ) -> Option<Action> {
        if area.is_empty() {
            return None;
        }
        let layout = self.layout(area.width, label);
        let start = self.mouse_scroll_start(&layout, usize::from(area.height));
        let (width, inset) = self.text_width.get();
        let positions = self.editor.caret_stops(line_edit::Wrap {
            width,
            first_row_inset: inset,
        });
        let row = (start + usize::from(mouse.row.saturating_sub(area.y)))
            .min(positions.last().expect("end caret").row);
        let column = usize::from(mouse.column.saturating_sub(area.x));
        let offset = positions
            .iter()
            .rfind(|p| p.row == row && p.column <= column)
            .or_else(|| positions.iter().find(|p| p.row == row))
            .expect("visual row has caret")
            .byte;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.editor.set_cursor(offset);
                self.mouse.anchor = Some(offset);
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                let anchor = self.mouse.anchor?;
                self.editor.select(anchor, offset);
            }
            _ => return None,
        }
        self.mouse.scroll.set(Some((
            start,
            self.editor.cursor(),
            self.editor.text().len(),
        )));
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            self.mouse.anchor = None;
            return self.editor.selected_text().map(|text| Action::Copy {
                text: text.to_owned(),
                label: "Copied selection".into(),
            });
        }
        None
    }

    pub(super) fn highlight_mouse_selection(&self, area: Rect, buf: &mut Buffer, start: usize) {
        use unicode_segmentation::UnicodeSegmentation;
        let Some(range) = self.editor.selection_range() else {
            return;
        };
        let (width, inset) = self.text_width.get();
        for line_edit::CaretStop { byte, column, row } in self.editor.caret_stops(line_edit::Wrap {
            width,
            first_row_inset: inset,
        }) {
            if !range.contains(&byte) || row < start || row >= start + usize::from(area.height) {
                continue;
            }
            let grapheme = self.editor.text()[byte..]
                .graphemes(true)
                .next()
                .expect("selected grapheme");
            let x = area.x + column as u16;
            for x in x..(x + grapheme.width().max(1) as u16).min(area.right()) {
                buf[(x, area.y + (row - start) as u16)]
                    .modifier
                    .insert(ratatui::style::Modifier::REVERSED);
            }
        }
    }

    pub(super) fn mouse_scroll_start(&self, layout: &PromptLayout, height: usize) -> usize {
        let maximum = layout.lines.len().saturating_sub(height);
        if let Some((start, cursor, length)) = self.mouse.scroll.get()
            && cursor == self.editor.cursor()
            && length == self.editor.text().len()
        {
            return start.min(maximum);
        }
        self.mouse.scroll.set(None);
        // The tail of the prompt shows unless the caret sits above it.
        layout.caret.1.min(maximum)
    }

    pub(in crate::ui) fn scroll_mouse(&self, area: Rect, label: &str, down: bool) {
        if area.is_empty() {
            return;
        }
        let layout = self.layout(area.width, label);
        let height = usize::from(area.height);
        let start = self.mouse_scroll_start(&layout, height);
        let start = if down {
            start
                .saturating_add(1)
                .min(layout.lines.len().saturating_sub(height))
        } else {
            start.saturating_sub(1)
        };
        self.mouse.scroll.set(Some((
            start,
            self.editor.cursor(),
            self.editor.text().len(),
        )));
    }
}
