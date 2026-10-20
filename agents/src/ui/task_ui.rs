//! Per-task interface state parked while another task is shown.
#![allow(dead_code)]

use super::{
    SectionScroll, draft_controls::Selector, event_search::EventSearch, line_edit::LineEdit,
    prompt_box::PromptBox, question_view::QuestionView,
};
use crate::{event::Event, model::EventSort};

#[derive(Default)]
pub(super) struct TaskUi {
    pub focus: Focus,
    pub event_list: EventList,
    pub event_detail: EventDetail,
    pub task_details: TaskDetails,
    pub prompt: PromptBox,
    pub question: Option<QuestionView>,
}

#[derive(Default)]
pub(super) struct EventList {
    pub position: EventPosition,
    pub selected: Option<EventId>,
    pub search: EventSearch,
    pub sort: EventSort,
}

#[derive(Default)]
pub(super) struct EventDetail {
    pub mode: EventDetailMode,
    pub scroll: SectionScroll,
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
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum EventDetailMode {
    #[default]
    Event,
    Terminal,
}
