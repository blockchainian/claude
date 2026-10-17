use super::*;
use crate::model::DraftId;
use crate::providers::InputChoices;
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
    pub setup: DraftSetup,
    /// The task's name, typed before its first prompt.
    pub name: Option<String>,
    pub input: prompt_box::PromptBox,
    /// The branch the task runs on instead of the repository's checked-out one.
    pub branch_choice: Option<crate::tasks::BranchChoice>,
    pub job: Option<String>,
    pub working: bool,
    pub error: Option<String>,
}

impl Draft {
    pub(super) fn heading(&self) -> PathBuf {
        match crate::repository::repository_identity(&self.setup.cwd) {
            Some(repository) => repository.primary_root.join(repository.relative_cwd),
            None => self.setup.cwd.clone(),
        }
    }
}

impl View {
    pub(crate) fn set_input_choices(&mut self, choices: HashMap<ProviderId, InputChoices>) {
        self.input_choices = choices;
    }

    pub(crate) fn add_draft(&mut self, setup: DraftSetup) -> DraftId {
        let id = self.next_draft;
        self.next_draft += 1;
        self.drafts.push(Draft {
            id,
            setup,
            name: None,
            input: prompt_box::PromptBox::default(),
            branch_choice: None,
            job: None,
            working: false,
            error: None,
        });
        self.status_filter = 0;
        self.select_draft(id);
        self.open_repositories(self.startup_cwd.clone(), None);
        self.input_box.focused = true;
        id
    }

    /// Moves the draft into its new worktree; its task is created there next.
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
        draft.setup.branch = branch;
        draft.branch_choice = None;
    }

    pub(crate) fn draft_created(&mut self, id: DraftId, result: io::Result<String>) {
        let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) else {
            return;
        };
        draft.working = false;
        match result {
            Ok(job) => {
                draft.job = Some(job);
                draft.error = None;
            }
            Err(error) => {
                let reason = error.to_string();
                draft.error = Some(reason.clone());
                if self.selected_draft == Some(id) {
                    self.input_box.state = prompt_box::SendState::Failed(reason);
                } else {
                    draft.input.state = prompt_box::SendState::Failed(reason);
                }
            }
        }
    }

    pub(crate) fn draft_directory_changed(&mut self, id: DraftId, cwd: PathBuf, branch: String) {
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) {
            draft.setup.cwd = cwd;
            draft.setup.branch = branch;
            draft.error = None;
        }
    }

    pub(super) fn selected_draft(&self) -> Option<&Draft> {
        self.selected_draft
            .and_then(|id| self.drafts.iter().find(|draft| draft.id == id))
    }

    /// Leaves the selected draft: drops it unless its task is already being created, which keeps it
    /// until the task's session arrives.
    pub(super) fn leave_draft(&mut self) {
        if let Some(id) = self.selected_draft.take() {
            self.close_draft_selector(id);
            self.drafts
                .retain(|draft| draft.id != id || draft.job.is_some());
            self.input_box = prompt_box::PromptBox::default();
        }
    }

    /// Removes the selected draft unless its task is already being created.
    pub(super) fn discard_selected_draft(&mut self) {
        if self
            .selected_draft()
            .is_some_and(|draft| draft.working || draft.job.is_some())
        {
            return;
        }
        self.leave_draft();
        self.reconcile_selection();
        self.show_selected_panels();
    }

    pub(super) fn select_draft(&mut self, id: DraftId) {
        if self.selected_draft == Some(id) {
            return;
        }
        self.leave_draft();
        self.selected_draft = Some(id);
        self.show_selected_panels();
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id) {
            self.input_box = std::mem::take(&mut draft.input);
        }
    }

    pub(super) fn reconcile_drafts(&mut self) {
        let selected = self.selected_draft;
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
        self.drafts
            .retain(|draft| !arrived.iter().any(|(id, _)| *id == draft.id));
        if let Some((_, session)) = arrived.into_iter().find(|(id, _)| Some(*id) == selected) {
            self.selected_draft = None;
            self.input_box = prompt_box::PromptBox::default();
            if let Some(index) = self.rows.iter().position(|row| row.id == session) {
                self.select(index);
            }
        }
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
        let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
        view.handle_paste(text);
        view.set_sessions(Vec::new(), false);
        assert_eq!(view.selected_draft, Some(first));
        assert_eq!(view.selected_id(), None);
        assert_eq!(view.input_box.editor.text(), text);
        let second = view.add_draft(setup(directory.path()));
        assert_ne!(first, second);
        assert_eq!(view.drafts.len(), 1);
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
        view.draft_worktree_created(second, Err(std::io::Error::other("taken")));
        assert_eq!(view.drafts[0].error.as_deref(), Some("taken"));
        view.draft_created(first, Ok("stale-job-id".into()));
        view.draft_created(second, Ok("real-job-id".into()));
        assert_eq!(view.drafts[0].job.as_deref(), Some("real-job-id"));
    }
}
