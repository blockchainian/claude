use crate::model::{EventSort, Grouping};
use crate::providers::{ProviderId, SessionKey, WeeklyUsage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageView {
    #[default]
    Compact,
    Detailed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DraftBranch {
    Unchosen,
    Existing(String),
    New(String),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct StoredDraftInput {
    pub text: String,
    pub images: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StoredDraft {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub input: StoredDraftInput,
    pub cwd: PathBuf,
    pub repository_confirmed: bool,
    pub branch: DraftBranch,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub job: Option<String>,
    pub error: Option<String>,
    #[serde(
        serialize_with = "serialize_draft_time",
        deserialize_with = "deserialize_draft_time"
    )]
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn serialize_draft_time<S: serde::Serializer>(
    time: &chrono::DateTime<chrono::Utc>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&time.to_rfc3339())
}

fn deserialize_draft_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<chrono::DateTime<chrono::Utc>, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    pub worktree_root: Option<PathBuf>,
    pub double_click_ms: u64,
    pub grouping: Grouping,
    pub sorting: EventSort,
    pub pinned: Vec<SessionKey>,
    /// A `.tmTheme` file whose accents color the session titles.
    pub theme: Option<PathBuf>,
    /// A `.tmTheme` file that highlights code on a light background instead of the bundled `light`.
    pub light_theme: Option<PathBuf>,
    /// A `.tmTheme` file that highlights code on a dark background instead of the bundled `dark`.
    pub dark_theme: Option<PathBuf>,
    pub usage_view: UsageView,
    pub usage: HashMap<ProviderId, WeeklyUsage>,
    pub drafts: Vec<StoredDraft>,
    pub draft_notices: Vec<String>,
}

#[derive(Default, Deserialize, Serialize)]
struct StoredPreferences {
    config: StoredConfig,
    state: StoredState,
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct StoredConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree_root: Option<PathBuf>,
    double_click_ms: u64,
    grouping: String,
    sorting: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    light_theme: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dark_theme: Option<PathBuf>,
}

impl Default for StoredConfig {
    fn default() -> Self {
        Self {
            worktree_root: default_worktree_root(),
            double_click_ms: default_double_click_ms(),
            grouping: Grouping::default().as_str().into(),
            sorting: EventSort::default().as_str().into(),
            theme: None,
            light_theme: None,
            dark_theme: None,
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct StoredState {
    pinned: Vec<String>,
    usage_view: UsageView,
    usage: StoredUsage,
    drafts: Vec<serde_json::Value>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct StoredUsage {
    weekly: HashMap<String, StoredWeeklyUsage>,
}

#[derive(Deserialize, Serialize)]
struct StoredWeeklyUsage {
    used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resets_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reset_credits: Option<u64>,
}

impl From<&WeeklyUsage> for StoredWeeklyUsage {
    fn from(usage: &WeeklyUsage) -> Self {
        Self {
            used_percent: usage.used_percent,
            resets_at: usage.resets_at.map(|time| time.to_rfc3339()),
            reset_credits: usage.reset_credits,
        }
    }
}

/// New worktrees go under `~/.agents/worktrees` unless `agents.json` names another root.
fn worktree_root(home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    home.filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".agents/worktrees"))
}

fn default_worktree_root() -> Option<PathBuf> {
    worktree_root(std::env::var_os("HOME"))
}

fn default_double_click_ms() -> u64 {
    300
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            worktree_root: default_worktree_root(),
            double_click_ms: default_double_click_ms(),
            grouping: Grouping::default(),
            sorting: EventSort::default(),
            pinned: Vec::new(),
            theme: None,
            light_theme: None,
            dark_theme: None,
            usage_view: UsageView::default(),
            usage: HashMap::new(),
            drafts: Vec::new(),
            draft_notices: Vec::new(),
        }
    }
}

impl Preferences {
    pub fn load(config_dir: &Path) -> Self {
        let Some(stored) = fs::read(config_dir.join("agents.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StoredPreferences>(&bytes).ok())
        else {
            return Self::default();
        };
        Self::from_stored(stored).unwrap_or_default()
    }

    fn from_stored(stored: StoredPreferences) -> Option<Self> {
        let grouping = Grouping::parse(&stored.config.grouping)?;
        let mut usage = HashMap::new();
        for (name, value) in stored.state.usage.weekly {
            let provider = match name.as_str() {
                "claude" => ProviderId::Claude,
                "codex" => ProviderId::Codex,
                _ => continue,
            };
            let resets_at = value
                .resets_at
                .map(|time| time.parse::<chrono::DateTime<chrono::Utc>>())
                .transpose()
                .ok()?;
            let mut weekly = WeeklyUsage::from_used_percent(value.used_percent, resets_at).ok()?;
            weekly.reset_credits = value.reset_credits;
            usage.insert(provider, weekly);
        }
        let mut drafts = Vec::new();
        let mut draft_notices = Vec::new();
        for (index, value) in stored.state.drafts.into_iter().enumerate() {
            match serde_json::from_value::<StoredDraft>(value) {
                Ok(draft)
                    if !draft.name.trim().is_empty()
                        && matches!(draft.provider.as_str(), "claude" | "codex") =>
                {
                    drafts.push(draft)
                }
                Ok(_) => draft_notices.push(format!("Dropped invalid draft {}", index + 1)),
                Err(error) => draft_notices.push(format!("Dropped draft {}: {error}", index + 1)),
            }
        }
        Some(Self {
            worktree_root: stored.config.worktree_root,
            double_click_ms: stored.config.double_click_ms,
            grouping,
            sorting: EventSort::parse(&stored.config.sorting).unwrap_or_default(),
            pinned: stored
                .state
                .pinned
                .into_iter()
                .filter_map(|value| value.parse().ok())
                .collect(),
            theme: stored.config.theme,
            light_theme: stored.config.light_theme,
            dark_theme: stored.config.dark_theme,
            usage_view: stored.state.usage_view,
            usage,
            drafts,
            draft_notices,
        })
    }

    pub fn save_pinned(&self, config_dir: &Path) -> io::Result<()> {
        update(config_dir, |document| {
            document["state"]["pinned"] = serde_json::to_value(
                self.pinned
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            )?;
            Ok(())
        })
    }

    pub fn save_drafts(&self, config_dir: &Path) -> io::Result<()> {
        update(config_dir, |document| {
            document["state"]["drafts"] = serde_json::to_value(
                self.drafts
                    .iter()
                    .filter(|draft| !draft.name.trim().is_empty())
                    .collect::<Vec<_>>(),
            )?;
            Ok(())
        })
    }

    pub fn save_usage_view(&self, config_dir: &Path) -> io::Result<()> {
        update(config_dir, |document| {
            document["state"]["usage_view"] = serde_json::to_value(self.usage_view)?;
            Ok(())
        })
    }

    pub fn save_usage(
        &self,
        config_dir: &Path,
        provider: ProviderId,
        usage: &WeeklyUsage,
    ) -> io::Result<()> {
        update(config_dir, |document| {
            document["state"]["usage"]["weekly"][provider.to_string()] =
                serde_json::to_value(StoredWeeklyUsage::from(usage))?;
            Ok(())
        })
    }

    pub fn save_grouping(&self, config_dir: &Path) -> io::Result<()> {
        update(config_dir, |document| {
            document["config"]["grouping"] = self.grouping.as_str().into();
            Ok(())
        })
    }

    pub fn save_sorting(&self, config_dir: &Path) -> io::Result<()> {
        update(config_dir, |document| {
            document["config"]["sorting"] = self.sorting.as_str().into();
            Ok(())
        })
    }

    #[cfg(test)]
    pub fn save(&self, config_dir: &Path) -> io::Result<()> {
        update(config_dir, |document| {
            *document = serde_json::to_value(StoredPreferences {
                config: StoredConfig {
                    worktree_root: self.worktree_root.clone(),
                    double_click_ms: self.double_click_ms,
                    grouping: self.grouping.as_str().into(),
                    sorting: self.sorting.as_str().into(),
                    theme: self.theme.clone(),
                    light_theme: self.light_theme.clone(),
                    dark_theme: self.dark_theme.clone(),
                },
                state: StoredState {
                    pinned: self.pinned.iter().map(ToString::to_string).collect(),
                    usage_view: self.usage_view,
                    drafts: self
                        .drafts
                        .iter()
                        .filter(|draft| !draft.name.trim().is_empty())
                        .map(serde_json::to_value)
                        .collect::<Result<_, _>>()?,
                    usage: StoredUsage {
                        weekly: self
                            .usage
                            .iter()
                            .map(|(provider, usage)| {
                                (provider.to_string(), StoredWeeklyUsage::from(usage))
                            })
                            .collect(),
                    },
                },
            })?;
            Ok(())
        })
    }
}

fn update(
    config_dir: &Path,
    change: impl FnOnce(&mut serde_json::Value) -> io::Result<()>,
) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    // Lock a stable inode: the preferences file itself is replaced by rename.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config_dir.join(".agents.lock"))?;
    lock.lock()?;
    let destination = config_dir.join("agents.json");
    let mut document = match fs::read(&destination) {
        Ok(bytes) => {
            let value = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
            value
                .filter(|value| {
                    serde_json::from_value::<StoredPreferences>(value.clone())
                        .ok()
                        .and_then(Preferences::from_stored)
                        .is_some()
                })
                .unwrap_or(serde_json::to_value(StoredPreferences::default())?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            serde_json::to_value(StoredPreferences::default())?
        }
        Err(error) => return Err(error),
    };
    change(&mut document)?;
    let temporary = config_dir.join(format!(".agents-{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        serde_json::to_writer(&mut file, &document)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
