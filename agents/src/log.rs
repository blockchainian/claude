use chrono::{DateTime, Utc};

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
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

/// A transcript line retained by its file position, decoded only when requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRecord {
    pub path: std::path::PathBuf,
    pub offset: u64,
}

impl Event {
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

/// Structural changes produced by one persistent log-reader refresh.
#[derive(Default)]
pub(crate) struct Update {
    pub events: Vec<Event>,
    pub prepended: usize,
    pub appended: usize,
    pub replaced: usize,
    pub reset: bool,
    pub parsed_bytes: u64,
    pub oldest_offset: u64,
    pub read_offset: u64,
}

pub(crate) trait Reader: Send {
    fn refresh(&mut self, requested: usize) -> std::io::Result<Update>;
}

/// One tool call of a Tool, Error or Denied row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    /// Left metadata and centered title, chosen by the provider.
    pub detail_left: String,
    pub detail_title: String,
    /// Draw calls even for events whose kind normally shows only a message.
    pub show_details: bool,
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

/// One labelled part of a tool call's input or output, as the open log event draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Section {
    /// Shell source; `prefix` bytes before the command remain part of copied text.
    Shell { text: String, prefix: usize },
    /// Free text using the shared reading column.
    Prose { text: String, markdown: bool },
    /// A visible section heading.
    Label(String),
    /// JSON displayed with indentation, syntax colors and multiline string values.
    Json { label: String, text: String },
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
