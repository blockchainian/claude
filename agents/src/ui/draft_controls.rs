use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Field {
    Model,
    Effort,
    Branch,
}
#[derive(Clone)]
pub(super) enum ControlTarget {
    Draft(crate::model::DraftId),
    Session(SessionKey),
}
pub(super) struct Picker {
    pub field: Field,
    pub target: ControlTarget,
    pub values: Vec<String>,
    pub selected: usize,
}
pub(super) struct BranchEditor {
    pub draft: crate::model::DraftId,
    pub editor: line_edit::LineEdit,
}

impl View {
    pub(super) fn open_field(&mut self, field: Field) {
        let (provider, target) = if let Some(draft) = self.selected_draft() {
            if draft.working || draft.job.is_some() {
                return;
            }
            if field == Field::Branch {
                if draft.setup.branch.is_empty() {
                    return;
                }
                let mut editor = line_edit::LineEdit::default();
                editor.set_text(draft.new_branch.as_deref().unwrap_or_default());
                self.branch_editor = Some(BranchEditor {
                    draft: draft.id,
                    editor,
                });
                self.input_box.focused = false;
                return;
            }
            (draft.setup.provider, ControlTarget::Draft(draft.id))
        } else if let Some(session) = self.selected_row() {
            if field == Field::Branch
                || !session.is_live_background()
                || session.input_disabled_reason().is_some()
            {
                return;
            }
            (
                session.id.provider,
                ControlTarget::Session(session.id.clone()),
            )
        } else {
            return;
        };
        let Some(choices) = self.input_choices.get(&provider) else {
            return;
        };
        let values = match field {
            Field::Model => choices.models.clone(),
            Field::Effort => choices.efforts.clone(),
            Field::Branch => return,
        };
        if values.is_empty() {
            return;
        }
        self.picker = Some(Picker {
            field,
            target,
            values,
            selected: 0,
        });
        self.input_box.focused = false;
    }
    pub(super) fn apply_choice(&mut self, index: usize) -> Option<Action> {
        let picker = self.picker.take()?;
        let value = picker.values.get(index)?.clone();
        match picker.target {
            ControlTarget::Draft(id) => {
                let draft = self.drafts.iter_mut().find(|draft| draft.id == id)?;
                match picker.field {
                    Field::Model => draft.setup.model = Some(value),
                    Field::Effort => draft.setup.effort = Some(value),
                    Field::Branch => {}
                };
                None
            }
            ControlTarget::Session(id) => match picker.field {
                Field::Model => Some(Action::SetModel { id, model: value }),
                Field::Effort => Some(Action::SetEffort { id, effort: value }),
                Field::Branch => None,
            },
        }
    }
    pub(super) fn control_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        let cancel = key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
        if cancel && (self.picker.is_some() || self.branch_editor.is_some()) {
            self.picker = None;
            if self.branch_editor.take().is_some() {
                self.input_box.focused = true;
            }
            return Some(None);
        }
        if let Some(picker) = self.picker.as_mut() {
            match key.code {
                KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
                KeyCode::Down => {
                    picker.selected = (picker.selected + 1).min(picker.values.len() - 1)
                }
                KeyCode::Enter => {
                    let selected = picker.selected;
                    return Some(self.apply_choice(selected));
                }
                _ => {}
            }
            return Some(None);
        }
        if self.branch_editor.is_some() {
            if key.code == KeyCode::Enter {
                let editor = self.branch_editor.take().expect("branch editor");
                self.input_box.focused = true;
                let draft = self
                    .drafts
                    .iter_mut()
                    .find(|draft| draft.id == editor.draft)?;
                let name = editor.editor.text().trim();
                draft.new_branch = (!name.is_empty()).then(|| name.to_owned());
                return Some(None);
            }
            self.branch_editor
                .as_mut()
                .expect("branch editor")
                .editor
                .handle(key);
            return Some(None);
        }
        None
    }
    pub(super) fn render_controls(&self, area: Rect, buf: &mut Buffer) {
        self.viewport.borrow_mut().picker_rows.clear();
        if let Some(picker) = &self.picker {
            let height = (picker.values.len() as u16).min(area.height);
            let rect = Rect::new(area.x, area.y, area.width, height);
            Clear.render(rect, buf);
            for (index, value) in picker.values.iter().take(usize::from(height)).enumerate() {
                let row = render::row(rect, index as u16, 1);
                if index == picker.selected {
                    render::fill_list_row(row, buf, style::selection_style());
                }
                render::line(
                    Line::from(format!(
                        "{} {value}",
                        if index == picker.selected {
                            PROMPT_MARKER
                        } else {
                            " "
                        }
                    ))
                    .style(if index == picker.selected {
                        style::selection_style()
                    } else {
                        Style::default()
                    }),
                    row,
                    buf,
                );
                self.viewport.borrow_mut().picker_rows.push((row, index));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft_view() -> View {
        let directory = tempfile::tempdir().unwrap();
        let mut view = View::default();
        view.set_input_choices(HashMap::from([(
            ProviderId::Claude,
            crate::providers::InputChoices {
                models: vec!["sonnet".into(), "haiku".into()],
                efforts: vec!["low".into(), "high".into()],
            },
        )]));
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: directory.path().into(),
            model: None,
            effort: None,
            branch: "main".into(),
        });
        view
    }
    #[test]
    fn branch_prompt_caret_and_picker_mouse_targets_match_cells() {
        let mut view = draft_view();
        let area = Rect::new(3, 2, 120, 40);
        let mut buffer = Buffer::empty(area);
        view.open_field(Field::Branch);
        view.branch_editor.as_mut().unwrap().editor.set_text("你好");
        view.render(area, &mut buffer);
        let prompt = view.center_layout(area).prompt;
        let rect = Rect::new(prompt.x + 1, prompt.y, prompt.width - 2, 1);
        assert_eq!(buffer[(rect.x + 11, rect.y)].symbol(), "❯");
        assert_eq!(buffer[(rect.x + 13, rect.y)].symbol(), "你");
        assert_eq!(view.cursor_pos(area), Some((rect.x + 17, rect.y)));
        view.branch_editor = None;
        view.open_field(Field::Model);
        view.render(area, &mut buffer);
        let (rect, index) = view.viewport.borrow().picker_rows[0];
        assert_eq!(index, 0);
        assert_eq!(buffer[(rect.x, rect.y)].symbol(), "❯");
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        };
        assert!(view.handle_mouse(click).is_none());
        assert!(
            view.handle_mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..click
            })
            .is_none()
        );
        assert!(view.picker.is_none());
        assert_eq!(view.drafts[0].setup.model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn plain_directory_draft_has_no_branch_control() {
        let mut view = draft_view();
        view.drafts[0].setup.branch.clear();
        view.open_field(Field::Branch);
        assert!(view.branch_editor.is_none());
        let area = Rect::new(0, 0, 120, 24);
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        assert!(
            view.viewport
                .borrow()
                .field_targets
                .iter()
                .all(|(_, field)| *field != Field::Branch)
        );
        view.open_field(Field::Model);
        assert!(view.picker.is_some());
        view.picker = None;
        view.open_field(Field::Effort);
        assert!(view.picker.is_some());
    }
}
