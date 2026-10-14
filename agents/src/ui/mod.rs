//! Command center adapted from OpenAI Codex's TUI (Apache-2.0).
mod formatting;
mod hints;
mod hyperlink;
mod input;
mod line_edit;
mod line_truncation;
mod log;
mod log_search;
mod markdown;
mod navigation;
mod render;
mod rows;
mod selection_tabs;
mod shortcut_help;
mod style;
#[cfg(test)]
pub use style::light_background;
pub use style::set_light_background;

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

const TASK_FILTERS: &[(&str, &[SessionStatus])] = &[
    (
        "Active",
        &[
            SessionStatus::Working,
            SessionStatus::Ready,
            SessionStatus::NeedsInput,
        ],
    ),
    ("Ready", &[SessionStatus::Ready]),
    ("Working", &[SessionStatus::Working]),
    ("Needs you", &[SessionStatus::NeedsInput]),
    ("Inactive", &[SessionStatus::Inactive]),
];

/// How long a copy confirmation stays in the footer.
const COPIED_LIFETIME: Duration = Duration::from_secs(1);

/// Longest gap between two clicks on one task that still opens it.
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

fn tab_for_status(status: Option<SessionStatus>, current_tab: usize) -> usize {
    match status {
        Some(status) if TASK_FILTERS[current_tab].1.contains(&status) => current_tab,
        Some(SessionStatus::Ready) => 1,
        Some(SessionStatus::Working) => 2,
        Some(SessionStatus::NeedsInput) => 3,
        Some(SessionStatus::Inactive) | None => 0,
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
    log_scroll: usize,
    /// Index into the task log of each drawn log row, top down, and whether its body was cut.
    log_rows: Vec<(usize, bool)>,
    /// Lines of the open log event scrolled off the top.
    log_open_scroll: usize,
    details: Rect,
    details_scroll: DetailsScroll,
    /// Screen rows of each copyable details body drawn by the last render, with its full text.
    details_copy: Vec<(std::ops::Range<u16>, String, &'static str)>,
    /// Screen cell of the selection marker drawn by the last render.
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
}

struct DeleteDialog {
    id: SessionKey,
    title: String,
    confirm: bool,
}

pub struct View {
    pub(crate) log: Vec<crate::log::Event>,
    pub(crate) log_session: Option<SessionKey>,
    pub(crate) log_filter: crate::log::Filter,
    pub(crate) log_search: log_search::LogSearch,
    /// The log event shown whole in the log panel, opened by clicking its cut row.
    log_open: Option<crate::log::Event>,
    capabilities: HashMap<ProviderId, Capabilities>,
    rows: Vec<Session>,
    project_groups: Vec<ProjectGroup>,
    selected: usize,
    grouping: Grouping,
    use_colors: bool,
    pinned: Vec<SessionKey>,
    hidden: HashSet<SessionKey>,
    status_filter: usize,
    search: line_edit::LineEdit,
    searching: bool,
    rename_target: Option<SessionKey>,
    rename: line_edit::LineEdit,
    delete_dialog: Option<DeleteDialog>,
    help: bool,
    loading: bool,
    has_more: bool,
    notice: Option<String>,
    viewport: RefCell<Viewport>,
    /// Task row and time of the last left click in the list.
    last_click: Option<(usize, Instant)>,
    /// Text under the last left click on a copyable body, and when it happened.
    last_copy_click: Option<(String, Instant)>,
    /// The confirmation of the last copy, shown at the footer's right end until it expires.
    copied: Option<(String, Instant)>,
    /// Accent slot of each task, handed out in the order tasks are first shown.
    title_slots: RefCell<HashMap<SessionKey, usize>>,
}

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
            log: Vec::new(),
            log_session: None,
            log_open: None,
            log_filter: crate::log::Filter::default(),
            log_search: log_search::LogSearch::default(),
            capabilities: HashMap::new(),
            rows: Vec::new(),
            project_groups: Vec::new(),
            selected: usize::MAX,
            grouping,
            use_colors,
            pinned: Vec::new(),
            hidden: HashSet::new(),
            status_filter: 0,
            search: line_edit::LineEdit::default(),
            searching: false,
            rename_target: None,
            rename: line_edit::LineEdit::default(),
            delete_dialog: None,
            help: false,
            loading: false,
            has_more: false,
            notice: None,
            viewport: RefCell::new(Viewport::default()),
            last_click: None,
            last_copy_click: None,
            copied: None,
            title_slots: RefCell::new(HashMap::new()),
        }
    }

    pub fn set_capabilities(&mut self, capabilities: HashMap<ProviderId, Capabilities>) {
        self.capabilities = capabilities;
    }

    /// Replace rows while retaining the selected session and view preferences.
    pub fn set_sessions(&mut self, mut sessions: Vec<Session>, has_more: bool) {
        let selected = self.selected_id().cloned();
        let status_filter = self.status_filter;
        let show_more = self.selected == usize::MAX && self.has_more;
        sessions.sort_by(|a, b| {
            a.status
                .cmp(&b.status)
                .then_with(|| session_order(a).cmp(&session_order(b)))
        });
        self.project_groups = sessions.iter().map(ProjectGroup::for_session).collect();
        self.rows = sessions;
        self.has_more = has_more;
        let selected_index = selected
            .as_ref()
            .and_then(|id| self.rows.iter().position(|s| &s.id == id));
        if selected.is_some()
            && selected_index.is_none_or(|index| !self.visible_indices().contains(&index))
        {
            let status = selected_index.map(|index| self.rows[index].status);
            self.status_filter = if self.rename_target.as_ref() == selected.as_ref()
                && status == Some(SessionStatus::Inactive)
            {
                TASK_FILTERS.len() - 1
            } else {
                tab_for_status(status, self.status_filter)
            };
        }
        self.selected = if show_more {
            usize::MAX
        } else {
            selected_index.unwrap_or_else(|| {
                self.selectable_indices()
                    .first()
                    .copied()
                    .unwrap_or(usize::MAX)
            })
        };
        self.reconcile_selection();
        if self.selected_id() != selected.as_ref() {
            self.log_search.clear();
            self.show_newest_log();
        }
        if self.selected_id() != selected.as_ref() || self.status_filter != status_filter {
            self.viewport.borrow_mut().details_scroll = DetailsScroll::default();
        }
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }
    pub fn set_pinned(&mut self, ids: &[SessionKey]) {
        self.pinned = ids.to_vec();
        self.reconcile_selection();
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
            self.set_selected(selected);
        }
        self.reconcile_selection();
    }
    pub fn set_notice(&mut self, notice: Option<String>) {
        self.notice = notice;
    }

    /// Shows `label` at the footer's right end until `COPIED_LIFETIME` after `now`.
    pub fn show_copied(&mut self, label: String, now: Instant) {
        self.copied = Some((label, now + COPIED_LIFETIME));
    }

    /// Clears an expired copy confirmation, and returns how long the shown one has left.
    pub fn expire_copied(&mut self, now: Instant) -> Option<Duration> {
        let (_, expires_at) = self.copied.as_ref()?;
        let expires_at = *expires_at;
        if now >= expires_at {
            self.copied = None;
            return None;
        }
        Some(expires_at - now)
    }
    /// Where the `›` of the selected row was drawn last, so the terminal cursor can rest on it.
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
        self.rows.iter().filter(move |child| {
            session.parent.is_none()
                && child.parent.as_ref() == Some(&session.id)
                && child.status == SessionStatus::Working
        })
    }

    pub fn selected_id(&self) -> Option<&SessionKey> {
        self.selected_row().map(|s| &s.id)
    }
    fn selected_is_subagent(&self) -> bool {
        self.selected_row()
            .is_some_and(|session| session.parent.is_some())
    }
    fn selected_row(&self) -> Option<&Session> {
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
            Grouping::Project => self.project_groups[left].key == self.project_groups[right].key,
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
                .sort_by_key(|i| (&self.project_groups[*i].key, session_order(&self.rows[*i]))),
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

    /// Closes the open log event and scrolls the log back to its newest row.
    fn show_newest_log(&mut self) {
        self.log_open = None;
        let mut viewport = self.viewport.borrow_mut();
        viewport.log_scroll = 0;
        viewport.log_open_scroll = 0;
    }

    fn set_selected(&mut self, selected: usize) {
        if self.selected != selected {
            self.log_search.clear();
            self.show_newest_log();
            self.viewport.borrow_mut().details_scroll = DetailsScroll::default();
        }
        self.selected = selected;
    }

    fn reconcile_selection(&mut self) {
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
            self.set_selected(visible.first().copied().unwrap_or(usize::MAX));
        }
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
        let dot = match session.status {
            SessionStatus::NeedsInput => "●".red(),
            SessionStatus::Working => "●".green(),
            SessionStatus::Ready => "○".cyan(),
            SessionStatus::Inactive => "○".dim(),
        };
        (session.status.label(), dot)
    }
}

#[cfg(test)]
mod tests;
