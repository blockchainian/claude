use super::navigation::ListRow;
use super::*;
use crate::model::DraftKey;
use crate::repository::{BrowseEntry, browse_entries};
use crate::state::DraftBranch;
use crate::tasks::BranchChoice;
use std::cell::Cell;
use std::fs;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Field {
    WeeklyUsage,
    Model,
    Effort,
    Branch,
    Directory,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum TaskKey {
    Draft(DraftKey),
    Session(SessionKey),
}

impl TaskKey {
    pub(super) fn session(&self) -> Option<&SessionKey> {
        match self {
            Self::Session(id) => Some(id),
            Self::Draft(_) => None,
        }
    }
}

/// The list the task details panel shows to choose a new task's repository or branch, or a task's
/// model or effort. Clicks and Enter apply immediately. A right click or Esc cancels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Origin {
    Wizard,
    Border,
}

pub(super) struct Selector {
    pub origin: Origin,
    pub kind: SelectorKind,
    pub selected: usize,
    /// The choice already in effect; choosing it again leaves a session alone.
    pub current: Option<usize>,
    pub scroll: Cell<usize>,
}

pub(super) enum SelectorKind {
    Repository {
        draft: DraftKey,
        dir: PathBuf,
        entries: Vec<BrowseEntry>,
    },
    /// The repository's branches above the input naming a new one, whose choice index is
    /// `branches.len()`.
    Branch {
        draft: DraftKey,
        branches: Vec<String>,
        editor: line_edit::LineEdit,
        editing: bool,
    },
    Model {
        target: TaskKey,
        values: Vec<String>,
        labels: Vec<String>,
        families: Vec<String>,
    },
    Effort {
        target: TaskKey,
        values: Vec<String>,
        labels: Vec<String>,
    },
}

const NEW_BRANCH: &str = "Create new branch";

