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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventFilter {
    #[default]
    All,
    Message,
    Tool,
}

impl EventFilter {
    pub fn is_all(&self) -> bool {
        *self == Self::All
    }

    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Message,
            Self::Message => Self::Tool,
            Self::Tool => Self::All,
        }
    }

    pub fn includes(self, kind: &Kind) -> bool {
        let category = match kind {
            Kind::User | Kind::QueuedUser => Self::All,
            Kind::Assistant
            | Kind::Compaction
            | Kind::QueuedSystem
            | Kind::Agent
            | Kind::Session => Self::Message,
            Kind::Tool | Kind::Error | Kind::Denied => Self::Tool,
        };
        self == Self::All || self == category
    }
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

/// An image content block inside a transcript record, read on demand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageBlock {
    pub media_type: String,
    pub record: SourceRecord,
    /// The JSON pointer of the block's base64 data within the record.
    pub pointer: String,
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
                let line = std::str::from_utf8(&line)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
                serde_json::from_str::<serde_json::Value>(line)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
                Ok(line.trim_end_matches(['\r', '\n']).to_owned())
            })
            .collect::<std::io::Result<Vec<_>>>()
            .map(|records| records.join("\n"))
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

    /// Decodes one event from each group, reading only the referenced lines.
    /// Unreadable records or groups that do not decode to one event yield `None`.
    fn read_events(&mut self, groups: &[Vec<SourceRecord>]) -> std::io::Result<Vec<Option<Event>>>;
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
    /// Centered title selected from the tool rule.
    pub title: String,
    /// Draw calls even for events whose kind normally shows only a message.
    pub show_event_detail: bool,
    /// The provider's stable call identifier.
    pub id: String,
    pub summary: String,
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

impl ToolCall {
    /// Tool metadata beside the detail title, derived when rendered.
    pub(crate) fn detail_left(&self) -> String {
        if self.title.trim().is_empty()
            || self.agent_message.is_some()
            || !crate::providers::tool::rule(&self.name).label
        {
            String::new()
        } else {
            crate::providers::tool::display_tool_name(&self.name)
        }
    }
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
    /// Background output read from its current file contents.
    OutputFile(std::path::PathBuf),
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
    /// An image block; drawn inline when the terminal supports images, else as its placeholder text.
    Image(ImageBlock),
    /// Code highlighted by `lang`, a syntax token or file extension such as "bash" or "rs".
    Code {
        label: String,
        lang: String,
        text: String,
    },
    /// Unified diff hunks highlighted by the changed file's language.
    Diff {
        label: String,
        lang: String,
        text: String,
    },
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

#[cfg(test)]
#[path = "event/raw_tests.rs"]
mod raw_tests;

impl Section {
    /// Reads the current background output, retaining message-specific formatting.
    pub(crate) fn read_output_file(path: &std::path::Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                return Self::Placeholder(if error.kind() == std::io::ErrorKind::NotFound {
                    "File no longer exists".to_owned()
                } else {
                    error.to_string()
                });
            }
        };
        let records = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()
            .ok()
            .filter(|records| {
                records.iter().all(|record| record["type"].is_string())
                    && records.iter().any(|record| {
                        matches!(record["type"].as_str(), Some("assistant" | "user"))
                            && record["message"]["role"] == record["type"]
                            && record["message"]["content"].is_array()
                    })
            });
        let Some(records) = records else {
            return crate::codex_await_output::section(text);
        };
        let handback = records
            .iter()
            .filter(|record| record["type"] == "assistant")
            .filter_map(|record| record["message"]["content"].as_array())
            .flatten()
            .rfind(|block| block["type"] == "tool_use" && block["name"] == "SubagentHandback")
            .and_then(|block| block["input"]["message"].as_str())
            .map(str::to_owned);
        let message = handback.or_else(|| {
            let last = records.last()?;
            if last["type"] != "assistant" {
                return None;
            }
            last["message"]["content"]
                .as_array()?
                .iter()
                .map(|block| {
                    (block["type"] == "text")
                        .then(|| block["text"].as_str())
                        .flatten()
                })
                .collect::<Option<Vec<_>>>()
                .map(|parts| parts.join("\n"))
        });
        Section::Markdown {
            label: String::new(),
            text: message.unwrap_or_default(),
        }
    }
}
