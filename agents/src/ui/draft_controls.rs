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
/// model or effort, and the input naming a new task. Repository and branch clicks apply
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
    },
    /// The input naming the new task, typed after its branch.
    Name {
        draft: DraftId,
        editor: line_edit::LineEdit,
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

const NEW_BRANCH: &str = "New branch name";
const NEW_TASK: &str = "New task";

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
            SelectorKind::Name { .. } => 0,
            SelectorKind::Model { values, .. } | SelectorKind::Effort { values, .. } => {
                values.len()
            }
        }
    }

    #[cfg(test)]
    pub(super) fn field(&self) -> Field {
        match &self.kind {
            SelectorKind::Repository { .. } => Field::Directory,
            SelectorKind::Branch { .. } | SelectorKind::Name { .. } => Field::Branch,
            SelectorKind::Model { .. } => Field::Model,
            SelectorKind::Effort { .. } => Field::Effort,
        }
    }

    /// The input row at the panel's bottom and the placeholder it shows while empty.
    pub(super) fn input(&self) -> Option<(&line_edit::LineEdit, &'static str)> {
        match &self.kind {
            SelectorKind::Branch { editor, .. } => Some((editor, NEW_BRANCH)),
            SelectorKind::Name { editor, .. } => Some((editor, NEW_TASK)),
            _ => None,
        }
    }

    /// The input row's editor while the keys type into it: always for the task's name, and for
    /// the new branch's name while its row is the selected choice.
    pub(super) fn active_editor(&mut self) -> Option<&mut line_edit::LineEdit> {
        match &mut self.kind {
            SelectorKind::Branch {
                branches, editor, ..
            } if self.selected == branches.len() => Some(editor),
            SelectorKind::Name { editor, .. } => Some(editor),
            _ => None,
        }
    }

    pub(super) fn input_active(&self) -> bool {
        match &self.kind {
            SelectorKind::Branch { branches, .. } => self.selected == branches.len(),
            SelectorKind::Name { .. } => true,
            _ => false,
        }
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
            SelectorKind::Repository { draft, .. }
            | SelectorKind::Branch { draft, .. }
            | SelectorKind::Name { draft, .. } => Some(*draft),
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
            SelectorKind::Branch { branches, .. } => branches
                .iter()
                .enumerate()
                .map(|(index, branch)| ListRow::Choice(index, Line::from(branch.clone())))
                .collect(),
            SelectorKind::Name { .. } => Vec::new(),
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

    /// Moves the selected draft from its prompt to its branch, or straight to its name outside a
    /// repository.
    pub(super) fn start_branch_step(&mut self) {
        let Some(id) = self.selected_draft else {
            return;
        };
        self.set_step(id, Step::Branch);
        if !self.open_branches(false) {
            self.open_name(id);
        }
    }

    fn set_step(&mut self, id: DraftId, step: Step) {
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) {
            draft.step = step;
        }
    }

    /// Lists the selected draft's branches, selecting its current choice or the new branch's
    /// input; false when the draft is outside a repository or its branches cannot be read.
    fn open_branches(&mut self, select_current: bool) -> bool {
        let Some(draft) = self.selected_draft() else {
            return false;
        };
        if draft.setup.branch.is_empty() {
            return false;
        }
        let Ok(branches) = crate::tasks::local_branches(&draft.setup.cwd) else {
            return false;
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
            },
            selected,
        ));
        self.focus_selector();
        true
    }

    fn open_name(&mut self, id: DraftId) {
        self.set_step(id, Step::Name);
        self.selector = Some(Selector::new(
            SelectorKind::Name {
                draft: id,
                editor: line_edit::LineEdit::default(),
            },
            0,
        ));
        self.focus_selector();
    }

    /// Records the chosen branch, then asks for the task's name in the new-task flow.
    fn choose_branch(&mut self, id: DraftId, branch: String, new: bool) {
        let Some(draft) = self.drafts.iter_mut().find(|item| item.id == id) else {
            return;
        };
        draft.branch_choice = if new {
            Some(BranchChoice::New(branch))
        } else {
            (branch != draft.setup.branch).then_some(BranchChoice::Existing(branch))
        };
        if draft.step == Step::Branch {
            self.open_name(id);
        } else {
            self.close_selector();
        }
    }

    /// Creates the task from its name, the prompt box's text and the chosen branch.
    fn create_task(&mut self, id: DraftId, name: String) -> Option<Action> {
        let draft = self.drafts.iter().find(|draft| draft.id == id)?;
        if draft.working || draft.job.is_some() {
            return None;
        }
        self.input_box.history = self.prompt_history.clone();
        // The prompt box submits its text the way it does for a task, recording the history.
        self.input_box.focused = true;
        let Some(prompt_box::PromptAction::Submit(prompt)) = self
            .input_box
            .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            return None;
        };
        let draft = self.drafts.iter().find(|draft| draft.id == id)?;
        let action = Action::CreateTask {
            draft_id: id,
            provider: draft.setup.provider,
            branch: draft.branch_choice.clone(),
            request: crate::tasks::NewTask {
                cwd: draft.setup.cwd.clone(),
                name: Some(name),
                model: draft.setup.model.clone(),
                effort: draft.setup.effort.clone(),
                prompt,
                images: self.input_box.editor.image_paths(),
            },
        };
        self.submit_draft(id);
        Some(action)
    }

    /// Esc or a right click: the name step returns to the branch step, which returns to the
    /// prompt; any other selector closes.
    pub(super) fn cancel_selector(&mut self) {
        let naming = matches!(
            &self.selector,
            Some(Selector {
                kind: SelectorKind::Name { .. },
                ..
            })
        );
        if naming
            && let Some(id) = self.selected_draft
            && self
                .selected_draft()
                .is_some_and(|draft| draft.step == Step::Name)
        {
            self.set_step(id, Step::Branch);
            if self.open_branches(true) {
                return;
            }
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
            self.set_step(id, Step::Prompt);
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
                    self.close_selector();
                    Some(action)
                }
            },
            SelectorKind::Branch {
                draft,
                branches,
                editor,
            } => {
                let draft = *draft;
                // The input row applies the typed name; an empty name stays put.
                let (branch, new) = match branches.get(index) {
                    Some(branch) => (branch.clone(), false),
                    None => (editor.text().trim().to_owned(), true),
                };
                if !branch.is_empty() {
                    self.choose_branch(draft, branch, new);
                }
                None
            }
            SelectorKind::Name { draft, editor } => {
                let name = editor.text().trim().to_owned();
                let draft = *draft;
                if name.is_empty() {
                    return None;
                }
                self.create_task(draft, name)
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
        // An empty input row still walks the list with the arrow and page keys; text makes
        // every other key line editing.
        let navigation = matches!(
            key.code,
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
        ) && key.modifiers.is_empty();
        if let Some(editor) = selector.active_editor()
            && !(navigation && editor.is_empty())
        {
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
        let input = selector.input().filter(|_| area.height > 1);
        let list = if input.is_some() {
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
        if let Some((editor, placeholder)) = input {
            let row = super::log::search_input_rect(area);
            if editor.is_empty() {
                render::line(Line::from(placeholder.dim()), row, buf);
            } else {
                render::render_input_row("", editor, row, buf);
            }
            if let Some(index) = selector.create_index() {
                rows.push((row, index));
            }
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

    #[test]
    fn branch_step_lists_branches_above_the_new_branch_input_and_leads_to_the_name() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        view.handle_paste("unfinished task");
        let area = Rect::new(0, 0, 160, 40);
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.drafts[0].step, Step::Branch);
        assert!(!view.input_box.focused);
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        let rows = panel_rows(&view, area);
        assert_eq!(rows[..2], ["feature/a", "main"]);
        assert_eq!(rows.last().map(String::as_str), Some("New branch name"));
        assert!(!rows.contains(&"Create new branch".to_owned()));
        let input = view.viewport.borrow().details_input;
        assert_eq!(input.y, view.viewport.borrow().details.bottom() - 1);
        assert_eq!(view.cursor_pos(area), Some((input.x, input.y)));
        assert!(view.center_footer_hints().to_string().contains("back"));
        // Enter on the empty input stays put; the arrows walk the list while it is empty.
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.drafts[0].step, Step::Branch);
        view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(view.selector.as_ref().unwrap().selected, 1);
        screen(&view, area);
        assert_eq!(view.cursor_pos(area), None);
        view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        // Text makes the keys line editing: j and k type, Left moves the caret.
        for code in [KeyCode::Char('j'), KeyCode::Char('k')] {
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        view.handle_paste("你好");
        view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        view.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        view.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let rows = panel_rows(&view, area);
        assert_eq!(rows.last().map(String::as_str), Some("jk/你 x好"));
        assert_eq!(view.cursor_pos(area).unwrap().1, input.y);
        assert_eq!(view.input_box.editor.text(), "unfinished task");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            view.drafts[0].branch_choice,
            Some(BranchChoice::New("jk/你x好".into()))
        );
        assert_eq!(view.drafts[0].step, Step::Name);
        assert!(!view.input_box.focused);
        assert_eq!(view.input_box.editor.text(), "unfinished task");
        assert!(view.input_box.history.borrow().is_empty());
        let rows = panel_rows(&view, area);
        assert_eq!(rows.last().map(String::as_str), Some("New task"));
        // Esc from the name returns to the branch list with the typed name kept.
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(view.drafts[0].step, Step::Branch);
        let rows = panel_rows(&view, area);
        assert_eq!(rows.last().map(String::as_str), Some("jk/你 x好"));
        view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        // Esc from the branch list returns to the prompt with the draft kept.
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(view.drafts[0].step, Step::Prompt);
        assert!(view.selector.is_none());
        assert!(view.input_box.focused);
        assert_eq!(view.input_box.editor.text(), "unfinished task");
        // Reopening keeps the typed name; clearing it frees the arrows, and an existing branch
        // by keyboard, then the name, creates the task.
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.drafts[0].step, Step::Branch);
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        for _ in 0.."jk/你x好".chars().count() {
            view.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }
        view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            view.drafts[0].branch_choice,
            Some(BranchChoice::Existing("feature/a".into()))
        );
        assert_eq!(view.drafts[0].step, Step::Name);
        view.handle_paste("named task");
        let action = view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let Some(Action::CreateTask {
            request, branch, ..
        }) = action
        else {
            panic!("{action:?}");
        };
        assert_eq!(request.name.as_deref(), Some("named task"));
        assert_eq!(request.prompt, "unfinished task");
        assert_eq!(branch, Some(BranchChoice::Existing("feature/a".into())));
        assert!(view.selector.is_none());
    }

    #[test]
    fn a_single_click_on_a_branch_row_applies_it_like_a_repository() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        view.handle_paste("unfinished task");
        let area = Rect::new(0, 0, 160, 40);
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        screen(&view, area);
        // A click on the input row only selects it.
        let input = row_of(&view, 2);
        assert_eq!(click(&mut view, input), None);
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        assert_eq!(view.drafts[0].step, Step::Branch);
        let feature = row_of(&view, 0);
        assert_eq!(click(&mut view, feature), None);
        assert!(view.pending_click_in(Instant::now()).is_none());
        assert_eq!(
            view.drafts[0].branch_choice,
            Some(BranchChoice::Existing("feature/a".into()))
        );
        assert_eq!(view.drafts[0].step, Step::Name);
        assert!(matches!(
            view.selector.as_ref().unwrap().kind,
            SelectorKind::Name { .. }
        ));
        // Outside the flow, the branch field on the rule applies and returns to the prompt.
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(view.drafts[0].step, Step::Prompt);
        view.open_field(Field::Branch);
        assert_eq!(view.selector.as_ref().unwrap().selected, 0);
        screen(&view, area);
        let main = row_of(&view, 1);
        assert_eq!(click(&mut view, main), None);
        assert_eq!(view.drafts[0].branch_choice, None);
        assert!(view.selector.is_none());
        assert!(view.input_box.focused);
        assert_eq!(view.drafts[0].step, Step::Prompt);
    }

    #[test]
    fn branch_input_enter_and_escape_preserve_draft_and_prompt() {
        let directory = tempfile::tempdir().unwrap();
        let mut view = draft_view(directory.path(), directory.path());
        view.close_selector();
        view.handle_paste("saved input");
        view.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let cursor = view.input_box.editor.cursor();
        let label = view.input_label();
        let draft = view.drafts[0].id;
        view.selector = Some(Selector::new(
            SelectorKind::Branch {
                draft,
                branches: vec!["main".into()],
                editor: line_edit::LineEdit::default(),
            },
            1,
        ));
        view.focus_selector();
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!view.input_box.focused);
        assert_eq!(view.input_label(), label);
        view.handle_paste("feature/input");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.input_label(), label);
        assert_eq!(view.input_box.editor.text(), "saved input");
        assert_eq!(view.input_box.editor.cursor(), cursor);
        assert!(view.prompt_history.borrow().is_empty());
        assert_eq!(
            view.drafts[0].branch_choice,
            Some(BranchChoice::New("feature/input".into()))
        );
        assert!(view.selector.is_none());
        assert!(view.input_box.focused);
        view.selector = Some(Selector::new(
            SelectorKind::Branch {
                draft,
                branches: vec!["main".into()],
                editor: line_edit::LineEdit::default(),
            },
            1,
        ));
        view.focus_selector();
        view.handle_paste("dropped");
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(view.drafts.len(), 1);
        assert_eq!(view.input_box.editor.text(), "saved input");
        assert!(view.input_box.focused);
        assert!(view.selector.is_none());
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
