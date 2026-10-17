//! Command center adapted from OpenAI Codex's TUI (Apache-2.0).
mod activity_strip;
pub(crate) mod commits;
mod draft_controls;
#[cfg(test)]
mod draft_flow_tests;
mod drafts;
use draft_controls::{BranchEditor, Field, Picker};
mod formatting;
pub(crate) use drafts::DraftSetup;
#[cfg(test)]
mod chronological_log_tests;
mod centering;
mod content;
mod detail_model;
mod highlight;
mod hints;
mod hyperlink;
mod input;
mod line_edit;
mod line_truncation;
mod links;
mod log;
mod log_search;
mod markdown;
mod navigation;
mod paths;
mod prompt_box;
mod question_flow;
mod question_view;
mod render;
mod rows;
mod selection_tabs;
mod shell_commands;
mod shortcut_help;
mod style;
mod text_selection;
mod text_selection_layout;
mod timeline;
mod title_row;
mod title_rule;
mod tool_detail;
mod usage_chart;
pub(crate) mod vscode;
pub use highlight::use_theme as use_highlight_theme;
#[cfg(test)]
pub use style::light_background;
pub use style::set_light_background;
pub(crate) use timeline::timeline_seconds;

use crate::model::{Action, Grouping, Session, SessionStatus};
use crate::providers::{Capabilities, ProviderId, SessionKey};
use chrono::{DateTime, Utc};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    path::PathBuf,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;

const PROMPT_MARKER: &str = "❯";
const DISABLED_PROMPT_MARKER: &str = "❮";

const TASK_FILTERS: &[(&str, &[SessionStatus])] = &[
    (
        "Active",
        &[
            SessionStatus::Working,
            SessionStatus::Ready,
            SessionStatus::NeedsInput,
        ],
    ),
    ("Inactive", &[SessionStatus::Inactive]),
];

/// How long a copy confirmation or a notice stays on screen.
const MESSAGE_LIFETIME: Duration = Duration::from_secs(1);

/// Longest gap between two clicks on one task that still opens it.
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(300);

fn tab_for_status(status: Option<SessionStatus>, current_tab: usize) -> usize {
    match status {
        Some(status) if TASK_FILTERS[current_tab].1.contains(&status) => current_tab,
        Some(SessionStatus::Ready | SessionStatus::Working | SessionStatus::NeedsInput) => 0,
        Some(SessionStatus::Inactive) => 1,
        None => 0,
    }
}