impl Selector {
    pub(super) fn new(kind: SelectorKind, selected: usize) -> Self {
        Self {
            origin: Origin::Border,
            kind,
            selected,
            current: None,
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

    fn draft(&self) -> Option<DraftKey> {
        match &self.kind {
            SelectorKind::Repository { draft, .. } | SelectorKind::Branch { draft, .. } => {
                Some(*draft)
            }
            SelectorKind::Model { target, .. } | SelectorKind::Effort { target, .. } => {
                match target {
                    TaskKey::Draft(draft) => Some(*draft),
                    TaskKey::Session(_) => None,
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
                    super::formatting::format_directory_display(dir, None).dim(),
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
            SelectorKind::Model {
                labels, families, ..
            } => {
                let mut rows = Vec::new();
                let mut previous = None;
                for (index, (label, family)) in labels.iter().zip(families).enumerate() {
                    if previous != Some(family) {
                        if previous.is_some() {
                            rows.push(ListRow::Text(Line::default()));
                        }
                        rows.push(ListRow::Text(Line::from(family.clone().dim())));
                        previous = Some(family);
                    }
                    rows.push(ListRow::Choice(index, Line::from(label.clone())));
                }
                rows
            }
            SelectorKind::Effort { labels: values, .. } => values
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
        let mut entries = browse_entries(&dir);
        let current = current.and_then(|current| fs::canonicalize(current).ok());
        if let Some(path) = &current
            && path.join(".git").is_dir()
            && !entries.iter().any(|entry| matches!(entry, BrowseEntry::Repository(entry) if fs::canonicalize(entry).ok().as_ref() == Some(path)))
        {
            entries.push(BrowseEntry::Repository(path.clone()));
        }
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
        self.set_active_panel(Panel::TaskDetails);
    }

    /// Runs the field rule only for a wizard transition.
    pub(super) fn advance_draft(&mut self, prompt_enter: bool) -> Option<Action> {
        let draft = self.selected_draft()?;
        if draft.working || draft.job.is_some() {
            return None;
        }
        let id = draft.id;
        if self
            .drafts
            .iter_mut()
            .find(|draft| draft.id == id)?
            .error
            .take()
            .is_some()
        {
            self.drafts_dirty = true;
        }
        let draft = self.drafts.iter_mut().find(|draft| draft.id == id)?;
        if draft.setup.branch.is_empty() {
            draft.repository_confirmed = false;
        }
        let draft = self.selected_draft()?;
        if draft.name.is_none() {
            self.open_name(id);
        } else if !draft.repository_confirmed {
            let repository = crate::repository::repository_identity(&draft.setup.cwd)
                .map_or_else(|| draft.setup.cwd.clone(), |identity| identity.primary_root);
            let dir = repository
                .parent()
                .map_or_else(|| repository.clone(), Path::to_path_buf);
            self.name_target = None;
            self.open_repositories(dir, Some(&repository));
            self.selector.as_mut()?.origin = Origin::Wizard;
        } else if draft.branch == DraftBranch::Unchosen {
            self.name_target = None;
            self.open_branches(true);
            if let Some(selector) = &mut self.selector {
                selector.origin = Origin::Wizard;
            }
        } else if prompt_enter {
            return self.create_task(id);
        } else {
            self.close_selector();
            self.set_active_panel(Panel::Prompt);
        }
        None
    }

    /// Enter in a nonempty prompt is the only task creation entry point.
    pub(super) fn start_branch_step(&mut self) -> Option<Action> {
        if self.input_box.editor.prompt_text().trim().is_empty() {
            return None;
        }
        self.advance_draft(true)
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
                self.show_error(format!("Cannot read branches: {error}"));
                return false;
            }
        };
        let current = match &draft.branch {
            DraftBranch::Unchosen => draft.setup.branch.as_str(),
            DraftBranch::Existing(name) | DraftBranch::New(name) => name.as_str(),
        };
        let selected = branches
            .iter()
            .position(|branch| select_current && branch == current)
            .unwrap_or(branches.len());
        let mut editor = line_edit::LineEdit::default();
        if let Some(name) = &draft.pending_branch {
            editor.set_text(name);
        } else if let DraftBranch::New(name) = &draft.branch {
            editor.set_text(name);
        }
        let editing = draft.pending_branch.is_some();
        let selected = if editing { branches.len() } else { selected };
        self.selector = Some(Selector::new(
            SelectorKind::Branch {
                draft: draft.id,
                branches,
                editor,
                editing,
            },
            selected,
        ));
        self.set_active_panel(Panel::TaskDetails);
        true
    }

    pub(super) fn open_name(&mut self, id: DraftKey) {
        if self
            .drafts
            .iter()
            .find(|draft| draft.id == id)
            .is_none_or(|draft| draft.working || draft.job.is_some())
        {
            return;
        }
        self.dismiss_selector();
        if self.name_target != Some(id) {
            if let Some(name) = self
                .drafts
                .iter()
                .find(|draft| draft.id == id)
                .and_then(|draft| draft.name.as_deref())
            {
                self.rename.set_text(name);
            } else {
                self.rename.clear();
            }
        }
        self.rename_target = None;
        self.name_target = Some(id);
        self.set_active_panel(Panel::TaskDetails);
    }

    /// Branch picks finish their selector; only wizard picks advance the rule.
    fn choose_branch(
        &mut self,
        id: DraftKey,
        branch: String,
        new: bool,
        advance: bool,
    ) -> Option<Action> {
        let origin = self.selector.as_ref()?.origin;
        let draft = self.drafts.iter_mut().find(|item| item.id == id)?;
        if draft.working || draft.job.is_some() {
            return None;
        }
        draft.branch = if new {
            DraftBranch::New(branch)
        } else {
            DraftBranch::Existing(branch)
        };
        draft.repository_confirmed = true;
        if advance {
            self.close_selector();
        } else {
            self.dismiss_selector();
        }
        self.drafts
            .iter_mut()
            .find(|draft| draft.id == id)
            .unwrap()
            .pending_branch = None;
        self.draft_changed(id);
        if advance && origin == Origin::Wizard {
            self.advance_draft(false)
        } else {
            None
        }
    }

    /// Creates directly from the prompt box, keeping its contents until hand-off.
    fn create_task(&mut self, id: DraftKey) -> Option<Action> {
        let draft = self.drafts.iter().find(|draft| draft.id == id)?;
        if draft.working || draft.job.is_some() {
            return None;
        }
        let name = draft.name.as_ref().expect("task name before creation");
        let branch = match &draft.branch {
            DraftBranch::Unchosen => None,
            DraftBranch::Existing(name) if name == &draft.setup.branch => None,
            DraftBranch::Existing(name) => Some(BranchChoice::Existing(name.clone())),
            DraftBranch::New(name) => Some(BranchChoice::New(name.clone())),
        };
        let action = Action::CreateTask {
            draft_key: id,
            provider: draft.setup.provider,
            branch,
            request: crate::tasks::NewTask {
                cwd: draft.setup.cwd.clone(),
                name: Some(name.clone()),
                model: draft.setup.model.clone(),
                effort: draft.setup.effort.clone(),
                prompt: self.input_box.editor.prompt_text(),
                images: self.input_box.editor.image_paths(),
            },
        };
        self.submit_draft(id);
        Some(action)
    }

    /// Esc clears a branch editor first, then closes only the selector.
    pub(super) fn cancel_selector(&mut self) {
        if self
            .selected_draft()
            .is_some_and(|draft| draft.working || draft.job.is_some())
        {
            return;
        }
        if let Some(Selector {
            kind:
                SelectorKind::Branch {
                    draft,
                    editor,
                    editing: true,
                    ..
                },
            ..
        }) = &mut self.selector
        {
            editor.clear();
            if let Some(DraftBranch::New(name)) = self
                .drafts
                .iter()
                .find(|item| item.id == *draft)
                .map(|draft| &draft.branch)
            {
                editor.set_text(name);
            }
            if let Some(Selector {
                kind: SelectorKind::Branch { editing, .. },
                ..
            }) = &mut self.selector
            {
                *editing = false;
            }
            return;
        }
        self.close_selector();
    }

    pub(super) fn open_field(&mut self, field: Field) {
        self.dismiss_selector();
        let (provider, target, model, effort) = if let Some(draft) = self.selected_draft() {
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
                    return;
                }
                Field::Branch => {
                    self.open_branches(true);
                    return;
                }
                Field::Model | Field::Effort => {}
                Field::WeeklyUsage => return,
            }
            (
                draft.setup.provider,
                TaskKey::Draft(draft.id),
                self.draft_model(draft).map(str::to_owned),
                self.draft_effort(draft).map(str::to_owned),
            )
        } else if let Some(session) = self.selected_row() {
            if matches!(field, Field::Branch | Field::Directory) || !session.can_resume() {
                return;
            }
            (
                session.id.provider,
                TaskKey::Session(session.id.clone()),
                session.model.clone(),
                session.effort.clone(),
            )
        } else {
            return;
        };
        let Some(choices) = self.input_choices.get(&provider) else {
            return;
        };
        let current_model = model.as_deref().and_then(|model| choices.model(model));
        let mut models: Vec<_> = choices.models.iter().collect();
        // Stable sorting keeps both first-seen families and their models in catalog order.
        models.sort_by_key(|model| {
            choices
                .models
                .iter()
                .position(|choice| choice.family == model.family)
                .expect("model family belongs to the catalog")
        });
        let (values, current) = match field {
            Field::Model => (
                models
                    .iter()
                    .map(|model| model.id.clone())
                    .collect::<Vec<_>>(),
                current_model.map(|model| model.id.clone()),
            ),
            _ => (
                current_model
                    .map(|model| model.efforts.clone())
                    .unwrap_or_default(),
                effort,
            ),
        };
        if values.is_empty() {
            return;
        }
        let current = current.and_then(|current| values.iter().position(|value| *value == current));
        let labels = values
            .iter()
            .map(|value| {
                if field == Field::Model {
                    self.model_text(provider, value)
                } else {
                    value.clone()
                }
            })
            .collect();
        let kind = if field == Field::Model {
            SelectorKind::Model {
                target,
                values,
                labels,
                families: models.iter().map(|model| model.family.clone()).collect(),
            }
        } else {
            SelectorKind::Effort {
                target,
                values,
                labels,
            }
        };
        let mut selector = Selector::new(kind, current.unwrap_or_default());
        selector.current = current;
        self.selector = Some(selector);
        self.set_active_panel(Panel::TaskDetails);
    }

    /// Keeps an unapplied branch name when leaving its selector.
    pub(super) fn dismiss_selector(&mut self) {
        if let Some(Selector {
            kind:
                SelectorKind::Branch {
                    draft,
                    editor,
                    editing,
                    ..
                },
            ..
        }) = self.selector.take()
        {
            let draft = self
                .drafts
                .iter_mut()
                .find(|item| item.id == draft)
                .expect("branch selector draft exists");
            draft.pending_branch = editing.then(|| editor.text().to_owned());
        }
    }

    /// Returns a side edit to its name editor or prompt home.
    pub(super) fn close_selector(&mut self) {
        self.dismiss_selector();
        if let Some(id) = self.selected_draft {
            if self
                .selected_draft()
                .is_some_and(|draft| draft.name.is_none())
            {
                self.open_name(id);
                return;
            }
            self.name_target = None;
            self.rename.clear();
        }
    }

    /// Drops a selector that belongs to `draft`.
    pub(super) fn close_draft_selector(&mut self, draft: DraftKey) {
        if self.name_target == Some(draft) {
            self.name_target = None;
            self.rename.clear();
        }
        if self
            .selector
            .as_ref()
            .is_some_and(|selector| selector.draft() == Some(draft))
        {
            self.dismiss_selector();
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
            ..
        }) = &mut self.selector
        {
            *entries = browse_entries(&dir);
            *browsed = dir;
            *selected = 0;
            scroll.set(0);
        }
    }

