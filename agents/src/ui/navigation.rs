use super::*;

pub(super) enum CenterRow {
    Group(usize),
    Task(usize),
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
        self.selected = selected;
    }
}