#[derive(Default)]
struct Viewport {
    scroll: usize,
    page_height: usize,
    tabs: Vec<(usize, Rect)>,
    tab_bar: Rect,
    list: Rect,
    list_rows: Rect,
    first_entry: usize,
    log: Rect,
    log_detail: Rect,
    prompt: Rect,
    field_targets: Vec<(Rect, Field)>,
    picker_rows: Vec<(Rect, usize)>,
    /// Index of the first visible event in the chronological shown log.
    log_scroll: usize,
    /// Events that arrived below the rows on screen since the log left its newest row.
    log_unseen: usize,
    /// Where the last render drew the button that jumps back to the newest log row.
    log_jump: Rect,
    log_borders: [Rect; 2],
    log_detail_jump: Rect,
    /// Index into the task log of each drawn log row, top down.
    log_rows: Vec<usize>,
    /// Lines of the selected event scrolled off the top.
    log_detail_scroll: usize,
    log_detail_max: usize,
    /// The selected event's layout, cached until its content or size changes.
    log_detail_layout: Option<log::DetailLayout>,
    log_detail_other_layout: Option<log::DetailLayout>,
    log_text: text_selection::TextRegion,
    details_text: text_selection::TextRegion,
    task_text: text_selection::TextRegion,
    log_list_text: text_selection::TextRegion,
    task_scroll_max: usize,
    details: Rect,
    details_scroll: DetailsScroll,
    /// Screen rows of each copyable details body drawn by the last render, with its full text.
    details_copy: Vec<(std::ops::Range<u16>, String, &'static str)>,
    details_background: Vec<(std::ops::Range<u16>, crate::model::BackgroundTask)>,
    /// Links drawn by the last render of each panel: the task list, the details and the log.
    list_links: Vec<links::DrawnLink>,
    details_links: Vec<links::DrawnLink>,
    log_links: Vec<links::DrawnLink>,
    /// Screen cell at the start of the selected row in the last render.
    marker: Option<(u16, u16)>,
}

#[derive(Default)]
struct DetailsScroll {
    reveal: Vec<usize>,
    hidden: Vec<usize>,
    bodies: Vec<std::ops::Range<usize>>,
    offset: usize,
    content_top: u16,
    /// Maximum plain offset; None when the budget layout applies.
    fallback_max: Option<usize>,
    /// A fingerprint of each body's text, parallel to `reveal`.
    keys: Vec<u64>,
    /// The body fingerprints the task showed when the user left it, until its first render back.
    left_keys: Option<Vec<u64>>,
}

struct BackgroundReturn {
    event: Option<usize>,
    detail_focused: bool,
    scroll: usize,
    search: log_search::LogSearch,
}

/// What a task's details and log panels showed when the user last left it.
#[derive(Default)]
struct TaskPanels {
    input_box: prompt_box::PromptBox,
    details_scroll: DetailsScroll,
    log_search: log_search::LogSearch,
}

#[derive(Clone, Copy)]
enum Removal {
    Archive,
    Delete,
}

/// Codex's confirmation before archiving or deleting a task.
struct ConfirmDialog {
    removal: Removal,
    id: SessionKey,
    title: String,
    confirm: bool,
}

impl ConfirmDialog {
    fn action(&self) -> Action {
        match self.removal {
            Removal::Archive => Action::Archive(self.id.clone()),
            Removal::Delete => Action::Delete(self.id.clone()),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Panel {
    #[default]
    TaskList,
    LogList,
    LogDetail,
    Details,
    Prompt,
    Other,
}

pub struct View {
    mouse_focus: Panel,
    startup_cwd: PathBuf,
    drafts: Vec<drafts::Draft>,
    selected_draft: Option<crate::model::DraftId>,
    next_draft: crate::model::DraftId,
    input_choices: HashMap<ProviderId, crate::providers::InputChoices>,
    picker: Option<Picker>,
    branch_editor: Option<BranchEditor>,
    pub(crate) log: Vec<crate::log::Event>,
    pub(crate) log_session: Option<SessionKey>,
    pub(crate) log_search: log_search::LogSearch,
    /// The selected event, shared by the list cursor and detail panel.
    log_selected: Option<usize>,
    log_background_event: Option<crate::log::Event>,
    log_detail_focused: bool,
    log_detail_raw: bool,
    log_list_focused: bool,
    /// Older wheel steps waiting for App to extend the loaded transcript window.
    log_pending_older: usize,
    pub(crate) log_start_requested: bool,
    log_background_return: Option<BackgroundReturn>,
    log_detail_search: log_search::LogSearch,
    input_box: prompt_box::PromptBox,
    prompt_history: std::rc::Rc<RefCell<Vec<line_edit::LineEdit>>>,
    questions: question_flow::Questions,
    capabilities: HashMap<ProviderId, Capabilities>,
    rows: Vec<Session>,
    project_groups: Vec<ProjectGroup>,
    selected: usize,
    grouping: Grouping,
    use_colors: bool,
    pinned: Vec<SessionKey>,
    hidden: HashSet<SessionKey>,
    status_filter: usize,
    /// The task each tab shows selected when it is entered, by tab index.
    tab_selections: Vec<Option<SessionKey>>,
    /// The task the user chose last, or the refresh followed to a new tab.
    chosen: Option<SessionKey>,
    search: line_edit::LineEdit,
    searching: bool,
    rename_target: Option<SessionKey>,
    rename: line_edit::LineEdit,
    confirm_dialog: Option<ConfirmDialog>,
    help: bool,
    loading: bool,
    has_more: bool,
    /// A notice at the task list's bottom-left, shown until it expires.
    notice: Option<(String, Instant)>,
    viewport: RefCell<Viewport>,
    /// Task row and time of the last left click in the list.
    last_click: Option<(usize, Instant)>,
    double_click_interval: Duration,
    pending_click: Option<input::PendingClick>,
    last_detail_click: Option<(SessionKey, Option<usize>, Instant)>,
    /// Button, origin and drag state of the held mouse press.
    mouse_press: Option<(MouseButton, u16, u16, bool)>,
    /// Text under the last left click on a copyable body, and when it happened.
    last_copy_click: Option<(String, Instant)>,
    /// The confirmation of the last copy, shown at the footer's right end until it expires.
    copied: Option<(String, Instant)>,
    text_selection: RefCell<Option<text_selection::Selection>>,
    /// Accent slot of each task, handed out in the order tasks are first shown.
    title_slots: RefCell<HashMap<SessionKey, usize>>,
    /// The task whose details and log panels are on screen.
    panels_task: Option<SessionKey>,
    /// The panels of each task the user left, by task.
    task_panels: HashMap<SessionKey, TaskPanels>,
    /// The usage bins of the task the app read last.
    usage: Option<(
        SessionKey,
        std::collections::BTreeMap<i64, crate::usage::Bin>,
    )>,
    /// How many times taller than wide one terminal cell looks.
    cell_aspect: f64,
    /// The timeline bins some task worked in.
    activity: std::collections::BTreeSet<i64>,
}

/// Cell height over width assumed when the terminal reports no pixels.
const DEFAULT_CELL_ASPECT: f64 = 2.0;

impl Default for View {
    fn default() -> Self {
        Self::new(Grouping::Project, true)
    }
}

/// Sessions in the same checkout and subdirectory share a project; linked worktrees join their primary checkout.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectGroup {
    key: (PathBuf, PathBuf),
    heading: PathBuf,
}

impl ProjectGroup {
    fn directory(&self) -> Option<&Path> {
        (!self.heading.as_os_str().is_empty()).then_some(self.heading.as_path())
    }

    fn key(&self) -> (bool, &(PathBuf, PathBuf)) {
        (self.directory().is_none(), &self.key)
    }

    fn for_session(session: &Session) -> Self {
        match crate::repository::repository_identity(&session.cwd) {
            Some(identity) => Self {
                key: (identity.common_dir, identity.relative_cwd.clone()),
                heading: identity.primary_root.join(identity.relative_cwd),
            },
            None => Self {
                key: (session.cwd.clone(), PathBuf::new()),
                heading: session.cwd.clone(),
            },
        }
    }
}

fn session_order(session: &Session) -> (std::cmp::Reverse<DateTime<Utc>>, &SessionKey) {
    (std::cmp::Reverse(session.updated_at), &session.id)
}

impl View {
    /// Underlines every link the last frame drew in `buf`.
    pub fn underline_links(&self, buf: &mut Buffer) {
        let viewport = self.viewport.borrow();
        for links in [
            &viewport.list_links,
            &viewport.details_links,
            &viewport.log_links,
        ] {
            links::underline(buf, links);
        }
    }

    pub fn new(grouping: Grouping, use_colors: bool) -> Self {
        Self {
            mouse_focus: Panel::TaskList,
            startup_cwd: std::env::current_dir().expect("dashboard startup directory is available"),
            log: Vec::new(),
            log_session: None,
            log_selected: None,
            log_background_event: None,
            log_detail_focused: false,
            log_detail_raw: false,
            log_list_focused: false,
            log_pending_older: 0,
            log_start_requested: false,
            log_background_return: None,
            log_detail_search: log_search::LogSearch::default(),
            log_search: log_search::LogSearch::default(),
            drafts: Vec::new(),
            selected_draft: None,
            next_draft: 1,
            input_choices: HashMap::new(),
            picker: None,
            branch_editor: None,
            input_box: prompt_box::PromptBox::default(),
            prompt_history: Default::default(),
            questions: question_flow::Questions::default(),
            capabilities: HashMap::new(),
            rows: Vec::new(),
            project_groups: Vec::new(),
            selected: usize::MAX,
            grouping,
            use_colors,
            pinned: Vec::new(),
            hidden: HashSet::new(),
            status_filter: 0,
            tab_selections: vec![None; TASK_FILTERS.len()],
            chosen: None,
            search: line_edit::LineEdit::default(),
            searching: false,
            rename_target: None,
            rename: line_edit::LineEdit::default(),
            confirm_dialog: None,
            help: false,
            loading: false,
            has_more: false,
            notice: None,
            viewport: RefCell::new(Viewport::default()),
            last_click: None,
            double_click_interval: DOUBLE_CLICK_INTERVAL,
            pending_click: None,
            last_detail_click: None,
            mouse_press: None,
            last_copy_click: None,
            copied: None,
            text_selection: RefCell::new(None),
            title_slots: RefCell::new(HashMap::new()),
            panels_task: None,
            task_panels: HashMap::new(),
            usage: None,
            cell_aspect: DEFAULT_CELL_ASPECT,
            activity: std::collections::BTreeSet::new(),
        }
    }

    /// Takes the terminal's cell height over width, or a common one when the terminal reports
    /// no pixels.
    pub(crate) fn set_cell_aspect(&mut self, aspect: Option<f64>) {
        self.cell_aspect = aspect.unwrap_or(DEFAULT_CELL_ASPECT);
    }

    pub(crate) fn set_double_click_ms(&mut self, milliseconds: u64) {
        self.double_click_interval = Duration::from_millis(milliseconds);
    }

    pub(crate) fn pasting_prompt(&self) -> bool {
        self.confirm_dialog.is_none()
            && !self.help
            && self.picker.is_none()
            && self.branch_editor.is_none()
            && self.input_box.focused
            && !self.input_box.searching_history()
            && self.input_disabled().is_none()
    }

    pub(crate) fn attach_image(&mut self, image: crate::images::ImageAttachment) {
        if self.pasting_prompt() {
            self.input_box.editor.insert_image(image);
        }
    }

    pub(crate) fn handle_paste(&mut self, text: &str) {
        if self.confirm_dialog.is_some() || self.help || self.picker.is_some() {
            return;
        }
        if self.question_paste(text) {
            return;
        }
        if let Some(editor) = &mut self.branch_editor {
            editor.editor.insert(text);
        } else if self.input_box.focused {
            self.input_box.paste(text);
        } else if self.rename_target.is_some() {
            self.rename.insert(text);
        } else if self.log_detail_search.editing {
            self.log_detail_search.query.insert(text);
            self.jump_detail_match();
        } else if self.log_search.editing {
            self.log_search.query.insert(text);
            self.rematch_log();
            self.show_newest_log();
        } else if self.searching {
            self.search.insert(text);
            let selected = self
                .selectable_indices()
                .first()
                .copied()
                .unwrap_or(usize::MAX);
            self.select(selected);
        }
    }

    fn input_label(&self) -> String {
        if self.selected_draft().is_some() {
            return "New task".into();
        }
        self.selected_row()
            .map(|session| session.display_title().to_owned())
            .unwrap_or_else(|| "No task".into())
    }

    fn input_disabled(&self) -> Option<&'static str> {
        if let Some(draft) = self.selected_draft() {
            return (draft.working || draft.job.is_some())
                .then_some("Creating task; waiting for session");
        }
        match self.selected_row() {
            Some(session) => session.input_disabled_reason(),
            None => Some("Select a task to type"),
        }
    }

    fn focus_input(&mut self) {
        if self.input_disabled().is_none() {
            self.input_box.focused = true;
        }
    }

    fn task_input(&mut self, id: &SessionKey) -> &mut prompt_box::PromptBox {
        if self.selected_id() == Some(id) {
            &mut self.input_box
        } else {
            &mut self.task_panels.entry(id.clone()).or_default().input_box
        }
    }

    pub(crate) fn input_sending(
        &mut self,
        id: &SessionKey,
        input: &crate::providers::SessionInput,
    ) {
        if let crate::providers::SessionInput::Answer { ask, .. } = input {
            self.questions.begin(ask);
            return;
        }
        let target = self.task_input(id);
        if let crate::providers::SessionInput::Prompt(text)
        | crate::providers::SessionInput::PromptWithImages { text, .. } = input
        {
            target.begin_send(text);
        }
        target.state = prompt_box::SendState::Sending;
    }

    pub(crate) fn input_complete(
        &mut self,
        id: &SessionKey,
        input: &crate::providers::SessionInput,
        result: std::io::Result<()>,
    ) {
        if let crate::providers::SessionInput::Answer { ask, .. } = input {
            self.questions.complete(ask, &result);
            if let Err(error) = result {
                self.set_notice(Some(error.to_string()));
            }
            return;
        }
        let target = self.task_input(id);
        let error = result.as_ref().err().map(ToString::to_string);
        if matches!(
            input,
            crate::providers::SessionInput::Prompt(_)
                | crate::providers::SessionInput::PromptWithImages { .. }
        ) {
            target.finish_send(result.is_ok());
        }
        target.state = match result {
            Ok(()) => prompt_box::SendState::Ready,
            Err(error) => prompt_box::SendState::Failed(error.to_string()),
        };
        if let Some(error) = error {
            self.set_notice(Some(error));
        }
    }

    pub(crate) fn input_failed(&mut self, reason: String) {
        self.input_box.state = prompt_box::SendState::Failed(reason.clone());
        self.set_notice(Some(reason));
    }

    pub fn set_capabilities(&mut self, capabilities: HashMap<ProviderId, Capabilities>) {
        self.capabilities = capabilities;
    }

    /// Replace rows while retaining the selected session and view preferences.
    pub fn set_sessions(&mut self, mut sessions: Vec<Session>, has_more: bool) {
        let selected = self.selected_id().cloned();
        let selected_parent = self.selected_row().and_then(|s| s.parent.clone());
        let status_filter = self.status_filter;
        let show_more = self.selected == usize::MAX && self.has_more;
        sessions.sort_by(|a, b| {
            a.status
                .cmp(&b.status)
                .then_with(|| session_order(a).cmp(&session_order(b)))
        });
        self.project_groups = sessions.iter().map(ProjectGroup::for_session).collect();
        self.rows = sessions;
        let rows = &self.rows;
        self.task_panels
            .retain(|id, _| rows.iter().any(|session| &session.id == id));
        self.has_more = has_more;
        let mut selected_index = selected
            .as_ref()
            .and_then(|id| self.rows.iter().position(|s| &s.id == id));
        if selected.is_some()
            && selected_index.is_none_or(|index| !self.visible_indices().contains(&index))
        {
            let status = selected_index.map(|index| self.rows[index].status);
            if self.rename_target.as_ref() == selected.as_ref()
                && status == Some(SessionStatus::Inactive)
            {
                self.status_filter = TASK_FILTERS.len() - 1;
            } else if let Some(parent) = selected_parent {
                selected_index = self
                    .visible_indices()
                    .into_iter()
                    .find(|&index| self.rows[index].id == parent);
            } else {
                self.status_filter = tab_for_status(status, self.status_filter);
            }
        }
        self.selected = if show_more {
            usize::MAX
        } else {
            selected_index.unwrap_or_else(|| self.fallback_selection())
        };
        self.reconcile_selection();
        if self.status_filter != status_filter && self.selected_id() == selected.as_ref() {
            self.remember_selection();
        }
        self.reconcile_drafts();
        self.show_selected_panels();
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }
    pub fn set_pinned(&mut self, ids: &[SessionKey]) {
        self.pinned = ids.to_vec();
        self.reconcile_selection();
    }
    /// Lists a hidden task again, as when its deletion failed.
    pub fn unhide(&mut self, id: &SessionKey) {
        self.hidden.remove(id);
    }
    pub fn hide(&mut self, id: &SessionKey) {
        let visible = self.selectable_indices();
        let position = visible
            .iter()
            .position(|i| *i == self.selected)
            .unwrap_or(0);
        let selected = self.selected_id() == Some(id);
        self.hidden.insert(id.to_owned());
        if selected {
            let visible = self.selectable_indices();
            let selected = visible
                .get(position.min(visible.len().saturating_sub(1)))
                .copied()
                .unwrap_or(usize::MAX);
            self.select(selected);
        }
        self.reconcile_selection();
    }
    /// Shows `notice` at the task list's bottom-left for `MESSAGE_LIFETIME`.
    pub fn set_notice(&mut self, notice: Option<String>) {
        self.set_notice_at(notice, Instant::now());
    }

    fn set_notice_at(&mut self, notice: Option<String>, now: Instant) {
        self.notice = notice.map(|text| (text, now + MESSAGE_LIFETIME));
    }

    /// Shows `label` at the footer's right end until `MESSAGE_LIFETIME` after `now`.
    pub fn show_copied(&mut self, label: String, now: Instant) {
        self.copied = Some((label, now + MESSAGE_LIFETIME));
    }

    /// Clears an expired copy confirmation and notice, and returns how long the first of the
    /// shown ones has left.
    pub fn expire_messages(&mut self, now: Instant) -> Option<Duration> {
        let copied = expire(&mut self.copied, now);
        let notice = expire(&mut self.notice, now);
        copied.into_iter().chain(notice).min()
    }
    /// Where the selected row starts, so the terminal cursor can rest on it.
    pub fn marker_pos(&self) -> Option<(u16, u16)> {
        self.viewport.borrow().marker
    }

    pub fn grouping(&self) -> Grouping {
        self.grouping
    }
    pub fn has_background(&self) -> bool {
        self.selected_row().is_some_and(|session| {
            !session.background.is_empty() || self.subagents_for(session).next().is_some()
        })
    }
    fn subagents_for<'a>(&'a self, session: &'a Session) -> impl Iterator<Item = &'a Session> {
        let mut children: Vec<_> = self
            .rows
            .iter()
            .filter(|child| {
                session.parent.is_none()
                    && child.parent.as_ref() == Some(&session.id)
                    && child.status == SessionStatus::Working
            })
            .collect();
        children.sort_by_key(|child| (child.started_at, &child.id));
        children.into_iter()
    }

    pub fn selected_id(&self) -> Option<&SessionKey> {
        self.selected_row().map(|s| &s.id)
    }
    fn selected_row(&self) -> Option<&Session> {
        if self.selected_draft.is_some() {
            return None;
        }
        self.rows
            .get(self.selected)
            .filter(|_| self.visible_indices().contains(&self.selected))
    }
    fn group_index(&self, index: usize) -> Option<usize> {
        match self.rows[index].parent.as_ref() {
            None => Some(index),
            Some(parent) => self
                .rows
                .iter()
                .position(|s| s.parent.is_none() && &s.id == parent),
        }
    }
    fn is_pinned(&self, index: usize) -> bool {
        self.group_index(index)
            .is_some_and(|parent| self.pinned.contains(&self.rows[parent].id))
    }
    fn same_group(&self, grouping: Grouping, left: usize, right: usize) -> bool {
        if grouping == Grouping::Status && self.rows[left].status != self.rows[right].status {
            return false;
        }
        let (Some(left), Some(right)) = (self.group_index(left), self.group_index(right)) else {
            return false;
        };
        if self.is_pinned(left) || self.is_pinned(right) {
            return self.is_pinned(left) && self.is_pinned(right);
        }
        match grouping {
            Grouping::Project => {
                self.project_groups[left].key() == self.project_groups[right].key()
            }
            Grouping::Status => true,
            Grouping::Model => self.rows[left].model_name() == self.rows[right].model_name(),
        }
    }
    fn matches_search(&self, s: &Session) -> bool {
        let search = self.search.text().to_lowercase();
        search.is_empty()
            || format!(
                "{} {} {} {}",
                s.name.as_deref().unwrap_or(""),
                s.title,
                s.prompt,
                s.cwd.display()
            )
            .to_lowercase()
            .contains(&search)
    }
    fn visible_indices(&self) -> Vec<usize> {
        let filter = TASK_FILTERS[self.status_filter].1;
        let matches = |s: &Session| self.matches_search(s);
        let mut children = std::collections::HashMap::<&SessionKey, Vec<usize>>::new();
        for (index, session) in self.rows.iter().enumerate() {
            if let Some(parent) = session.parent.as_ref()
                && !self.hidden.contains(&session.id)
                && filter.contains(&session.status)
            {
                children.entry(parent).or_default().push(index);
            }
        }
        for siblings in children.values_mut() {
            siblings.sort_by_key(|&index| session_order(&self.rows[index]));
        }
        let (mut pinned, mut visible): (Vec<_>, Vec<_>) = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.parent.is_none()
                    && !self.hidden.contains(&s.id)
                    && ((filter.contains(&s.status) && matches(s))
                        || children.get(&s.id).is_some_and(|indices| {
                            indices
                                .iter()
                                .any(|&i| matches(s) || matches(&self.rows[i]))
                        }))
            })
            .map(|(i, _)| i)
            .partition(|i| self.is_pinned(*i));
        pinned.sort_by_key(|i| self.pinned.iter().position(|id| *id == self.rows[*i].id));
        match self.grouping {
            Grouping::Project => visible
                .sort_by_key(|i| (self.project_groups[*i].key(), session_order(&self.rows[*i]))),
            Grouping::Status => {}
            Grouping::Model => {
                visible.sort_by_key(|i| (self.rows[*i].model_name(), session_order(&self.rows[*i])))
            }
        }
        pinned.extend(visible);
        let mut rows = Vec::new();
        for parent in pinned {
            if filter.contains(&self.rows[parent].status) {
                rows.push(parent);
            }
            if let Some(siblings) = children.get(&self.rows[parent].id) {
                rows.extend(
                    siblings
                        .iter()
                        .copied()
                        .filter(|&i| matches(&self.rows[parent]) || matches(&self.rows[i])),
                );
            }
        }
        if self.grouping == Grouping::Status {
            rows.sort_by_key(|&index| (!self.is_pinned(index), self.rows[index].status));
        }
        rows
    }
    fn selectable_indices(&self) -> Vec<usize> {
        let mut visible = self.visible_indices();
        if self.has_more && TASK_FILTERS[self.status_filter].1 == [SessionStatus::Inactive] {
            visible.push(usize::MAX);
        }
        visible
    }
    pub(crate) fn log_scroll(&self) -> usize {
        self.viewport.borrow().log_scroll
    }

