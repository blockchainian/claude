use super::drafts::Step;
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
/// model or effort. Repository and branch clicks apply
/// immediately; other lists apply on double click or Enter. A right click or Esc cancels.
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
    /// The repository's branches above the input naming a new one, whose choice index is
    /// `branches.len()`.
    Branch {
        draft: DraftId,
        branches: Vec<String>,
        editor: line_edit::LineEdit,
        editing: bool,
    },
    Model {
        target: ControlTarget,
        values: Vec<String>,
        labels: Vec<String>,
    },
    Effort {
        target: ControlTarget,
        values: Vec<String>,
    },
}

const NEW_BRANCH: &str = "Create new branch";

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

    pub(super) fn branch_editor(&self) -> Option<&line_edit::LineEdit> {
        match &self.kind {
            SelectorKind::Branch {
                editor,
                editing: true,
                ..
            } => Some(editor),
            _ => None,
        }
    }

    pub(super) fn active_editor(&mut self) -> Option<&mut line_edit::LineEdit> {
        match &mut self.kind {
            SelectorKind::Branch {
                editor,
                editing: true,
                ..
            } => Some(editor),
            _ => None,
        }
    }

    pub(super) fn input_active(&self) -> bool {
        self.branch_editor().is_some()
    }

    /// The choice index of the new branch's input row.
    fn create_index(&self) -> Option<usize> {
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
            SelectorKind::Branch {
                branches, editing, ..
            } => {
                let mut rows: Vec<_> = branches
                    .iter()
                    .enumerate()
                    .map(|(index, branch)| ListRow::Choice(index, Line::from(branch.clone())))
                    .collect();
                if !editing {
                    rows.push(ListRow::Choice(branches.len(), Line::from(NEW_BRANCH)));
                }
                rows
            }
            SelectorKind::Model { labels: values, .. } | SelectorKind::Effort { values, .. } => {
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| ListRow::Choice(index, Line::from(value.clone())))
                    .collect()
            }
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

    /// Saves the submitted prompt and clears its box before choosing a branch.
    pub(super) fn start_branch_step(&mut self) -> Option<Action> {
        let id = self.selected_draft?;
        if self.selected_draft()?.name.is_none() {
            self.open_name(id);
            return None;
        }
        self.input_box.history = self.prompt_history.clone();
        let Some(prompt_box::PromptAction::Submit(_)) = self
            .input_box
            .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            return None;
        };
        let prompt = std::mem::take(&mut self.input_box.editor);
        self.input_box.submitting = false;
        self.drafts.iter_mut().find(|draft| draft.id == id)?.prompt = Some(prompt);
        self.set_step(id, Step::Branch);
        if self.open_branches(false) {
            None
        } else if self.selected_draft()?.setup.branch.is_empty() {
            self.create_task(id)
        } else {
            None
        }
    }

    fn set_step(&mut self, id: DraftId, step: Step) {
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) {
            draft.step = step;
        }
    }

    /// Lists local branches, selecting the current branch or the create button.
    fn open_branches(&mut self, select_current: bool) -> bool {
        let Some(draft) = self.selected_draft() else {
            return false;
        };
        if draft.setup.branch.is_empty() {
            return false;
        }
        let branches = match crate::tasks::local_branches(&draft.setup.cwd) {
            Ok(branches) => branches,
            Err(error) => {
                self.set_notice(Some(format!("Cannot read branches: {error}")));
                return false;
            }
        };
        let current = draft
            .branch_choice
            .as_ref()
            .map_or(draft.setup.branch.as_str(), BranchChoice::name);
        let selected = branches
            .iter()
            .position(|branch| select_current && branch == current)
            .unwrap_or(branches.len());
        let mut editor = line_edit::LineEdit::default();
        if let Some(BranchChoice::New(name)) = &draft.branch_choice {
            editor.set_text(name);
        }
        self.selector = Some(Selector::new(
            SelectorKind::Branch {
                draft: draft.id,
                branches,
                editor,
                editing: false,
            },
            selected,
        ));
        self.focus_selector();
        true
    }

    pub(super) fn open_name(&mut self, id: DraftId) {
        self.set_step(id, Step::Name);
        self.selector = None;
        if self.name_target != Some(id) {
            self.rename.clear();
        }
        self.rename_target = None;
        self.name_target = Some(id);
        self.focus_selector();
    }

    /// Records the chosen branch and creates a task once its prompt is saved.
    fn choose_branch(&mut self, id: DraftId, branch: String, new: bool) -> Option<Action> {
        let draft = self.drafts.iter_mut().find(|item| item.id == id)?;
        draft.branch_choice = if new {
            Some(BranchChoice::New(branch))
        } else {
            (branch != draft.setup.branch).then_some(BranchChoice::Existing(branch))
        };
        if draft.step == Step::Branch {
            self.create_task(id)
        } else {
            self.close_selector();
            None
        }
    }

    /// Creates the task from its saved name, prompt and chosen branch.
    fn create_task(&mut self, id: DraftId) -> Option<Action> {
        let draft = self.drafts.iter().find(|draft| draft.id == id)?;
        if draft.working || draft.job.is_some() {
            return None;
        }
        let name = draft.name.as_ref().expect("task name before creation");
        let prompt = draft.prompt.as_ref().expect("task prompt before creation");
        let action = Action::CreateTask {
            draft_id: id,
            provider: draft.setup.provider,
            branch: draft.branch_choice.clone(),
            request: crate::tasks::NewTask {
                cwd: draft.setup.cwd.clone(),
                name: Some(name.clone()),
                model: draft.setup.model.clone(),
                effort: draft.setup.effort.clone(),
                prompt: prompt.prompt_text(),
                images: prompt.image_paths(),
            },
        };
        self.submit_draft(id);
        Some(action)
    }

    /// Esc leaves branch editing first; the branch list cancels the draft.
    pub(super) fn cancel_selector(&mut self) {
        if let Some(Selector {
            kind: SelectorKind::Branch {
                editor, editing, ..
            },
            ..
        }) = &mut self.selector
        {
            if *editing {
                editor.clear();
                *editing = false;
                return;
            }
            self.discard_selected_draft();
            return;
        }
        self.close_selector();
    }

    pub(super) fn open_field(&mut self, field: Field) {
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
                    self.open_branches(true);
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
            let labels = values
                .iter()
                .map(|model| self.model_text(provider, model))
                .collect();
            SelectorKind::Model {
                target,
                values,
                labels,
            }
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

    /// Closes the selector; a draft's keys go back to its prompt, restarting its flow there.
    pub(super) fn close_selector(&mut self) {
        self.selector = None;
        if let Some(id) = self.selected_draft {
            if self.name_target == Some(id) {
                self.open_name(id);
                return;
            }
            self.set_step(id, Step::Prompt);
            self.mouse_focus = Panel::Prompt;
            self.input_box.focused = true;
        }
    }

    /// Drops a selector that belongs to `draft`.
    pub(super) fn close_draft_selector(&mut self, draft: DraftId) {
        if self.name_target == Some(draft) {
            self.name_target = None;
            self.rename.clear();
        }
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

    /// A single click's effect once no second click followed: folders open.
    pub(super) fn single_choice(&mut self, index: usize) -> Option<Action> {
        let selector = self.selector.as_mut()?;
        if let SelectorKind::Repository { entries, .. } = &mut selector.kind {
            match entries.get(index)? {
                BrowseEntry::Parent(dir) | BrowseEntry::Folder(dir) => {
                    let dir = dir.clone();
                    self.enter_folder(dir);
                }
                BrowseEntry::Repository(_) => {}
            }
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
                    let draft = *draft;
                    self.open_name(draft);
                    Some(action)
                }
            },
            SelectorKind::Branch {
                draft,
                branches,
                editor,
                editing,
            } => {
                let draft = *draft;
                if index == branches.len() && !editing {
                    if let SelectorKind::Branch { editing, .. } = &mut self.selector.as_mut()?.kind
                    {
                        *editing = true;
                    }
                    return None;
                }
                // The input row applies the typed name; an empty name stays put.
                let (branch, new) = match branches.get(index) {
                    Some(branch) => (branch.clone(), false),
                    None => (editor.text().trim().to_owned(), true),
                };
                if !branch.is_empty() {
                    return self.choose_branch(draft, branch, new);
                }
                None
            }
            SelectorKind::Model { target, values, .. }
            | SelectorKind::Effort { target, values } => {
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

    /// Handles typing, navigation and choice in the details selector.
    pub(super) fn selector_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        let selector = self.selector.as_mut()?;
        // The selector has the keys only after a click in the details panel or on a border field.
        if self.input_box.focused || self.mouse_focus != Panel::Details {
            return None;
        }
        let cancel = key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
        if cancel {
            self.cancel_selector();
            return Some(None);
        }
        if key.code == KeyCode::Enter {
            let selected = selector.selected;
            return Some(self.apply_choice(selected));
        }
        if let Some(editor) = selector.active_editor() {
            editor.handle(key);
            return Some(None);
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
        // The input row takes the panel's bottom row, below the list.
        let branch = selector.branch_editor().filter(|_| area.height > 0);
        let list = if branch.is_some() {
            render::row(area, 0, area.height - 1)
        } else {
            area
        };
        let mut rows = super::navigation::render_list(
            &selector.rows(),
            selector.selected,
            &selector.scroll,
            list,
            buf,
        );
        if let Some(editor) = branch {
            let row = super::log::search_input_rect(area);
            render::render_input_row(&format!("New branch {PROMPT_MARKER} "), editor, row, buf);
            rows.push((row, selector.create_index().expect("branch input choice")));
            self.viewport.borrow_mut().details_input = row;
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
        assert!(!view.input_box.focused);
        assert_eq!(view.name_target, view.selected_draft);
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

    fn repository(directory: &Path) -> PathBuf {
        let repository = directory.join("repository");
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
        repository
    }
    fn panel_rows(view: &View, area: Rect) -> Vec<String> {
        let rendered = screen(view, area);
        let details = view.viewport.borrow().details;
        rendered
            .lines()
            .skip(usize::from(details.y))
            .take(usize::from(details.height))
            .map(|line| {
                line.chars()
                    .skip(usize::from(details.x))
                    .take(usize::from(details.width))
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    fn prompt_step(view: &mut View) {
        let id = view.selected_draft.unwrap();
        view.open_name(id);
        assert_eq!(view.drafts[0].step, Step::Name);
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        assert_eq!(view.drafts[0].step, Step::Name);
        view.handle_paste("named task");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.input_label(), "named task");
        assert!(view.input_box.focused);
        view.handle_paste("unfinished task");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }

    #[test]
    fn branch_button_enters_an_editor_and_escape_discards_text_then_draft() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        prompt_step(&mut view);
        assert!(view.input_box.editor.is_empty());
        assert_eq!(view.input_label(), "named task");
        let area = Rect::new(0, 0, 160, 40);
        let rows = panel_rows(&view, area);
        assert_eq!(&rows[..3], &["feature/a", "main", "Create new branch"]);
        assert!(!view.selector.as_ref().unwrap().input_active());
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        assert_eq!(view.cursor_pos(area), None);
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let rows = panel_rows(&view, area);
        assert_eq!(rows.last().unwrap().trim_end(), "New branch ❯");
        let input = view.viewport.borrow().details_input;
        assert_eq!(view.cursor_pos(area), Some((input.x + 13, input.y)));
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        view.handle_paste("feature/你好");
        view.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        view.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(
            panel_rows(&view, area)
                .last()
                .unwrap()
                .contains("feature/你 x好")
        );
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!view.selector.as_ref().unwrap().input_active());
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(panel_rows(&view, area).last().unwrap().trim_end() == "New branch ❯");
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.drafts.is_empty());
        assert!(view.selector.is_none());
        assert!(view.input_box.editor.is_empty());
    }

    #[test]
    fn clicking_the_branch_editor_does_not_submit_its_text() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        prompt_step(&mut view);
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.handle_paste("feature/probe");
        screen(&view, Rect::new(0, 0, 160, 40));
        let input = view.viewport.borrow().details_input;
        assert_eq!(click(&mut view, input), None);
        assert!(view.selector.as_ref().unwrap().input_active());
        assert_eq!(view.drafts[0].step, Step::Branch);
    }

    #[test]
    fn new_task_panel_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        let area = Rect::new(0, 0, 160, 16);
        let mut states = Vec::new();
        states.push(panel_rows(&view, area).join("\n"));
        prompt_step(&mut view);
        states.push(panel_rows(&view, area).join("\n"));
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.handle_paste("feature/probe");
        states.push(panel_rows(&view, area).join("\n"));
        let actual = states.join("\n---\n") + "\n";
        if std::env::var_os("UPDATE_DRAFT_SNAPSHOT").is_some() {
            std::fs::write("tests/ui_snapshots/new_task_steps.txt", &actual).unwrap();
        } else {
            assert_eq!(
                actual,
                include_str!("../../tests/ui_snapshots/new_task_steps.txt")
            );
        }
    }

    #[test]
    fn branch_keyboard_and_single_click_create_with_the_saved_name_and_prompt() {
        for mouse in [false, true] {
            for new in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let repository = repository(directory.path());
                let mut view = draft_view(&repository, directory.path());
                prompt_step(&mut view);
                let area = Rect::new(0, 0, 160, 40);
                screen(&view, area);
                let action = if new {
                    if mouse {
                        let button = row_of(&view, 2);
                        assert_eq!(click(&mut view, button), None);
                    } else {
                        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    }
                    view.handle_paste("feature/probe");
                    view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                } else if mouse {
                    let feature = row_of(&view, 0);
                    click(&mut view, feature)
                } else {
                    view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
                    view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
                    view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                };
                let Some(Action::CreateTask {
                    request, branch, ..
                }) = action
                else {
                    panic!("{action:?}")
                };
                assert_eq!(request.name.as_deref(), Some("named task"));
                assert_eq!(request.prompt, "unfinished task");
                assert_eq!(
                    branch,
                    Some(if new {
                        BranchChoice::New("feature/probe".into())
                    } else {
                        BranchChoice::Existing("feature/a".into())
                    })
                );
                assert!(view.selector.is_none());
                assert!(view.pending_click_in(Instant::now()).is_none());
            }
        }
    }

    #[test]
    fn plain_directory_draft_has_no_branch_control() {
        let directory = tempfile::tempdir().unwrap();
        let mut view = draft_view(directory.path(), directory.path());
        view.drafts[0].setup.branch.clear();
        view.open_field(Field::Branch);
        assert!(view.selector.is_none());
        assert_eq!(view.name_target, view.selected_draft);
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
