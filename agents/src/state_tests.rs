use super::*;

#[test]
fn nested_config_and_state_load() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("agents.json"),
        r#"{"config":{"grouping":"model","double_click_ms":200},"state":{"pinned":["codex:cached"]}}"#).unwrap();
    let preferences = Preferences::load(dir.path());
    assert_eq!(preferences.grouping, Grouping::Model);
    assert_eq!(preferences.double_click_ms, 200);
    assert_eq!(preferences.pinned, vec!["codex:cached".parse().unwrap()]);
}

#[test]
fn worktree_root_defaults() {
    assert_eq!(
        worktree_root(Some("/Users/probe".into())),
        Some(PathBuf::from("/Users/probe/.agents/worktrees"))
    );
    assert_eq!(worktree_root(Some("".into())), None);
    assert_eq!(worktree_root(None), None);
}

fn usage(percent: f64, credits: Option<u64>) -> WeeklyUsage {
    let mut usage =
        WeeklyUsage::from_used_percent(percent, Some("2026-10-23T15:59:59Z".parse().unwrap()))
            .unwrap();
    usage.reset_credits = credits;
    usage
}

#[test]
fn preferences_round_trip_and_corrupt_defaults() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(Preferences::load(dir.path()), Preferences::default());
    let preferences = Preferences {
        worktree_root: Some(dir.path().join("trees")),
        double_click_ms: 200,
        grouping: Grouping::Model,
        pinned: vec![
            "claude:second".parse().unwrap(),
            "codex:first".parse().unwrap(),
        ],
        theme: Some(PathBuf::from("custom.tmTheme")),
        light_theme: Some(PathBuf::from("light.tmTheme")),
        dark_theme: Some(PathBuf::from("dark.tmTheme")),
        drafts: Vec::new(),
        draft_notices: Vec::new(),
        usage_view: UsageView::Detailed,
        usage: HashMap::from([
            (ProviderId::Claude, usage(21.0, Some(1))),
            (ProviderId::Codex, usage(16.0, None)),
        ]),
    };
    preferences.save(dir.path()).unwrap();
    assert_eq!(Preferences::load(dir.path()), preferences);
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("agents.json")).unwrap()).unwrap();
    assert_eq!(stored["config"]["double_click_ms"], 200);
    assert_eq!(stored["state"]["usage_view"], "detailed");
    assert_eq!(
        stored["state"]["usage"]["weekly"]["claude"]["used_percent"],
        21.0
    );
    assert_eq!(
        stored["state"]["usage"]["weekly"]["claude"]["reset_credits"],
        1
    );
    assert!(
        stored["state"]["usage"]["weekly"]["codex"]
            .get("reset_credits")
            .is_none()
    );
    fs::write(dir.path().join("agents.json"), b"broken").unwrap();
    assert_eq!(Preferences::load(dir.path()), Preferences::default());
    Preferences::default().save_usage_view(dir.path()).unwrap();
    assert_eq!(Preferences::load(dir.path()), Preferences::default());
}

#[test]
fn unsupported_format_and_invalid_values_use_defaults() {
    let dir = tempfile::tempdir().unwrap();
    for json in [
        r#"{"grouping":"model","pinned":["codex:old"],"double_click_ms":450}"#,
        r#"{"config":{"grouping":"other"},"state":{}}"#,
        r#"{"config":{},"state":{"usage_view":"other"}}"#,
        r#"{"config":{},"state":{"usage":{"weekly":{"claude":{"used_percent":101}}}}}"#,
        r#"{"config":{},"state":{"usage":{"weekly":{"claude":{"used_percent":21,"resets_at":"broken"}}}}}"#,
    ] {
        fs::write(dir.path().join("agents.json"), json).unwrap();
        assert_eq!(Preferences::load(dir.path()), Preferences::default());
    }
}

#[test]
fn invalid_pins_are_dropped_and_optional_values_omitted() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("agents.json"), r#"{"config":{"grouping":"model"},"state":{"pinned":["old-id","claude:kept","codex:kept","unknown:id","claude:"]}}"#).unwrap();
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
    assert!(stored["config"].get("theme").is_none());
    assert!(stored["config"].get("light_theme").is_none());
    assert!(stored["config"].get("dark_theme").is_none());
}

#[test]
fn stale_writers_only_replace_their_field() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("agents.json"), r#"{"config":{"grouping":"project","double_click_ms":450,"custom":"keep"},"state":{"pinned":[],"custom":"keep"}}"#).unwrap();
    let mut pins = Preferences::load(dir.path());
    let mut view = pins.clone();
    let mut grouping = pins.clone();
    pins.pinned = vec!["claude:pinned".parse().unwrap()];
    view.usage_view = UsageView::Detailed;
    grouping.grouping = Grouping::Model;
    pins.save_pinned(dir.path()).unwrap();
    view.save_usage_view(dir.path()).unwrap();
    pins.save_usage(dir.path(), ProviderId::Claude, &usage(21.0, Some(1)))
        .unwrap();
    view.save_usage(dir.path(), ProviderId::Codex, &usage(16.0, Some(2)))
        .unwrap();
    grouping.save_grouping(dir.path()).unwrap();
    let preferences = Preferences::load(dir.path());
    assert_eq!(preferences.pinned, pins.pinned);
    assert_eq!(preferences.usage_view, UsageView::Detailed);
    assert_eq!(preferences.grouping, Grouping::Model);
    assert_eq!(preferences.double_click_ms, 450);
    assert_eq!(preferences.usage.len(), 2);
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("agents.json")).unwrap()).unwrap();
    assert_eq!(stored["config"]["custom"], "keep");
    assert_eq!(stored["state"]["custom"], "keep");
}

