use super::navigation::ListRow;
use super::*;
use crate::model::DraftId;
use crate::repository::{BrowseEntry, browse_entries};
use crate::tasks::BranchChoice;
use std::cell::Cell;
use std::fs;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Field {
    Model,
    Effort,
    Branch,
    Directory,
}
#[derive(Clone)]
pub(super) enum ControlTarget {
    Draft(DraftId),
    Session(SessionKey),
}

/// The list the details panel shows to choose a new task's repository or branch, or a task's
/// model or effort. Repository clicks apply immediately; other lists apply on double click
/// or Enter. A right click or Esc cancels.
pub(super) struct Selector {
    pub kind: SelectorKind,
    pub selected: usize,
    pub scroll: Cell<usize>,
}

pub(super) enum SelectorKind {
    Repository {
        draft: DraftId,
        dir: PathBuf,
        entries: Vec<BrowseEntry>,
    },
    /// The repository's branches, then the operation that names a new one.
    Branch {
        draft: DraftId,
        branches: Vec<String>,
        editor: Option<line_edit::LineEdit>,
    },
    Model {
        target: ControlTarget,
        values: Vec<String>,
    },
    Effort {
        target: ControlTarget,
        values: Vec<String>,
    },
}

const CREATE_BRANCH: &str = "Create new branch";

impl Selector {
    pub(super) fn new(kind: SelectorKind, selected: usize) -> Self {
        Self {
            kind,
            selected,
            scroll: Cell::new(0),
        }
    }

    pub(super) fn len(&self) -> usize {
        match &self.kind {
            SelectorKind::Repository { entries, .. } => entries.len(),
            SelectorKind::Branch { branches, .. } => branches.len() + 1,
            SelectorKind::Model { values, .. } | SelectorKind::Effort { values, .. } => {
                values.len()
            }
        }
    }

    #[cfg(test)]
    pub(super) fn field(&self) -> Field {
        match &self.kind {
            SelectorKind::Repository { .. } => Field::Directory,
            SelectorKind::Branch { .. } => Field::Branch,
            SelectorKind::Model { .. } => Field::Model,
            SelectorKind::Effort { .. } => Field::Effort,
        }
    }

    pub(super) fn editing(&self) -> bool {
        matches!(
            &self.kind,
            SelectorKind::Branch {
                editor: Some(_),
                ..
            }
        )
    }

    pub(super) fn branch_editor(&self) -> Option<&line_edit::LineEdit> {
        match &self.kind {
            SelectorKind::Branch { editor, .. } => editor.as_ref(),
            _ => None,
        }
    }

    /// The choice that opens the new branch's name editor.
    pub(super) fn create_index(&self) -> Option<usize> {
        match &self.kind {
            SelectorKind::Branch { branches, .. } => Some(branches.len()),
            _ => None,
        }
    }

    fn draft(&self) -> Option<DraftId> {
        match &self.kind {
            SelectorKind::Repository { draft, .. } | SelectorKind::Branch { draft, .. } => {
                Some(*draft)
            }
            SelectorKind::Model { target, .. } | SelectorKind::Effort { target, .. } => {
                match target {
                    ControlTarget::Draft(draft) => Some(*draft),
                    ControlTarget::Session(_) => None,
                }
            }
        }
    }

    fn rows(&self) -> Vec<ListRow> {
        let name = |path: &Path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        };
        match &self.kind {
            SelectorKind::Repository { dir, entries, .. } => {
                let mut rows = vec![ListRow::Text(Line::from(
                    super::formatting::format_directory_display(dir).dim(),
                ))];
                rows.extend(entries.iter().enumerate().map(|(index, entry)| {
                    let text = match entry {
                        BrowseEntry::Parent(_) => Line::from("..".dim()),
                        BrowseEntry::Folder(path) => {
                            Line::from(Span::styled(name(path), style::time_style()))
                        }
                        BrowseEntry::Repository(path) => Line::from(name(path)),
                    };
                    ListRow::Choice(index, text)
                }));
                rows
            }
            SelectorKind::Branch { branches, .. } => {
                let mut rows: Vec<_> = branches
                    .iter()
                    .enumerate()
                    .map(|(index, branch)| ListRow::Choice(index, Line::from(branch.clone())))
                    .collect();
                rows.push(ListRow::Text(Line::default()));
                rows.push(ListRow::Choice(branches.len(), Line::from(CREATE_BRANCH)));
                rows
            }
            SelectorKind::Model { values, .. } | SelectorKind::Effort { values, .. } => values
                .iter()
                .enumerate()
                .map(|(index, value)| ListRow::Choice(index, Line::from(value.clone())))
                .collect(),
        }
    }
}