    pub(crate) fn clamp_log_scroll(&mut self, max: usize) {
        let mut viewport = self.viewport.borrow_mut();
        viewport.log_scroll = viewport.log_scroll.min(max);
    }

    pub(crate) fn log_max_scroll(&self) -> usize {
        let height = usize::from(
            self.viewport
                .borrow()
                .log
                .height
                .saturating_sub(u16::from(self.log_search.active())),
        );
        self.matching_log_count().saturating_sub(height)
    }

    pub(crate) fn log_read_count(&self, visible: usize, has_older: bool) -> usize {
        let loaded = self.log.len().max(visible);
        if has_older
            && (self.log_page_pending() || (self.log_scroll() == 0 && !self.following_log()))
        {
            loaded.saturating_add(visible.max(self.log_pending_older).max(1))
        } else {
            loaded
        }
    }

    /// Selects the newest shown event and resumes following the log.
    fn show_newest_log(&mut self) {
        self.select_log_event(None);
        self.log_detail_search.clear();
        let max = self.log_max_scroll();
        let mut viewport = self.viewport.borrow_mut();
        viewport.log_scroll = max;
        viewport.log_detail_scroll = 0;
        viewport.log_unseen = 0;
        self.log_pending_older = 0;
        self.log_start_requested = false;
        self.log_background_return = None;
    }

