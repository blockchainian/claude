use super::*;
use crate::model::DraftId;
use crate::providers::InputChoices;
use crate::state::{DraftBranch, StoredDraft};
use std::io;

pub(crate) struct DraftSetup {
    pub provider: ProviderId,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Empty when the draft directory is outside a Git working tree.
    pub branch: String,
}

pub(super) struct Draft {
    pub id: DraftId,
    pub defaults: crate::tasks::TaskDefaults,
    pub setup: DraftSetup,
    pub input: prompt_box::PromptBox,
    pub name: Option<String>,
    pub branch: DraftBranch,
    pub pending_branch: Option<String>,
    pub repository_confirmed: bool,
    pub continue_after_directory: bool,
    pub job: Option<String>,
    pub working: bool,
    pub error: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Draft {
    pub(super) fn heading(&self) -> PathBuf {
        match crate::repository::repository_identity(&self.setup.cwd) {
            Some(repository) => repository.primary_root.join(repository.relative_cwd),
            None => self.setup.cwd.clone(),
        }
    }

    fn key(&self) -> SessionKey {
        SessionKey {
            provider: self.setup.provider,
            id: self.id.to_string(),
        }
    }

    pub(super) fn branch_name(&self) -> &str {
        match &self.branch {
            DraftBranch::Unchosen => &self.setup.branch,
            DraftBranch::Existing(name) | DraftBranch::New(name) => name,
        }
    }
}

impl Session {
    fn from_draft(draft: &Draft, input: &line_edit::LineEdit) -> Self {
        let name = draft.name.as_ref().expect("named draft row").clone();
        Self {
            id: draft.key(),
            parent: None,
            cwd: draft.setup.cwd.clone(),
            name: Some(name.clone()),
            title: name,
            prompt: input.text().to_owned(),
            detail_sources: [None, None, None],
            prompt_images: Vec::new(),
            queued: Vec::new(),
            prompt_at: None,
            turn_messages: 0,
            last_record_at: None,
            cache_expires_at: None,
            model: draft.setup.model.clone(),
            effort: draft.setup.effort.clone(),
            git_branch: (!draft.branch_name().is_empty()).then(|| draft.branch_name().to_owned()),
            pr: None,
            status: SessionStatus::New,
            process: None,
            background_job: None,
            updated_at: draft.updated_at,
            started_at: None,
            last_message: None,
            last_message_at: None,
            activity: None,
            ask: None,
            pending_questions: Vec::new(),
            background: Vec::new(),
            tasks: Vec::new(),
            last_prompt: None,
            transcript: None,
            archived: false,
        }
    }
}

impl View {
    pub(crate) fn set_input_choices(&mut self, choices: HashMap<ProviderId, InputChoices>) {
        self.input_choices = choices;
        for index in 0..self.drafts.len() {
            self.drafts[index].defaults = self.draft_defaults(&self.drafts[index].setup);
        }
    }

    fn draft_defaults(&self, setup: &DraftSetup) -> crate::tasks::TaskDefaults {
        let choices = self.input_choices.get(&setup.provider);
        let model = setup
            .model
            .clone()
            .or_else(|| choices?.defaults.model.clone());
        let effort = setup.effort.clone().or_else(|| {
            let choices = choices?;
            choices
                .model(model.as_deref()?)
                .and_then(|model| model.default_effort.clone())
                .or_else(|| {
                    (model == choices.defaults.model)
                        .then(|| choices.defaults.effort.clone())
                        .flatten()
                })
        });
        crate::tasks::TaskDefaults { model, effort }
    }

    pub(crate) fn set_draft_defaults(&mut self, cwd: &Path, defaults: crate::tasks::TaskDefaults) {
        for draft in &mut self.drafts {
            if draft.setup.cwd == cwd {
                draft.defaults = defaults.clone();
            }
        }
    }

    pub(super) fn frozen_draft(&self) -> bool {
        self.selected_draft()
            .is_some_and(|draft| draft.working || draft.job.is_some())
    }

