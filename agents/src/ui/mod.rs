//! Command center adapted from OpenAI Codex's TUI (Apache-2.0).
mod formatting;
mod hints;
mod hyperlink;
mod input;
mod line_truncation;
mod log;
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
use std::{cell::RefCell, collections::HashSet, path::Path, path::PathBuf};
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
    details: Rect,
    details_scroll: DetailsScroll,
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
    id: String,
    title: String,
    confirm: bool,
}

pub struct View {
    pub(crate) log: Vec<crate::sessions::log::Event>,
    pub(crate) log_session: Option<String>,
    pub(crate) log_filter: crate::sessions::log::Filter,
    rows: Vec<Session>,
    project_groups: Vec<ProjectGroup>,
    selected: usize,
    grouping: Grouping,
    use_colors: bool,
    pinned: Vec<String>,
    hidden: HashSet<String>,
    status_filter: usize,
    search: String,
    searching: bool,
    rename_target: Option<String>,
    rename: String,
    delete_dialog: Option<DeleteDialog>,
    help: bool,
    loading: bool,
    has_more: bool,
    notice: Option<String>,
    viewport: RefCell<Viewport>,
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

impl View {
    pub fn new(grouping: Grouping, use_colors: bool) -> Self {
        Self {
            log: Vec::new(),
            log_session: None,
            log_filter: crate::sessions::log::Filter::default(),
            rows: Vec::new(),
            project_groups: Vec::new(),
            selected: usize::MAX,
            grouping,
            use_colors,
            pinned: Vec::new(),
            hidden: HashSet::new(),
            status_filter: 0,
            search: String::new(),
            searching: false,
            rename_target: None,
            rename: String::new(),
            delete_dialog: None,
            help: false,
            loading: false,
            has_more: false,
            notice: None,
            viewport: RefCell::new(Viewport::default()),
        }
    }

    /// Replace rows while retaining the selected session and view preferences.
    pub fn set_sessions(&mut self, mut sessions: Vec<Session>, has_more: bool) {
        let selected = self.selected_id().map(str::to_owned);
        let status_filter = self.status_filter;
        let show_more = self.selected == usize::MAX && self.has_more;
        sessions.sort_by(|a, b| {
            a.status
                .cmp(&b.status)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        self.project_groups = sessions.iter().map(ProjectGroup::for_session).collect();
        self.rows = sessions;
        self.has_more = has_more;
        let selected_index = selected
            .as_deref()
            .and_then(|id| self.rows.iter().position(|s| s.id == id));
        if selected.is_some()
            && selected_index.is_none_or(|index| !self.visible_indices().contains(&index))
        {
            self.status_filter = tab_for_status(
                selected_index.map(|index| self.rows[index].status),
                self.status_filter,
            );
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
        if self.selected_id() != selected.as_deref() {
            self.viewport.borrow_mut().log_scroll = 0;
        }
        if self.selected_id() != selected.as_deref() || self.status_filter != status_filter {
            self.viewport.borrow_mut().details_scroll = DetailsScroll::default();
        }
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }
    pub fn set_pinned(&mut self, ids: &[String]) {
        self.pinned = ids.to_vec();
        self.reconcile_selection();
    }
    pub fn hide(&mut self, id: &str) {
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
    /// Where the `›` of the selected row was drawn last, so the terminal cursor can rest on it.
    pub fn marker_pos(&self) -> Option<(u16, u16)> {
        self.viewport.borrow().marker
    }

    pub fn grouping(&self) -> Grouping {
        self.grouping
    }
    pub fn has_background(&self) -> bool {
        self.selected_row()
            .is_some_and(|session| !session.background.is_empty())
    }
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_row().map(|s| s.id.as_str())
    }
    fn selected_row(&self) -> Option<&Session> {
        self.rows
            .get(self.selected)
            .filter(|_| self.visible_indices().contains(&self.selected))
    }
    fn is_pinned(&self, index: usize) -> bool {
        self.pinned.contains(&self.rows[index].id)
    }
    fn same_group(&self, grouping: Grouping, left: usize, right: usize) -> bool {
        if self.is_pinned(left) || self.is_pinned(right) {
            return self.is_pinned(left) && self.is_pinned(right);
        }
        match grouping {
            Grouping::Project => self.project_groups[left].key == self.project_groups[right].key,
            Grouping::Status => self.rows[left].status == self.rows[right].status,
            Grouping::Model => self.rows[left].model_name() == self.rows[right].model_name(),
        }
    }
    fn visible_indices(&self) -> Vec<usize> {
        let search = self.search.to_lowercase();
        let filter = TASK_FILTERS[self.status_filter].1;
        let (mut pinned, mut visible): (Vec<_>, Vec<_>) = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                !self.hidden.contains(&s.id)
                    && (self.rename_target.as_deref() == Some(&s.id) || filter.contains(&s.status))
                    && (search.is_empty()
                        || format!(
                            "{} {} {} {}",
                            s.name.as_deref().unwrap_or(""),
                            s.title,
                            s.prompt,
                            s.cwd.display()
                        )
                        .to_lowercase()
                        .contains(&search))
            })
            .map(|(i, _)| i)
            .partition(|i| self.is_pinned(*i));
        pinned.sort_by_key(|i| self.pinned.iter().position(|id| *id == self.rows[*i].id));
        match self.grouping {
            Grouping::Project => visible.sort_by_key(|i| {
                (
                    &self.project_groups[*i].key,
                    std::cmp::Reverse(self.rows[*i].updated_at),
                )
            }),
            Grouping::Status => {}
            Grouping::Model => visible.sort_by_key(|i| {
                (
                    self.rows[*i].model_name(),
                    std::cmp::Reverse(self.rows[*i].updated_at),
                )
            }),
        }
        pinned.extend(visible);
        pinned
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

    fn set_selected(&mut self, selected: usize) {
        if self.selected != selected {
            let mut viewport = self.viewport.borrow_mut();
            viewport.details_scroll = DetailsScroll::default();
            viewport.log_scroll = 0;
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
    fn title_style(&self, id: &str) -> Style {
        if self.use_colors {
            Style::default().fg(style::thread_color(id))
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