    fn set_selected(&mut self, selected: usize) {
        self.leave_draft();
        self.selected = selected;
        self.show_selected_panels();
    }

    /// Keeps the details and log panels with their task: saves what the task the user left
    /// showed, and shows what the selected task showed when the user last left it.
    fn show_selected_panels(&mut self) {
        self.sync_questions();
        let selected = self.selected_id().cloned();
        if self.panels_task == selected {
            return;
        }
        self.text_selection.borrow_mut().take();
        let viewport = self.viewport.get_mut();
        self.input_box.focused = false;
        let left = TaskPanels {
            input_box: std::mem::take(&mut self.input_box),
            details_scroll: std::mem::take(&mut viewport.details_scroll),
            log_search: std::mem::take(&mut self.log_search),
        };
        if let Some(id) = self.panels_task.take() {
            self.task_panels.insert(id, left);
        }
        let mut shown = selected
            .as_ref()
            .and_then(|id| self.task_panels.remove(id))
            .unwrap_or_default();
        if shown.details_scroll.left_keys.is_none() {
            shown.details_scroll.left_keys = Some(std::mem::take(&mut shown.details_scroll.keys));
        }
        viewport.details_scroll = shown.details_scroll;
        viewport.log_scroll = 0;
        viewport.log_unseen = 0;
        viewport.log_detail_scroll = 0;
        viewport.log_detail_layout = None;
        viewport.log_detail_other_layout = None;
        self.input_box = shown.input_box;
        self.log_search = shown.log_search;
        self.log_selected = None;
        self.log_background_event = None;
        self.log_detail_focused = false;
        self.log_list_focused = false;
        self.log_pending_older = 0;
        self.log_start_requested = false;
        self.log_background_return = None;
        self.log_detail_search.clear();
        self.pending_click = None;
        self.panels_task = selected;
    }

