use super::rollout::{summary, tail_filtered};
use crate::log::{Event, Filter, Kind};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex-inner")
        .join(format!("{name}.jsonl"))
}

fn events(name: &str) -> Vec<Event> {
    tail_filtered(&fixture(name), 100, &Filter::default()).unwrap()
}

fn records(name: &str) -> Vec<Value> {
    std::fs::read_to_string(fixture(name))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn assert_record_timing(event: &Event, record: &Value) {
    let payload = &record["payload"];
    let end = DateTime::<Utc>::from_timestamp_millis(payload["completed_at_ms"].as_i64().unwrap())
        .unwrap();
    let duration = &payload["item"]["duration"];
    let start = if duration.is_object() {
        end - TimeDelta::seconds(duration["secs"].as_i64().unwrap())
            - TimeDelta::nanoseconds(duration["nanos"].as_i64().unwrap())
    } else {
        DateTime::<Utc>::from_timestamp_millis(payload["started_at_ms"].as_i64().unwrap()).unwrap()
    };
    assert_eq!(event.started_at, start);
    assert_eq!(event.ended_at, Some(end));
}

#[test]
fn inline_array_exec_shows_each_completed_command_in_record_order() {
    let entries = events("inline-array");
    let completed: Vec<_> = records("inline-array")
        .into_iter()
        .filter(|record| record["payload"]["item"]["type"] == "CommandExecution")
        .collect();
    assert_eq!(entries.len(), 2);
    for (entry, record) in entries.iter().zip(&completed) {
        assert_eq!(entry.kind, Kind::Tool);
        let command = record["payload"]["item"]["command"][2].as_str().unwrap();
        assert_eq!(entry.body, format!("exec_command {command}"));
        assert_record_timing(entry, record);
        assert!(!entry.body.contains("Promise.all"));
    }
    assert_eq!(entries[1].previous_end, entries[0].ended_at);
}

#[test]
fn single_command_uses_exit_code_and_inner_duration() {
    let entries = events("command");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_eq!(entries[0].body, "exec_command type gwta (exit 1)");
    assert_record_timing(&entries[0], &records("command")[1]);
    let activity = summary(&fixture("command")).unwrap().activity.unwrap();
    assert_eq!(activity.label, entries[0].body);
}

#[test]
fn file_change_uses_record_paths() {
    let entries = events("file-change");
    assert_eq!(entries.len(), 1);
    let record = &records("file-change")[0];
    let path = record["payload"]["item"]["changes"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    assert_eq!(entries[0].body, "apply_patch");
    assert_eq!(entries[0].path.as_deref(), Some(path.as_str()));
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_record_timing(&entries[0], record);
}

#[test]
fn mcp_call_uses_record_server_tool_and_duration() {
    let entries = events("mcp");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "claude.ask_claude");
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_record_timing(&entries[0], &records("mcp")[0]);
}

#[test]
fn pure_js_exec_and_output_never_become_activity() {
    assert!(events("pure-js").is_empty());
    assert!(summary(&fixture("pure-js")).unwrap().activity.is_none());
}

#[test]
fn long_command_written_after_wrapper_output_retains_command_duration() {
    let entries = events("late-command");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "exec_command sleep 300");
    assert_record_timing(&entries[0], &records("late-command")[2]);
    assert!(entries[0].ended_at.unwrap() - entries[0].started_at > TimeDelta::seconds(299));
}

#[test]
fn web_extension_is_tool_activity() {
    let entries = events("extension");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].body,
        "web.search site:pond.dflow.net \"dev-quote-api.dflow.net\""
    );
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_record_timing(&entries[0], &records("extension")[0]);
}

#[test]
fn image_view_retains_image_path_for_rendering() {
    let entries = events("image-view");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "view_image");
    let records = records("image-view");
    let path = records[0]["payload"]["item"]["path"].as_str().unwrap();
    assert_eq!(
        entries[0].path.as_deref(),
        Some(path.strip_prefix("file://").unwrap())
    );
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_record_timing(&entries[0], &records[0]);
}

#[test]
fn subagent_activity_is_rendered() {
    let entries = events("subagent");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "subagent interacted (/root)");
    assert_eq!(entries[0].kind, Kind::Tool);
}

#[test]
fn collaboration_tool_activity_is_rendered() {
    let entries = events("collaboration");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "wait");
    assert_eq!(entries[0].kind, Kind::Tool);
}

#[test]
fn inner_activity_respects_filter_and_page_count() {
    let path = fixture("inline-array");
    let entries = events("inline-array");
    let mut tool = Filter::default();
    tool.toggle(&[Kind::Tool]);
    let page = tail_filtered(&path, 1, &tool).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].body, entries[1].body);
    assert_eq!(page[0].previous_end, entries[0].ended_at);
    let mut assistant = Filter::default();
    assistant.toggle(&[Kind::Assistant]);
    assert!(tail_filtered(&path, 100, &assistant).unwrap().is_empty());
    assert!(tail_filtered(&path, 0, &tool).unwrap().is_empty());
}

#[test]
fn non_activity_completed_items_do_not_become_tool_events() {
    assert!(events("non-activity").is_empty());
    assert!(
        summary(&fixture("non-activity"))
            .unwrap()
            .activity
            .is_none()
    );
}
