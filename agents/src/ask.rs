//! Questions shared by session providers and their answer payloads.

use crate::providers::SessionKey;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ask {
    pub id: String,
    pub kind: AskKind,
    pub session: SessionKey,
    pub since: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AskKind {
    Questions {
        questions: Vec<Question>,
        blocking: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Question {
    /// Opaque provider answer key.
    pub id: String,
    pub header: String,
    pub text: String,
    pub options: Vec<QuestionOption>,
    pub multi: bool,
    pub other: bool,
    pub secret: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
}

/// Collected answers keyed by each question's opaque answer key.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Answer {
    #[serde(default)]
    pub rejected: bool,
    pub answers: BTreeMap<String, QuestionAnswer>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuestionAnswer {
    /// Selected option labels in selection order.
    pub answers: Vec<String>,
    /// Free text supplied in addition to or instead of selected options.
    pub other: Option<String>,
}
