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
                if draft.setup.branch != draft.setup.trunk {
                    return;
                }
                self.branch_editor = Some(BranchEditor {
                    draft: draft.id,
                    editor: line_edit::LineEdit::default(),
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
        if let Some(picker) = self.picker.as_mut() {
            match key.code {
                KeyCode::Esc => self.picker = None,
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
            if key.code == KeyCode::Esc {
                self.branch_editor = None;
                return Some(None);
            }
            if key.code == KeyCode::Enter {
                let editor = self.branch_editor.take().expect("branch editor");
                let draft = self
                    .drafts
                    .iter_mut()
                    .find(|draft| draft.id == editor.draft)?;
                if editor.editor.is_empty() {
                    return Some(None);
                }
                draft.working = true;
                draft.error = None;
                return Some(Some(Action::CreateWorktree {
                    draft_id: draft.id,
                    cwd: draft.setup.cwd.clone(),
                    branch: editor.editor.text().to_owned(),
                }));
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
                render::line(
                    Line::from(format!(
                        "{} {value}",
                        if index == picker.selected { "›" } else { " " }
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
        if let Some(editor) = &self.branch_editor
            && let Some((rect, _)) = self
                .viewport
                .borrow()
                .field_targets
                .iter()
                .find(|(_, field)| *field == Field::Branch)
        {
            render::render_input_row("Branch › ", &editor.editor, *rect, buf);
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
            trunk: "main".into(),
        });
        view
    }
    #[test]
    fn picker_changes_only_selected_draft_field_and_can_cancel() {
        let mut view = draft_view();
        view.open_field(Field::Model);
        assert!(view.picker.is_some());
        assert_eq!(view.apply_choice(1), None);
        assert_eq!(
            view.selected_draft().unwrap().setup.model.as_deref(),
            Some("haiku")
        );
        assert_eq!(view.selected_draft().unwrap().setup.effort, None);
        view.open_field(Field::Effort);
        view.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        view.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Right),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert!(view.picker.is_none());
        assert!(view.selected_draft().unwrap().setup.effort.is_none());
    }
    #[test]
    fn branch_editor_only_opens_for_trunk_draft_and_cancels_without_mutation() {
        let mut view = draft_view();
        view.open_field(Field::Branch);
        assert!(view.branch_editor.is_some());
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.branch_editor.is_none());
        assert_eq!(view.selected_draft().unwrap().setup.branch, "main");
        view.drafts[0].setup.branch = "feature".into();
        view.open_field(Field::Branch);
        assert!(view.branch_editor.is_none());
    }
    #[test]
    fn branch_enter_emits_worktree_for_stable_draft_and_keeps_request_directory() {
        let mut view = draft_view();
        let draft = view.selected_draft().unwrap();
        let id = draft.id;
        let cwd = draft.setup.cwd.clone();
        view.open_field(Field::Branch);
        for ch in "feature/input".chars() {
            view.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::CreateWorktree {
                draft_id: id,
                cwd,
                branch: "feature/input".into()
            })
        );
        assert!(view.selected_draft().unwrap().working);
        assert_eq!(
            view.input_disabled(),
            Some("Creating task; waiting for session")
        );
    }
}