impl View {
    /// Opens the repository browser for the selected draft at `dir`, selecting `current` if listed.
    pub(super) fn open_repositories(&mut self, dir: PathBuf, current: Option<&Path>) {
        let Some(draft) = self.selected_draft else {
            return;
        };
        let entries = browse_entries(&dir);
        let current = current.and_then(|current| fs::canonicalize(current).ok());
        let selected = entries
            .iter()
            .position(|entry| {
                matches!(entry, BrowseEntry::Repository(path)
                    if fs::canonicalize(path).ok() == current)
            })
            .unwrap_or_default();
        self.selector = Some(Selector::new(
            SelectorKind::Repository {
                draft,
                dir,
                entries,
            },
            selected,
        ));
    }

    pub(super) fn open_field(&mut self, field: Field) {
        if self.selector.as_ref().is_some_and(Selector::editing) {
            self.close_selector();
        }
        let (provider, target, model) = if let Some(draft) = self.selected_draft() {
            if draft.working || draft.job.is_some() {
                return;
            }
            match field {
                Field::Directory => {
                    let repository = crate::repository::repository_identity(&draft.setup.cwd)
                        .map_or_else(|| draft.setup.cwd.clone(), |identity| identity.primary_root);
                    let dir = repository
                        .parent()
                        .map_or_else(|| repository.clone(), Path::to_path_buf);
                    self.open_repositories(dir, Some(&repository));
                    self.focus_selector();
                    return;
                }
                Field::Branch => {
                    if draft.setup.branch.is_empty() {
                        return;
                    }
                    let Ok(branches) = crate::tasks::local_branches(&draft.setup.cwd) else {
                        return;
                    };
                    let current = draft
                        .branch_choice
                        .as_ref()
                        .map_or(draft.setup.branch.as_str(), BranchChoice::name);
                    let selected = branches
                        .iter()
                        .position(|branch| branch == current)
                        .unwrap_or_default();
                    self.selector = Some(Selector::new(
                        SelectorKind::Branch {
                            draft: draft.id,
                            branches,
                            editor: None,
                        },
                        selected,
                    ));
                    self.focus_selector();
                    return;
                }
                Field::Model | Field::Effort => {}
            }
            (
                draft.setup.provider,
                ControlTarget::Draft(draft.id),
                draft.setup.model.clone(),
            )
        } else if let Some(session) = self.selected_row() {
            if matches!(field, Field::Branch | Field::Directory)
                || !session.is_live_background()
                || session.input_disabled_reason().is_some()
            {
                return;
            }
            (
                session.id.provider,
                ControlTarget::Session(session.id.clone()),
                session.model.clone(),
            )
        } else {
            return;
        };
        let Some(choices) = self.input_choices.get(&provider) else {
            return;
        };
        let current_model = model.as_deref().and_then(|model| choices.model(model));
        let (values, current) = match field {
            Field::Model => (
                choices
                    .models
                    .iter()
                    .map(|model| model.id.clone())
                    .collect::<Vec<_>>(),
                current_model.map(|model| model.id.clone()),
            ),
            _ => (
                current_model
                    .map(|model| model.efforts.clone())
                    .unwrap_or_default(),
                match &target {
                    ControlTarget::Draft(_) => self
                        .selected_draft()
                        .and_then(|draft| draft.setup.effort.clone()),
                    ControlTarget::Session(_) => self
                        .selected_row()
                        .and_then(|session| session.effort.clone()),
                },
            ),
        };
        if values.is_empty() {
            return;
        }
        let selected = current
            .and_then(|current| values.iter().position(|value| *value == current))
            .unwrap_or_default();
        let kind = if field == Field::Model {
            SelectorKind::Model { target, values }
        } else {
            SelectorKind::Effort { target, values }
        };
        self.selector = Some(Selector::new(kind, selected));
        self.focus_selector();
    }

    /// Gives the selector the keys, as a click in the details panel does.
    fn focus_selector(&mut self) {
        self.mouse_focus = Panel::Details;
        self.input_box.focused = false;
    }

    /// Closes the selector; a draft's keys go back to its prompt.
    pub(super) fn close_selector(&mut self) {
        self.selector = None;
        if self.selected_draft.is_some() {
            self.mouse_focus = Panel::Prompt;
            self.input_box.focused = true;
        }
    }

    /// Drops a selector that belongs to `draft`.
    pub(super) fn close_draft_selector(&mut self, draft: DraftId) {
        if self
            .selector
            .as_ref()
            .is_some_and(|selector| selector.draft() == Some(draft))
        {
            self.selector = None;
        }
    }

