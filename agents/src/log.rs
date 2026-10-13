use chrono::{DateTime, Utc};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    User,
    Model,
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
    pub path: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// The preceding event's end, including hidden turn starters for Model replies.
    pub previous_end: Option<DateTime<Utc>>,
}