    pub(super) fn draft_of(&self, key: &SessionKey) -> Option<&Draft> {
        self.drafts
            .iter()
            .find(|draft| draft.setup.provider == key.provider && draft.id.to_string() == key.id)
    }

    pub(super) fn draft_rows(&self) -> Vec<Session> {
        self.drafts
            .iter()
            .filter(|draft| draft.name.is_some())
            .map(|draft| {
                let editor = if self.selected_draft == Some(draft.id) {
                    &self.input_box.editor
                } else {
                    &draft.input.editor
                };
                Session::from_draft(draft, editor)
            })
            .collect()
    }

    pub(super) fn draft_changed(&mut self, id: DraftId) {
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) {
            draft.updated_at = chrono::Utc::now();
            if draft.name.is_some() {
                self.drafts_dirty = true;
            }
        }
        self.refresh_draft_rows();
        if self.selected_draft == Some(id)
            && self
                .selected_draft()
                .is_some_and(|draft| draft.name.is_some())
        {
            self.panels_task = self.selected_id().cloned();
        }
    }

    fn refresh_draft_rows(&mut self) {
        let selected = self.selected_id().cloned();
        self.rows.retain(|row| row.status != SessionStatus::New);
        self.rows.extend(self.draft_rows());
        self.rows.sort_by(|a, b| {
            a.status
                .cmp(&b.status)
                .then_with(|| session_order(a).cmp(&session_order(b)))
        });
        self.project_groups = self.rows.iter().map(ProjectGroup::for_session).collect();
        if let Some(id) = self.selected_draft {
            if let Some(index) = self.rows.iter().position(|row| row.id.id == id.to_string()) {
                self.selected = index;
            }
        } else if let Some(key) = selected
            && let Some(index) = self.rows.iter().position(|row| row.id == key)
        {
            self.selected = index;
        }
    }

    pub(crate) fn add_draft(&mut self, setup: DraftSetup) -> DraftId {
        self.leave_draft();
        let id = DraftId {
            millis: chrono::Utc::now().timestamp_millis() as u64,
            counter: self.next_draft,
        };
        self.next_draft += 1;
        let defaults = self.draft_defaults(&setup);
        self.drafts.push(Draft {
            id,
            defaults,
            setup,
            input: prompt_box::PromptBox::default(),
            name: None,
            branch: DraftBranch::Unchosen,
            pending_branch: None,
            repository_confirmed: false,
            continue_after_directory: false,
            job: None,
            working: false,
            error: None,
            updated_at: chrono::Utc::now(),
        });
        self.status_filter = 0;
        self.select_draft(id);
        self.open_name(id);
        id
    }

