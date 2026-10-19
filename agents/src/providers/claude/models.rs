//! Reads the model catalog Claude Code caches for the signed-in claude.ai account.

use crate::providers::{InputChoices, ModelChoice};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Cache {
    version: u32,
    catalog: Catalog,
}

#[derive(Deserialize)]
struct Catalog {
    config: Config,
}

#[derive(Deserialize)]
struct Config {
    models: Vec<Model>,
    #[serde(default)]
    provider_alias_targets: std::collections::HashMap<String, AliasTarget>,
}

#[derive(Deserialize)]
struct AliasTarget {
    default: String,
}

#[derive(Deserialize)]
struct Model {
    id: String,
    name: String,
    section: Option<String>,
    #[serde(default)]
    disabled: bool,
    min_claude_code_version: Option<String>,
    runtime: Option<Runtime>,
}

#[derive(Deserialize)]
struct Runtime {
    #[serde(default)]
    effort_levels: Vec<String>,
    default_effort: Option<String>,
}

/// The cache Claude Code itself reads: named by the account's organization and a hash of its id.
fn cache_path(config_dir: &Path) -> Option<PathBuf> {
    let account = super::account::read(config_dir).ok()?;
    let hash = Sha256::digest(account.account_uuid.to_lowercase().as_bytes());
    let hash: String = hash
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Some(config_dir.join(format!(
        "cache/model-catalog/{}-{hash}-cc.json",
        account.organization_uuid
    )))
}

/// Selectable models, main section first; `version` is the installed Claude Code's.
pub fn input_choices(config_dir: &Path, version: Option<&str>) -> InputChoices {
    let Some(cache) = cache_path(config_dir)
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Cache>(&bytes).ok())
        .filter(|cache| cache.version == 2)
    else {
        return InputChoices {
            defaults: super::defaults::builtin(),
            ..InputChoices::default()
        };
    };
    let config = cache.catalog.config;
    let mut models: Vec<_> = config
        .models
        .into_iter()
        .filter(|model| !model.disabled && model.section.as_deref() != Some("deprecated"))
        .filter(|model| {
            model
                .min_claude_code_version
                .as_deref()
                .zip(version)
                .is_none_or(|(minimum, version)| version_at_least(version, minimum))
        })
        .collect();
    models.sort_by_key(|model| model.section.as_deref() != Some("main"));
    InputChoices {
        defaults: super::defaults::builtin(),
        models: models
            .into_iter()
            .map(|model| {
                let runtime = model.runtime;
                ModelChoice {
                    id: model.id,
                    label: model.name,
                    efforts: runtime
                        .as_ref()
                        .map(|runtime| runtime.effort_levels.clone())
                        .unwrap_or_default(),
                    default_effort: runtime.and_then(|runtime| runtime.default_effort),
                }
            })
            .collect(),
        display_names: Default::default(),
        aliases: config
            .provider_alias_targets
            .into_iter()
            .map(|(alias, target)| (alias, target.default))
            .collect(),
    }
}

/// The installed Claude Code's version, from `claude --version`.
pub fn installed_version() -> Option<String> {
    let output = std::process::Command::new("claude")
        .arg("--version")
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

fn version_at_least(version: &str, minimum: &str) -> bool {
    let parts = |text: &str| -> Vec<u64> {
        text.split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    parts(version) >= parts(minimum)
}

/// Writes the recorded model catalog as the cache of a placeholder signed-in account.
#[cfg(test)]
pub fn write_fixture_cache(config_dir: &Path) {
    tests::write_cache(config_dir, tests::CATALOG);
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const CATALOG: &str =
        include_str!("../../../tests/fixtures/claude-model-catalog.json");

    pub(super) fn write_cache(config_dir: &Path, catalog: &str) {
        fs::write(
            config_dir.join(".claude.json"),
            r#"{"oauthAccount":{"organizationUuid":"org-1","accountUuid":"ACCOUNT-1"}}"#,
        )
        .unwrap();
        let hash = Sha256::digest(b"account-1");
        let hash: String = hash
            .iter()
            .take(6)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let directory = config_dir.join("cache/model-catalog");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(format!("org-1-{hash}-cc.json")), catalog).unwrap();
    }

    #[test]
    fn reads_the_signed_in_accounts_catalog_and_resolves_aliases() {
        let directory = tempfile::tempdir().unwrap();
        write_cache(directory.path(), CATALOG);
        let ids = |version| {
            input_choices(directory.path(), Some(version))
                .models
                .into_iter()
                .map(|model| model.id)
                .collect::<Vec<_>>()
        };
        let choices = input_choices(directory.path(), Some("2.1.302"));
        assert_eq!(
            ids("2.1.302")[..5],
            [
                "claude-opus-5-5",
                "claude-fable-5-1",
                "claude-sonnet-5-5",
                "claude-haiku-5-5",
                "claude-haiku-4-5-20251001"
            ]
        );
        assert!(!ids("2.1.279").contains(&"claude-opus-5-5".to_owned()));
        assert!(ids("2.1.279").contains(&"claude-fable-5-1".to_owned()));
        let opus = choices.model("claude-opus-5-5[1m]").unwrap();
        assert_eq!(opus.efforts, ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(opus.default_effort.as_deref(), Some("medium"));
        assert_eq!(choices.model("opus").unwrap().id, "claude-opus-5");
        assert_eq!(
            choices.model("claude-opus-4-6").unwrap().efforts,
            ["low", "medium", "high", "max"]
        );
        assert!(choices.model("haiku").unwrap().efforts.is_empty());
    }

    #[test]
    fn missing_or_unknown_caches_offer_nothing() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            input_choices(directory.path(), None),
            InputChoices {
                defaults: super::super::defaults::builtin(),
                ..InputChoices::default()
            }
        );
        write_cache(
            directory.path(),
            &CATALOG.replacen("\"version\": 2", "\"version\": 3", 1),
        );
        assert_eq!(
            input_choices(directory.path(), None),
            InputChoices {
                defaults: super::super::defaults::builtin(),
                ..InputChoices::default()
            }
        );
    }
}
