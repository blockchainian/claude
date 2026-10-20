pub(crate) mod claude;
pub(crate) mod codex;
pub(crate) mod text;
pub(crate) mod tool;

use crate::event;
use crate::model::{Action, Session};
use std::{
    collections::HashMap,
    fmt, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    str::FromStr,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProviderId {
    Claude,
    Codex,
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => f.write_str("claude"),
            Self::Codex => f.write_str("codex"),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionKey {
    pub provider: ProviderId,
    pub id: String,
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.id)
    }
}

impl FromStr for SessionKey {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (provider, id) = value.split_once(':').ok_or("Missing provider")?;
        let provider = match provider {
            "claude" => ProviderId::Claude,
            "codex" => ProviderId::Codex,
            _ => return Err("Unknown provider"),
        };
        if id.is_empty() {
            return Err("Empty session id");
        }
        Ok(Self {
            provider,
            id: id.to_owned(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Capabilities {
    pub resume: bool,
    pub fork: bool,
    pub rename: bool,
    pub delete: bool,
    /// Interrupts the running turn; the session stays.
    pub stop: bool,
    pub archive: bool,
}

pub struct Snapshot {
    pub sessions: Vec<Session>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionInput {
    Prompt(String),
    PromptWithImages {
        text: String,
        images: Vec<PathBuf>,
    },
    Answer {
        ask: crate::ask::Ask,
        answer: crate::ask::Answer,
        transcript: PathBuf,
    },
    Interrupt,
    SetModel(String),
    SetEffort(String),
}

/// A selectable model and the efforts it accepts, as its provider's own model cache lists them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelChoice {
    pub id: String,
    /// The provider's own name for the model, such as `Opus 5.5`.
    pub label: String,
    pub family: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

/// The models a provider's cache offers; empty when the provider has no cache to read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputChoices {
    pub defaults: crate::tasks::TaskDefaults,
    pub models: Vec<ModelChoice>,
    /// Short names, such as `opus`, mapped to the model id they stand for.
    pub aliases: HashMap<String, String>,
    /// Catalog names, including models unavailable for selection.
    pub display_names: HashMap<String, String>,
}

impl InputChoices {
    /// The model `name` names, by id or alias, ignoring a context-size suffix such as `[1m]`.
    pub fn model(&self, name: &str) -> Option<&ModelChoice> {
        let name = name.split('[').next().unwrap_or(name);
        let id = self.aliases.get(name).map_or(name, String::as_str);
        self.models.iter().find(|model| model.id == id)
    }

    /// The provider's own name for the model `name` names, by id or alias.
    pub fn label(&self, name: &str) -> Option<&str> {
        let id = name.split('[').next().unwrap_or(name);
        let id = self.aliases.get(id).map_or(id, String::as_str);
        self.display_names
            .get(id)
            .map(String::as_str)
            .or_else(|| self.model(name).map(|model| model.label.as_str()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeeklyUsage {
    pub used_percent: f64,
    pub reset_credits: Option<u64>,
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl WeeklyUsage {
    pub fn left_percent(&self) -> u8 {
        (100.0 - self.used_percent).round() as u8
    }

    pub fn from_used_percent(
        used: f64,
        resets_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> io::Result<Self> {
        if !used.is_finite() || !(0.0..=100.0).contains(&used) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid usage percentage",
            ));
        }
        Ok(Self {
            used_percent: used,
            reset_credits: None,
            resets_at,
        })
    }
}

pub type WeeklyUsageJob = Box<dyn FnOnce() -> io::Result<Option<WeeklyUsage>> + Send>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UsageWindow {
    FiveHour,
    Weekly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResetCreditOffer {
    pub window: UsageWindow,
    pub resets_at: chrono::DateTime<chrono::Utc>,
    pub credit_id: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResetCreditRequest {
    pub offer: ResetCreditOffer,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountUsage {
    pub weekly: Option<WeeklyUsage>,
    pub reset_offers: Vec<ResetCreditOffer>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResetCreditOutcome {
    Reset,
    AlreadyRedeemed,
    NothingToReset,
    NoCredit,
    Cooldown,
    Ineligible,
    Unavailable,
}

pub struct ResetCreditRedemption {
    pub outcome: ResetCreditOutcome,
    pub reason: Option<String>,
    pub usage: AccountUsage,
}

pub type AccountUsageJob = Box<dyn FnOnce() -> io::Result<AccountUsage> + Send>;
pub type ResetCreditJob = Box<dyn FnOnce() -> io::Result<ResetCreditRedemption> + Send>;

pub type Job = Box<dyn FnOnce() -> io::Result<()> + Send>;
pub type CreateJob = Box<dyn FnOnce() -> io::Result<Output> + Send>;

/// The program that shows a session and the bytes that detach its client on close.
#[derive(Debug)]
pub struct TerminalCommand {
    pub command: Command,
    pub detach: Option<&'static [u8]>,
}

/// How input is delivered while a session's terminal is open.
pub enum TerminalInput<'a> {
    Paste {
        text: &'a str,
        submit: &'static [u8],
        /// Readies the program's input before the paste, such as clearing text left in it.
        prepare: Option<fn(&mut dyn ScreenInput) -> io::Result<()>>,
    },
    Write(&'static [u8]),
    /// The terminal closes and the input takes the provider's own path.
    Close,
}

/// A terminal running a provider's program, driven from an input job.
pub trait ScreenInput {
    /// Waits until the complete screen satisfies `ready`, then returns it.
    fn wait_for(&mut self, stage: &str, ready: &dyn Fn(&str) -> bool) -> io::Result<String>;
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;
}

pub trait Provider {
    /// Prepares provider integrations when the dashboard starts.
    fn setup(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// Starts this session in the dashboard's terminal, or explains why it cannot open.
    fn terminal_command(&self, _session: &Session) -> Result<TerminalCommand, &'static str> {
        Err("Terminal is not supported")
    }
    /// Routes input through the open terminal; `None` keeps the normal provider path.
    fn terminal_input<'a>(&self, _input: &'a SessionInput) -> Option<TerminalInput<'a>> {
        None
    }
    fn id(&self) -> ProviderId;
    fn display_name(&self) -> &'static str;
    /// Whether a published terminal screen still shows startup progress.
    fn terminal_screen_is_loading(&self, _screen: &str) -> bool {
        false
    }
    /// Directories watched for changes; missing directories are skipped.
    fn watch_roots(&self) -> io::Result<Vec<PathBuf>>;
    fn capabilities(&self) -> Capabilities;
    fn send(&self, _session: &Session, _input: SessionInput) -> Option<Job> {
        None
    }
    fn create(&self, _request: &crate::tasks::NewTask) -> Option<io::Result<CreateJob>> {
        None
    }
    /// Builds a background request without performing network or credential I/O.
    fn weekly_usage(&self) -> Option<WeeklyUsageJob> {
        None
    }
    fn account_usage(&self) -> Option<AccountUsageJob> {
        self.weekly_usage().map(|job| {
            Box::new(move || {
                Ok(AccountUsage {
                    weekly: job()?,
                    reset_offers: Vec::new(),
                })
            }) as AccountUsageJob
        })
    }
    /// Builds a redemption request; only the confirmed action executes it.
    fn redeem_reset_credit(&self, _request: ResetCreditRequest) -> Option<ResetCreditJob> {
        None
    }
    fn input_choices(&self) -> InputChoices {
        InputChoices::default()
    }
    fn task_defaults(&self, _cwd: &Path) -> Option<io::Result<crate::tasks::TaskDefaults>> {
        None
    }
    fn created_job(&self, _output: &Output) -> io::Result<String> {
        Err(io::ErrorKind::Unsupported.into())
    }
    /// Returns this provider's sessions, including children with provider-qualified parents.
    fn refresh(&mut self, pages: usize) -> Snapshot;
    /// Discovers live sessions and the requested recent history page.
    fn discover(&mut self, pages: usize) -> Snapshot {
        self.refresh(pages)
    }
    /// Discovers direct children of the supplied live-session tree frontier.
    fn children(&mut self, _parents: &[SessionKey]) -> Snapshot {
        Snapshot {
            sessions: vec![],
            has_more: false,
        }
    }
    /// Reads details for the selected sessions.
    fn enrich(&mut self, snapshot: Snapshot) -> Snapshot {
        snapshot
    }
    #[cfg(test)]
    fn event_panel(
        &self,
        session: &Session,
        count: usize,
        sidechains: bool,
    ) -> io::Result<Vec<event::Event>>;
    /// Returns a resume, fork or new-session command when available.
    fn launch(&self, action: &Action, session: Option<&Session>) -> Option<Command>;
    fn rename(&self, session: &Session, name: &str) -> io::Result<()>;
    /// Returns the job that permanently deletes the session, to run off the UI thread.
    fn delete(&self, session: &Session) -> io::Result<Job>;
    /// Returns the archive job to run off the UI thread.
    fn archive(&self, session: &Session) -> io::Result<Job>;
}

pub fn configured() -> io::Result<(crate::sessions::Hub, PathBuf)> {
    let provider = claude::ClaudeProvider::from_config()?;
    let preferences_dir = provider.preferences_dir();
    let mut providers: Vec<Box<dyn Provider>> = vec![
        Box::new(provider),
        Box::new(codex::CodexProvider::from_config()?),
    ];
    for provider in &mut providers {
        provider.setup()?;
    }
    Ok((crate::sessions::Hub::new(providers), preferences_dir))
}

/// Opens a reader whose decoded records and event state survive refreshes.
pub(crate) fn event_reader(session: &Session) -> io::Result<Box<dyn event::Reader>> {
    let path = session
        .transcript
        .clone()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Session has no transcript"))?;
    Ok(match session.id.provider {
        ProviderId::Claude => Box::new(claude::event_reader::Reader::new(
            path,
            session.parent.is_some(),
        )),
        ProviderId::Codex => Box::new(codex::rollout::Reader::new(path, session.parent.is_some())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_key_display_parse_round_trip() {
        let key = SessionKey {
            provider: ProviderId::Claude,
            id: "abc:123".into(),
        };
        assert_eq!(key.to_string(), "claude:abc:123");
        assert_eq!(key.to_string().parse::<SessionKey>(), Ok(key));
    }

    #[test]
    fn session_key_rejects_missing_unknown_or_empty_parts() {
        for value in ["uuid", "", ":uuid", "unknown:uuid", "claude:"] {
            assert!(value.parse::<SessionKey>().is_err(), "{value}");
        }
    }
}