    pub(crate) fn draft_worktree_created(
        &mut self,
        id: DraftId,
        result: io::Result<(PathBuf, String)>,
    ) {
        let (cwd, branch) = match result {
            Ok(created) => created,
            Err(error) => return self.draft_created(id, Err(error)),
        };
        let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) else {
            return;
        };
        draft.setup.cwd = cwd;
        draft.setup.branch = branch.clone();
        draft.branch = DraftBranch::Existing(branch);
        self.draft_changed(id);
    }

    pub(crate) fn draft_created(&mut self, id: DraftId, result: io::Result<String>) {
        let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) else {
            return;
        };
        draft.working = false;
        match result {
            Ok(job) => draft.job = Some(job),
            Err(error) => draft.error = Some(error.to_string()),
        }
        self.draft_changed(id);
    }

    pub(crate) fn draft_directory_changed(&mut self, id: DraftId, cwd: PathBuf, branch: String) {
        let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) else {
            return;
        };
        if draft.working || draft.job.is_some() {
            return;
        }
        if branch.is_empty() {
            self.draft_directory_failed(id, io::Error::other("Not a Git repository"));
            return;
        }
        let identity =
            |cwd: &Path| crate::repository::repository_identity(cwd).map(|repo| repo.primary_root);
        if identity(&draft.setup.cwd) != identity(&cwd) {
            draft.branch = DraftBranch::Unchosen;
            draft.pending_branch = None;
        }
        draft.setup.cwd = cwd;
        draft.setup.branch = branch;
        draft.repository_confirmed = true;
        let advance = std::mem::take(&mut draft.continue_after_directory);
        if self.selected_draft == Some(id) {
            self.close_selector();
            if advance {
                self.advance_draft(false);
            }
        }
        self.draft_changed(id);
    }

    pub(crate) fn draft_directory_failed(&mut self, id: DraftId, error: io::Error) {
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) {
            draft.continue_after_directory = false;
            draft.error = Some(error.to_string());
        }
        if self.selected_draft == Some(id) {
            self.close_selector();
        }
        self.draft_changed(id);
    }

    pub(super) fn selected_draft(&self) -> Option<&Draft> {
        self.selected_draft
            .and_then(|id| self.drafts.iter().find(|draft| draft.id == id))
    }

    pub(super) fn leave_draft(&mut self) {
        if let Some(id) = self.selected_draft.take() {
            self.close_draft_selector(id);
            if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id)
                && draft.name.is_some()
            {
                self.input_box.editor.preserve_images();
                draft.input = std::mem::take(&mut self.input_box);
                draft.updated_at = chrono::Utc::now();
                self.drafts_dirty = true;
            }
            self.drafts
                .retain(|draft| draft.id != id || draft.name.is_some());
            self.input_box = prompt_box::PromptBox::default();
        }
    }

    pub(super) fn discard_selected_draft(&mut self) {
        if let Some(id) = self.selected_draft {
            self.remove_draft(id);
        }
    }

    fn remove_draft(&mut self, id: DraftId) {
        if self
            .drafts
            .iter()
            .any(|draft| draft.id == id && (draft.working || draft.job.is_some()))
        {
            return;
        }
        self.close_draft_selector(id);
        if self.selected_draft == Some(id) {
            self.selected_draft = None;
            self.input_box = prompt_box::PromptBox::default();
        }
        self.drafts.retain(|draft| draft.id != id);
        self.drafts_dirty = true;
        self.refresh_draft_rows();
        self.reconcile_selection();
        self.show_selected_panels();
        self.set_active_panel(Panel::TaskList);
    }

    pub(super) fn cancel_confirmation(&mut self) {
        self.confirm_dialog = None;
    }

    pub(super) fn confirm_draft_delete(&mut self) {
        if self.frozen_draft() {
            return;
        }
        if let Some(draft) = self.selected_draft() {
            if let Some(name) = &draft.name {
                self.confirm_dialog = Some(ConfirmDialog {
                    action: ConfirmAction::Delete(draft.key()),
                    title: name.clone(),
                    confirm: false,
                });
            } else {
                self.discard_selected_draft();
            }
        }
    }

    pub(super) fn confirm_delete(&mut self, key: SessionKey) -> Option<Action> {
        if let Some(id) = self.draft_of(&key).map(|draft| draft.id) {
            self.remove_draft(id);
            None
        } else {
            Some(Action::Delete(key))
        }
    }

    /// Saves a draft title without advancing the wizard.
    pub(super) fn accept_draft_name(&mut self) -> bool {
        let id = self.name_target.take().expect("draft title target is set");
        let draft = self
            .drafts
            .iter_mut()
            .find(|draft| draft.id == id)
            .expect("title draft is selected");
        let first_name = draft.name.is_none();
        draft.name = Some(self.rename.text().trim().to_owned());
        self.rename.clear();
        self.draft_changed(id);
        first_name
    }

    pub(super) fn cancel_name(&mut self) {
        self.name_target = None;
        self.rename_target = None;
        self.rename.clear();
        if self
            .selected_draft()
            .is_some_and(|draft| draft.name.is_none())
        {
            self.discard_selected_draft();
        } else if self.selected_draft.is_some() {
            self.prepare_prompt();
        } else {
            self.reconcile_selection();
        }
    }

    pub(super) fn select_draft(&mut self, id: DraftId) {
        if self.selected_draft == Some(id) {
            return;
        }
        self.leave_draft();
        self.selected_draft = Some(id);
        if let Some(index) = self.rows.iter().position(|row| row.id.id == id.to_string()) {
            self.selected = index;
        }
        self.show_selected_panels();
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) {
            self.input_box = std::mem::take(&mut draft.input);
        }

        self.prepare_prompt();
    }

    pub(super) fn submit_draft(&mut self, id: DraftId) {
        self.close_draft_selector(id);
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) {
            draft.working = true;
        }
        self.draft_changed(id);
    }

    pub(super) fn reconcile_drafts(&mut self) {
        let arrived: Vec<_> = self
            .drafts
            .iter()
            .filter_map(|draft| {
                let job = draft.job.as_ref()?;
                self.rows
                    .iter()
                    .find(|session| {
                        session
                            .background_job
                            .as_ref()
                            .is_some_and(|background| &background.id == job)
                    })
                    .map(|session| (draft.id, session.id.clone()))
            })
            .collect();
        if arrived.is_empty() {
            return;
        }
        let target = arrived
            .iter()
            .find(|(id, _)| self.selected_draft == Some(*id))
            .or_else(|| arrived.last())
            .map(|(_, key)| key.clone());
        self.leave_draft();
        self.drafts
            .retain(|draft| !arrived.iter().any(|(id, _)| *id == draft.id));
        self.drafts_dirty = true;
        self.refresh_draft_rows();
        if let Some(key) = target.and_then(|key| self.rows.iter().position(|row| row.id == key)) {
            self.status_filter = tab_for_status(self.rows[key].status);
            self.select(key);
        }
    }

    pub(crate) fn stored_drafts(&self) -> Vec<StoredDraft> {
        self.drafts
            .iter()
            .filter_map(|draft| {
                let name = draft.name.clone()?;
                let editor = if self.selected_draft == Some(draft.id) {
                    &self.input_box.editor
                } else {
                    &draft.input.editor
                };
                editor.preserve_images();
                Some(StoredDraft {
                    id: draft.id.to_string(),
                    provider: draft.setup.provider.to_string(),
                    name,
                    input: editor.stored_input(),
                    cwd: draft.setup.cwd.clone(),
                    repository_confirmed: draft.repository_confirmed,
                    branch: draft.branch.clone(),
                    model: draft.setup.model.clone(),
                    effort: draft.setup.effort.clone(),
                    job: draft.job.clone(),
                    error: draft.error.clone(),
                    updated_at: draft.updated_at,
                })
            })
            .collect()
    }

    pub(crate) fn take_drafts_action(&mut self) -> Option<Action> {
        std::mem::take(&mut self.drafts_dirty).then_some(Action::PersistDrafts)
    }

    pub(crate) fn set_drafts(&mut self, stored: &[StoredDraft]) {
        self.draft_jobs_pending = true;
        self.drafts_dirty = true;
        for value in stored {
            let (Ok(id), Ok(key)) = (
                value.id.parse::<DraftId>(),
                format!("{}:{}", value.provider, value.id).parse::<SessionKey>(),
            ) else {
                self.set_notice(Some("Invalid draft identity".into()));
                continue;
            };
            let mut error = value.error.clone();
            let mut repository_confirmed = value.repository_confirmed;
            let mut branch = value.branch.clone();
            let checkout = match crate::app::draft_location(value.cwd.clone()) {
                Ok((_, branch)) => branch,
                Err(failure) => {
                    error = Some(failure.to_string());
                    String::new()
                }
            };
            if !value.cwd.is_dir() {
                repository_confirmed = false;
                branch = DraftBranch::Unchosen;
                error = Some("Repository directory is missing".into());
            } else if checkout.is_empty() {
                repository_confirmed = false;
                branch = DraftBranch::Unchosen;
                error = Some("Not a Git repository".into());
            } else if let DraftBranch::Existing(name) = &branch
                && crate::tasks::local_branches(&value.cwd)
                    .is_ok_and(|branches| !branches.contains(name))
            {
                branch = DraftBranch::Unchosen;
                error = Some("Selected branch is missing".into());
            }
            let mut input = prompt_box::PromptBox::default();
            input.editor = line_edit::LineEdit::restore_input(&value.input);
            self.drafts.push(Draft {
                id,
                defaults: crate::tasks::TaskDefaults::default(),
                setup: DraftSetup {
                    provider: key.provider,
                    cwd: value.cwd.clone(),
                    branch: checkout,
                    model: value.model.clone(),
                    effort: value.effort.clone(),
                },
                input,
                name: Some(value.name.clone()),
                branch,
                pending_branch: None,
                repository_confirmed,
                continue_after_directory: false,
                working: false,
                job: value.job.clone(),
                error,
                updated_at: value.updated_at,
            });
        }
        self.refresh_draft_rows();
    }

    pub(crate) fn repair_draft_jobs(&mut self) {
        self.reconcile_drafts();
        for draft in &mut self.drafts {
            if draft.job.take().is_some() {
                draft.error = Some("Creation was interrupted".into());
                self.drafts_dirty = true;
            }
        }
        self.refresh_draft_rows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(directory: &Path) -> DraftSetup {
        DraftSetup {
            provider: ProviderId::Claude,
            cwd: directory.into(),
            model: None,
            effort: None,
            branch: "main".into(),
        }
    }
    #[test]
    fn draft_keeps_identity_selection_and_exact_input_across_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let mut view = View::default();
        let first = view.add_draft(setup(directory.path()));
        view.set_active_panel(Panel::Details);
        view.handle_paste("probe");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.close_selector();
        view.set_active_panel(Panel::Prompt);
        let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
        view.handle_paste(text);
        view.set_sessions(Vec::new(), false);
        assert_eq!(view.selected_draft, Some(first));
        assert!(view.selected_id().is_some());
        assert_eq!(view.input_box.editor.text(), text);
        let second = view.add_draft(setup(directory.path()));
        assert_ne!(first, second);
        assert_eq!(view.drafts.len(), 2);
        assert!(view.input_box.editor.is_empty());
    }
    #[test]
    fn worker_results_target_stable_draft_not_a_replaced_one() {
        let directory = tempfile::tempdir().unwrap();
        let mut view = View::default();
        let first = view.add_draft(setup(directory.path()));
        let second = view.add_draft(setup(directory.path()));
        view.draft_worktree_created(first, Ok((directory.path().join("stale"), "stale".into())));
        view.draft_worktree_created(
            second,
            Ok((directory.path().join("worktree"), "feature".into())),
        );
        assert_eq!(view.selected_draft, Some(second));
        assert_eq!(view.drafts.len(), 1);
        assert_eq!(view.drafts[0].setup.branch, "feature");
        view.draft_created(first, Ok("stale-job-id".into()));
        view.draft_created(second, Ok("real-job-id".into()));
        assert_eq!(view.drafts[0].job.as_deref(), Some("real-job-id"));
        view.draft_worktree_created(second, Err(std::io::Error::other("taken")));
        assert_eq!(view.drafts.len(), 1);
        assert_eq!(view.drafts[0].error.as_deref(), Some("taken"));
    }
}