    fn enter_folder(&mut self, dir: PathBuf) {
        if let Some(Selector {
            kind:
                SelectorKind::Repository {
                    dir: browsed,
                    entries,
                    ..
                },
            selected,
            scroll,
        }) = &mut self.selector
        {
            *entries = browse_entries(&dir);
            *browsed = dir;
            *selected = 0;
            scroll.set(0);
        }
    }

    /// A single click's effect once no second click followed: folders open, and the branch list's
    /// last row starts naming a new branch.
    pub(super) fn single_choice(&mut self, index: usize) -> Option<Action> {
        let selector = self.selector.as_mut()?;
        match &mut selector.kind {
            SelectorKind::Repository { entries, .. } => match entries.get(index)? {
                BrowseEntry::Parent(dir) | BrowseEntry::Folder(dir) => {
                    let dir = dir.clone();
                    self.enter_folder(dir);
                }
                BrowseEntry::Repository(_) => {}
            },
            SelectorKind::Branch {
                branches, editor, ..
            } if index == branches.len() => {
                editor.get_or_insert_with(line_edit::LineEdit::default);
                self.focus_selector();
            }
            _ => {}
        }
        None
    }

    /// Applies the choice at `index`: a double click, or Enter.
    pub(super) fn apply_choice(&mut self, index: usize) -> Option<Action> {
        let selector = self.selector.as_ref()?;
        match &selector.kind {
            SelectorKind::Repository { draft, entries, .. } => match entries.get(index)? {
                BrowseEntry::Parent(_) | BrowseEntry::Folder(_) => self.single_choice(index),
                BrowseEntry::Repository(cwd) => {
                    let action = Action::ChangeDraftDirectory {
                        draft_id: *draft,
                        cwd: cwd.clone(),
                    };
                    self.close_selector();
                    Some(action)
                }
            },
            SelectorKind::Branch {
                draft, branches, ..
            } => {
                let Some(branch) = branches.get(index).cloned() else {
                    return self.single_choice(index);
                };
                let draft = *draft;
                self.close_selector();
                let draft = self.drafts.iter_mut().find(|item| item.id == draft)?;
                draft.branch_choice =
                    (branch != draft.setup.branch).then_some(BranchChoice::Existing(branch));
                None
            }
            SelectorKind::Model { target, values } | SelectorKind::Effort { target, values } => {
                let value = values.get(index)?.clone();
                let target = target.clone();
                let model = matches!(selector.kind, SelectorKind::Model { .. });
                self.close_selector();
                match target {
                    ControlTarget::Draft(id) => {
                        let draft = self.drafts.iter_mut().find(|draft| draft.id == id)?;
                        if model {
                            draft.setup.model = Some(value);
                        } else {
                            draft.setup.effort = Some(value);
                        }
                        None
                    }
                    ControlTarget::Session(id) => Some(if model {
                        Action::SetModel { id, model: value }
                    } else {
                        Action::SetEffort { id, effort: value }
                    }),
                }
            }
        }
    }

