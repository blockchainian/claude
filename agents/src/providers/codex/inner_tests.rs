use super::rollout::{summary, tail};
use crate::log::{Event, Kind};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex-inner")
        .join(format!("{name}.jsonl"))
}

fn events(name: &str) -> Vec<Event> {
    tail(&fixture(name), 100).unwrap()
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
    assert_eq!(entries[0].body, "type (1)");
    assert_record_timing(&entries[0], &records("command")[1]);
    let activity = summary(&fixture("command")).unwrap().activity.unwrap();
    assert_eq!(activity.label, entries[0].body);
    assert_eq!(activity.exit_code, Some(1));
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
    assert_eq!(entries[0].body, format!("Apply Patch {path}"));
    assert_eq!(&entries[0].body[entries[0].paths[0].clone()], path);
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_record_timing(&entries[0], record);
}

#[test]
fn mcp_call_uses_record_server_tool_and_duration() {
    let entries = events("mcp");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "Claude Ask Claude");
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
    assert_eq!(entries[0].body, "Extension");
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_record_timing(&entries[0], &records("extension")[0]);
}

#[test]
fn image_view_retains_image_path_for_rendering() {
    let entries = events("image-view");
    assert_eq!(entries.len(), 1);
    let records = records("image-view");
    assert_eq!(entries[0].body, "ImageView");
    assert!(entries[0].paths.is_empty());
    let crate::log::Section::Structured { text, .. } = &entries[0].calls[0].input[0] else {
        panic!("image arguments require JSON");
    };
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap()["path"],
        records[0]["payload"]["item"]["path"]
    );
    assert_eq!(entries[0].kind, Kind::Tool);
    assert_record_timing(&entries[0], &records[0]);
}

#[test]
fn subagent_activity_is_rendered() {
    assert!(events("subagent").is_empty());
}

#[test]
fn collaboration_tool_activity_is_rendered() {
    assert!(events("collaboration").is_empty());
}

#[test]
fn completed_subagent_items_have_finished_calls_without_pairing_item_ids() {
    assert!(events("subagent-completed").is_empty());
}

#[test]
fn completed_subagent_calls_finish_as_each_record_arrives() {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let mut reader = super::rollout::Reader::new(file.path().to_path_buf(), false);
    for record in records("subagent-completed") {
        writeln!(file, "{record}").unwrap();
        let update = reader.refresh(100).unwrap();
        assert!(
            update
                .events
                .iter()
                .flat_map(|event| &event.calls)
                .all(|call| call.output.is_some())
        );
    }
}

#[test]
fn completed_collaboration_calls_finish_for_every_tool_and_status() {
    for tool in [
        "spawn_agent",
        "send_input",
        "resume_agent",
        "close_agent",
        "wait",
    ] {
        for status in ["completed", "failed"] {
            let mut record = records("collaboration").remove(0);
            record["payload"]["item"]["tool"] = tool.into();
            record["payload"]["item"]["status"] = status.into();
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), format!("{record}\n")).unwrap();
            let entries = tail(file.path(), 100).unwrap();
            assert!(entries.is_empty(), "{tool}: {status}");
        }
    }
}

#[test]
fn completed_items_without_result_bodies_are_finished() {
    for name in [
        "subagent",
        "collaboration",
        "extension",
        "image-view",
        "image-generation",
    ] {
        for entry in events(name) {
            assert!(
                entry.calls.iter().all(|call| call.output.is_some()),
                "{name}: {entry:?}"
            );
        }
    }
}

#[test]
fn inner_activity_respects_page_count() {
    let path = fixture("inline-array");
    let entries = events("inline-array");
    assert_eq!(tail(&path, 1).unwrap(), entries[entries.len() - 1..]);
    assert!(tail(&path, 0).unwrap().is_empty());
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
    assert_eq!(entries[0].body, "Apply Patch /workspace/project/design.md");
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
    assert_eq!(activity.label, "Apply Patch /workspace/project/design.md");
    assert_eq!(
        activity.at,
        DateTime::from_timestamp_millis(1781329074517).unwrap()
    );
}

fn check_completed_record(mut record: Value, change: impl FnOnce(&mut Value), valid: bool) {
    change(&mut record);
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), format!("{record}\n")).unwrap();
    assert_eq!(tail(file.path(), 100).is_ok(), valid, "{record}");
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
    let entries = tail(file.path(), 100).unwrap();
    assert_eq!(entries[0].duration, None);
    assert_eq!(entries[0].started_at, entries[0].ended_at.unwrap());
    assert!(summary(file.path()).unwrap().activity.is_some());
}