#[test]
fn concurrent_processes_preserve_independent_fields() {
    let dir = tempfile::tempdir().unwrap();
    Preferences::default().save(dir.path()).unwrap();
    let mut children = Vec::new();
    for field in ["pins", "view", "claude", "codex"] {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "state::tests::state_writer_process",
                    "--nocapture",
                ])
                .env("AGENTS_STATE_TEST_DIRECTORY", dir.path())
                .env("AGENTS_STATE_TEST_FIELD", field)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let preferences = Preferences::load(dir.path());
    assert_eq!(
        preferences.pinned,
        vec!["claude:concurrent".parse().unwrap()]
    );
    assert_eq!(preferences.usage_view, UsageView::Detailed);
    assert_eq!(
        preferences.usage.get(&ProviderId::Claude),
        Some(&usage(21.0, Some(1)))
    );
    assert_eq!(
        preferences.usage.get(&ProviderId::Codex),
        Some(&usage(16.0, Some(2)))
    );
}

#[test]
fn state_writer_process() {
    let Some(directory) = std::env::var_os("AGENTS_STATE_TEST_DIRECTORY") else {
        return;
    };
    let field = std::env::var("AGENTS_STATE_TEST_FIELD").unwrap();
    let dir = Path::new(&directory);
    let mut preferences = Preferences::load(dir);
    preferences.pinned = vec!["claude:concurrent".parse().unwrap()];
    preferences.usage_view = UsageView::Detailed;
    for _ in 0..25 {
        match field.as_str() {
            "pins" => preferences.save_pinned(dir).unwrap(),
            "view" => preferences.save_usage_view(dir).unwrap(),
            "claude" => preferences
                .save_usage(dir, ProviderId::Claude, &usage(21.0, Some(1)))
                .unwrap(),
            "codex" => preferences
                .save_usage(dir, ProviderId::Codex, &usage(16.0, Some(2)))
                .unwrap(),
            _ => panic!("unexpected state writer"),
        }
    }
}

#[test]
fn missing_optional_usage_fields_stay_absent() {
    let dir = tempfile::tempdir().unwrap();
    let preferences = Preferences::default();
    let weekly = WeeklyUsage::from_used_percent(16.0, None).unwrap();
    preferences
        .save_usage(dir.path(), ProviderId::Codex, &weekly)
        .unwrap();
    assert_eq!(
        Preferences::load(dir.path()).usage.get(&ProviderId::Codex),
        Some(&weekly)
    );
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("agents.json")).unwrap()).unwrap();
    let codex = &stored["state"]["usage"]["weekly"]["codex"];
    assert!(codex.get("resets_at").is_none());
    assert!(codex.get("reset_credits").is_none());
}

fn draft_fixture(name: &str) -> StoredDraft {
    serde_json::from_value(serde_json::json!({
        "id":"draft-1-1", "provider":"claude", "name":name,
        "input":{"text":"half typed", "images":["/tmp/image.png"]},
        "cwd":"/tmp", "repository_confirmed":true,
        "branch":{"existing":"main"}, "model":"opus", "effort":"high",
        "job":null, "error":null, "updated_at":"2026-10-18T09:00:00Z"
    }))
    .unwrap()
}

#[test]
fn drafts_round_trip_drop_unnamed_and_malformed_without_losing_other_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut preferences = Preferences {
        pinned: vec!["claude:kept".parse().unwrap()],
        ..Preferences::default()
    };
    preferences
        .usage
        .insert(ProviderId::Claude, usage(21.0, Some(1)));
    preferences.save(dir.path()).unwrap();
    preferences.drafts = vec![draft_fixture("Fix login"), draft_fixture("")];
    preferences.save_drafts(dir.path()).unwrap();
    let loaded = Preferences::load(dir.path());
    assert_eq!(loaded.drafts, vec![draft_fixture("Fix login")]);
    let destination = dir.path().join("agents.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&destination).unwrap()).unwrap();
    document["state"]["drafts"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id":"broken"}));
    fs::write(&destination, serde_json::to_vec(&document).unwrap()).unwrap();
    let loaded = Preferences::load(dir.path());
    assert_eq!(loaded.drafts, vec![draft_fixture("Fix login")]);
    assert_eq!(loaded.draft_notices.len(), 1);
    assert_eq!(loaded.pinned, preferences.pinned);
    assert_eq!(loaded.usage, preferences.usage);
    loaded.save_usage_view(dir.path()).unwrap();
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(&destination).unwrap()).unwrap();
    assert_eq!(after["state"]["drafts"], document["state"]["drafts"]);
}