    /// Selects the row the user chose.
    fn select(&mut self, selected: usize) {
        self.set_selected(selected);
        self.remember_selection();
    }

    /// Makes the selected task the selection of every tab that lists it.
    fn remember_selection(&mut self) {
        let Some((id, status)) = self.selected_row().map(|s| (s.id.clone(), s.status)) else {
            return;
        };
        for (tab, (_, statuses)) in TASK_FILTERS.iter().enumerate() {
            if statuses.contains(&status) {
                self.tab_selections[tab] = Some(id.clone());
            }
        }
        self.chosen = Some(id);
    }

    fn reconcile_selection(&mut self) {
        if self.selected_draft.is_some() && self.status_filter == 0 {
            return;
        }
        let visible = self.selectable_indices();
        if let Some(id) = &self.rename_target
            && !visible
                .iter()
                .any(|i| self.rows.get(*i).is_some_and(|s| &s.id == id))
        {
            self.rename_target = None;
            self.rename.clear();
        }
        if !visible.contains(&self.selected) {
            self.set_selected(self.fallback_selection());
        }
    }

    /// The row a tab selects when no task chosen in it is still listed: the task or subagent that
    /// waited longest. Show more stands for tasks older than every listed one, so it wins where it
    /// is listed.
    fn fallback_selection(&self) -> usize {
        let selectable = self.selectable_indices();
        if selectable.contains(&usize::MAX) {
            return usize::MAX;
        }
        let prefer_active = self.status_filter == 0
            && selectable
                .iter()
                .any(|&index| self.rows[index].status != SessionStatus::Inactive);
        selectable
            .into_iter()
            .filter(|&index| !prefer_active || self.rows[index].status != SessionStatus::Inactive)
            .max_by_key(|&index| session_order(&self.rows[index]))
            .unwrap_or(usize::MAX)
    }
    /// Gives a task the next accent the first time it is shown, so neighbors differ and the task
    /// keeps its color while the list changes.
    fn title_color(&self, id: &SessionKey) -> Color {
        let mut slots = self.title_slots.borrow_mut();
        let next = slots.len();
        style::thread_color(*slots.entry(id.clone()).or_insert(next))
    }
    fn title_style(&self, id: &SessionKey) -> Style {
        if self.use_colors {
            Style::default().fg(self.title_color(id))
        } else {
            Style::default()
        }
    }
    fn status(session: &Session) -> (&'static str, Span<'static>) {
        (
            session.status.label(),
            Self::status_indicator(session.status),
        )
    }
    fn status_indicator(status: SessionStatus) -> Span<'static> {
        match status {
            SessionStatus::NeedsInput => "●".red(),
            SessionStatus::Working => "●".green(),
            SessionStatus::Ready => "○".cyan(),
            SessionStatus::Inactive => "○".dim(),
        }
    }
}

#[cfg(test)]
mod rename_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod text_selection_tests;

/// Clears `message` once `now` reaches its expiry, and returns how long it has left.
fn expire(message: &mut Option<(String, Instant)>, now: Instant) -> Option<Duration> {
    let (_, expires_at) = message.as_ref()?;
    let left = expires_at
        .checked_duration_since(now)
        .filter(|left| !left.is_zero());
    if left.is_none() {
        *message = None;
    }
    left
}

#[cfg(test)]
mod background_order_tests;
