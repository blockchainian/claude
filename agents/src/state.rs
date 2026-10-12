//! Persisted command-center preferences.

use crate::model::Grouping;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Preferences {
    pub grouping: Grouping,
    pub pinned: Vec<String>,
    /// A `.tmTheme` file whose accents color the session titles.
    pub theme: Option<PathBuf>,
}

#[derive(Deserialize, Serialize)]
struct StoredPreferences {
    grouping: String,
    pinned: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme: Option<PathBuf>,
}

impl Preferences {
    pub fn load(config_dir: &Path) -> Self {
        let Some(stored) = fs::read(config_dir.join("agents.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StoredPreferences>(&bytes).ok())
        else {
            return Self::default();
        };
        let Some(grouping) = Grouping::parse(&stored.grouping) else {
            return Self::default();
        };
        Self {
            grouping,
            pinned: stored.pinned,
            theme: stored.theme,
        }
    }

    pub fn save(&self, config_dir: &Path) -> io::Result<()> {
        fs::create_dir_all(config_dir)?;
        let destination = config_dir.join("agents.json");
        let temporary = config_dir.join(format!(".agents-{}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = (|| {
            serde_json::to_writer(
                &mut file,
                &StoredPreferences {
                    grouping: self.grouping.as_str().into(),
                    pinned: self.pinned.clone(),
                    theme: self.theme.clone(),
                },
            )?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, destination)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_corrupt_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Preferences::load(dir.path()), Preferences::default());
        let preferences = Preferences {
            grouping: Grouping::Model,
            pinned: vec!["second".into(), "first".into()],
            theme: Some(PathBuf::from("custom.tmTheme")),
        };
        preferences.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()), preferences);
        preferences.save(dir.path()).unwrap();
        fs::write(dir.path().join("agents.json"), b"broken").unwrap();
        assert_eq!(Preferences::load(dir.path()), Preferences::default());
    }
}