fn image_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex-images")
        .join(format!("{name}.jsonl"))
}

#[test]
fn prompt_image_wrappers_are_not_log_text() {
    for name in ["wrapped", "fallback"] {
        let records: Vec<Value> = std::fs::read_to_string(image_fixture(name))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let entries = tail(&image_fixture(name), 100).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, Kind::User);
        assert_eq!(entries[0].body, records[0]["payload"]["content"][3]["text"]);
    }
}

fn relocated_image_records(name: &str, path: &Path) -> Vec<Value> {
    let mut records: Vec<Value> = std::fs::read_to_string(image_fixture(name))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for record in &mut records {
        let content = if record["type"] == "response_item" {
            &mut record["payload"]["content"]
        } else {
            &mut record["payload"]["item"]["content"]
        };
        for block in content.as_array_mut().unwrap() {
            if block["type"] == "local_image" {
                block["path"] = path.to_str().unwrap().into();
            } else if let Some(text) = block["text"].as_str()
                && text.starts_with("<image name=")
                && text.contains(" path=\"")
            {
                block["text"] =
                    format!("<image name=[Image #1] path=\"{}\">", path.display()).into();
            }
        }
    }
    records
}

fn write_image_records(path: &Path, records: &[Value]) {
    std::fs::write(
        path,
        records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>(),
    )
    .unwrap();
}

#[test]
fn prompt_image_paths_use_wrapper_then_paired_completed_event() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("attachment.png");
    std::fs::write(&image, b"image").unwrap();
    let rollout = dir.path().join("rollout.jsonl");
    for name in ["wrapped", "fallback"] {
        let records = relocated_image_records(name, &image);
        write_image_records(&rollout, &records);
        let entries = tail(&rollout, 1).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].images,
            [crate::log::Image {
                label: "[Image #1]".into(),
                path: image.clone()
            }]
        );
        // The wrapper is enough when the completed event has not been written yet.
        write_image_records(&rollout, &records[..1]);
        assert_eq!(
            tail(&rollout, 1).unwrap()[0].images.len(),
            usize::from(name == "wrapped")
        );
    }
    std::fs::remove_file(&image).unwrap();
    for name in ["wrapped", "fallback"] {
        write_image_records(&rollout, &relocated_image_records(name, &image));
        assert!(tail(&rollout, 1).unwrap()[0].images.is_empty());
    }
}

#[test]
fn prompt_image_pairing_does_not_borrow_another_prompts_paths() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("attachment.png");
    std::fs::write(&image, b"image").unwrap();
    let rollout = dir.path().join("rollout.jsonl");
    let first = relocated_image_records("fallback", &image);
    let second = relocated_image_records("wrapped", &dir.path().join("missing.png"));
    // An older prompt's completed event cannot supply the latest prompt.
    write_image_records(
        &rollout,
        &[first[0].clone(), first[1].clone(), second[0].clone()],
    );
    let entries = tail(&rollout, 100).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].images.len(), 1);
    assert!(entries[1].images.is_empty());
    // A newer unrelated completed prompt cannot supply the older prompt either.
    write_image_records(&rollout, &[second[0].clone(), first[1].clone()]);
    assert!(tail(&rollout, 100).unwrap()[0].images.is_empty());
}

#[test]
fn named_image_blocks_without_prompt_text_show_their_labels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    let mut records = relocated_image_records("wrapped", &dir.path().join("missing.png"));
    // Cut the actual image triple from the upstream message, retaining its
    // recorded label and bytes; no synthetic image protocol is introduced.
    records[0]["payload"]["content"]
        .as_array_mut()
        .unwrap()
        .truncate(3);
    write_image_records(&path, &records[..1]);
    let entries = tail(&path, 100).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].body, "[Image #1]");
    assert!(entries[0].images.is_empty());
}