#[cfg(test)]
mod persistence_flow_tests {
    use super::*;
    fn named() -> View {
        let mut view = View::default();
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: std::env::temp_dir(),
            model: None,
            effort: None,
            branch: String::new(),
        });
        view.set_active_panel(Panel::Details);
        view.handle_paste("Persistent task");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view
    }
    #[test]
    fn named_draft_is_an_active_row_with_a_stable_key() {
        let view = named();
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].status.label(), "New");
        assert!(view.selected_id().unwrap().id.starts_with("draft-"));
    }
    #[test]
    fn leaving_named_draft_keeps_its_prompt() {
        let mut view = named();
        view.close_selector();
        view.set_active_panel(Panel::Prompt);
        view.handle_paste("half prompt");
        view.leave_draft();
        assert_eq!(view.drafts.len(), 1);
        assert_eq!(view.drafts[0].input.editor.text(), "half prompt");
    }
    #[test]
    fn creation_failure_keeps_editable_prompt() {
        let mut view = named();
        view.close_selector();
        view.set_active_panel(Panel::Prompt);
        view.handle_paste("retry prompt");
        let id = view.drafts[0].id;
        view.submit_draft(id);
        view.draft_created(id, Err(io::Error::other("creation failed")));
        assert_eq!(view.drafts.len(), 1);
        assert!(!view.drafts[0].working);
        assert_eq!(view.input_box.editor.text(), "retry prompt");
    }
    #[test]
    fn named_draft_escape_opens_delete_confirmation() {
        let mut view = named();
        view.close_selector();
        view.set_active_panel(Panel::Prompt);
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.confirm_dialog.is_some());
        assert_eq!(view.drafts.len(), 1);
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::providers::Provider;

    fn session() -> Session {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/probe");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_bytes!("../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        crate::providers::claude::ClaudeProvider::new(dir.path().into())
            .refresh(1)
            .sessions
            .remove(0)
    }
    fn named(view: &mut View, path: &Path, name: &str) -> DraftId {
        let id = view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: path.into(),
            model: None,
            effort: None,
            branch: String::new(),
        });
        view.set_active_panel(Panel::Details);
        view.handle_paste(name);
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.close_selector();
        view.set_active_panel(Panel::Prompt);
        id
    }
    #[test]
    fn two_drafts_restore_fields_text_images_and_focus_when_selected_by_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = View::default();
        let first = named(&mut view, dir.path(), "First");
        view.handle_paste("half prompt");
        view.attach_image(
            crate::images::ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap(),
        );
        let paths = view.input_box.editor.image_paths();
        let text = view.input_box.editor.text().to_owned();
        view.drafts[0].setup.model = Some("opus".into());
        let second = named(&mut view, dir.path(), "Second");
        view.handle_paste("second prompt");
        view.set_sessions(Vec::new(), false);
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[0].id.id, second.to_string());
        let index = view
            .rows
            .iter()
            .position(|row| row.id.id == first.to_string())
            .unwrap();
        view.select(index);
        assert_eq!(view.selected_id().unwrap().id, first.to_string());
        assert_eq!(view.input_box.editor.text(), text);
        assert_eq!(view.input_box.editor.image_paths(), paths);
        assert_eq!(
            view.selected_draft().unwrap().setup.model.as_deref(),
            Some("opus")
        );
        assert!(view.prompt_focused());
        assert!(view.selector.is_none());
        view.select(
            view.rows
                .iter()
                .position(|row| row.id.id == second.to_string())
                .unwrap(),
        );
        assert_eq!(view.input_box.editor.text(), "second prompt");
        for path in paths {
            std::fs::remove_file(path).unwrap();
        }
    }
    #[test]
    fn draft_sort_filter_and_attention_follow_named_but_keep_unnamed_modal() {
        let dir = tempfile::tempdir().unwrap();
        for named_draft in [false, true] {
            let mut view = View::default();
            let mut row = session();
            row.status = SessionStatus::Working;
            view.set_sessions(vec![row.clone()], false);
            let id = if named_draft {
                named(&mut view, dir.path(), "Draft")
            } else {
                view.add_draft(DraftSetup {
                    provider: ProviderId::Claude,
                    cwd: dir.path().into(),
                    model: None,
                    effort: None,
                    branch: String::new(),
                })
            };
            row.status = SessionStatus::NeedsInput;
            view.set_sessions(vec![row.clone()], false);
            if named_draft {
                assert_eq!(view.selected_id(), Some(&row.id));
                assert_eq!(view.rows[0].status, SessionStatus::NeedsInput);
                assert_eq!(view.rows[1].status, SessionStatus::New);
                view.status_filter = 1;
                assert!(view.visible_indices().is_empty());
            } else {
                assert_eq!(view.selected_draft, Some(id));
            }
        }
    }
    #[test]
    fn deleting_draft_by_backspace_uses_confirmation_and_persists_without_delete_action() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = View::default();
        named(&mut view, dir.path(), "Delete me");
        view.set_active_panel(Panel::TaskList);
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            None
        );
        assert_eq!(view.confirm_dialog.as_ref().unwrap().title, "Delete me");
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            None
        );
        assert!(view.drafts.is_empty());
        assert!(view.stored_drafts().is_empty());
        assert_eq!(view.take_drafts_action(), Some(Action::PersistDrafts));
    }
    #[test]
    fn load_repairs_missing_directory_branch_and_interrupted_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let mut original = View::default();
        named(&mut original, dir.path(), "Recover");
        let mut stored = original.stored_drafts();
        stored[0].cwd = dir.path().join("missing");
        stored[0].branch = DraftBranch::Existing("gone".into());
        stored[0].repository_confirmed = true;
        stored[0].job = Some("interrupted-job".into());
        let mut view = View::default();
        view.set_drafts(&stored);
        assert!(!view.drafts[0].repository_confirmed);
        assert_eq!(view.drafts[0].branch, DraftBranch::Unchosen);
        assert!(view.drafts[0].error.is_some());
        view.set_sessions(Vec::new(), false);
        assert!(view.drafts[0].job.is_none());
        assert_eq!(
            view.drafts[0].error.as_deref(),
            Some("Creation was interrupted")
        );
        let mut arrived = session();
        arrived.background_job = Some(crate::model::BackgroundJob {
            id: "interrupted-job".into(),
            needs: None,
            awaits_prompt: false,
        });
        let key = arrived.id.clone();
        let mut view = View::default();
        view.set_drafts(&stored);
        view.set_sessions(vec![arrived], false);
        assert!(view.drafts.is_empty());
        assert_eq!(view.selected_id(), Some(&key));
        assert!(view.stored_drafts().is_empty());
    }
    #[test]
    fn loaded_branch_tracks_checkout_changes_and_missing_local_branch() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(
                status.status.success(),
                "{}",
                String::from_utf8_lossy(&status.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&[
            "-c",
            "user.name=Probe",
            "-c",
            "user.email=probe@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "probe",
        ]);
        git(&["branch", "feature"]);
        let mut original = View::default();
        named(&mut original, dir.path(), "Branch recovery");
        let mut stored = original.stored_drafts();
        stored[0].branch = DraftBranch::Existing("main".into());
        git(&["checkout", "feature"]);
        let mut view = View::default();
        view.set_drafts(&stored);
        assert_eq!(view.drafts[0].setup.branch, "feature");
        assert_eq!(view.drafts[0].branch, DraftBranch::Existing("main".into()));
        git(&["branch", "-D", "main"]);
        let mut view = View::default();
        view.set_drafts(&stored);
        assert_eq!(view.drafts[0].branch, DraftBranch::Unchosen);
        assert!(view.drafts[0].error.is_some());
    }
}

