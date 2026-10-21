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
        tasks: HashMap::new(),
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
    assert_eq!(stored["config"]["usage_view"], "detailed");
    assert!(stored["state"].get("usage_view").is_none());
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
        r#"{"config":{"usage_view":"other"},"state":{}}"#,
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
    let mut sorting = pins.clone();
    let task = "codex:sorted".parse().unwrap();
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
    sorting
        .save_task_sort(dir.path(), &task, EventSort::Duration)
        .unwrap();
    let preferences = Preferences::load(dir.path());
    assert_eq!(preferences.pinned, pins.pinned);
    assert_eq!(preferences.usage_view, UsageView::Detailed);
    assert_eq!(preferences.grouping, Grouping::Model);
    assert_eq!(preferences.double_click_ms, 450);
    assert_eq!(preferences.usage.len(), 2);
    assert_eq!(preferences.task_sort(&task), EventSort::Duration);
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("agents.json")).unwrap()).unwrap();
    assert_eq!(stored["config"]["custom"], "keep");
    assert_eq!(stored["state"]["custom"], "keep");
    assert_eq!(stored["config"]["usage_view"], "detailed");
    assert!(stored["state"].get("usage_view").is_none());
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

#[test]
fn global_sorting_and_state_usage_view_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("agents.json"),
        r#"{"config":{"grouping":"model","sorting":"duration"},"state":{"usage_view":"detailed"}}"#,
    )
    .unwrap();
    let preferences = Preferences::load(dir.path());
    assert_eq!(preferences.grouping, Grouping::Model);
    assert_eq!(preferences.usage_view, UsageView::Compact);
    preferences.save(dir.path()).unwrap();
    assert!(document(dir.path())["config"].get("sorting").is_none());
}

fn document(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join("agents.json")).unwrap()).unwrap()
}

#[test]
fn task_state_round_trip_and_independent_writers() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("agents.json"), r#"{"config":{"custom":"keep","usage_view":"detailed"},"state":{"custom":42,"tasks":{"claude:one":{"custom":true}}}}"#).unwrap();
    let one = "claude:one".parse().unwrap();
    let two = "codex:two".parse().unwrap();
    let mut sorting = Preferences::load(dir.path());
    let mut answers = sorting.clone();
    let ask: StoredAsk = serde_json::from_value(serde_json::json!({
        "id":"ask-1", "page":1,
        "answers":{"color":{"answers":["Red"],"other":"extra"}}
    }))
    .unwrap();
    sorting
        .save_task_sort(dir.path(), &one, EventSort::Duration)
        .unwrap();
    answers
        .save_task_ask(dir.path(), &one, Some(ask.clone()))
        .unwrap();
    sorting
        .save_task_sort(dir.path(), &two, EventSort::Duration)
        .unwrap();
    let loaded = Preferences::load(dir.path());
    assert_eq!(loaded.task_sort(&one), EventSort::Duration);
    assert_eq!(loaded.task_sort(&two), EventSort::Duration);
    assert_eq!(loaded.task_ask(&one), Some(&ask));
    assert_eq!(loaded.usage_view, UsageView::Detailed);
    let stored = document(dir.path());
    assert_eq!(stored["config"]["custom"], "keep");
    assert_eq!(stored["state"]["custom"], 42);
    assert_eq!(stored["state"]["tasks"]["claude:one"]["custom"], true);
    assert_eq!(
        stored["state"]["tasks"]["claude:one"]["ask"],
        serde_json::to_value(&ask).unwrap()
    );
    sorting
        .save_task_sort(dir.path(), &one, EventSort::Time)
        .unwrap();
    assert!(
        document(dir.path())["state"]["tasks"]["claude:one"]
            .get("sort")
            .is_none()
    );
    assert_eq!(Preferences::load(dir.path()).task_ask(&one), Some(&ask));
    answers.forget_task(dir.path(), &one).unwrap();
    assert!(
        document(dir.path())["state"]["tasks"]
            .get("claude:one")
            .is_none()
    );
    assert_eq!(
        Preferences::load(dir.path()).task_sort(&two),
        EventSort::Duration
    );
}

