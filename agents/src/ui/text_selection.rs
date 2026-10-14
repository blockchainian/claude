use super::*;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Panel {
    Details,
    Log,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Anchor {
    segment: usize,
    offset: usize,
}

#[derive(Clone)]
pub(super) struct Source {
    pub id: usize,
    pub text: String,
    pub label: String,
}

#[derive(Clone)]
pub(super) struct DrawnRow {
    pub segment: usize,
    pub x: u16,
    pub y: u16,
    /// Screen columns relative to x, paired with original source byte ranges.
    pub cells: Vec<(Range<u16>, Range<usize>)>,
}

#[derive(Default)]
pub(super) struct TextRegion {
    pub area: Rect,
    pub sources: Vec<Source>,
    pub rows: Vec<DrawnRow>,
}

impl TextRegion {
    fn hit(&self, x: u16, y: u16, strict: bool) -> Option<Anchor> {
        let row = if strict {
            if !self.area.contains((x, y).into()) {
                return None;
            }
            self.rows.iter().find(|row| row.y == y)?
        } else {
            self.rows
                .iter()
                .filter(|row| !row.cells.is_empty())
                .min_by_key(|row| row.y.abs_diff(y))?
        };
        let column = x.saturating_sub(row.x);
        if let Some((_, bytes)) = row.cells.iter().find(|(cells, _)| cells.contains(&column)) {
            return Some(Anchor {
                segment: row.segment,
                offset: bytes.start,
            });
        }
        if strict {
            return None;
        }
        let first = row.cells.first()?;
        let last = row.cells.last()?;
        let offset = if column < first.0.start {
            first.1.start
        } else if column >= last.0.end {
            last.1.end
        } else {
            row.cells
                .iter()
                .find(|(cells, _)| cells.start > column)?
                .1
                .start
        };
        Some(Anchor {
            segment: row.segment,
            offset,
        })
    }
}

pub(super) struct Selection {
    panel: Panel,
    start: Anchor,
    end: Anchor,
    pointer: (u16, u16),
    dragged: bool,
    sources: Vec<Source>,
}

impl Selection {
    fn bounds(&self) -> (Anchor, Anchor) {
        (self.start.min(self.end), self.start.max(self.end))
    }

    fn range(&self, source: &Source) -> Option<Range<usize>> {
        let (start, end) = self.bounds();
        if !self.dragged || start == end || source.id < start.segment || source.id > end.segment {
            return None;
        }
        let from = if source.id == start.segment {
            start.offset
        } else {
            0
        };
        let to = if source.id == end.segment {
            end.offset
        } else {
            source.text.len()
        };
        Some(from..to)
    }

    fn copy(&self) -> Option<Action> {
        let parts: Vec<_> = self
            .sources
            .iter()
            .filter_map(|source| {
                let range = self.range(source)?;
                Some(&source.text[range])
            })
            .collect();
        let text = parts.join("\n");
        if text.is_empty() {
            return None;
        }
        let label = if self.start.segment == self.end.segment {
            self.sources
                .iter()
                .find(|source| source.id == self.start.segment)?
                .label
                .clone()
        } else {
            "Copied selection".to_owned()
        };
        Some(Action::Copy { text, label })
    }
}

impl View {
    fn text_region(viewport: &Viewport, panel: Panel) -> &TextRegion {
        match panel {
            Panel::Details => &viewport.details_text,
            Panel::Log => &viewport.log_text,
        }
    }

    pub(super) fn begin_text_selection(&mut self, x: u16, y: u16) {
        let viewport = self.viewport.borrow();
        let selection = [Panel::Details, Panel::Log].into_iter().find_map(|panel| {
            let region = Self::text_region(&viewport, panel);
            let anchor = region.hit(x, y, true)?;
            Some(Selection {
                panel,
                start: anchor,
                end: anchor,
                pointer: (x, y),
                dragged: false,
                sources: region.sources.clone(),
            })
        });
        *self.text_selection.borrow_mut() = selection;
    }

    pub(super) fn extend_text_selection(&mut self, x: u16, y: u16) {
        let mut selection = self.text_selection.borrow_mut();
        let Some(selection) = selection.as_mut() else {
            return;
        };
        selection.dragged = true;
        selection.pointer = (x, y);
        if let Some(anchor) =
            Self::text_region(&self.viewport.borrow(), selection.panel).hit(x, y, false)
        {
            selection.end = anchor;
        }
        self.last_copy_click = None;
    }

    pub(super) fn finish_text_selection(&mut self, x: u16, y: u16) -> Option<Action> {
        if self
            .text_selection
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.dragged)
        {
            self.extend_text_selection(x, y);
        }
        self.text_selection.borrow_mut().take()?.copy()
    }

    pub(crate) fn text_selection_dragging(&self) -> bool {
        self.text_selection
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.dragged)
    }

    /// Move the held gesture's panel by one row; render remaps its endpoint afterward.
    pub(crate) fn tick_text_selection(&mut self) -> bool {
        let selection = self.text_selection.borrow();
        let Some(selection) = selection.as_ref().filter(|selection| selection.dragged) else {
            return false;
        };
        let mut viewport = self.viewport.borrow_mut();
        let area = Self::text_region(&viewport, selection.panel).area;
        if area.is_empty() {
            return false;
        }
        let forward = if selection.pointer.1 <= area.top() {
            false
        } else if selection.pointer.1 >= area.bottom().saturating_sub(1) {
            true
        } else {
            return false;
        };
        match selection.panel {
            Panel::Log => {
                let previous = viewport.log_open_scroll;
                viewport.log_open_scroll = if forward {
                    previous.saturating_add(1).min(viewport.log_open_max)
                } else {
                    previous.saturating_sub(1)
                };
                previous != viewport.log_open_scroll
            }
            Panel::Details => {
                let state = &mut viewport.details_scroll;
                if let Some(max) = state.fallback_max {
                    let previous = state.offset;
                    state.offset = if forward {
                        previous.saturating_add(1).min(max)
                    } else {
                        previous.saturating_sub(1)
                    };
                    return previous != state.offset;
                }
                let can_scroll = |index: usize| {
                    state
                        .reveal
                        .get(index)
                        .zip(state.hidden.get(index))
                        .is_some_and(|(reveal, hidden)| {
                            if forward {
                                reveal < hidden
                            } else {
                                *reveal > 0
                            }
                        })
                };
                let target = if can_scroll(selection.end.segment) {
                    selection.end.segment
                } else if let Some(target) =
                    (0..state.reveal.len()).find(|&index| can_scroll(index))
                {
                    target
                } else {
                    return false;
                };
                let (Some(reveal), Some(hidden)) =
                    (state.reveal.get_mut(target), state.hidden.get(target))
                else {
                    unreachable!("scroll target has a body budget");
                };
                let previous = *reveal;
                *reveal = if forward {
                    reveal.saturating_add(1).min(*hidden)
                } else {
                    reveal.saturating_sub(1)
                };
                previous != *reveal
            }
        }
    }

    pub(super) fn highlight_text_selection(&self, panel: Panel, buf: &mut Buffer) {
        let viewport = self.viewport.borrow();
        let region = Self::text_region(&viewport, panel);
        let mut selection = self.text_selection.borrow_mut();
        let Some(selection) = selection
            .as_mut()
            .filter(|selection| selection.panel == panel && selection.dragged)
        else {
            return;
        };
        if let Some(anchor) = region.hit(selection.pointer.0, selection.pointer.1, false) {
            selection.end = anchor;
        }
        for row in &region.rows {
            let Some(source) = selection
                .sources
                .iter()
                .find(|source| source.id == row.segment)
            else {
                continue;
            };
            let Some(range) = selection.range(source) else {
                continue;
            };
            for (cells, bytes) in &row.cells {
                if range.start < bytes.end && bytes.start < range.end {
                    for x in cells.clone() {
                        buf[(row.x + x, row.y)]
                            .modifier
                            .insert(ratatui::style::Modifier::REVERSED);
                    }
                }
            }
        }
    }
}
