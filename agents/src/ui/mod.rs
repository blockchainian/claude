//! Command center adapted from OpenAI Codex's TUI (Apache-2.0).
pub(crate) mod commits;
mod draft_controls;
#[cfg(test)]
mod draft_flow_tests;
mod drafts;
use draft_controls::{Field, Selector};
mod footer;
mod formatting;
pub(crate) use drafts::DraftSetup;
mod centering;
#[cfg(test)]
mod chronological_event_tests;
mod content;
mod event_agent;
mod event_detail;
mod event_list;
#[cfg(test)]
mod event_messages_tests;
#[cfg(test)]
#[path = "event_scroll_tests.rs"]
mod event_scroll_tests;
mod event_search;
pub(crate) mod highlight;
mod hints;
mod hyperlink;
mod inline_images;
mod input;
mod key_name;
mod line_edit;
mod line_truncation;
mod links;
mod markdown;
mod navigation;
mod paths;
mod prompt_box;
mod question_flow;
mod question_view;
mod render;
mod reset_credit;
mod rows;
#[path = "section_scroll.rs"]
mod section_scroll;
mod selection_tabs;
pub(crate) mod shell_commands;
mod shortcut_help;
mod structured_text;
mod style;
#[cfg(test)]
mod task_event_list_tests;
mod task_ui;
pub use task_ui::PinnedRow;
use task_ui::{EventRow, TaskUi};
mod terminal_panel;
pub(crate) mod text;
mod text_selection;
mod text_selection_layout;
mod timeline;
mod title_row;
mod title_rule;
mod tool_detail;
mod usage_chart;
pub(crate) mod vscode;
mod weekly_usage;
mod window_navigation;
pub use highlight::use_theme as use_highlight_theme;
pub use style::light_background;
pub use style::set_light_background;
pub(crate) use timeline::timeline_seconds;

use crate::model::{Action, Grouping, Session, SessionStatus};
use crate::providers::{Capabilities, ProviderId, SessionKey};
use crate::state::TaskPins;
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
            SessionStatus::New,
            SessionStatus::Working,
            SessionStatus::Ready,
            SessionStatus::NeedsInput,
        ],
    ),
    ("Inactive", &[SessionStatus::Inactive]),
];

/// How long footer feedback stays on screen, unless a key press clears it first.
const MESSAGE_LIFETIME: Duration = Duration::from_secs(1);
const ERROR_LIFETIME: Duration = Duration::from_secs(5);

/// Longest gap between two clicks on one task that still opens it.
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(300);

fn tab_for_status(status: SessionStatus) -> usize {
    match status {
        SessionStatus::Inactive => 1,
        _ => 0,
    }
}

/// Follow reactivated tasks to Active, but stay in the current tab when a task stops.
fn tab_after_refresh(status: Option<SessionStatus>, current_tab: usize) -> usize {
    match status {
        Some(SessionStatus::Inactive) => current_tab,
        Some(_) | None => 0,
    }
}

#[cfg(test)]
pub(crate) fn cell_text(symbol: &str) -> &str {
    symbol
        .strip_prefix("\x1b[4:4m")
        .and_then(|s| s.strip_suffix("\x1b[24m"))
        .unwrap_or(symbol)
}

struct Overlay {
    area: Rect,
    modal: bool,
}