#[test]
fn default_sort_and_empty_task_entries_are_removed() {
    let dir = tempfile::tempdir().unwrap();
    let key = "codex:task".parse().unwrap();
    let mut preferences = Preferences::default();
    preferences
        .save_task_sort(dir.path(), &key, EventSort::Time)
        .unwrap();
    assert!(
        document(dir.path())["state"]["tasks"]
            .get("codex:task")
            .is_none()
    );
    preferences
        .save_task_sort(dir.path(), &key, EventSort::Duration)
        .unwrap();
    preferences
        .save_task_sort(dir.path(), &key, EventSort::Time)
        .unwrap();
    assert!(
        document(dir.path())["state"]["tasks"]
            .get("codex:task")
            .is_none()
    );
    preferences
        .save_task_ask(
            dir.path(),
            &key,
            Some(StoredAsk {
                id: "ask-1".into(),
                page: 0,
                answers: Default::default(),
            }),
        )
        .unwrap();
    preferences.save_task_ask(dir.path(), &key, None).unwrap();
    assert!(
        document(dir.path())["state"]["tasks"]
            .get("codex:task")
            .is_none()
    );
    assert_eq!(preferences.task_sort(&key), EventSort::Time);
    assert_eq!(preferences.task_ask(&key), None);
}

#[test]
fn task_write_failure_keeps_preferences_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("not-a-directory");
    fs::write(&config_dir, "occupied").unwrap();
    let key = "codex:task".parse().unwrap();
    let mut preferences = Preferences::default();
    let before = preferences.clone();
    assert!(
        preferences
            .save_task_sort(&config_dir, &key, EventSort::Duration)
            .is_err()
    );
    assert_eq!(preferences, before);
    assert!(
        preferences
            .save_task_ask(
                &config_dir,
                &key,
                Some(StoredAsk {
                    id: "ask-1".into(),
                    page: 0,
                    answers: Default::default(),
                })
            )
            .is_err()
    );
    assert_eq!(preferences, before);
    assert!(preferences.forget_task(&config_dir, &key).is_err());
    assert_eq!(preferences, before);
}

fn pinned_fixture_events() -> Vec<PinnedEvent> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-web-search.jsonl");
    let transcript = fs::read_to_string(&path).unwrap();
    let mut offset = 0;
    transcript
        .split_inclusive('\n')
        .map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            let pin = PinnedEvent {
                source: record["uuid"].as_str().unwrap().to_owned(),
                records: vec![crate::event::SourceRecord {
                    path: path.clone(),
                    offset,
                }],
            };
            offset += line.len() as u64;
            pin
        })
        .collect()
}

#[test]
fn task_pinned_events_round_trip_order_and_replace() {
    let dir = tempfile::tempdir().unwrap();
    let key = "claude:pinned".parse().unwrap();
    let mut preferences = Preferences::default();
    assert!(preferences.task_pinned_events(&key).is_empty());
    let mut pins = pinned_fixture_events();
    assert!(pins.len() > 1);
    pins.reverse();
    preferences
        .save_task_pinned_events(dir.path(), &key, pins.clone())
        .unwrap();
    assert_eq!(preferences.task_pinned_events(&key), pins);
    assert_eq!(Preferences::load(dir.path()).task_pinned_events(&key), pins);
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("agents.json")).unwrap()).unwrap();
    assert_eq!(
        stored["state"]["tasks"][key.to_string()]["pinned_events"],
        serde_json::to_value(&pins).unwrap()
    );
    pins.truncate(1);
    preferences
        .save_task_pinned_events(dir.path(), &key, pins.clone())
        .unwrap();
    assert_eq!(Preferences::load(dir.path()).task_pinned_events(&key), pins);
}

#[test]
fn task_pinned_events_preserve_other_fields_and_remove_empty_entries() {
    let dir = tempfile::tempdir().unwrap();
    let key = "claude:pinned".parse().unwrap();
    let other = "codex:other".parse().unwrap();
    let mut preferences = Preferences::default();
    let mut stale_writer = preferences.clone();
    let pins = pinned_fixture_events();
    preferences
        .save_task_sort(dir.path(), &key, EventSort::Duration)
        .unwrap();
    preferences
        .save_task_sort(dir.path(), &other, EventSort::Duration)
        .unwrap();
    let ask = StoredAsk {
        id: "question".into(),
        page: 0,
        answers: BTreeMap::new(),
    };
    preferences
        .save_task_ask(dir.path(), &key, Some(ask.clone()))
        .unwrap();
    stale_writer
        .save_task_pinned_events(dir.path(), &key, pins.clone())
        .unwrap();
    assert_eq!(stale_writer.task_sort(&key), EventSort::Duration);
    assert_eq!(stale_writer.task_ask(&key), Some(&ask));
    preferences
        .save_task_sort(dir.path(), &key, EventSort::Time)
        .unwrap();
    assert_eq!(preferences.task_pinned_events(&key), pins);
    preferences
        .save_task_pinned_events(dir.path(), &key, Vec::new())
        .unwrap();
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("agents.json")).unwrap()).unwrap();
    assert!(
        stored["state"]["tasks"][key.to_string()]
            .get("pinned_events")
            .is_none()
    );
    let loaded = Preferences::load(dir.path());
    assert!(loaded.task_pinned_events(&key).is_empty());
    assert_eq!(loaded.task_ask(&key), Some(&ask));
    assert_eq!(loaded.task_sort(&other), EventSort::Duration);
    preferences.save_task_ask(dir.path(), &key, None).unwrap();
    assert!(!preferences.tasks.contains_key(&key));
    preferences
        .save_task_pinned_events(dir.path(), &key, pins)
        .unwrap();
    preferences
        .save_task_pinned_events(dir.path(), &key, Vec::new())
        .unwrap();
    assert!(!Preferences::load(dir.path()).tasks.contains_key(&key));
}

