pub(crate) mod claude;
pub(crate) mod codex;

use crate::log;
use crate::model::{Action, Session};
use std::{
    fmt, io,
    path::PathBuf,
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
    PromptWithImages { text: String, images: Vec<PathBuf> },
    Interrupt,
    SetModel(String),
    SetEffort(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputChoices {
    pub models: Vec<String>,
    pub efforts: Vec<String>,
}

pub type Job = Box<dyn FnOnce() -> io::Result<()> + Send>;

pub trait Provider {
    fn id(&self) -> ProviderId;
    /// Directories watched for changes; missing directories are skipped.
    fn watch_roots(&self) -> io::Result<Vec<PathBuf>>;
    fn capabilities(&self) -> Capabilities;
    fn send(&self, _session: &Session, _input: SessionInput) -> Option<Job> {
        None
    }
    fn create(&self, _request: &crate::tasks::NewTask) -> Option<io::Result<Command>> {
        None
    }
    fn input_choices(&self) -> InputChoices {
        InputChoices::default()
    }
    fn task_defaults(&self) -> Option<io::Result<crate::tasks::TaskDefaults>> {
        None
    }
    fn created_job(&self, _output: &Output) -> io::Result<String> {
        Err(io::ErrorKind::Unsupported.into())
    }
    /// Returns this provider's sessions, including children with provider-qualified parents.
    fn refresh(&mut self, pages: usize) -> Snapshot;
    #[cfg(test)]
    fn log(&self, session: &Session, count: usize, sidechains: bool)
    -> io::Result<Vec<log::Event>>;
    /// Returns a resume, fork or new-session command when available.
    fn launch(&self, action: &Action, session: Option<&Session>) -> Option<Command>;
    fn rename(&self, session: &Session, name: &str) -> io::Result<()>;
    fn delete(&self, session: &Session) -> io::Result<()>;
    fn archive(&self, session: &Session) -> io::Result<()>;
}

pub fn configured() -> io::Result<(crate::sessions::Hub, PathBuf)> {
    let provider = claude::ClaudeProvider::from_config()?;
    let preferences_dir = provider.preferences_dir();
    Ok((
        crate::sessions::Hub::new(vec![
            Box::new(provider),
            Box::new(codex::CodexProvider::from_config()?),
        ]),
        preferences_dir,
    ))
}

/// Opens a reader whose decoded records and event state survive refreshes.
pub(crate) fn log_reader(session: &Session) -> io::Result<Box<dyn log::Reader>> {
    let path = session
        .transcript
        .clone()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Session has no transcript"))?;
    Ok(match session.id.provider {
        ProviderId::Claude => Box::new(claude::log::Reader::new(path, session.parent.is_some())),
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
