//! Sessions as the command center sees them, independent of where they were read from.

use crate::providers::SessionKey;
use chrono::DateTime;
use chrono::Utc;
use std::path::PathBuf;

/// Status groups in display order; the enum order is the sort order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SessionStatus {
    /// The session waits for the user: a permission prompt or a question.
    NeedsInput,
    /// A turn is running (registry status `busy` or `shell`).
    Working,
    /// A live session with no running turn (registry status `idle`).
    Ready,
    /// No live process; only the transcript remains.
    Inactive,
}

impl SessionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NeedsInput => "Needs input",
            Self::Working => "Working",
            Self::Ready => "Ready",
            Self::Inactive => "Inactive",
        }
    }

    /// Maps a registry status string (`busy`, `shell`, `idle`, `waiting`).
    pub fn from_registry(status: &str) -> Option<Self> {
        match status {
            "waiting" => Some(Self::NeedsInput),
            "busy" | "shell" => Some(Self::Working),
            "idle" => Some(Self::Ready),
            _ => None,
        }
    }
}

/// A live process behind a session, read from `~/.claude/sessions/<pid>.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Process {
    pub pid: u32,
    /// Registry status string as written by Claude Code.
    pub status: String,
    /// `interactive`, `bg`, `daemon` or `daemon-worker`.
    pub kind: Option<String>,
    pub version: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Activity {
    pub label: String,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundTask {
    pub waits_for_codex: bool,
    pub description: String,
    pub output: PathBuf,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    /// Provider-qualified session identity.
    pub id: SessionKey,
    /// Parent session id for a subagent.
    pub parent: Option<SessionKey>,
    pub cwd: PathBuf,
    /// Name the user gave the session (`/rename` or `--name`).
    pub name: Option<String>,
    /// Best available title when there is no user name: the AI title, else the first prompt.
    pub title: String,
    /// Latest real user prompt, up to 512 chars, for the details panel.
    pub prompt: String,
    pub queued: Vec<String>,
    pub prompt_at: Option<DateTime<Utc>>,
    pub turn_messages: usize,
    pub last_record_at: Option<DateTime<Utc>>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub git_branch: Option<String>,
    pub pr: Option<PullRequest>,
    pub status: SessionStatus,
    pub process: Option<Process>,
    pub updated_at: DateTime<Utc>,
    /// Creation time of the subagent transcript.
    pub started_at: Option<DateTime<Utc>>,
    /// Latest assistant text, up to 512 chars.
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub background: Vec<BackgroundTask>,
    /// Latest user prompt, up to 512 chars.
    pub last_prompt: Option<String>,
    pub transcript: Option<PathBuf>,
}

impl Session {
    pub fn display_title(&self) -> &str {
        let title = if self.parent.is_some() {
            &self.title
        } else {
            self.name.as_deref().unwrap_or(&self.title)
        };
        title.trim().lines().next().unwrap_or("Untitled session")
    }

    pub fn latest_activity(&self) -> Option<&Activity> {
        self.activity
            .as_ref()
            .filter(|activity| self.last_message_at.is_none_or(|at| activity.at > at))
    }

    pub fn model_name(&self) -> &str {
        self.model
            .as_deref()
            .filter(|model| !model.is_empty())
            .unwrap_or("Unknown")
    }
}

/// What the view asks the app to do; the view never touches processes or files itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// Enter: jump to a live session's terminal, or resume an inactive one.
    Open(SessionKey),
    /// `o`: resume the session in this terminal (`claude --resume <id>`).
    Resume(SessionKey),
    /// `f`: resume as a new session id (`claude --resume <id> --fork-session`).
    Fork(SessionKey),
    /// `n`: start a new session in the given directory.
    New(Option<PathBuf>),
    /// `h`: hide the session until the next launch.
    Hide(SessionKey),
    /// `p`: pin or unpin the session.
    TogglePin { id: SessionKey, pinned: bool },
    /// `g`: the grouping changed; persist it.
    PersistGrouping(Grouping),
    /// `r`: set the session display name.
    Rename { id: SessionKey, name: String },
    /// Backspace, then confirmation: permanently delete the session transcript.
    Delete(SessionKey),
    /// `w`, `a`: no Claude Code equivalent yet; show the notice.
    Unsupported(&'static str),
    /// "Show more" was activated: load the next page of inactive sessions.
    ShowMore,
    /// Esc on the top level: return to the last launched session, or quit.
    Back,
    /// Ctrl-c.
    Quit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Grouping {
    #[default]
    Project,
    Status,
    Model,
}

impl Grouping {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Status => "status",
            Self::Model => "model",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "status" => Some(Self::Status),
            "model" => Some(Self::Model),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Status => "Status",
            Self::Model => "Model",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Project => Self::Status,
            Self::Status => Self::Model,
            Self::Model => Self::Project,
        }
    }
}