#[derive(Default)]
struct Viewport {
    area: Rect,
    navigation_layout: Option<render::CenterLayout>,
    scroll: usize,
    task_selection: Option<SessionKey>,
    page_height: usize,
    tabs: Vec<(usize, Rect)>,
    tab_bar: Rect,
    list: Rect,
    search: Rect,
    list_rows: Rect,
    /// Status dots of task rows with subtasks; a click folds or unfolds that task's subtree.
    fold_toggles: Vec<(Rect, SessionKey)>,
    first_entry: usize,
    event_panel: Rect,
    event_detail: Rect,
    /// Where the session terminal was drawn inside the event detail panel.
    terminal: Rect,
    prompt: Rect,
    field_targets: Vec<(Rect, Field)>,
    picker_rows: Vec<(Rect, usize)>,
    overlays: Vec<Overlay>,
    /// The task details panel's bottom input row for a new branch.
    task_details_input: Rect,
    /// The shared task name editor on the task details panel's lower rule.
    title_input: Rect,
    /// The selected task's name on the task details panel's lower rule.
    task_details_title: Rect,
    confirm_options: Vec<(Rect, bool)>,
    hint_targets: Vec<hints::HintTarget>,
    /// Where the last render drew the button that jumps back to the newest event row.
    event_jump: Rect,
    /// Space reserved for the New button or duration label.
    event_label: Rect,
    event_borders: [Rect; 2],
    output_borders: [Rect; 2],
    /// Index into the task event list of each drawn event row, top down.
    event_rows: Vec<EventRow>,
    event_timeline_rows: usize,
    event_separator: Option<usize>,
    event_detail_max: usize,
    /// The title of the section on the detail's top row once that title scrolls off.
    event_detail_footer_title: Option<(Line<'static>, event_detail::EventDetailColumns)>,
    /// The selected event's layout, cached until its content or size changes.
    event_detail_layout: Option<event_list::EventDetailLayout>,
    /// Images the last render drew in the event detail, top down.
    event_detail_images: Vec<event_detail::DrawnImage>,
    event_detail_columns: Option<event_detail::EventDetailColumns>,
    event_text: text_selection::TextRegion,
    task_details_text: text_selection::TextRegion,
    task_text: text_selection::TextRegion,
    event_list_text: text_selection::TextRegion,
    task_scroll_max: usize,
    task_details: Rect,
    task_details_scroll: SectionScroll,
    /// Screen rows of each copyable details body drawn by the last render, with its full text.
    task_details_copy: Vec<(std::ops::Range<u16>, String, &'static str)>,
    task_details_background: Vec<(std::ops::Range<u16>, crate::model::BackgroundTask)>,
    /// Links drawn by the last render of each panel: the task list, the details and the event list.
    list_links: Vec<links::DrawnLink>,
    task_details_links: Vec<links::DrawnLink>,
    event_links: Vec<links::DrawnLink>,
    /// Screen cell at the start of the selected row in the last render.
    marker: Option<(u16, u16)>,
}

#[derive(Default)]
struct SectionScroll {
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
    scroll: usize,
    search: event_search::EventSearch,
}

/// Confirmation before a destructive provider action.
struct ConfirmDialog {
    action: ConfirmAction,
    title: String,
    confirm: bool,
}

enum ConfirmAction {
    Delete(SessionKey),
    Reset {
        provider: ProviderId,
        request: crate::providers::ResetCreditRequest,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Panel {
    #[default]
    TaskList,
    EventList,
    EventDetail,
    TaskDetails,
    Prompt,
}

pub(crate) use crate::model::EventSort;

pub struct View {
    /// The single keyboard destination; selection and scroll positions do not own focus.
    active_panel: Panel,
    /// A browsed task has a parked focus to resume on entry.
    task_entry_pending: bool,
    /// Actions created by panel changes outside keyboard and mouse handlers.
    pending_actions: Vec<Action>,
    window_prefix: Option<KeyEvent>,
    height_zoom: Option<Panel>,
    width_zoom: Option<Panel>,
    /// The session whose program runs in the event detail panel, and its screen.
    terminal: Option<(SessionKey, crate::session_terminal::Screen)>,
    pub(crate) startup_cwd: PathBuf,
    drafts: Vec<drafts::Draft>,
    selected_draft: Option<crate::model::DraftKey>,
    next_draft: u64,
    drafts_dirty: bool,
    draft_jobs_pending: bool,
    input_choices: HashMap<ProviderId, crate::providers::InputChoices>,
    pub(crate) provider_names: Vec<(ProviderId, &'static str)>,
    pub(crate) weekly_usage: HashMap<ProviderId, crate::providers::WeeklyUsage>,
    pub(crate) usage_view: crate::state::UsageView,
    selector: Option<Selector>,
    pub(crate) event_panel: Vec<crate::event::Event>,
    pub(crate) event_session: Option<SessionKey>,
    pub(crate) event_list: task_ui::EventList,
    event_order: Vec<usize>,
    last_update: DateTime<Utc>,
    event_detail: task_ui::EventDetail,
    input_box: prompt_box::PromptBox,
    prompt_history: std::rc::Rc<RefCell<Vec<line_edit::LineEdit>>>,
    questions: question_flow::Questions,
    capabilities: HashMap<ProviderId, Capabilities>,
    rows: Vec<Session>,
    project_groups: Vec<ProjectGroup>,
    selected: usize,
    selection_history: Vec<(SessionKey, usize)>,
    interrupted_selection: Option<(SessionKey, usize)>,
    attention_selection: Option<SessionKey>,
    waiting_sessions: HashSet<SessionKey>,
    /// When the user left each waiting task the queue had brought forward.
    attention_skips: HashMap<SessionKey, DateTime<Utc>>,
    grouping: Grouping,
    use_colors: bool,
    pinned: TaskPins<SessionKey>,
    removing: HashSet<SessionKey>,
    /// Parent tasks whose subtree is folded away in each task list tab.
    collapsed: Vec<HashSet<SessionKey>>,
    status_filter: usize,
    /// The task each tab shows selected when it is entered, by tab index.
    tab_selections: Vec<Option<SessionKey>>,
    search: line_edit::LineEdit,
    searching: bool,
    rename_target: Option<SessionKey>,
    name_target: Option<crate::model::DraftKey>,
    /// The prompt of the turn Esc is stopping, returned to its input once the stop lands.
    stopping_prompt: Option<(SessionKey, String)>,
    rename: line_edit::LineEdit,
    confirm_dialog: Option<ConfirmDialog>,
    reset_incidents: Vec<reset_credit::ResetIncident>,
    reset_pending: HashSet<ProviderId>,
    help: bool,
    loading: bool,
    has_more: bool,
    viewport: RefCell<Viewport>,
    /// Inline images, present only while the terminal supports them.
    inline_images: RefCell<Option<inline_images::InlineImages>>,
    /// Task row and time of the last left click in the list.
    last_click: Option<(usize, Instant)>,
    double_click_interval: Duration,
    pending_click: Option<input::PendingClick>,
    last_event_detail_click: Option<(SessionKey, Option<usize>, Instant)>,
    /// Button, origin and drag state of the held mouse press.
    mouse_press: Option<(MouseButton, u16, u16, bool)>,
    /// The pressed panel owns the drag and release, including presses outside panels.
    mouse_panel: Option<(MouseButton, Option<Panel>)>,
    pressed_task: Option<SessionKey>,
    /// Text under the last left click on a copyable body, and when it happened.
    last_copy_click: Option<(String, Instant)>,
    /// The footer's right-side feedback, shown until it expires or a key press clears it.
    feedback: Option<(footer::Feedback, Instant)>,
    text_selection: RefCell<Option<text_selection::Selection>>,
    /// Accent slot of each task, handed out in the order tasks are first shown.
    title_slots: RefCell<HashMap<SessionKey, usize>>,
    /// The task whose details and event list panels are on screen.
    panels_task: Option<draft_controls::TaskKey>,
    /// The panels of each task the user left, by task.
    task_uis: HashMap<draft_controls::TaskKey, TaskUi>,
    /// The context percentage of the task the app read last.
    context_usage: Option<(SessionKey, u64)>,
    /// Visible token bins of all participating tasks and of tasks outside the selected family.
    usage: std::collections::BTreeMap<i64, crate::usage::Bin>,
    other_usage: std::collections::BTreeMap<i64, crate::usage::Bin>,
    /// How many times taller than wide one terminal cell looks.
    cell_aspect: f64,
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
    pub fn new(grouping: Grouping, use_colors: bool) -> Self {
        Self {
            active_panel: Panel::TaskList,
            task_entry_pending: false,
            pending_actions: Vec::new(),
            window_prefix: None,
            height_zoom: None,
            width_zoom: None,
            terminal: None,
            startup_cwd: std::env::current_dir().expect("dashboard startup directory is available"),
            event_panel: Vec::new(),
            event_session: None,
            event_detail: task_ui::EventDetail::default(),
            event_list: task_ui::EventList::default(),
            event_order: Vec::new(),
            last_update: Utc::now(),
            drafts: Vec::new(),
            selected_draft: None,
            next_draft: 1,
            drafts_dirty: false,
            draft_jobs_pending: false,
            input_choices: HashMap::new(),
            provider_names: Vec::new(),
            weekly_usage: HashMap::new(),
            usage_view: crate::state::UsageView::default(),
            selector: None,
            input_box: prompt_box::PromptBox::default(),
            prompt_history: Default::default(),
            questions: question_flow::Questions::default(),
            capabilities: HashMap::new(),
            rows: Vec::new(),
            project_groups: Vec::new(),
            selected: usize::MAX,
            selection_history: Vec::new(),
            interrupted_selection: None,
            attention_selection: None,
            waiting_sessions: HashSet::new(),
            attention_skips: HashMap::new(),
            grouping,
            use_colors,
            pinned: TaskPins::default(),
            removing: HashSet::new(),
            collapsed: vec![HashSet::new(); TASK_FILTERS.len()],
            status_filter: 0,
            tab_selections: vec![None; TASK_FILTERS.len()],
            search: line_edit::LineEdit::default(),
            searching: false,
            rename_target: None,
            name_target: None,
            stopping_prompt: None,
            rename: line_edit::LineEdit::default(),
            confirm_dialog: None,
            reset_incidents: Vec::new(),
            reset_pending: HashSet::new(),
            help: false,
            loading: false,
            has_more: false,
            viewport: RefCell::new(Viewport::default()),
            inline_images: RefCell::new(None),
            last_click: None,
            double_click_interval: DOUBLE_CLICK_INTERVAL,
            pending_click: None,
            last_event_detail_click: None,
            mouse_press: None,
            mouse_panel: None,
            pressed_task: None,
            last_copy_click: None,
            feedback: None,
            text_selection: RefCell::new(None),
            title_slots: RefCell::new(HashMap::new()),
            panels_task: None,
            task_uis: HashMap::new(),
            usage: Default::default(),
            other_usage: Default::default(),
            context_usage: None,
            cell_aspect: DEFAULT_CELL_ASPECT,
        }
    }

    pub(crate) fn set_inline_images(
        &mut self,
        supports_images: bool,
        cell_pixels: Option<(u16, u16)>,
    ) {
        *self.inline_images.get_mut() =
            supports_images.then(|| inline_images::InlineImages::new(cell_pixels));
        self.viewport.get_mut().event_detail_layout = None;
    }

    pub(crate) fn take_image_commands(&self) -> Vec<u8> {
        self.inline_images
            .borrow_mut()
            .as_mut()
            .map_or_else(Vec::new, |images| images.take_commands())
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
            && !self.frozen_draft()
            && self.prompt_focused()
            && !self.input_box.searching_history()
            && self.input_disabled().is_none()
    }

    pub(crate) fn attach_image(&mut self, image: crate::images::ImageAttachment) {
        if self.frozen_draft() {
            return;
        }
        if self.pasting_prompt() {
            if self
                .selected_draft()
                .is_some_and(|draft| draft.name.is_some())
            {
                image.preserve();
            }
            self.input_box.editor.insert_image(image);
        }
    }

    fn title_editing(&self) -> bool {
        self.rename_target.is_some() || self.name_target.is_some()
    }

    fn title_editor(&self) -> Option<&line_edit::LineEdit> {
        (self.title_editing() && self.selector.is_none()).then_some(&self.rename)
    }

    pub(crate) fn handle_paste(&mut self, text: &str) -> bool {
        if self.frozen_draft() {
            return false;
        }
        if self.confirm_dialog.is_some() || self.help {
            return false;
        }
        let task_details_active = self.active_panel == Panel::TaskDetails;
        if let Some(editor) = self
            .selector
            .as_mut()
            .filter(|_| task_details_active)
            .and_then(Selector::active_editor)
        {
            editor.insert(text);
            return true;
        }
        if self.selector.is_some() && task_details_active {
            return false;
        }
        if !self.searching && !self.task_search_editing() && self.question_paste(text) {
            return true;
        }
        if task_details_active && self.title_editor().is_some() {
            self.rename.insert(text);
        } else if self.prompt_focused() && self.input_disabled().is_none() {
            self.input_box.paste(text);
        } else if self.active_panel == Panel::EventDetail && self.event_detail.search.editing {
            self.event_detail.search.query.insert(text);
            self.jump_event_detail_match();
        } else if self.active_panel == Panel::EventList && self.event_list.search.editing {
            self.event_list.search.query.insert(text);
            self.rematch_event();
            self.show_newest_event();
        } else if self.searching {
            self.search.insert(text);
            let selected = self
                .selectable_indices()
                .first()
                .copied()
                .unwrap_or(usize::MAX);
            self.select(selected);
        } else {
            return false;
        }
        true
    }

    pub(crate) fn prepare_paste_prompt(&mut self) -> bool {
        if self.help
            || self.confirm_dialog.is_some()
            || self.frozen_draft()
            || self.input_disabled().is_some()
        {
            return false;
        }
        self.prepare_prompt();
        true
    }

    fn input_label(&self) -> String {
        if let Some(draft) = self.selected_draft() {
            return draft.name.clone().unwrap_or_default();
        }
        self.selected_row()
            .map(|session| session.display_title().to_owned())
            .unwrap_or_else(|| "No task".into())
    }

    fn input_disabled(&self) -> Option<&'static str> {
        if self.selected_draft().is_some() {
            return None;
        }
        match self.selected_row() {
            Some(session) => session.input_disabled_reason(),
            None => Some("Select a task to type"),
        }
    }

    fn prompt_focused(&self) -> bool {
        self.active_panel == Panel::Prompt && self.confirm_dialog.is_none()
    }

    pub(crate) fn prepare_prompt(&mut self) {
        self.restore_task_focus(self.input_focus());
    }

    fn task_input(&mut self, id: &SessionKey) -> &mut prompt_box::PromptBox {
        if self.selected_id() == Some(id) {
            &mut self.input_box
        } else {
            &mut self
                .task_uis
                .entry(draft_controls::TaskKey::Session(id.clone()))
                .or_default()
                .prompt
        }
    }

    /// Marks input as sending while preserving the attention queue and return task.
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
        target.state = prompt_box::SendState::Ready;
        target.submitting = true;
    }

    pub(crate) fn input_complete(
        &mut self,
        id: &SessionKey,
        input: &crate::providers::SessionInput,
        result: std::io::Result<()>,
    ) {
        if let crate::providers::SessionInput::Answer { ask, .. } = input {
            let focus = self.task_focus();
            self.questions.complete(ask, &result);
            if self.selected_id() == Some(id)
                && self.active_panel == Panel::Prompt
                && self.task_focus() != focus
            {
                self.set_active_panel(Panel::Prompt);
            }
            if let Err(error) = result {
                self.show_error(error.to_string());
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
        target.submitting = false;
        target.state = match result {
            Ok(()) => prompt_box::SendState::Ready,
            Err(error) => prompt_box::SendState::Failed(error.to_string()),
        };
        if let Some(error) = error {
            self.show_error(error);
        }
    }

    /// Returns the stopped turn's prompt to an empty input, as Claude Code does after Esc.
    pub(crate) fn stop_complete(&mut self, id: &SessionKey, stopped: bool) {
        let Some((target, prompt)) = self.stopping_prompt.take() else {
            return;
        };
        if !stopped || &target != id {
            return;
        }
        let input = self.task_input(id);
        if input.editor.is_empty() {
            input.editor.set_text(&prompt);
        }
    }

    pub(crate) fn input_failed(&mut self, reason: String) {
        self.input_box.state = prompt_box::SendState::Failed(reason.clone());
        self.show_error(reason);
    }

    pub fn set_capabilities(&mut self, capabilities: HashMap<ProviderId, Capabilities>) {
        self.capabilities = capabilities;
    }

    /// Replace rows while retaining the selected session and view preferences.
    pub fn set_sessions(&mut self, mut sessions: Vec<Session>, has_more: bool) {
        sessions.retain(|session| self.draft_of(&session.id).is_none());
        sessions.extend(self.draft_rows());
        for session in &sessions {
            if session.status == SessionStatus::Inactive
                && self.rows.iter().any(|previous| {
                    previous.id == session.id && previous.status != SessionStatus::Inactive
                })
            {
                if let Some(ui) = self
                    .task_uis
                    .get_mut(&draft_controls::TaskKey::Session(session.id.clone()))
                {
                    ui.focus = None;
                }
                if self.panels_task.as_ref()
                    == Some(&draft_controls::TaskKey::Session(session.id.clone()))
                {
                    self.task_entry_pending = false;
                }
            }
        }
        let had_sessions = !self.rows.is_empty();
        let selected = self.selected_id().cloned();
        if self.attention_selection.is_some() && self.attention_selection != selected {
            if let Some(id) = self.attention_selection.take() {
                let skipped_at = Utc::now();
                self.attention_skips.insert(id.clone(), skipped_at);
                self.pending_actions.push(Action::PersistSkip {
                    id,
                    skipped_at: Some(skipped_at),
                });
            }
            self.interrupted_selection = None;
        }
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
        self.task_uis.retain(|id, _| match id {
            draft_controls::TaskKey::Session(id) => rows.iter().any(|session| &session.id == id),
            draft_controls::TaskKey::Draft(id) => self.drafts.iter().any(|draft| &draft.id == id),
        });
        self.has_more = has_more;
        let mut selected_index = selected
            .as_ref()
            .and_then(|id| self.rows.iter().position(|s| &s.id == id));
        if selected.is_some()
            && selected_index.is_none_or(|index| !self.visible_indices().contains(&index))
        {
            let status = selected_index.map(|index| self.rows[index].status);
            if let Some(parent) = selected_parent {
                selected_index = self
                    .visible_indices()
                    .into_iter()
                    .find(|&index| self.rows[index].id == parent);
            } else {
                self.status_filter = tab_after_refresh(status, self.status_filter);
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
        self.reconcile_attention(selected.as_ref(), status_filter, had_sessions);
        self.reconcile_drafts();
        if std::mem::take(&mut self.draft_jobs_pending) {
            self.repair_draft_jobs();
        }
        self.show_selected_panels();
    }

    /// Follows waiting tasks until the queue is empty or the user chooses another task.
    fn reconcile_attention(
        &mut self,
        selected: Option<&SessionKey>,
        selected_tab: usize,
        had_sessions: bool,
    ) {
        let waiting: HashSet<_> = self
            .rows
            .iter()
            .filter(|row| row.status == SessionStatus::NeedsInput)
            .map(|row| row.id.clone())
            .collect();
        let answered: Vec<_> = self
            .attention_skips
            .keys()
            .filter(|id| !waiting.contains(*id) && self.rows.iter().any(|row| &row.id == *id))
            .cloned()
            .collect();
        for id in answered {
            self.attention_skips.remove(&id);
            self.pending_actions.push(Action::PersistSkip {
                id,
                skipped_at: None,
            });
        }
        if self
            .selected_draft()
            .is_some_and(|draft| draft.name.is_none())
        {
            self.waiting_sessions = waiting;
            return;
        }
        let tab = self.status_filter;
        self.status_filter = 0;
        let mut queue: Vec<_> = self
            .selectable_indices()
            .into_iter()
            .filter(|&index| {
                self.rows.get(index).is_some_and(|row| {
                    row.status == SessionStatus::NeedsInput && row.parent.is_none()
                })
            })
            .collect();
        queue.sort_by_key(|&index| self.attention_order(&self.rows[index]));
        self.status_filter = tab;
        let starts_waiting = had_sessions
            && queue
                .iter()
                .any(|&index| !self.waiting_sessions.contains(&self.rows[index].id));
        self.waiting_sessions = waiting;
        if let Some(id) = &self.attention_selection {
            if queue.iter().any(|&index| &self.rows[index].id == id) {
                return;
            }
        } else {
            if !starts_waiting
                || self
                    .selected_row()
                    .is_some_and(|row| row.status == SessionStatus::NeedsInput)
            {
                return;
            }
            self.interrupted_selection = selected.cloned().map(|id| (id, selected_tab));
        }
        if let Some(&index) = queue.first() {
            self.status_filter = 0;
            self.attention_selection = Some(self.rows[index].id.clone());
            self.set_selected(index);
        } else {
            self.attention_selection = None;
            if let Some((id, tab)) = self.interrupted_selection.take() {
                self.status_filter = tab;
                let index = self
                    .selectable_indices()
                    .into_iter()
                    .find(|&index| self.rows.get(index).is_some_and(|row| row.id == id))
                    .unwrap_or_else(|| self.fallback_selection());
                self.set_selected(index);
            } else {
                return;
            }
        }
        let focus = if self.attention_selection.is_some() && self.active_question().is_some() {
            task_ui::Focus::Question
        } else {
            self.task_focus()
        };
        if !self.restore_task_focus(focus) {
            self.set_active_panel(Panel::TaskList);
        }
        self.remember_selection();
        self.viewport.get_mut().scroll = 0;
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }
    pub fn set_pinned(&mut self, pins: &TaskPins<SessionKey>) {
        self.pinned = pins.clone();
        self.reconcile_selection();
    }
    /// Finishes a pending archive or deletion so refreshed rows can be listed.
    pub fn finish_removal(&mut self, id: &SessionKey) {
        self.removing.remove(id);
    }
    pub fn begin_removal(&mut self, id: &SessionKey) {
        let visible = self.selectable_indices();
        let position = visible
            .iter()
            .position(|i| *i == self.selected)
            .unwrap_or(0);
        let selected = self.selected_id() == Some(id);
        self.removing.insert(id.to_owned());
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
    /// Shows `text` at the footer's right end for `MESSAGE_LIFETIME`.
    pub fn show_info(&mut self, text: String) {
        self.show_feedback_at(footer::Feedback::Info(text), Instant::now());
    }

    /// Shows `text` in the error color at the footer's right end for `ERROR_LIFETIME`.
    pub fn show_error(&mut self, text: String) {
        self.show_feedback_at(footer::Feedback::Error(text), Instant::now());
    }

    /// Shows `label` at the footer's right end until `MESSAGE_LIFETIME` after `now`.
    pub fn show_copied(&mut self, label: String, now: Instant) {
        self.show_feedback_at(footer::Feedback::Info(label), now);
    }

    fn show_feedback_at(&mut self, feedback: footer::Feedback, now: Instant) {
        let lifetime = match feedback {
            footer::Feedback::Info(_) => MESSAGE_LIFETIME,
            footer::Feedback::Error(_) => ERROR_LIFETIME,
        };
        self.feedback = Some((feedback, now + lifetime));
    }

    /// Clears expired feedback and returns how long the shown feedback has left.
    pub fn expire_messages(&mut self, now: Instant) -> Option<Duration> {
        expire(&mut self.feedback, now)
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
            self.background_for(session).next().is_some()
                || self.subagents_for(session).next().is_some()
        })
    }
    fn background_for<'a>(
        &'a self,
        session: &'a Session,
    ) -> impl Iterator<Item = &'a crate::model::BackgroundTask> {
        self.rows
            .iter()
            .enumerate()
            .filter(move |(index, row)| {
                row.id == session.id
                    || self
                        .ancestors(*index)
                        .any(|ancestor| self.rows[ancestor].id == session.id)
            })
            .flat_map(|(_, row)| &row.background)
    }
    fn subagents_for<'a>(&'a self, session: &'a Session) -> impl Iterator<Item = &'a Session> {
        let mut children: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, child)| child.status == SessionStatus::Working)
            .filter(|(index, _)| {
                self.ancestors(*index)
                    .any(|ancestor| self.rows[ancestor].id == session.id)
            })
            .map(|(_, child)| child)
            .collect();
        // All working descendants share start order, regardless of depth; unknown starts come last.
        children.sort_by_key(|child| (child.started_at.is_none(), child.started_at, &child.id));
        children.into_iter()
    }

    pub fn selected_id(&self) -> Option<&SessionKey> {
        self.selected_row().map(|s| &s.id)
    }
    fn selected_row(&self) -> Option<&Session> {
        if self
            .selected_draft()
            .is_some_and(|draft| draft.name.is_none())
        {
            return None;
        }
        self.rows
            .get(self.selected)
            .filter(|_| self.visible_indices().contains(&self.selected))
    }
    fn ancestors(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let mut remaining = self.rows.len();
        std::iter::successors(Some(index), move |&index| {
            remaining = remaining.checked_sub(1)?;
            let parent = self.rows[index].parent.as_ref()?;
            self.rows.iter().position(|session| &session.id == parent)
        })
        .skip(1)
    }
    fn group_index(&self, index: usize) -> Option<usize> {
        let root = self.ancestors(index).last().unwrap_or(index);
        self.rows[root].parent.is_none().then_some(root)
    }
    fn can_pin(&self, index: usize) -> bool {
        self.rows[index].status != SessionStatus::Inactive
            || self
                .group_index(index)
                .is_some_and(|root| self.rows[root].status != SessionStatus::Inactive)
    }
    fn is_pinned(&self, index: usize) -> bool {
        self.pinned
            .for_tab(self.status_filter)
            .contains(&self.rows[index].id)
    }
    fn in_pinned_group(&self, index: usize) -> bool {
        self.is_pinned(index)
            || self
                .ancestors(index)
                .any(|ancestor| self.is_pinned(ancestor))
    }
    fn tree_index(&self, index: usize) -> Option<usize> {
        std::iter::once(index)
            .chain(self.ancestors(index))
            .find(|&index| self.is_pinned(index))
            .or_else(|| self.group_index(index))
    }
    fn same_group(&self, grouping: Grouping, left: usize, right: usize) -> bool {
        if self.in_pinned_group(left) || self.in_pinned_group(right) {
            return self.in_pinned_group(left) && self.in_pinned_group(right);
        }
        let (Some(left), Some(right)) = (self.group_index(left), self.group_index(right)) else {
            return false;
        };
        match grouping {
            Grouping::Project => {
                self.project_groups[left].key() == self.project_groups[right].key()
            }
            Grouping::Status => self.rows[left].status == self.rows[right].status,
            Grouping::Model => {
                self.session_model_text(&self.rows[left])
                    == self.session_model_text(&self.rows[right])
            }
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
        self.visible_indices_for_filter(self.status_filter)
    }
    fn visible_indices_for_filter(&self, tab: usize) -> Vec<usize> {
        let filter = TASK_FILTERS[tab].1;
        let pins = self.pinned.for_tab(tab);
        let is_pinned = |index: usize| pins.contains(&self.rows[index].id);
        let sessions: HashMap<_, _> = self.rows.iter().map(|s| (&s.id, s)).collect();
        let mut matching = HashSet::new();
        for (_, session) in self.rows.iter().enumerate().filter(|(index, session)| {
            filter.contains(&session.status)
                || (tab == 0 && is_pinned(*index) && self.can_pin(*index))
        }) {
            let mut current = Some(session);
            while let Some(session) = current {
                if self.removing.contains(&session.id) || !matching.insert(&session.id) {
                    break;
                }
                // Pinned subtrees stand on their own, without adding ancestor headers.
                if pins.contains(&session.id) {
                    break;
                }
                current = session
                    .parent
                    .as_ref()
                    .and_then(|id| sessions.get(id).copied());
            }
        }
        let mut children = std::collections::HashMap::<&SessionKey, Vec<usize>>::new();
        for (index, session) in self.rows.iter().enumerate() {
            if let Some(parent) = session.parent.as_ref()
                && !self.removing.contains(&session.id)
            {
                children.entry(parent).or_default().push(index);
            }
        }
        for siblings in children.values_mut() {
            siblings.sort_by_key(|&index| {
                let row = &self.rows[index];
                (self.waiting_first(row), session_order(row))
            });
        }
        let (mut pinned, mut visible): (Vec<_>, Vec<_>) = self
            .rows
            .iter()
            .enumerate()
            .filter(|(index, session)| {
                (session.parent.is_none() || is_pinned(*index))
                    && !self.removing.contains(&session.id)
                    && matching.contains(&session.id)
                    && self.matches_search(session)
            })
            .map(|(index, _)| index)
            .partition(|index| is_pinned(*index));
        pinned.sort_by_key(|index| pins.iter().position(|id| *id == self.rows[*index].id));
        match self.grouping {
            Grouping::Project => visible.sort_by_key(|index| {
                let row = &self.rows[*index];
                (
                    self.project_groups[*index].key(),
                    self.waiting_first(row),
                    session_order(row),
                )
            }),
            Grouping::Status => visible.sort_by_key(|index| {
                let row = &self.rows[*index];
                (row.status, self.waiting_first(row))
            }),
            Grouping::Model => visible.sort_by_key(|index| {
                let row = &self.rows[*index];
                (
                    self.session_model_text(row),
                    self.waiting_first(row),
                    session_order(row),
                )
            }),
        }
        pinned.extend(visible);
        let mut rows = Vec::new();
        let mut pending: Vec<_> = pinned.into_iter().rev().collect();
        while let Some(index) = pending.pop() {
            if !matching.contains(&self.rows[index].id) {
                continue;
            }
            rows.push(index);
            if self.collapsed[tab].contains(&self.rows[index].id) {
                continue;
            }
            if let Some(siblings) = children.get(&self.rows[index].id) {
                pending.extend(
                    siblings
                        .iter()
                        .rev()
                        .copied()
                        .filter(|&child| !is_pinned(child)),
                );
            }
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
    pub(crate) fn event_scroll(&self) -> usize {
        self.event_list.scroll.get()
    }

    pub(crate) fn clamp_event_scroll(&mut self, max: usize) {
        self.event_list
            .scroll
            .set(self.event_list.scroll.get().min(max));
    }

    pub(crate) fn event_max_scroll(&self) -> usize {
        self.event_max_scroll_for(self.event_content_height())
    }

    pub(crate) fn event_max_scroll_for(&self, visible: usize) -> usize {
        self.shown_events()
            .count()
            .saturating_sub(self.event_section_heights(visible).1)
    }

    pub(crate) fn event_read_count(&self, visible: usize, has_older: bool) -> usize {
        let loaded = self.event_panel.len().max(visible);
        if has_older
            && (self.event_page_pending()
                || (self.event_scroll() == 0
                    && !self.following_event()
                    && !matches!(self.selected_event_row(), Some(EventRow::Pinned(_)))))
        {
            loaded.saturating_add(visible.max(self.event_list.pending_older).max(1))
        } else {
            loaded
        }
    }

    /// Selects the newest shown event and resumes following the event list.
    fn show_newest_event(&mut self) {
        self.select_event(None);
        self.event_detail.search.clear();
        let max = self.event_max_scroll();
        self.event_list.scroll.set(max);
        self.event_detail.scroll.get_mut().offset = 0;
        self.event_list.unseen.set(0);
        self.event_list.pending_older = 0;
        self.event_list.start_requested = false;
        self.event_list.background_return = None;
    }

    fn set_selected(&mut self, selected: usize) {
        if self.selected == selected
            && self
                .selected_draft()
                .is_some_and(|draft| draft.name.is_some())
        {
            return;
        }
        self.leave_draft();
        self.viewport.get_mut().task_selection = None;
        self.selected = selected;
        self.selected_draft = self
            .rows
            .get(selected)
            .and_then(|row| self.draft_of(&row.id))
            .map(|draft| draft.id);
        self.show_selected_panels();
        if let Some(id) = self.selected_draft
            && let Some(draft) = self.drafts.iter_mut().find(|draft| draft.id == id)
        {
            self.input_box = std::mem::take(&mut draft.input);
        }
    }

    /// Keeps the details and event list panels with their task: saves what the task the user left
    /// showed, and shows what the selected task showed when the user last left it.
    fn show_selected_panels(&mut self) {
        let selected = self.selected_task_key();
        if self.panels_task == selected {
            self.show_questions();
            return;
        }
        self.park_event_list();
        let task_list_focused = self.active_panel == Panel::TaskList;
        let restores_task = self.has_parked_focus(selected.as_ref());
        let focus = self
            .panels_task
            .as_ref()
            .and_then(|key| self.task_uis.get(key))
            .and_then(|ui| ui.focus);
        let title = self
            .title_editing()
            .then(|| std::mem::take(&mut self.rename));
        self.rename_target = None;
        self.name_target = None;
        let mut selector = self.selector.take();
        let branch = selector
            .as_mut()
            .and_then(Selector::active_editor)
            .map(std::mem::take);
        self.text_selection.borrow_mut().take();
        let viewport = self.viewport.get_mut();
        let left = TaskUi {
            focus,
            event_detail: std::mem::take(&mut self.event_detail),
            prompt: std::mem::take(&mut self.input_box),
            task_details: task_ui::TaskDetails {
                title,
                branch,
                selector,
                scroll: std::mem::take(&mut viewport.task_details_scroll),
            },
            event_list: std::mem::take(&mut self.event_list),
            ..Default::default()
        };
        if let Some(id) = self.panels_task.take() {
            self.task_uis.insert(id, left);
        }
        let mut shown = selected
            .as_ref()
            .and_then(|id| self.task_uis.remove(id))
            .unwrap_or_default();
        if shown.task_details.scroll.left_keys.is_none() {
            shown.task_details.scroll.left_keys =
                Some(std::mem::take(&mut shown.task_details.scroll.keys));
        }
        self.selector = shown.task_details.selector;
        if let Some(branch) = shown.task_details.branch {
            *self
                .selector
                .as_mut()
                .and_then(Selector::active_editor)
                .expect("parked branch has an open selector") = branch;
        }
        if let Some(title) = shown.task_details.title {
            self.rename = title;
            match &selected {
                Some(draft_controls::TaskKey::Draft(id)) => self.name_target = Some(*id),
                Some(draft_controls::TaskKey::Session(id)) => self.rename_target = Some(id.clone()),
                None => unreachable!("title editor belongs to a task"),
            }
        }
        viewport.task_details_scroll = shown.task_details.scroll;
        viewport.event_detail_layout = None;
        self.input_box = shown.prompt;
        self.event_list = shown.event_list;
        self.event_list.restoring = true;
        self.event_detail = shown.event_detail;
        self.event_detail.scroll.get_mut().restoring = true;
        self.pending_click = None;
        if let Some(key) = &selected {
            self.task_uis.entry(key.clone()).or_default().focus = shown.focus;
        }
        self.panels_task = selected;
        self.questions.sync(&self.rows);
        self.task_entry_pending = task_list_focused && restores_task;
        if task_list_focused {
            self.set_active_panel(Panel::TaskList);
        } else if !self.restore_task_focus(shown.focus.unwrap_or_else(|| self.input_focus())) {
            self.set_active_panel(Panel::TaskList);
        }
    }

    /// Selects the row the user chose.
    fn select(&mut self, selected: usize) {
        if self.selected_draft.is_some() || self.selected != selected {
            self.remember_previous_task();
        }
        if let Some(row) = self.rows.get(selected)
            && !self.visible_indices().contains(&selected)
        {
            let tab = tab_for_status(row.status);
            if self.visible_indices_for_filter(tab).contains(&selected) {
                self.status_filter = tab;
            }
        }
        self.set_selected(selected);
        self.remember_selection();
    }

    fn remember_previous_task(&mut self) {
        if let Some(id) = self.selected_id().cloned() {
            let entry = (id, self.status_filter);
            if self.selection_history.last() != Some(&entry) {
                self.selection_history.push(entry);
            }
        }
    }

    fn select_previous_task(&mut self) {
        let current = self.selected_id().cloned();
        let filter = self.status_filter;
        while let Some((id, tab)) = self.selection_history.pop() {
            if current.as_ref() == Some(&id) {
                continue;
            }
            let Some(index) = self.rows.iter().position(|row| row.id == id) else {
                continue;
            };
            for candidate in [tab, 1 - tab] {
                self.status_filter = candidate;
                if self.selectable_indices().contains(&index) {
                    self.set_selected(index);
                    self.remember_selection();
                    self.viewport.get_mut().scroll = 0;
                    return;
                }
            }
        }
        self.status_filter = filter;
    }

    /// Remembers the selected task in its visible tab, including children of a different status.
    fn remember_selection(&mut self) {
        if let Some(id) = self.selected_id().cloned() {
            self.tab_selections[self.status_filter] = Some(id);
        }
    }

    fn reconcile_selection(&mut self) {
        if self
            .selected_draft()
            .is_some_and(|draft| draft.name.is_none())
            && self.status_filter == 0
        {
            return;
        }
        let visible = self.selectable_indices();
        if let Some(id) = &self.rename_target
            && !self.rows.iter().any(|session| &session.id == id)
        {
            self.rename_target = None;
            self.rename.clear();
        }
        if !visible.contains(&self.selected) {
            self.set_selected(self.fallback_selection());
        }
    }

    /// The row a tab selects when no task chosen in it is still listed: the first visible row in
    /// Inactive, or the longest-waiting active task or subagent in Active.
    fn fallback_selection(&self) -> usize {
        let selectable = self.selectable_indices();
        if self.status_filter == tab_for_status(SessionStatus::Inactive) {
            return selectable.first().copied().unwrap_or(usize::MAX);
        }
        let prefer_active = self.status_filter == 0
            && selectable
                .iter()
                .any(|&index| self.rows[index].status != SessionStatus::Inactive);
        selectable
            .into_iter()
            .filter(|&index| !prefer_active || self.rows[index].status != SessionStatus::Inactive)
            .max_by_key(|&index| {
                let row = &self.rows[index];
                (
                    std::cmp::Reverse(self.queue_position(row, row.updated_at)),
                    &row.id,
                )
            })
            .unwrap_or(usize::MAX)
    }

    /// Orders waiting tasks the way the attention queue visits them.
    fn attention_order<'a>(&self, row: &'a Session) -> ((bool, DateTime<Utc>), &'a SessionKey) {
        let since = row.ask.as_ref().map_or(row.updated_at, |ask| ask.since);
        (self.queue_position(row, since), &row.id)
    }

    /// Puts waiting tasks first among their neighbors, in attention queue order.
    fn waiting_first<'a>(
        &self,
        row: &'a Session,
    ) -> (bool, Option<((bool, DateTime<Utc>), &'a SessionKey)>) {
        let waiting = row.status == SessionStatus::NeedsInput;
        (!waiting, waiting.then(|| self.attention_order(row)))
    }

    /// Orders a task the user left in the attention queue after every task it has not skipped, by
    /// when it was left, until it starts waiting again.
    fn queue_position(&self, row: &Session, since: DateTime<Utc>) -> (bool, DateTime<Utc>) {
        match self.attention_skips.get(&row.id) {
            Some(&skipped) if skipped >= since => (true, skipped),
            _ => (false, since),
        }
    }

    pub(crate) fn restore_attention_skips(&mut self, preferences: &crate::state::Preferences) {
        self.attention_skips = preferences
            .tasks
            .keys()
            .filter_map(|id| Some((id.clone(), preferences.task_skipped_at(id)?)))
            .collect();
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
            SessionStatus::New => "○".cyan(),
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
fn expire<T>(message: &mut Option<(T, Instant)>, now: Instant) -> Option<Duration> {
    let (_, expires_at) = message.as_ref()?;
    let left = expires_at
        .checked_duration_since(now)
        .filter(|left| !left.is_zero());
    if left.is_none() {
        *message = None;
    }
    left
}

impl Viewport {
    fn event_row_at(&self, offset: usize) -> Option<EventRow> {
        let offset = match self.event_separator {
            Some(separator) if offset == separator => return None,
            Some(separator) if offset > separator => {
                offset - separator - 1 + self.event_timeline_rows
            }
            Some(_) if offset >= self.event_timeline_rows => return None,
            _ => offset,
        };
        self.event_rows.get(offset).copied()
    }
}

#[cfg(test)]
mod background_order_tests;
