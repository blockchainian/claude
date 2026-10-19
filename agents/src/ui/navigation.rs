use super::*;

/// Maps the dashboard list bindings to their navigation action.
pub(super) fn list_navigation(key: KeyEvent) -> Option<KeyCode> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('p' | 'k') => Some(KeyCode::Up),
            KeyCode::Char('n' | 'j') => Some(KeyCode::Down),
            KeyCode::Char('b') => Some(KeyCode::PageUp),
            KeyCode::Char('f') => Some(KeyCode::PageDown),
            _ => None,
        };
    }
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    match key.code {
        KeyCode::Char('k') => Some(KeyCode::Up),
        KeyCode::Char('j') => Some(KeyCode::Down),
        code @ (KeyCode::Up
        | KeyCode::Down
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Home
        | KeyCode::End) => Some(code),
        _ => None,
    }
}

/// The choice a list navigation key moves to among `len` choices: up and down wrap around.
pub(super) fn step(selected: usize, len: usize, page: usize, code: KeyCode) -> usize {
    if len == 0 {
        return 0;
    }
    match code {
        KeyCode::Up => (selected + len - 1) % len,
        KeyCode::Down => (selected + 1) % len,
        KeyCode::PageUp => selected.saturating_sub(page.max(1)),
        KeyCode::PageDown => (selected + page.max(1)).min(len - 1),
        KeyCode::Home => 0,
        KeyCode::End => len - 1,
        _ => selected,
    }
}

/// One row of a choice list: plain text or a choice.
pub(super) enum ListRow {
    Text(Line<'static>),
    Choice(usize, Line<'static>),
}

/// Draws `rows` from `scroll`, moved so the selected choice stays visible, highlighting it like
/// the task list's cursor row; returns each drawn choice's row.
pub(super) fn render_list(
    rows: &[ListRow],
    selected: usize,
    scroll: &std::cell::Cell<usize>,
    area: Rect,
    buf: &mut Buffer,
) -> Vec<(Rect, usize)> {
    let height = usize::from(area.height);
    let position = rows
        .iter()
        .position(|row| matches!(row, ListRow::Choice(index, _) if *index == selected))
        .unwrap_or_default();
    let mut start = scroll.get().min(rows.len().saturating_sub(height));
    if position < start {
        start = position;
    } else if position >= start + height {
        start = position + 1 - height;
    }
    scroll.set(start);
    let mut drawn = Vec::new();
    for (offset, row) in rows.iter().skip(start).take(height).enumerate() {
        let rect = render::row(area, offset as u16, 1);
        match row {
            ListRow::Text(text) => render::line(text.clone(), rect, buf),
            ListRow::Choice(index, text) => {
                let mut text = text.clone();
                if *index == selected {
                    render::fill_list_row(rect, buf, style::cursor_style());
                    text = text.patch_style(style::cursor_style());
                }
                render::line(text, rect, buf);
                drawn.push((rect, *index));
            }
        }
    }
    drawn
}

pub(super) enum CenterRow {
    Group(usize),
    Task(usize),
    Parent(usize),
    Gap,
    ShowMore,
}

impl View {
    pub(super) fn center_rows(&self, indices: &[usize], grouping: Grouping) -> Vec<CenterRow> {
        let mut entries = Vec::new();
        let mut previous = None;
        for index in indices.iter().copied() {
            if index == usize::MAX {
                entries.push(CenterRow::Gap);
                entries.push(CenterRow::ShowMore);
                continue;
            }
            if previous.is_none_or(|previous| !self.same_group(grouping, previous, index)) {
                if previous.is_some() {
                    entries.push(CenterRow::Gap);
                }
                entries.push(CenterRow::Group(index));
            }
            if let Some(parent) = self.group_index(index)
                && parent != index
                && previous.is_none_or(|previous| {
                    !self.same_group(grouping, previous, index)
                        || self.group_index(previous) != Some(parent)
                })
            {
                entries.push(CenterRow::Parent(parent));
            }
            entries.push(CenterRow::Task(index));
            previous = Some(index);
        }
        entries
    }

    pub(super) fn page_selection(&mut self, forward: bool) {
        let indices = self.selectable_indices();
        let mut state = self.viewport.borrow_mut();
        if indices.is_empty() {
            return;
        }
        let entries = self.center_rows(&indices, self.grouping);
        let tasks = entries
            .iter()
            .enumerate()
            .filter_map(|(position, row)| {
                if let CenterRow::Task(index) = row {
                    Some((position, *index))
                } else if matches!(row, CenterRow::ShowMore) {
                    Some((position, usize::MAX))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let current = tasks
            .iter()
            .find(|(_, index)| *index == self.selected)
            .map(|(position, _)| *position)
            .unwrap_or(tasks[0].0);
        let height = state.page_height.max(/*other*/ 1);
        let target = if forward {
            current.saturating_add(height)
        } else {
            current.saturating_sub(height)
        };
        let selected = if forward {
            tasks
                .partition_point(|(position, _)| *position < target)
                .min(tasks.len() - 1)
        } else {
            tasks
                .partition_point(|(position, _)| *position <= target)
                .saturating_sub(1)
        };
        let selected = tasks[selected].1;
        let scroll = state.scroll;
        state.scroll = (if forward {
            scroll.saturating_add(height)
        } else {
            scroll.saturating_sub(height)
        })
        .min(entries.len().saturating_sub(height));
        drop(state);
        self.select(selected);
    }
}
