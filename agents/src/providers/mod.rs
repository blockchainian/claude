pub(crate) mod claude;

use crate::model::Session;
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProviderId {
    Claude,
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => f.write_str("claude"),
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
    pub new: bool,
    pub rename: bool,
    pub delete: bool,
    pub stop: bool,
    pub archive: bool,
}

pub struct Snapshot {
    pub sessions: Vec<Session>,
    pub has_more: bool,
}

pub trait Provider {
    #[allow(
        dead_code,
        reason = "Action dispatch is separate from snapshot refresh"
    )]
    fn id(&self) -> ProviderId;
    #[allow(
        dead_code,
        reason = "Action dispatch is separate from snapshot refresh"
    )]
    fn capabilities(&self) -> Capabilities;
    /// Returns this provider's sessions, including children with provider-qualified parents.
    fn refresh(&mut self, pages: usize) -> Snapshot;
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
        for value in ["uuid", "", ":uuid", "codex:uuid", "claude:"] {
            assert!(value.parse::<SessionKey>().is_err(), "{value}");
        }
    }
}
