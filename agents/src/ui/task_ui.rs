//! Per-task interface state parked while another task is shown.

use super::{
    SectionScroll, draft_controls::Selector, event_search::EventSearch, line_edit::LineEdit,
    prompt_box::PromptBox, question_view::QuestionView,
};
use crate::{event::Event, model::EventSort};

#[derive(Default)]
pub(super) struct TaskUi {
    /// None until the task has owned the keyboard.
    pub focus: Option<Focus>,
    pub event_list: EventList,
    pub event_detail: EventDetail,
    pub task_details: TaskDetails,
    pub prompt: PromptBox,
    #[allow(dead_code)]
    pub question: Option<QuestionView>,
}

#[derive(Default)]
pub(crate) struct EventList {
    pub(super) position: EventPosition,
    pub(super) selected: Option<(bool, EventId)>,
    pub pinned: Vec<PinnedRow>,
    pub search: EventSearch,
    pub sort: EventSort,
    pub sort_initialized: bool,
    pub unseen: std::cell::Cell<usize>,
    pub pending_older: usize,
    pub has_older: bool,
    pub start_requested: bool,
    pub background: Option<Event>,
    pub(super) background_return: Option<super::BackgroundReturn>,
    /// Window-relative caches, discarded when the task is parked.
    pub cursor: Option<usize>,
    pub scroll: std::cell::Cell<usize>,
    pub restoring: bool,
    pub(super) tail: Option<EventId>,
}

#[derive(Default)]
pub(super) struct EventDetail {
    pub mode: EventDetailMode,
    pub scroll: std::cell::RefCell<super::event_detail::EventDetailScroll>,
    pub search: EventSearch,
}

#[derive(Default)]
pub(super) struct TaskDetails {
    pub title: Option<LineEdit>,
    pub branch: Option<LineEdit>,
    pub selector: Option<Selector>,
    pub scroll: SectionScroll,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Focus {
    #[default]
    Prompt,
    Question,
    Title,
    Branch,
    Selector,
    EventList,
    EventSearch,
    EventDetail,
    EventDetailSearch,
    TaskDetails,
}

/// The transcript record identifying an event; events without a source have no identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct EventId(pub String);

impl EventId {
    pub fn from_event(event: &Event) -> Option<Self> {
        event.source.clone().map(Self)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum EventPosition {
    #[default]
    Live,
    At {
        anchor: EventId,
        pinned: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum EventDetailMode {
    #[default]
    Event,
    Terminal,
}

/// An immutable event snapshot and its persisted record positions.
pub struct PinnedRow {
    pub pin: crate::state::PinnedEvent,
    pub event: Event,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EventRow {
    Pinned(usize),
    Timeline(usize),
}