#[test]
fn task_pinned_events_write_failure_keeps_memory() {
    let dir = tempfile::tempdir().unwrap();
    let key = "claude:pinned".parse().unwrap();
    let mut preferences = Preferences::default();
    preferences
        .save_task_pinned_events(dir.path(), &key, pinned_fixture_events())
        .unwrap();
    let before = preferences.clone();
    let file = dir.path().join("not-a-directory");
    fs::write(&file, "blocked").unwrap();
    assert!(
        preferences
            .save_task_pinned_events(&file, &key, Vec::new())
            .is_err()
    );
    assert_eq!(preferences, before);
}

#[test]
fn task_filter_round_trip_and_removal_preserves_sort() {
    let dir = tempfile::tempdir().unwrap();
    let key = "claude:filtered".parse().unwrap();
    let other = "codex:other".parse().unwrap();
    let mut preferences = Preferences::default();
    assert_eq!(preferences.task_filter(&key), EventFilter::All);
    preferences
        .save_task_filter(dir.path(), &key, EventFilter::Message)
        .unwrap();
    assert_eq!(
        document(dir.path())["state"]["tasks"]["claude:filtered"]["filter"],
        "message"
    );
    let mut loaded = Preferences::load(dir.path());
    assert_eq!(loaded.task_filter(&key), EventFilter::Message);
    assert_eq!(loaded.task_filter(&other), EventFilter::All);
    loaded
        .save_task_sort(dir.path(), &key, EventSort::Duration)
        .unwrap();
    preferences
        .save_task_filter(dir.path(), &key, EventFilter::All)
        .unwrap();
    assert!(
        document(dir.path())["state"]["tasks"]["claude:filtered"]
            .get("filter")
            .is_none()
    );
    let loaded = Preferences::load(dir.path());
    assert_eq!(loaded.task_filter(&key), EventFilter::All);
    assert_eq!(loaded.task_sort(&key), EventSort::Duration);
    preferences
        .save_task_filter(dir.path(), &key, EventFilter::Message)
        .unwrap();
    preferences
        .save_task_sort(dir.path(), &key, EventSort::Time)
        .unwrap();
    assert_eq!(
        Preferences::load(dir.path()).task_filter(&key),
        EventFilter::Message
    );
    preferences
        .save_task_filter(dir.path(), &key, EventFilter::All)
        .unwrap();
    assert!(
        document(dir.path())["state"]["tasks"]
            .get("claude:filtered")
            .is_none()
    );
}

#[test]
fn task_tool_filter_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let key = "claude:tools".parse().unwrap();
    let filter: EventFilter = serde_json::from_str("\"tool\"").unwrap();
    let mut preferences = Preferences::default();
    preferences
        .save_task_filter(dir.path(), &key, filter)
        .unwrap();
    assert_eq!(
        document(dir.path())["state"]["tasks"]["claude:tools"]["filter"],
        "tool"
    );
    assert_eq!(Preferences::load(dir.path()).task_filter(&key), filter);
}

#[test]
fn task_skipped_at_round_trip_and_removal_preserves_ask() {
    let dir = tempfile::tempdir().unwrap();
    let key = "claude:skipped".parse().unwrap();
    let ask = StoredAsk {
        id: "ask".into(),
        page: 0,
        answers: BTreeMap::new(),
    };
    let skipped_at = "2026-10-21T13:40:12.345Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let mut preferences = Preferences::default();
    preferences
        .save_task_ask(dir.path(), &key, Some(ask.clone()))
        .unwrap();
    preferences
        .save_task_skipped_at(dir.path(), &key, Some(skipped_at))
        .unwrap();
    assert_eq!(
        document(dir.path())["state"]["tasks"]["claude:skipped"]["skipped_at"],
        "2026-10-21T13:40:12.345+00:00"
    );
    let loaded = Preferences::load(dir.path());
    assert_eq!(loaded.task_skipped_at(&key), Some(skipped_at));

    preferences
        .save_task_skipped_at(dir.path(), &key, None)
        .unwrap();
    let loaded = Preferences::load(dir.path());
    assert_eq!(loaded.task_skipped_at(&key), None);
    assert_eq!(loaded.task_ask(&key), Some(&ask));
}