#[test]
fn session_prompt_images_belong_to_first_user_message() {
    use super::{CodexProvider, Provider};
    use rusqlite::Connection;
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let path = dir.path().join("rollout.jsonl");
    db.execute(
        "UPDATE threads SET rollout_path=? WHERE id='thread-0'",
        [path.to_str().unwrap()],
    )
    .unwrap();
    let mut provider = CodexProvider::new(dir.path().into(), dir.path().join("manager"));
    let image = dir.path().join("first.png");
    std::fs::write(&image, b"image").unwrap();
    for name in ["wrapped", "fallback"] {
        let first = relocated_image_records(name, &image);
        let prompt = first[0]["payload"]["content"][3]["text"].as_str().unwrap();
        db.execute(
            "UPDATE threads SET first_user_message=? WHERE id='thread-0'",
            [prompt],
        )
        .unwrap();
        let context: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-images/context.jsonl"
        ))
        .unwrap();
        let later = relocated_image_records("wrapped", &dir.path().join("missing.png"));
        write_image_records(
            &path,
            &[
                context,
                first[0].clone(),
                first[1].clone(),
                later[0].clone(),
                later[1].clone(),
            ],
        );
        let sessions = provider.refresh(1).sessions;
        let session = sessions.iter().find(|s| s.id.id == "thread-0").unwrap();
        assert_eq!(
            session.prompt_images,
            [crate::log::Image {
                label: "[Image #1]".into(),
                path: image.clone()
            }]
        );
        assert_eq!(session.prompt, prompt.chars().take(512).collect::<String>());
    }
    std::fs::remove_file(&image).unwrap();
    let session = provider
        .refresh(1)
        .sessions
        .into_iter()
        .find(|s| s.id.id == "thread-0")
        .unwrap();
    assert!(session.prompt_images.is_empty());
}

#[test]
fn first_prompt_images_stop_at_the_first_completed_user_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("attachment.png");
    std::fs::write(&image, b"image").unwrap();
    let path = dir.path().join("rollout.jsonl");
    let first = relocated_image_records("fallback", &dir.path().join("missing.png"));
    let later = relocated_image_records("wrapped", &image);
    let later_text = later[0]["payload"]["content"][3]["text"].as_str().unwrap();
    write_image_records(
        &path,
        &[
            first[0].clone(),
            first[1].clone(),
            later[0].clone(),
            later[1].clone(),
        ],
    );
    assert!(
        super::rollout::first_prompt(&path, later_text)
            .unwrap()
            .0
            .is_empty()
    );
    // The wrapper path is usable even before the following event is appended.
    write_image_records(&path, &later[..1]);
    assert_eq!(
        super::rollout::first_prompt(&path, later_text)
            .unwrap()
            .0
            .len(),
        1
    );
    // A text-only first prompt must not acquire later attachments.
    let mut plain = first[0].clone();
    plain["payload"]["content"] = Value::Array(vec![first[0]["payload"]["content"][3].clone()]);
    write_image_records(&path, &[plain, later[0].clone(), later[1].clone()]);
    let first_text = first[0]["payload"]["content"][3]["text"].as_str().unwrap();
    assert!(
        super::rollout::first_prompt(&path, first_text)
            .unwrap()
            .0
            .is_empty()
    );
}

#[test]
fn wrapper_path_precedes_event_path_and_unavailable_path_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("wrapper.png");
    let event = dir.path().join("event.png");
    for path in [&wrapper, &event] {
        std::fs::write(path, b"image").unwrap();
    }
    let path = dir.path().join("rollout.jsonl");
    let mut records = relocated_image_records("wrapped", &wrapper);
    records[1]["payload"]["item"]["content"][0]["path"] = event.to_str().unwrap().into();
    write_image_records(&path, &records);
    assert_eq!(tail(&path, 100).unwrap()[0].images[0].path, wrapper);
    std::fs::remove_file(&wrapper).unwrap();
    assert_eq!(tail(&path, 100).unwrap()[0].images[0].path, event);
    records[1]["payload"]["turn_id"] = "different-turn".into();
    write_image_records(&path, &records);
    assert!(tail(&path, 100).unwrap()[0].images.is_empty());
}

#[test]
fn shell_command_display_keeps_raw_name_command_output_and_timing() {
    let entries = events("shell");
    assert_eq!(entries.len(), 1);
    let event = &entries[0];
    let call = &event.calls[0];
    assert_eq!(event.body, "sleep");
    assert_eq!(call.name, "exec_command");
    assert_eq!(call.title, "sleep");
    assert_eq!(call.detail_title, "sleep");
    assert_eq!(call.detail_left, "Exec Command");
    assert_eq!(
        call.input,
        vec![crate::log::Section::Shell {
            text: "sleep 60".into(),
            prefix: 0,
        }]
    );
    assert_eq!(
        call.output,
        Some(vec![crate::log::Section::Text {
            label: "output".into(),
            text: String::new(),
        }])
    );
    assert_eq!(call.exit_code, None);
    assert_record_timing(event, &records("shell")[1]);
}

