use chrono::{DateTime, Utc};
use serde_json::Value;

pub(crate) fn tool_kind(name: &str) -> Kind {
    match name.rsplit('.').next() {
        Some("AskUserQuestion" | "request_user_input") => Kind::Assistant,
        _ => Kind::Tool,
    }
}

pub(crate) fn question_texts(input: &Value) -> Vec<String> {
    input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|question| question["question"].as_str())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn participant_text(name: Option<&str>, body: &str) -> String {
    match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if body.is_empty() => format!("({name})"),
        Some(name) => format!("({name}) {body}"),
        None => body.to_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    User,
    Assistant,
    Agent,
    Session,
    Tool,
    Error,
    Denied,
    QueuedUser,
    QueuedSystem,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter(std::collections::HashSet<Kind>);

impl Filter {
    pub fn toggle(&mut self, kinds: &[Kind]) {
        if kinds.iter().all(|kind| self.0.contains(kind)) {
            self.0.retain(|kind| !kinds.contains(kind));
        } else {
            self.0.extend(kinds.iter().cloned());
        }
    }

    pub fn matches(&self, kind: &Kind) -> bool {
        self.0.is_empty() || self.0.contains(kind)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: Kind,
    pub body: String,
    /// Byte ranges of the file paths inside `body`, in order.
    pub paths: Vec<std::ops::Range<usize>>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Known execution time; absent when a completed activity has no recorded start.
    pub duration: Option<chrono::Duration>,
    /// The preceding event's end, including hidden turn starters for Assistant replies.
    pub previous_end: Option<DateTime<Utc>>,
}
