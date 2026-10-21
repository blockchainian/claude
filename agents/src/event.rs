pub(crate) mod agent;

use chrono::{DateTime, Utc};

fn participant_name(name: Option<&str>) -> Option<&str> {
    name.map(str::trim).filter(|name| !name.is_empty())
}

/// The `sender → ` prefix a message from another participant opens with.
fn participant_prefix(name: Option<&str>) -> Option<String> {
    participant_name(name).map(|name| format!("{name} → "))
}

pub(crate) fn participant_text(name: Option<&str>, body: &str) -> String {
    match participant_name(name) {
        Some(name) if body.is_empty() => name.to_owned(),
        _ => participant_prefix(name).unwrap_or_default() + body,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    User,
    Compaction,
    Assistant,
    Agent,
    Session,
    Tool,
    Error,
    Denied,
    QueuedUser,
    QueuedSystem,
}
/// Transcript turn containing a row, including whether its boundaries are known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Turn {
    pub id: u64,
    pub started: bool,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub turn: Option<Turn>,
    /// Identity of the transcript record that produced this event.
    pub source: Option<String>,
    /// Completed tools use their recorded duration or completion clock.
    pub completed_time: bool,
    /// The activity continues outside the foreground turn.
    pub background: bool,
    pub source_records: Vec<SourceRecord>,
    pub kind: Kind,
    pub sender: Option<String>,
    pub body: String,
    /// Optional detail text and annotations, separate from the list body.
    pub detail_body: Option<MessageDetail>,
    /// Byte ranges of the file paths inside `body`, in order.
    pub paths: Vec<std::ops::Range<usize>>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Known execution time; absent when a completed activity has no recorded start.
    pub duration: Option<chrono::Duration>,
    /// The preceding event's end, including hidden turn starters for Assistant replies.
    pub previous_end: Option<DateTime<Utc>>,
    /// Images attached to this prompt whose files still exist.
    pub images: Vec<Image>,
    /// The tool calls a Tool, Error or Denied row stands for, in the order they ran.
    pub calls: Vec<ToolCall>,
}

/// Additional message content shown only in the event detail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageDetail {
    pub text: Option<String>,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    pub links: Vec<FileLink>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLink {
    pub range: std::ops::Range<usize>,
    pub path: std::path::PathBuf,
    pub line: u64,
}

/// A transcript line retained by its file position, decoded only when requested.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceRecord {
    pub path: std::path::PathBuf,
    pub offset: u64,
}

impl Event {
    /// Matches either the row source or any member call of a merged row.
    pub fn has_source(&self, id: &str) -> bool {
        self.source.as_deref() == Some(id) || self.calls.iter().any(|call| call.id == id)
    }

    pub fn shares_source(&self, other: &Self) -> bool {
        self.source
            .as_deref()
            .is_some_and(|id| other.has_source(id))
            || self.calls.iter().any(|call| other.has_source(&call.id))
    }

    /// The shared row summary, before Markdown rendering and width-dependent fitting.
    pub fn summary(&self) -> String {
        if let Some(summary) = agent::summary(self) {
            return summary;
        }
        let body = if self.kind == Kind::Agent {
            self.body.split_whitespace().collect::<Vec<_>>().join(" ")
        } else {
            self.body.clone()
        };
        participant_text(self.sender.as_deref(), &body)
    }

    pub fn activity(&self) -> crate::model::Activity {
        let offset = if agent::summary(self).is_some() {
            0
        } else {
            participant_prefix(self.sender.as_deref()).map_or(0, |prefix| prefix.len())
        };
        crate::model::Activity {
            paths: self
                .paths
                .iter()
                .map(|range| range.start + offset..range.end + offset)
                .collect(),
            exit_code: match self.calls.as_slice() {
                [call] => call.exit_code,
                _ => None,
            },
            kind: self.kind.clone(),
            label: self.summary(),
            at: self.started_at,
        }
    }