#[test]
fn chained_shell_subject_groups_subcommands_without_arguments() {
    let entries = events("git-chain");
    assert_eq!(entries.len(), 1);
    let event = &entries[0];
    let call = &event.calls[0];
    assert_eq!(event.body, "git {status, diff}, sed");
    assert_eq!(call.title, event.body);
    assert_eq!(call.detail_title, "git {status, diff}, sed");
    assert_eq!(call.detail_left, "Exec Command");
    assert!(event.paths.is_empty());
    assert_eq!(call.name, "exec_command");
    assert!(
        matches!(&call.input[0], crate::log::Section::Shell { text, .. } if text.contains("git status --short && git diff --name-only"))
    );
    assert_record_timing(event, &records("git-chain")[1]);
    assert_eq!(
        summary(&fixture("git-chain"))
            .unwrap()
            .activity
            .unwrap()
            .label,
        event.body
    );
}

#[test]
fn interaction_functions_win_over_completed_items() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-interaction/parent.jsonl");
    let rows = tail(&path, 100).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].body, "root → demo_sub · new task");
    assert_eq!(rows[0].calls[0].detail_left, "Spawn Agent");
    assert_eq!(
        rows[0].calls[0].input,
        vec![crate::log::Section::Markdown {
            label: "message".into(),
            text: "(encrypted)".into()
        }]
    );
    assert_eq!(rows[1].body, "Wait Agent");
    assert_eq!(
        rows[1].calls[0].input,
        vec![crate::log::Section::Prose {
            text: "70000".into(),
            markdown: false
        }]
    );
    assert_eq!(
        rows[1].calls[0].output,
        Some(vec![crate::log::Section::Text {
            label: "output".into(),
            text: "Wait completed.".into()
        }])
    );
    assert_eq!(rows[2].body, "demo_sub → root · 演示完成");
}

#[test]
fn interaction_messages_keep_route_and_merge_adjacent_sender() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-interaction/child.jsonl");
    let rows = tail(&path, 100).unwrap();
    assert_eq!(rows[0].body, "root → demo_sub · new task");
    assert_eq!(rows[0].calls[0].detail_left, "New Task");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex-interaction/messages.jsonl");
    let rows = tail(&path, 100).unwrap();
    assert!(
        rows.iter()
            .any(|row| row.body.contains("messages") && row.calls.len() > 1)
    );
    assert!(rows.iter().all(|row| !row.body.contains("encrypted")));
}

#[test]
fn interaction_message_group_survives_small_tail_page() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex-interaction/messages.jsonl");
    let all = tail(&path, 100).unwrap();
    for count in 1..all.len() {
        assert_eq!(tail(&path, count).unwrap(), all[all.len() - count..]);
    }
}

#[test]
fn protocol_titles_preserve_mixed_case_words() {
    assert_eq!(crate::providers::tool::title_name("ImageView"), "ImageView");
    assert_eq!(
        crate::providers::tool::title_name("FINAL_ANSWER"),
        "Final Answer"
    );
}

#[test]
fn child_function_message_uses_session_sender() {
    let child = include_str!("../../../tests/fixtures/codex-interaction/child.jsonl");
    let message = include_str!("../../../tests/fixtures/codex-collaboration/send_message.jsonl");
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        file.path(),
        format!("{}\n{}", child.lines().next().unwrap(), message),
    )
    .unwrap();
    let rows = tail(file.path(), 100).unwrap();
    assert_eq!(rows[0].body, "demo_sub → rename_gating");
    let mut reader = super::rollout::Reader::new(file.path().to_owned(), false);
    assert_eq!(
        reader.refresh(100).unwrap().events[0].body,
        "demo_sub → rename_gating"
    );
}

#[test]
fn child_real_command_and_extension_have_complete_details() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-interaction/child.jsonl");
    let rows = tail(&path, 100).unwrap();
    let command = rows.iter().find(|row| row.body == "sleep").unwrap();
    assert!(
        matches!(&command.calls[0].input[0], crate::log::Section::Shell { text, .. } if text == "sleep 60")
    );
    assert_eq!(command.duration.unwrap().num_seconds(), 59);
    assert!(rows.iter().any(|row| row.body == "Extension"));
}