    /// Handles branch naming and navigation in the details selector.
    pub(super) fn selector_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        let selector = self.selector.as_mut()?;
        if selector.editing() {
            match key.code {
                KeyCode::Enter if key.modifiers.is_empty() => {
                    let name = selector.branch_editor()?.text().trim().to_owned();
                    if !name.is_empty() {
                        let draft = selector.draft()?;
                        self.close_selector();
                        if let Some(draft) = self.drafts.iter_mut().find(|item| item.id == draft) {
                            draft.branch_choice = Some(BranchChoice::New(name));
                        }
                    }
                }
                KeyCode::Esc => self.close_selector(),
                _ => {
                    if let SelectorKind::Branch {
                        editor: Some(editor),
                        ..
                    } = &mut selector.kind
                    {
                        editor.handle(key);
                    }
                }
            }
            return Some(None);
        }
        // The selector has the keys only after a click in the details panel or on a border field.
        if self.input_box.focused || self.mouse_focus != Panel::Details {
            return None;
        }
        let cancel = key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
        if cancel {
            self.close_selector();
            return Some(None);
        }
        if key.code == KeyCode::Enter {
            let selected = selector.selected;
            return Some(self.apply_choice(selected));
        }
        let code = super::navigation::list_navigation(key)?;
        let page = self.viewport.borrow().details.height.saturating_sub(1);
        selector.selected =
            super::navigation::step(selector.selected, selector.len(), usize::from(page), code);
        Some(None)
    }

    pub(super) fn render_controls(&self, area: Rect, buf: &mut Buffer) {
        self.viewport.borrow_mut().picker_rows.clear();
        let Some(selector) = &self.selector else {
            return;
        };
        Clear.render(area, buf);
        let rows = super::navigation::render_list(
            &selector.rows(),
            selector.selected,
            &selector.scroll,
            area,
            buf,
        );
        if let Some(editor) = selector.branch_editor()
            && let Some((rect, _)) = rows
                .iter()
                .find(|(_, index)| Some(*index) == selector.create_index())
        {
            let (text, _) = editor.window(usize::from(rect.width.saturating_sub(1)));
            Clear.render(*rect, buf);
            render::line(
                Line::from(format!("❯ {text}")),
                Rect::new(
                    rect.x.saturating_sub(1),
                    rect.y,
                    rect.width.saturating_add(1),
                    1,
                ),
                buf,
            );
        }
        self.viewport.borrow_mut().picker_rows = rows;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft_view(cwd: &Path, config: &Path) -> View {
        crate::providers::claude::write_fixture_cache(config);
        let mut view = View::default();
        view.set_input_choices(HashMap::from([(
            ProviderId::Claude,
            crate::providers::Provider::input_choices(
                &crate::providers::claude::ClaudeProvider::new(config.into()),
            ),
        )]));
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: cwd.into(),
            model: Some("claude-opus-5-5".into()),
            effort: Some("medium".into()),
            branch: "main".into(),
        });
        view
    }
    fn git(directory: &Path, arguments: &[&str]) {
        let output = std::process::Command::new("git")
            .args(arguments)
            .current_dir(directory)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    fn click(view: &mut View, rect: Rect) -> Option<Action> {
        let press = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        };
        assert!(view.handle_mouse(press).is_none());
        view.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            ..press
        })
    }
    fn row_of(view: &View, index: usize) -> Rect {
        view.viewport
            .borrow()
            .picker_rows
            .iter()
            .find(|(_, row)| *row == index)
            .unwrap()
            .0
    }
    fn screen(view: &View, area: Rect) -> String {
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        buffer
            .content
            .chunks(usize::from(area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn model_choices_select_on_click_apply_on_double_click_or_enter_and_cancel() {
        let directory = tempfile::tempdir().unwrap();
        let mut view = draft_view(directory.path(), directory.path());
        let area = Rect::new(3, 2, 160, 40);
        view.open_field(Field::Model);
        assert!(!view.input_box.focused);
        assert_eq!(view.selector.as_ref().unwrap().selected, 0);
        screen(&view, area);
        let sonnet = row_of(&view, 2);
        assert_eq!(click(&mut view, sonnet), None);
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        let later = Instant::now() + Duration::from_secs(1);
        assert_eq!(view.flush_pending_click(later), None);
        assert_eq!(
            view.drafts[0].setup.model.as_deref(),
            Some("claude-opus-5-5")
        );
        assert_eq!(click(&mut view, sonnet), None);
        assert_eq!(click(&mut view, sonnet), None);
        assert!(view.selector.is_none());
        assert!(view.input_box.focused);
        assert_eq!(
            view.drafts[0].setup.model.as_deref(),
            Some("claude-sonnet-5-5")
        );
        view.open_field(Field::Effort);
        assert_eq!(view.selector.as_ref().unwrap().selected, 1);
        for code in [KeyCode::Char('j'), KeyCode::Down] {
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.drafts[0].setup.effort.as_deref(), Some("high"));
        view.open_field(Field::Effort);
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.selector.is_none());
        assert_eq!(view.drafts.len(), 1);
        view.open_field(Field::Effort);
        screen(&view, area);
        let row = row_of(&view, 0);
        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Up(MouseButton::Right),
        ] {
            view.handle_mouse(MouseEvent {
                kind,
                column: row.x,
                row: row.y,
                modifiers: KeyModifiers::NONE,
            });
        }
        assert!(view.selector.is_none());
        assert_eq!(view.drafts[0].setup.effort.as_deref(), Some("high"));
    }

    #[test]
    fn branch_list_ends_with_a_spaced_create_row_that_names_a_new_branch() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repository");
        std::fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "-b", "main"]);
        git(
            &repository,
            &[
                "-c",
                "user.name=Probe",
                "-c",
                "user.email=probe@example.test",
                "commit",
                "--allow-empty",
                "-m",
                "probe",
            ],
        );
        git(&repository, &["branch", "feature/a"]);
        let mut view = draft_view(&repository, directory.path());
        view.handle_paste("unfinished task");
        let area = Rect::new(0, 0, 160, 40);
        view.open_field(Field::Branch);
        assert_eq!(view.selector.as_ref().unwrap().selected, 1);
        let rendered = screen(&view, area);
        let details = view.center_layout(area).details;
        let rows: Vec<_> = rendered
            .lines()
            .skip(usize::from(details.y))
            .take(4)
            .map(|line| {
                line.chars()
                    .skip(usize::from(details.x))
                    .take(usize::from(details.width))
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect();
        assert_eq!(rows, ["feature/a", "main", "", "Create new branch"]);
        let feature = row_of(&view, 0);
        click(&mut view, feature);
        click(&mut view, feature);
        assert_eq!(
            view.drafts[0].branch_choice,
            Some(BranchChoice::Existing("feature/a".into()))
        );
        view.open_field(Field::Branch);
        assert_eq!(view.selector.as_ref().unwrap().selected, 0);
        screen(&view, area);
        let create = row_of(&view, 2);
        click(&mut view, create);
        assert!(!view.selector.as_ref().unwrap().editing());
        view.flush_pending_click(Instant::now() + Duration::from_secs(1));
        assert!(view.selector.as_ref().unwrap().editing());
        assert!(!view.input_box.focused);
        assert_eq!(view.input_label(), "New task");
        assert_eq!(view.input_box.editor.text(), "unfinished task");
        for code in [KeyCode::Char('j'), KeyCode::Char('k')] {
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        view.handle_paste("你好");
        screen(&view, area);
        assert_eq!(view.cursor_pos(area).unwrap().1, create.y);
        let rendered = screen(&view, area);
        assert!(rendered.contains("❯ jk/你 好"), "{rendered}");
        assert!(!rendered.contains("New branch"));
        assert_eq!(view.input_box.editor.text(), "unfinished task");
        assert!(!rendered.contains("Create new branch"));
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(view.selector.is_none());
        assert!(view.input_box.focused);
        assert_eq!(view.input_box.editor.text(), "unfinished task");
        assert!(view.input_box.history.borrow().is_empty());
        assert_eq!(
            view.drafts[0].branch_choice,
            Some(BranchChoice::New("jk/你好".into()))
        );
        view.open_field(Field::Branch);
        let main = {
            screen(&view, area);
            row_of(&view, 1)
        };
        click(&mut view, main);
        click(&mut view, main);
        assert_eq!(view.drafts[0].branch_choice, None);
    }

    #[test]
    fn branch_input_enter_and_escape_preserve_draft_and_prompt() {
        for named in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut view = draft_view(directory.path(), directory.path());
            view.close_selector();
            if named {
                view.drafts[0].name = Some("task".into());
            }
            view.handle_paste("saved input");
            view.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
            let cursor = view.input_box.editor.cursor();
            let label = view.input_label();
            let draft = view.drafts[0].id;
            view.selector = Some(Selector::new(
                SelectorKind::Branch {
                    draft,
                    branches: vec!["main".into()],
                    editor: None,
                },
                1,
            ));
            view.focus_selector();
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert!(!view.input_box.focused);
            assert_eq!(view.input_label(), label);
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(view.input_label(), label);
            view.handle_paste("feature/input");
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(view.input_label(), label);
            assert_eq!(view.input_box.editor.text(), "saved input");
            assert_eq!(view.input_box.editor.cursor(), cursor);
            assert!(view.prompt_history.borrow().is_empty());
            view.selector = Some(Selector::new(
                SelectorKind::Branch {
                    draft,
                    branches: vec!["main".into()],
                    editor: None,
                },
                1,
            ));
            view.focus_selector();
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert_eq!(view.drafts.len(), 1);
            assert_eq!(view.input_box.editor.text(), "saved input");
            assert!(view.input_box.focused);
            assert!(view.selector.is_none());
        }
    }

    #[test]
    fn plain_directory_draft_has_no_branch_control() {
        let directory = tempfile::tempdir().unwrap();
        let mut view = draft_view(directory.path(), directory.path());
        view.drafts[0].setup.branch.clear();
        view.open_field(Field::Branch);
        assert_eq!(view.selector.as_ref().unwrap().field(), Field::Directory);
        let area = Rect::new(0, 0, 120, 24);
        screen(&view, area);
        assert!(
            view.viewport
                .borrow()
                .field_targets
                .iter()
                .all(|(_, field)| *field != Field::Branch)
        );
        view.open_field(Field::Model);
        assert!(view.selector.is_some());
        view.open_field(Field::Effort);
        assert!(view.selector.is_some());
    }
}