    /// Opens the folder at the chosen repository row.
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

    /// Applies the choice at `index`: a click, or Enter.
    pub(super) fn apply_choice(&mut self, index: usize) -> Option<Action> {
        if self
            .selected_draft()
            .is_some_and(|draft| draft.working || draft.job.is_some())
        {
            return None;
        }
        let selector = self.selector.as_ref()?;
        match &selector.kind {
            SelectorKind::Repository { draft, entries, .. } => match entries.get(index)? {
                BrowseEntry::Parent(_) | BrowseEntry::Folder(_) => self.single_choice(index),
                BrowseEntry::Repository(cwd) => {
                    let action = Action::ChangeDraftDirectory {
                        draft_key: *draft,
                        cwd: cwd.clone(),
                    };
                    let id = *draft;
                    let wizard = selector.origin == Origin::Wizard;
                    let draft = self.drafts.iter_mut().find(|draft| draft.id == id)?;
                    draft.repository_confirmed = true;
                    draft.continue_after_directory = wizard;
                    self.close_selector();
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
                    return self.choose_branch(draft, branch, new, true);
                }
                None
            }
            SelectorKind::Model { target, values, .. }
            | SelectorKind::Effort { target, values, .. } => {
                let value = values.get(index)?.clone();
                // A session already running with the chosen value has nothing to change.
                if matches!(target, TaskKey::Session(_)) && selector.current == Some(index) {
                    self.close_selector();
                    return None;
                }
                let target = target.clone();
                let model = matches!(selector.kind, SelectorKind::Model { .. });
                self.close_selector();
                match target {
                    TaskKey::Draft(id) => {
                        let draft = self.drafts.iter().find(|draft| draft.id == id)?;
                        let default = if model {
                            draft.defaults.model.as_deref()
                        } else {
                            self.draft_default_effort(draft)
                        };
                        let value = (default != Some(value.as_str())).then_some(value);
                        let draft = self.drafts.iter_mut().find(|draft| draft.id == id)?;
                        if model {
                            draft.setup.model = value;
                        } else {
                            draft.setup.effort = value;
                        }
                        self.draft_changed(id);
                        None
                    }
                    TaskKey::Session(id) => Some(if model {
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
        // The selector has the keys only after a click in the task details panel or on a border field.
        if self.active_panel != Panel::TaskDetails {
            return None;
        }
        let selector = self.selector.as_mut()?;
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
        let page = self.viewport.borrow().task_details.height.saturating_sub(1);
        selector.selected =
            super::navigation::step(selector.selected, selector.len(), usize::from(page), code);
        Some(None)
    }

    pub(super) fn render_controls(&self, area: Rect, buf: &mut Buffer) {
        self.viewport.borrow_mut().picker_rows.clear();
        let Some(selector) = &self.selector else {
            return;
        };
        self.clear_overlay(area, false, buf);
        // Reserve the branch control and a gap when one branch still fits above it.
        let create = selector.create_index().filter(|_| area.height > 0);
        let choices = selector.rows();
        let reserved = if create.is_some() {
            1 + u16::from(area.height >= 3 && !choices.is_empty())
        } else {
            0
        };
        let height = usize::from(area.height - reserved).min(choices.len()) as u16;
        let top_aligned = matches!(
            selector.kind,
            SelectorKind::Model { .. } | SelectorKind::Effort { .. }
        ) && self
            .viewport
            .borrow()
            .navigation_layout
            .is_some_and(|layout| layout.kind == render::ScreenLayout::Small);
        let offset = if top_aligned {
            0
        } else {
            area.height - reserved - height
        };
        let list = render::row(area, offset, height);
        let mut rows = super::navigation::render_list(
            &choices,
            selector.selected,
            &selector.scroll,
            list,
            buf,
        );
        if let Some(index) = create {
            let row = render::row(area, area.height.saturating_sub(1), 1);
            if let Some(editor) = selector.branch_editor() {
                render::render_input_row(&format!("New branch {PROMPT_MARKER} "), editor, row, buf);
                self.viewport.borrow_mut().task_details_input = row;
            } else {
                let label = match &selector.kind {
                    SelectorKind::Branch { draft, .. } => match &self
                        .drafts
                        .iter()
                        .find(|item| item.id == *draft)
                        .expect("branch selector draft exists")
                        .branch
                    {
                        DraftBranch::New(name) => name.as_str(),
                        _ => NEW_BRANCH,
                    },
                    _ => unreachable!("branch control"),
                };
                super::navigation::render_list(
                    &[ListRow::Choice(index, Line::from(label.to_owned()))],
                    selector.selected,
                    &Cell::new(0),
                    row,
                    buf,
                );
            }
            rows.push((row, index));
        }
        self.viewport.borrow_mut().picker_rows = rows;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("picker_layout_tests.rs");
    include!("model_family_tests.rs");

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
            .map(|row| {
                row.iter()
                    .map(|cell| cell_text(cell.symbol()))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn model_and_effort_choices_apply_on_click_or_enter_and_cancel() {
        let directory = tempfile::tempdir().unwrap();
        let mut view = draft_view(directory.path(), directory.path());
        let area = Rect::new(3, 2, 160, 40);
        view.set_active_panel(Panel::TaskDetails);
        view.open_field(Field::Model);
        assert!(!view.prompt_focused());
        let selector = view.selector.as_ref().unwrap();
        let SelectorKind::Model { values, .. } = &selector.kind else {
            panic!("expected model choices");
        };
        assert_eq!(values[selector.selected], "claude-opus-5-5");
        let sonnet_index = values
            .iter()
            .position(|value| value == "claude-sonnet-5-5")
            .unwrap();
        screen(&view, area);
        let sonnet = row_of(&view, sonnet_index);
        assert_eq!(click(&mut view, sonnet), None);
        assert!(view.selector.is_none());
        assert!(!view.prompt_focused());
        assert_eq!(view.name_target, view.selected_draft);
        assert_eq!(
            view.drafts[0].setup.model.as_deref(),
            Some("claude-sonnet-5-5")
        );
        assert!(view.pending_click.is_none());
        view.set_active_panel(Panel::TaskDetails);
        view.open_field(Field::Effort);
        let selector = view.selector.as_ref().unwrap();
        let SelectorKind::Effort { values, .. } = &selector.kind else {
            panic!("expected effort choices");
        };
        let low_index = values.iter().position(|value| value == "low").unwrap();
        screen(&view, area);
        let low = row_of(&view, low_index);
        assert_eq!(click(&mut view, low), None);
        assert!(view.selector.is_none());
        assert!(view.pending_click.is_none());
        assert_eq!(view.drafts[0].setup.effort.as_deref(), Some("low"));
        view.set_active_panel(Panel::TaskDetails);
        view.open_field(Field::Effort);
        let selector = view.selector.as_ref().unwrap();
        let SelectorKind::Effort { values, .. } = &selector.kind else {
            panic!("expected effort choices");
        };
        assert_eq!(values[selector.selected], "low");
        for code in [KeyCode::Char('j'), KeyCode::Down, KeyCode::Down] {
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.drafts[0].setup.effort.as_deref(), Some("high"));
        view.set_active_panel(Panel::TaskDetails);
        view.open_field(Field::Effort);
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.selector.is_none());
        assert_eq!(view.drafts.len(), 1);
        view.set_active_panel(Panel::TaskDetails);
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
        let task_details = view.viewport.borrow().task_details;
        rendered
            .lines()
            .skip(usize::from(task_details.y))
            .take(usize::from(task_details.height))
            .map(|line| {
                line.chars()
                    .skip(usize::from(task_details.x))
                    .take(usize::from(task_details.width))
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn draft_selector_and_branch_editor_park_across_switches() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        prompt_step(&mut view);
        let first = view.selected_draft.unwrap();
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.handle_paste("feature/half");
        view.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let selector = view.selector.as_ref().unwrap();
        let selected = selector.selected;
        let caret = selector.branch_editor().unwrap().window(80).1;
        let branch = view.selected_draft().unwrap().branch.clone();
        view.set_active_panel(Panel::Prompt);
        assert_eq!(view.selected_draft().unwrap().branch, branch);
        assert!(view.selector.as_ref().unwrap().branch_editor().is_some());
        view.set_active_panel(Panel::TaskDetails);
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: directory.path().into(),
            model: None,
            effort: None,
            branch: String::new(),
        });
        view.select_draft(first);
        assert_eq!(view.active_panel, Panel::TaskDetails);
        let selector = view.selector.as_ref().unwrap();
        assert_eq!(selector.selected, selected);
        assert_eq!(selector.branch_editor().unwrap().text(), "feature/half");
        assert_eq!(selector.branch_editor().unwrap().window(80).1, caret);
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            view.selected_draft().unwrap().branch,
            DraftBranch::New("feature/half".into())
        );
        assert!(view.selector.is_none());
    }

    fn prompt_step(view: &mut View) {
        let id = view.selected_draft.unwrap();
        view.set_active_panel(Panel::TaskDetails);
        view.open_name(id);
        assert!(view.name_target.is_some());
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        assert!(view.name_target.is_some());
        view.handle_paste("named task");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.input_label(), "named task");
        view.close_selector();
        view.set_active_panel(Panel::Prompt);
        view.drafts[0].repository_confirmed = true;
        view.handle_paste("unfinished task");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.set_active_panel(Panel::TaskDetails);
        view.selector.as_mut().unwrap().selected = 2;
    }

    #[test]
    fn branch_button_enters_an_editor_and_escape_discards_text_then_draft() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        prompt_step(&mut view);
        assert_eq!(view.input_box.editor.text(), "unfinished task");
        assert_eq!(view.input_label(), "named task");
        let area = Rect::new(0, 0, 160, 40);
        let rows = panel_rows(&view, area);
        assert!(rows[..rows.len() - 4].iter().all(String::is_empty));
        assert_eq!(
            &rows[rows.len() - 4..rows.len() - 1],
            &["feature/a", "main", ""]
        );
        assert_eq!(rows.last().unwrap(), NEW_BRANCH);
        assert_eq!(
            row_of(&view, 2).y,
            view.viewport.borrow().task_details.bottom() - 1
        );
        assert!(!view.selector.as_ref().unwrap().input_active());
        assert_eq!(view.selector.as_ref().unwrap().selected, 2);
        assert_eq!(view.cursor_pos(area), None);
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let rows = panel_rows(&view, area);
        assert_eq!(rows.last().unwrap().trim_end(), "New branch ❯");
        let input = view.viewport.borrow().task_details_input;
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
        assert_eq!(view.drafts.len(), 1);
        assert!(view.selector.is_none());
        assert_eq!(view.input_box.editor.text(), "unfinished task");
        view.set_active_panel(Panel::Prompt);
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.confirm_dialog.is_some());
    }

    #[test]
    fn branch_button_stays_below_scrolling_choices_with_matching_highlight_and_hits() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        prompt_step(&mut view);
        let branches: Vec<_> = (0..30).map(|index| format!("branch/{index:02}")).collect();
        if let SelectorKind::Branch {
            branches: choices, ..
        } = &mut view.selector.as_mut().unwrap().kind
        {
            *choices = branches.clone();
        }
        view.selector.as_mut().unwrap().selected = branches.len();
        let area = Rect::new(3, 2, 160, 16);
        for code in [
            None,
            Some(KeyCode::Up),
            Some(KeyCode::Up),
            Some(KeyCode::Down),
            Some(KeyCode::Down),
            Some(KeyCode::Down),
            Some(KeyCode::Up),
        ] {
            if let Some(code) = code {
                view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
            }
            let mut buffer = Buffer::empty(area);
            view.render(area, &mut buffer);
            let viewport = view.viewport.borrow();
            let bottom = viewport.task_details.bottom() - 1;
            let selected = view.selector.as_ref().unwrap().selected;
            assert_eq!(row_of(&view, branches.len()).y, bottom);
            assert_eq!(
                viewport.picker_rows.len(),
                usize::from(viewport.task_details.height - 1)
            );
            for (rect, index) in &viewport.picker_rows {
                let expected = if *index == branches.len() {
                    NEW_BRANCH
                } else {
                    &branches[*index]
                };
                let text: String = (rect.x..rect.right())
                    .map(|x| cell_text(buffer[(x, rect.y)].symbol()))
                    .collect();
                assert_eq!(text.trim_end(), expected);
                assert_eq!(
                    buffer[(rect.right() - 1, rect.y)].bg == style::cursor_style().bg.unwrap(),
                    *index == selected
                );
                if *index < branches.len() {
                    assert!(rect.y < bottom);
                }
            }
            assert!(
                viewport
                    .picker_rows
                    .iter()
                    .any(|(_, index)| *index == selected)
            );
        }
        let button = row_of(&view, branches.len());
        assert_eq!(
            click(&mut view, Rect::new(button.right() - 1, button.y, 1, 1)),
            None
        );
        assert!(view.selector.as_ref().unwrap().input_active());
        screen(&view, area);
        assert_eq!(view.viewport.borrow().task_details_input, button);
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
        let input = view.viewport.borrow().task_details_input;
        assert_eq!(click(&mut view, input), None);
        assert!(view.selector.as_ref().unwrap().input_active());
        assert!(view.selector.is_some());
    }

    #[test]
    fn new_task_panel_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let mut view = draft_view(&repository, directory.path());
        view.set_active_panel(Panel::TaskDetails);
        let area = Rect::new(0, 0, 160, 16);
        let prompt_rows = |view: &View| {
            screen(view, area)
                .lines()
                .filter(|line| line.contains("named task"))
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut states = Vec::new();
        states.push(panel_rows(&view, area).join("\n"));
        prompt_step(&mut view);
        states.push(panel_rows(&view, area).join("\n"));
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.handle_paste("feature/probe");
        states.push(panel_rows(&view, area).join("\n"));
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(view.selector.is_none());
        states.push(prompt_rows(&view));
        view.draft_created(
            view.selected_draft.unwrap(),
            Err(std::io::Error::other("Creation was interrupted")),
        );
        assert_eq!(
            view.selection_failure(),
            Some(super::footer::Feedback::Error(
                "Creation was interrupted".into()
            ))
        );
        states.push(panel_rows(&view, area).join("\n"));
        view.set_active_panel(Panel::Prompt);
        assert!(matches!(
            view.handle_key(KeyCode::Enter.into()),
            Some(Action::CreateTask { .. })
        ));
        view.draft_created(view.selected_draft.unwrap(), Ok("launch-job".into()));
        states.push(prompt_rows(&view));
        let actual = states.join("\n---\n").trim_end().to_owned() + "\n";
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
                assert_eq!(action, None);
                assert!(view.prompt_focused());
                let action = view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
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
        view.set_active_panel(Panel::TaskDetails);
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
        view.set_active_panel(Panel::TaskDetails);
        view.open_field(Field::Model);
        assert!(view.selector.is_some());
        view.set_active_panel(Panel::TaskDetails);
        view.open_field(Field::Effort);
        assert!(view.selector.is_some());
    }
}