    #[cfg(test)]
    pub fn raw_text(&self) -> std::io::Result<String> {
        use std::io::{BufRead, Seek};
        let mut records = self.source_records.iter().collect::<Vec<_>>();
        records.sort_by(|a, b| (&a.path, a.offset).cmp(&(&b.path, b.offset)));
        records.dedup();
        records
            .into_iter()
            .map(|record| {
                let mut file = std::fs::File::open(&record.path)?;
                file.seek(std::io::SeekFrom::Start(record.offset))?;
                let mut line = Vec::new();
                std::io::BufReader::new(file).read_until(b'\n', &mut line)?;
                let value: serde_json::Value = serde_json::from_slice(&line)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
                serde_json::to_string_pretty(&value)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
            })
            .collect::<std::io::Result<Vec<_>>>()
            .map(|records| records.join("\n\n"))
    }
}

/// Structural changes produced by one persistent transcript-reader refresh.
#[derive(Default)]
pub(crate) struct Update {
    /// Complete shown rows for a structural change; may be empty for an unchanged read.
    pub events: Vec<Event>,
    pub prepended: usize,
    pub appended: usize,
    pub replaced: usize,
    pub reset: bool,
    pub parsed_bytes: u64,
    /// Older rows remain on disk or decoded outside the shown window.
    pub has_older: bool,
    pub oldest_offset: u64,
    pub read_offset: u64,
}

pub(crate) trait Reader: Send {
    fn refresh(&mut self, requested: usize) -> std::io::Result<Update>;

    /// Decodes one event from each group of records, reading only those lines.
    /// An entry is `None` when its lines cannot be read or decode to no single event.
    fn read_events(&mut self, groups: &[Vec<SourceRecord>]) -> std::io::Result<Vec<Option<Event>>>;
}

impl SourceRecord {
    /// The JSON record on the transcript line starting at this offset.
    /// `None` when the file, the line or its JSON is unavailable.
    pub(crate) fn read(&self) -> Option<serde_json::Value> {
        use std::io::{BufRead, Seek};
        let mut file = std::fs::File::open(&self.path).ok()?;
        file.seek(std::io::SeekFrom::Start(self.offset)).ok()?;
        let mut line = Vec::new();
        std::io::BufReader::new(file)
            .read_until(b'\n', &mut line)
            .ok()?;
        serde_json::from_slice(&line).ok()
    }
}

/// One tool call of a Tool, Error or Denied row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    /// Recorded model settings for an agent creation event.
    pub model_settings: Option<ModelSettings>,
    pub agent_message: Option<agent::Message>,
    /// Per-message clock for collaboration sections.
    pub started_at: Option<DateTime<Utc>>,
    /// Calls such as agent creation and messages have no result block.
    pub hide_output: bool,
    pub agent_thread_id: Option<String>,
    pub agent_path: Option<String>,
    /// Left metadata and centered title, chosen by the provider.
    pub event_detail_left: String,
    pub event_detail_title: String,
    /// Draw calls even for events whose kind normally shows only a message.
    pub show_event_detail: bool,
    /// The provider's stable call identifier.
    pub id: String,
    pub title: String,
    /// The provider's tool identifier.
    pub name: String,
    pub input: Vec<Section>,
    /// A completed command's non-zero exit status.
    pub exit_code: Option<i64>,
    /// `None` while the call is running.
    pub output: Option<Vec<Section>>,
    pub is_error: bool,
    /// How long a completed call in a merged event ran, from its request to its result.
    pub duration: Option<chrono::Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelSettings {
    pub provider: crate::providers::ProviderId,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// One labelled part of a tool call's input or output, as the open event draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Section {
    /// Shell source; `prefix` bytes before the command remain part of copied text.
    Shell { text: String, prefix: usize },
    /// Free text using the shared reading column.
    Prose { text: String, markdown: bool },
    /// A visible section heading.
    Label(String),
    /// Registered structured content with formatting and syntax colors.
    Structured { label: String, text: String },
    /// An unsupported content block, displayed dimly without its payload.
    Placeholder(String),
    /// Code highlighted by `lang`, a syntax token or file extension such as "bash" or "rs".
    Code {
        label: String,
        lang: String,
        text: String,
    },
    /// Unified diff hunks; the label is the changed file's path.
    Diff { label: String, text: String },
    /// Prose rendered as Markdown.
    Markdown { label: String, text: String },
    /// Plain text wrapped at the panel width.
    Text { label: String, text: String },
}

/// An image attached to a prompt: its placeholder in the prompt text, such as
/// `[Image #1]`, and the file it was attached from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub label: String,
    pub path: std::path::PathBuf,
}

#[cfg(test)]
#[path = "event/agent_tests.rs"]
mod agent_tests;