#[cfg(test)]
mod confirmation_attention_tests {
    use super::*;
    use crate::providers::Provider;

    #[test]
    fn confirmation_cancel_enter_returns_to_draft_prompt() {
        let mut view = View::default();
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: std::env::temp_dir(),
            model: None,
            effort: None,
            branch: String::new(),
        });
        view.set_active_panel(Panel::Details);
        view.handle_paste("Keep this draft");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.close_selector();
        view.set_active_panel(Panel::Prompt);
        view.confirm_draft_delete();
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        assert!(view.confirm_dialog.is_none());
        assert!(view.prompt_focused());
    }

    #[test]
    fn confirmation_deletes_its_draft_after_attention_selects_another_session() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("projects/probe");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_bytes!("../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut row = crate::providers::claude::ClaudeProvider::new(directory.path().into())
            .refresh(1)
            .sessions
            .remove(0);
        let mut view = View::default();
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: directory.path().into(),
            model: None,
            effort: None,
            branch: String::new(),
        });
        view.set_active_panel(Panel::Details);
        view.handle_paste("Delete this draft");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.close_selector();
        view.set_active_panel(Panel::Prompt);
        view.confirm_draft_delete();
        row.status = SessionStatus::NeedsInput;
        let key = row.id.clone();
        view.set_sessions(vec![row], false);
        assert_eq!(view.selected_id(), Some(&key));
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            None
        );
        assert!(view.drafts.is_empty());
        assert_eq!(view.selected_id(), Some(&key));
        assert_eq!(view.take_drafts_action(), Some(Action::PersistDrafts));
    }
}
