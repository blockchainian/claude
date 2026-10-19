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
    pub exit_code: Option<i64>,
    pub kind: crate::log::Kind,
    pub label: String,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundTask {
    pub description: String,
    pub output: Option<PathBuf>,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundJob {
    pub id: String,
    pub needs: Option<String>,
    /// The job ended its turn waiting for a reply, which the user sends as a prompt.
    pub awaits_prompt: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTask {
    pub id: String,
    pub subject: String,
    pub status: TaskStatus,
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
    /// Source record or tool-call identities for activity, update and prompt.
    pub detail_sources: [Option<String>; 3],
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
    pub background_job: Option<BackgroundJob>,
    pub updated_at: DateTime<Utc>,
    /// Start of the session or thread, independent of its process and activity.
    pub started_at: Option<DateTime<Utc>>,
    /// Latest assistant text, up to 512 chars.
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub ask: Option<crate::ask::Ask>,
    pub pending_questions: Vec<String>,
    pub background: Vec<BackgroundTask>,
    /// The session's own to-do list, replayed from its transcript.
    pub tasks: Vec<SessionTask>,
    /// Latest user prompt, up to 512 chars.
    pub last_prompt: Option<String>,
    pub transcript: Option<PathBuf>,
    /// Archived by its provider; still listed as inactive.
    pub archived: bool,
}

#[cfg(test)]
pub use crate::providers::claude::FOREGROUND_SESSION;

impl Session {
    /// Whether provider session operations apply; display nesting does not make a thread a subagent.
    pub fn can_resume(&self) -> bool {
        self.parent.as_ref().is_none_or(|parent| {
            self.id.provider == ProviderId::Codex && parent.provider == ProviderId::Claude
        })
    }

    pub fn is_live_background(&self) -> bool {
        self.id.provider == ProviderId::Claude
            && self.background_job.is_some()
            && self
                .process
                .as_ref()
                .is_some_and(|process| process.label.as_deref() == Some("bg"))
    }

    pub fn input_disabled_reason(&self) -> Option<&'static str> {
        if !self.can_resume() {
            Some("")
        } else if self.id.provider == ProviderId::Claude
            && self
                .process
                .as_ref()
                .is_some_and(|process| process.label.as_deref() != Some("bg"))
        {
            Some("/bg")
        } else {
            None
        }
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
}

pub type DraftId = u64;

/// What the view asks the app to do; the view never touches processes or files itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Batch(Vec<Action>),
    SendPrompt {
        id: SessionKey,
        text: String,
    },
    AnswerQuestions {
        ask: crate::ask::Ask,
        answer: crate::ask::Answer,
    },
    PasteImage,
    SendImages {
        id: SessionKey,
        text: String,
        images: Vec<PathBuf>,
    },
    /// `n` or a project heading double click: add a draft in that project.
    NewDraft {
        cwd: PathBuf,
    },
    /// Creates a task, first moving it to the checkout of `branch` when one is chosen.
    CreateTask {
        draft_id: DraftId,
        provider: ProviderId,
        request: crate::tasks::NewTask,
        branch: Option<crate::tasks::BranchChoice>,
    },
    /// Moves a new task into another repository.
    ChangeDraftDirectory {
        draft_id: DraftId,
        cwd: PathBuf,
    },
    SetModel {
        id: SessionKey,
        model: String,
    },
    SetEffort {
        id: SessionKey,
        effort: String,
    },
    /// Build a provider resume command.
    Resume(SessionKey),
    /// Enter, Right, Ctrl-L, `o` or a right click on the log detail: run the session's
    /// program in the log detail panel.
    OpenTerminal(SessionKey),
    /// A right click on the session terminal: detach it and show the log detail again.
    CloseTerminal,
    /// Keys and mouse reports for the session terminal.
    TerminalInput(Vec<u8>),
    /// A paste into the session terminal.
    TerminalPaste(String),
    /// `f`: resume as a new session id.
    Fork(SessionKey),
    /// Start a session when there is no previous task to return to.
    New,
    /// `h`: hide the session until the next launch.
    Hide(SessionKey),
    /// `p`: pin or unpin the session.
    TogglePin {
        id: SessionKey,
        pinned: bool,
    },
    /// `g`: the grouping changed; persist it.
    PersistGrouping(Grouping),
    PersistUsageView(crate::state::UsageView),
    /// `r`: set the session display name.
    Rename {
        id: SessionKey,
        name: String,
    },
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
    /// Ctrl-c.
    Quit,
    /// Double click on an open log event or a details body: copy its text to the
    /// clipboard and confirm with `label`.
    Copy {
        text: String,
        label: String,
    },
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
