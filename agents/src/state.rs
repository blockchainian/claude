//! Persisted command-center preferences.

use crate::model::Grouping;
use crate::providers::SessionKey;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preferences {
    pub double_click_ms: u64,
    pub grouping: Grouping,
    pub pinned: Vec<SessionKey>,
    /// A `.tmTheme` file whose accents color the session titles.
    pub theme: Option<PathBuf>,
    /// A `.tmTheme` file that highlights code on a light background instead of the bundled `light`.
    pub light_theme: Option<PathBuf>,
    /// A `.tmTheme` file that highlights code on a dark background instead of the bundled `dark`.
    pub dark_theme: Option<PathBuf>,
}

#[derive(Deserialize, Serialize)]
struct StoredPreferences {
    #[serde(default = "default_double_click_ms")]
    double_click_ms: u64,
    grouping: String,
    pinned: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    light_theme: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dark_theme: Option<PathBuf>,
}

fn default_double_click_ms() -> u64 {
    300
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            double_click_ms: default_double_click_ms(),
            grouping: Grouping::default(),
            pinned: Vec::new(),
            theme: None,
            light_theme: None,
            dark_theme: None,
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
        let Some(grouping) = Grouping::parse(&stored.grouping) else {
            return Self::default();
        };
        Self {
            double_click_ms: stored.double_click_ms,
            grouping,
            pinned: stored
                .pinned
                .into_iter()
                .filter_map(|value| value.parse().ok())
                .collect(),
            theme: stored.theme,
            light_theme: stored.light_theme,
            dark_theme: stored.dark_theme,
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
                    double_click_ms: self.double_click_ms,
                    grouping: self.grouping.as_str().into(),
                    pinned: self.pinned.iter().map(ToString::to_string).collect(),
                    theme: self.theme.clone(),
                    light_theme: self.light_theme.clone(),
                    dark_theme: self.dark_theme.clone(),
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
    fn double_click_interval_defaults_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Preferences::load(dir.path()).double_click_ms, 300);
        fs::write(
            dir.path().join("agents.json"),
            r#"{"grouping":"project","pinned":[],"double_click_ms":450}"#,
        )
        .unwrap();
        let preferences = Preferences::load(dir.path());
        assert_eq!(preferences.double_click_ms, 450);
        preferences.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()), preferences);
    }

    #[test]
    fn unprefixed_pins_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("agents.json"),
            r#"{"grouping":"model","pinned":["old-id","claude:kept","codex:kept","unknown:id","claude:"]}"#,
        )
        .unwrap();
        let preferences = Preferences::load(dir.path());
        assert_eq!(preferences.grouping, Grouping::Model);
        assert_eq!(
            preferences
                .pinned
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["claude:kept", "codex:kept"]
        );
        preferences.save(dir.path()).unwrap();
        let stored: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("agents.json")).unwrap()).unwrap();
        assert_eq!(
            stored["pinned"],
            serde_json::json!(["claude:kept", "codex:kept"])
        );
    }

    #[test]
    fn preferences_round_trip_and_corrupt_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Preferences::load(dir.path()), Preferences::default());
        let preferences = Preferences {
            double_click_ms: 300,
            grouping: Grouping::Model,
            pinned: vec![
                "claude:second".parse().unwrap(),
                "claude:first".parse().unwrap(),
            ],
            theme: Some(PathBuf::from("custom.tmTheme")),
            light_theme: Some(PathBuf::from("light.tmTheme")),
            dark_theme: Some(PathBuf::from("dark.tmTheme")),
        };
        preferences.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()), preferences);
        preferences.save(dir.path()).unwrap();
        fs::write(dir.path().join("agents.json"), b"broken").unwrap();
        assert_eq!(Preferences::load(dir.path()), Preferences::default());
    }
}
