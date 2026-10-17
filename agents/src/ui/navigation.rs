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

pub(super) enum CenterRow {
    Group(usize),
    DraftGroup(crate::model::DraftId),
    Draft(crate::model::DraftId),
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
        if self.status_filter == 0 {
            for draft in &self.drafts {
                let group = entries.iter().position(|entry| match entry {
                    CenterRow::Group(index) => {
                        self.project_groups[*index].heading == draft.heading()
                    }
                    CenterRow::DraftGroup(id) => self
                        .drafts
                        .iter()
                        .any(|other| other.id == *id && other.heading() == draft.heading()),
                    _ => false,
                });
                if let Some(group) = group {
                    let next = entries
                        .iter()
                        .enumerate()
                        .skip(group + 1)
                        .find(|(_, entry)| {
                            matches!(
                                entry,
                                CenterRow::Group(_) | CenterRow::DraftGroup(_) | CenterRow::Gap
                            )
                        })
                        .map_or(entries.len(), |(index, _)| index);
                    entries.insert(next, CenterRow::Draft(draft.id));
                } else {
                    if !entries.is_empty() {
                        entries.push(CenterRow::Gap);
                    }
                    entries.push(CenterRow::DraftGroup(draft.id));
                    entries.push(CenterRow::Draft(draft.id));
                }
            }
        }
        entries
    }

    pub(super) fn page_selection(&mut self, forward: bool) {
        if self.status_filter == 0 && !self.drafts.is_empty() {
            let entries = self.center_rows(&self.selectable_indices(), self.grouping);
            let targets: Vec<_> = entries
                .iter()
                .enumerate()
                .filter_map(|(position, row)| match row {
                    CenterRow::Task(index) => Some((position, Some(*index), None)),
                    CenterRow::Draft(id) => Some((position, None, Some(*id))),
                    CenterRow::ShowMore => Some((position, Some(usize::MAX), None)),
                    _ => None,
                })
                .collect();
            if targets.is_empty() {
                return;
            }
            let current = targets
                .iter()
                .find(|(_, index, draft)| {
                    if let Some(id) = draft {
                        self.selected_draft == Some(*id)
                    } else {
                        self.selected_draft.is_none() && *index == Some(self.selected)
                    }
                })
                .map_or(0, |(position, _, _)| *position);
            let height = self.viewport.borrow().page_height.max(1);
            let desired = if forward {
                current.saturating_add(height)
            } else {
                current.saturating_sub(height)
            };
            let target = if forward {
                targets
                    .iter()
                    .find(|(position, _, _)| *position >= desired)
                    .unwrap_or_else(|| targets.last().unwrap())
            } else {
                targets
                    .iter()
                    .rev()
                    .find(|(position, _, _)| *position <= desired)
                    .unwrap_or(&targets[0])
            };
            self.viewport.borrow_mut().scroll = target.0.min(entries.len().saturating_sub(height));
            if let Some(id) = target.2 {
                self.select_draft(id);
            } else if let Some(index) = target.1 {
                self.select(index);
            }
            return;
        }
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
