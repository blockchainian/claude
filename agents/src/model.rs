//! Sessions as the command center sees them, independent of where they were read from.

use crate::providers::{ProviderId, SessionKey};
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
}

/// A live process behind a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Process {
    pub pid: u32,
    pub label: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Activity {
    pub kind: crate::log::Kind,
    pub label: String,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundTask {
    pub description: String,
    pub output: PathBuf,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    /// Provider-qualified session identity.
    pub id: SessionKey,
    /// Display parent for nesting, including codex-manager ownership links.
    pub parent: Option<SessionKey>,
    pub cwd: PathBuf,
    /// Name the user gave the session (`/rename` or `--name`).
    pub name: Option<String>,
    /// Best available title when there is no user name: the AI title, else the first prompt.
    pub title: String,
    /// Latest real user prompt, up to 512 chars, for the details panel.
    pub prompt: String,
    /// Images attached to `prompt` whose files still exist.
    pub prompt_images: Vec<crate::log::Image>,
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
    /// Start of the session or thread, independent of its process and activity.
    pub started_at: Option<DateTime<Utc>>,
    /// Latest assistant text, up to 512 chars.
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub pending_questions: Vec<String>,
    pub background: Vec<BackgroundTask>,
    /// Latest user prompt, up to 512 chars.
    pub last_prompt: Option<String>,
    pub transcript: Option<PathBuf>,
}

impl Session {
    /// Whether provider session operations apply; display nesting does not make a thread a subagent.
    pub fn can_resume(&self) -> bool {
        self.parent.as_ref().is_none_or(|parent| {
            self.id.provider == ProviderId::Codex && parent.provider == ProviderId::Claude
        })
    }

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
    /// `o`: resume the session in this terminal.
    Resume(SessionKey),
    /// `f`: resume as a new session id.
    Fork(SessionKey),
    /// `n`: start a new Claude session in the dashboard's directory.
    New,
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
    /// `a`, then confirmation: archive the session and its child agents.
    Archive(SessionKey),
    /// `x`: interrupt the session's running turn.
    Stop(SessionKey),
    /// Unsupported provider operations show a notice.
    Unsupported(&'static str),
    /// "Show more" was activated: load the next page of inactive sessions.
    ShowMore,
    /// Esc on the top level: return to the last launched session, or quit.
    Back,
    /// Ctrl-c.
    Quit,
    /// Double click on an open log event or a details body: copy its text to the
    /// clipboard and confirm with `label`.
    Copy { text: String, label: String },
    /// Click on a drawn path, image placeholder or pull request link: open the
    /// file, directory or URL with macOS `open`.
    OpenPath(PathBuf),
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
