use super::*;
use std::collections::BTreeMap;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Panel {
    TaskDetails,
    Event,
    Tasks,
    EventList,
}

impl Panel {
    fn is_list(self) -> bool {
        matches!(self, Self::Tasks | Self::EventList)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Anchor {
    segment: usize,
    row: usize,
    offset: usize,
}

#[derive(Clone)]
pub(super) struct Source {
    pub id: usize,
    pub text: std::rc::Rc<str>,
    pub label: String,
}

#[derive(Clone, Default)]
pub(super) struct DrawnRow {
    pub segment: usize,
    pub index: usize,
    pub x: u16,
    pub y: u16,
    /// Screen columns relative to x, paired with original source byte ranges.
    pub cells: Vec<(Range<u16>, Range<usize>)>,
    /// None starts a display line; Some joins a soft wrap, restoring a removed space if true.
    pub continuation: Option<bool>,
    pub text: String,
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
        let first = row.cells.first()?.0.start;
        let last = row.cells.last()?.0.end;
        let column = x.saturating_sub(row.x);
        if strict && !row.cells.iter().any(|(cells, _)| cells.contains(&column)) {
            return None;
        }
        Some(Anchor {
            segment: row.segment,
            row: row.index,
            offset: usize::from(column.clamp(first, last)),
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
    /// Drawn rows retained while scrolling, in layout order within each source.
    rows: BTreeMap<usize, BTreeMap<usize, DrawnRow>>,
}

impl Selection {
    fn remember_rows(&mut self, region: &TextRegion) {
        for row in &region.rows {
            self.rows
                .entry(row.segment)
                .or_default()
                .insert(row.index, row.clone());
        }
    }

    fn bounds(&self) -> (Anchor, Anchor) {
        (self.start.min(self.end), self.start.max(self.end))
    }

    fn range(&self, row: &DrawnRow) -> Option<Range<usize>> {
        let (start, end) = self.bounds();
        let key = (row.segment, row.index);
        if !self.dragged
            || start == end
            || key < (start.segment, start.row)
            || key > (end.segment, end.row)
        {
            return None;
        }
        let from = if key == (start.segment, start.row) {
            start.offset
        } else {
            0
        };
        let to = if key == (end.segment, end.row) {
            end.offset
        } else {
            usize::MAX
        };
        Some(from..to)
    }

    fn copy(&self) -> Option<Action> {
        let parts: Vec<_> = self
            .sources
            .iter()
            .filter_map(|source| {
                let (start, end) = self.bounds();
                if source.id < start.segment || source.id > end.segment {
                    return None;
                }
                let mut text = String::new();
                let mut separator = String::new();
                for row in self
                    .rows
                    .get(&source.id)
                    .into_iter()
                    .flat_map(|rows| rows.values())
                {
                    let Some(range) = self.range(row) else {
                        continue;
                    };
                    if !text.is_empty() || (source.id > start.segment && row.cells.is_empty()) {
                        separator.push_str(match row.continuation {
                            None => "\n",
                            Some(true) => " ",
                            Some(false) => "",
                        });
                    }
                    let mut visible = String::new();
                    for (columns, _) in &row.cells {
                        if usize::from(columns.start) >= range.end
                            || usize::from(columns.end) <= range.start
                        {
                            continue;
                        }
                        let from = column_byte(&row.text, usize::from(columns.start), false);
                        let to = column_byte(&row.text, usize::from(columns.end), true);
                        visible.push_str(&row.text[from..to]);
                    }
                    if !visible.is_empty() {
                        text.push_str(&separator);
                        separator.clear();
                        text.push_str(&visible);
                    }
                }
                if self.panel.is_list() {
                    text.truncate(text.trim_end_matches(' ').len());
                }
                Some(text)
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
            Panel::TaskDetails => &viewport.task_details_text,
            Panel::Event => &viewport.event_text,
            Panel::Tasks => &viewport.task_text,
            Panel::EventList => &viewport.event_list_text,
        }
    }

    pub(super) fn cancel_event_detail_selection(&self) {
        if self
            .text_selection
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.panel == Panel::Event)
        {
            self.text_selection.borrow_mut().take();
        }
    }

    pub(super) fn begin_text_selection(&mut self, x: u16, y: u16) {
        let viewport = self.viewport.borrow();
        let selection = [
            Panel::TaskDetails,
            Panel::Event,
            Panel::Tasks,
            Panel::EventList,
        ]
        .into_iter()
        .find_map(|panel| {
            let region = Self::text_region(&viewport, panel);
            let anchor = region.hit(x, y, true)?;
            let mut selection = Selection {
                panel,
                start: anchor,
                end: anchor,
                pointer: (x, y),
                dragged: false,
                sources: region.sources.clone(),
                rows: BTreeMap::new(),
            };
            selection.remember_rows(region);
            Some(selection)
        });
        *self.text_selection.borrow_mut() = selection;
    }

    pub(super) fn extend_text_selection(&mut self, x: u16, y: u16) {
        let panel = {
            let mut selection = self.text_selection.borrow_mut();
            let Some(selection) = selection.as_mut() else {
                return;
            };
            if selection.panel.is_list() && !selection.dragged && selection.pointer == (x, y) {
                return;
            }
            selection.dragged = true;
            selection.pointer = (x, y);
            if let Some(anchor) =
                Self::text_region(&self.viewport.borrow(), selection.panel).hit(x, y, false)
            {
                selection.end = anchor;
            }
            selection.panel
        };
        self.last_copy_click = None;
        if panel.is_list() {
            self.last_click = None;
        }
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
        let mut detail = self.event_detail.scroll.borrow_mut();
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
            Panel::Tasks => {
                let previous = viewport.scroll;
                viewport.scroll = if forward {
                    previous.saturating_add(1).min(viewport.task_scroll_max)
                } else {
                    previous.saturating_sub(1)
                };
                previous != viewport.scroll
            }
            Panel::EventList => {
                let previous = self.event_list.scroll.get();
                self.event_list.scroll.set(if forward {
                    previous.saturating_add(1)
                } else {
                    previous.saturating_sub(1)
                });
                previous != self.event_list.scroll.get()
            }
            Panel::Event => {
                let previous = detail.offset;
                detail.offset = if forward {
                    previous.saturating_add(1).min(viewport.event_detail_max)
                } else {
                    previous.saturating_sub(1)
                };
                previous != detail.offset
            }
            Panel::TaskDetails => {
                let state = &mut viewport.task_details_scroll;
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
                let body = state.bodies.iter().position(|rows| {
                    rows.contains(
                        &(usize::from(selection.pointer.1.saturating_sub(area.y)) + state.offset),
                    )
                });
                let target = if let Some(body) = body.filter(|&body| can_scroll(body)) {
                    body
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
        let mut viewport = self.viewport.borrow_mut();
        let region = match panel {
            Panel::TaskDetails => &mut viewport.task_details_text,
            Panel::Event => &mut viewport.event_text,
            Panel::Tasks => &mut viewport.task_text,
            Panel::EventList => &mut viewport.event_list_text,
        };
        for row in &mut region.rows {
            row.text.clear();
            let mut x = row.x;
            while x < region.area.right() {
                let symbol = buf[(x, row.y)].symbol();
                row.text.push_str(symbol);
                x += symbol.width().max(1) as u16;
            }
        }
        let mut selection = self.text_selection.borrow_mut();
        let Some(selection) = selection
            .as_mut()
            .filter(|selection| selection.panel == panel)
        else {
            return;
        };
        selection.remember_rows(region);
        if panel.is_list() || panel == Panel::TaskDetails {
            for source in &region.sources {
                if let Some(cached) = selection
                    .sources
                    .iter_mut()
                    .find(|cached| cached.id == source.id)
                {
                    *cached = source.clone();
                } else {
                    selection.sources.push(source.clone());
                }
            }
            selection.sources.sort_by_key(|source| source.id);
        }
        if !selection.dragged {
            return;
        }
        if let Some(anchor) = region.hit(selection.pointer.0, selection.pointer.1, false) {
            selection.end = anchor;
        }
        for row in &region.rows {
            let Some(range) = selection.range(row) else {
                continue;
            };
            for (cells, _) in &row.cells {
                if range.start < usize::from(cells.end) && usize::from(cells.start) < range.end {
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

/// Snap a display-column boundary to a whole rendered grapheme.
fn column_byte(text: &str, column: usize, end: bool) -> usize {
    let mut x = 0;
    for (byte, grapheme) in text.grapheme_indices(true) {
        let next = x + grapheme.width();
        if column < next {
            return if end && column > x {
                byte + grapheme.len()
            } else {
                byte
            };
        }
        x = next;
    }
    text.len()
}

impl View {
    pub(super) fn cancel_list_selection(&self, panel: Panel) {
        if self.selecting_list(panel) {
            self.text_selection.borrow_mut().take();
        }
    }

    /// Keeps a list selection on its rows after their ids grew by `rows`.
    pub(super) fn shift_list_selection(&self, panel: Panel, rows: usize) {
        let mut selection = self.text_selection.borrow_mut();
        let Some(selection) = selection
            .as_mut()
            .filter(|selection| selection.panel == panel)
        else {
            return;
        };
        selection.start.segment += rows;
        selection.end.segment += rows;
        for source in &mut selection.sources {
            source.id += rows;
        }
        selection.rows = std::mem::take(&mut selection.rows)
            .into_iter()
            .map(|(id, mut cached)| {
                for row in cached.values_mut() {
                    row.segment += rows;
                }
                (id + rows, cached)
            })
            .collect();
    }

    pub(super) fn selecting_list(&self, panel: Panel) -> bool {
        self.text_selection
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.panel == panel)
    }

    /// Capture terminal cells before adding the gesture's highlight.
    pub(super) fn capture_list_text(
        &self,
        panel: Panel,
        area: Rect,
        first: usize,
        buf: &mut Buffer,
    ) {
        let mut region = TextRegion {
            area,
            ..TextRegion::default()
        };
        for y in area.top()..area.bottom() {
            let id = first + usize::from(y - area.y);
            let mut text = String::new();
            let mut cells = Vec::new();
            let mut x = area.x;
            while x < area.right() {
                let symbol = buf[(x, y)].symbol();
                let width = symbol.width().max(1) as u16;
                let byte = text.len();
                text.push_str(symbol);
                cells.push((
                    x - area.x..(x - area.x + width).min(area.width),
                    byte..text.len(),
                ));
                x += width;
            }
            region.sources.push(Source {
                id,
                text: text.into(),
                label: "Copied selection".into(),
            });
            region.rows.push(DrawnRow {
                segment: id,
                x: area.x,
                y,
                cells,
                ..Default::default()
            });
        }
        match panel {
            Panel::Tasks => self.viewport.borrow_mut().task_text = region,
            Panel::EventList => self.viewport.borrow_mut().event_list_text = region,
            _ => unreachable!("screen cells belong to a list"),
        }
        self.highlight_text_selection(panel, buf);
    }
}
