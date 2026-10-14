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
    let bodies: Vec<_> = entries.iter().map(|entry| entry.body.as_str()).collect();
    assert_eq!(bodies, ["cat, git {log, status}, shasum", "gh pr"]);
    for (entry, record) in entries.iter().zip(&completed) {
        assert_eq!(entry.kind, Kind::Tool);
        assert_record_timing(entry, record);
    }
    assert_eq!(entries[1].previous_end, entries[0].ended_at);
}

#[test]
fn single_command_uses_exit_code_and_inner_duration() {
    let entries = events("command");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_eq!(entries[0].body, "type (exit 1)");
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
    assert_eq!(entries[0].body, format!("apply_patch {path}"));
    assert_eq!(&entries[0].body[entries[0].paths[0].clone()], path);
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
    assert_eq!(entries[0].body, "sleep");
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
    let records = records("image-view");
    let path = records[0]["payload"]["item"]["path"].as_str().unwrap();
    let path = path.strip_prefix("file://").unwrap();
    assert_eq!(entries[0].body, format!("view_image {path}"));
    assert_eq!(&entries[0].body[entries[0].paths[0].clone()], path);
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

#[test]
fn file_change_without_start_log_anchors_at_completion() {
    let entries = events("file-change-without-start");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "apply_patch /workspace/project/design.md");
    let completion = DateTime::from_timestamp_millis(1781329074517).unwrap();
    assert_eq!(entries[0].started_at, completion);
    assert_eq!(entries[0].ended_at, Some(completion));
    assert_eq!(entries[0].duration, None);
}

#[test]
fn file_change_without_start_summary_succeeds() {
    let activity = summary(&fixture("file-change-without-start"))
        .unwrap()
        .activity
        .unwrap();
    assert_eq!(activity.label, "apply_patch /workspace/project/design.md");
    assert_eq!(
        activity.at,
        DateTime::from_timestamp_millis(1781329074517).unwrap()
    );
}

fn check_completed_record(mut record: Value, change: impl FnOnce(&mut Value), valid: bool) {
    change(&mut record);
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), format!("{record}\n")).unwrap();
    assert_eq!(
        tail_filtered(file.path(), 100, &Filter::default()).is_ok(),
        valid,
        "{record}"
    );
    assert_eq!(summary(file.path()).is_ok(), valid, "{record}");
}

#[test]
fn completed_record_protocol_defaults_and_optional_types() {
    let file_change = records("file-change").remove(0);
    check_completed_record(
        file_change.clone(),
        |r| {
            r["payload"]
                .as_object_mut()
                .unwrap()
                .remove("completed_at_ms");
        },
        true,
    );
    check_completed_record(
        file_change.clone(),
        |r| {
            r["payload"]["completed_at_ms"] = Value::Null;
        },
        false,
    );
    check_completed_record(
        file_change.clone(),
        |r| {
            r["payload"]["started_at_ms"] = Value::Null;
        },
        true,
    );
    check_completed_record(
        file_change.clone(),
        |r| {
            r["payload"]["started_at_ms"] = "bad".into();
        },
        false,
    );
    check_completed_record(
        file_change,
        |r| {
            r["payload"]["item"].as_object_mut().unwrap().remove("id");
        },
        false,
    );
    let collab = records("collaboration").remove(0);
    check_completed_record(
        collab.clone(),
        |r| {
            r["payload"]["item"]
                .as_object_mut()
                .unwrap()
                .remove("receiver_thread_ids");
        },
        true,
    );
    check_completed_record(
        collab,
        |r| {
            r["payload"]["item"]["receiver_thread_ids"] = Value::Null;
        },
        false,
    );
    let command = records("command").remove(1);
    check_completed_record(
        command.clone(),
        |r| {
            r["payload"]["item"]["exit_code"] = "bad".into();
        },
        false,
    );
    check_completed_record(
        command.clone(),
        |r| {
            r["payload"]["item"]["exit_code"] = i64::from(i32::MAX).saturating_add(1).into();
        },
        false,
    );
    check_completed_record(
        command.clone(),
        |r| {
            r["payload"]["item"]["exit_code"] = Value::Null;
            r["payload"]["item"]["duration"] = Value::Null;
        },
        true,
    );
    check_completed_record(
        command,
        |r| {
            r["payload"]["item"]["command"] = serde_json::json!([]);
        },
        true,
    );
    let extension = records("extension").remove(0);
    check_completed_record(
        extension.clone(),
        |r| {
            r["payload"]["item"]
                .as_object_mut()
                .unwrap()
                .remove("query");
        },
        false,
    );
    check_completed_record(
        extension,
        |r| {
            r["payload"]["item"]["query"] = Value::Null;
        },
        false,
    );
}

#[test]
fn completed_message_and_extension_fields_follow_protocol() {
    let message: Value = include_str!("../../../tests/fixtures/codex-rollout.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|record| record["payload"]["item"]["type"] == "AgentMessage")
        .unwrap();
    check_completed_record(message.clone(), |_| {}, true);
    for key in ["id", "content"] {
        check_completed_record(
            message.clone(),
            |r| {
                r["payload"]["item"].as_object_mut().unwrap().remove(key);
            },
            false,
        );
    }
    check_completed_record(
        message.clone(),
        |r| {
            r["payload"]["item"]["content"][0]
                .as_object_mut()
                .unwrap()
                .remove("text");
        },
        false,
    );
    check_completed_record(
        message,
        |r| {
            r["payload"]["item"]["content"] = "bad".into();
        },
        false,
    );
    let image = records("image-generation").remove(0);
    check_completed_record(
        image.clone(),
        |r| {
            r["payload"]["item"]
                .as_object_mut()
                .unwrap()
                .remove("revisedPrompt");
        },
        true,
    );
    check_completed_record(
        image.clone(),
        |r| {
            r["payload"]["item"]["revisedPrompt"] = Value::Null;
        },
        true,
    );
    check_completed_record(
        image,
        |r| {
            r["payload"]["item"]["revisedPrompt"] = 42.into();
        },
        false,
    );
}

#[test]
fn command_without_start_has_unknown_duration_even_with_item_duration() {
    let mut record = records("command").remove(1);
    assert!(record["payload"]["item"]["duration"].is_object());
    record["payload"]
        .as_object_mut()
        .unwrap()
        .remove("started_at_ms");
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), format!("{record}\n")).unwrap();
    let entries = tail_filtered(file.path(), 100, &Filter::default()).unwrap();
    assert_eq!(entries[0].duration, None);
    assert_eq!(entries[0].started_at, entries[0].ended_at.unwrap());
    assert!(summary(file.path()).unwrap().activity.is_some());
}
