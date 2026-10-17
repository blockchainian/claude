use super::Client;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetNameParams<'a> {
    thread_id: &'a str,
    name: &'a str,
}

#[derive(Deserialize)]
struct SetNameResponse {}

pub(in crate::providers::codex) fn set_name(
    client: &mut Client,
    thread_id: &str,
    name: &str,
) -> io::Result<()> {
    let _: SetNameResponse =
        client.request("thread/name/set", SetNameParams { thread_id, name })?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadParams<'a> {
    thread_id: &'a str,
}

#[derive(Deserialize)]
struct ArchiveResponse {}

pub(in crate::providers::codex) fn archive(client: &mut Client, thread_id: &str) -> io::Result<()> {
    let _: ArchiveResponse = client.request("thread/archive", ThreadParams { thread_id })?;
    Ok(())
}

#[derive(Deserialize)]
struct DeleteResponse {}

/// Interrupts any running turn and unloads the thread before removing its persisted data.
pub(in crate::providers::codex) fn delete(client: &mut Client, thread_id: &str) -> io::Result<()> {
    let _: DeleteResponse = client.request("thread/delete", ThreadParams { thread_id })?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnsListParams<'a> {
    thread_id: &'a str,
    limit: u32,
    sort_direction: &'static str,
}

#[derive(Deserialize)]
struct TurnsListResponse {
    data: Vec<Turn>,
}

#[derive(Deserialize)]
struct Turn {
    id: String,
    status: String,
}

/// Returns the id of the thread's in-progress turn, if any.
pub(in crate::providers::codex) fn running_turn(
    client: &mut Client,
    thread_id: &str,
) -> io::Result<Option<String>> {
    let response: TurnsListResponse = client.request(
        "thread/turns/list",
        TurnsListParams {
            thread_id,
            limit: 1,
            sort_direction: "desc",
        },
    )?;
    Ok(response
        .data
        .into_iter()
        .find(|turn| turn.status == "inProgress")
        .map(|turn| turn.id))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InterruptParams<'a> {
    thread_id: &'a str,
    turn_id: &'a str,
}

#[derive(Deserialize)]
struct InterruptResponse {}

pub(in crate::providers::codex) fn interrupt(
    client: &mut Client,
    thread_id: &str,
    turn_id: &str,
) -> io::Result<()> {
    let _: InterruptResponse =
        client.request("turn/interrupt", InterruptParams { thread_id, turn_id })?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResumeParams<'a> {
    thread_id: &'a str,
    exclude_turns: bool,
}

#[derive(Deserialize)]
struct ResumeResponse {
    #[serde(rename = "thread")]
    _thread: serde_json::Value,
}

/// Loads an idle thread or rejoins its running turn without changing its configuration.
pub(in crate::providers::codex) fn resume(client: &mut Client, thread_id: &str) -> io::Result<()> {
    let _: ResumeResponse = client.request(
        "thread/resume",
        ResumeParams {
            thread_id,
            exclude_turns: true,
        },
    )?;
    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum UserInput<'a> {
    Text { text: &'a str },
    LocalImage { path: &'a Path },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptParams<'a> {
    thread_id: &'a str,
    input: Vec<UserInput<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_turn_id: Option<&'a str>,
}

impl<'a> PromptParams<'a> {
    fn new(
        thread_id: &'a str,
        text: &'a str,
        images: &'a [PathBuf],
        expected_turn_id: Option<&'a str>,
    ) -> Self {
        let mut input = Vec::new();
        if !text.is_empty() {
            input.push(UserInput::Text { text });
        }
        input.extend(images.iter().map(|path| UserInput::LocalImage { path }));
        Self {
            thread_id,
            input,
            expected_turn_id,
        }
    }
}

#[derive(Deserialize)]
struct StartResponse {
    #[serde(rename = "turn")]
    _turn: Turn,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SteerResponse {
    turn_id: String,
}

pub(in crate::providers::codex) fn prompt(
    client: &mut Client,
    thread_id: &str,
    text: &str,
    images: &[PathBuf],
) -> io::Result<()> {
    resume(client, thread_id)?;
    let running = running_turn(client, thread_id)?;
    let params = PromptParams::new(thread_id, text, images, running.as_deref());
    match running.as_deref() {
        Some(expected) => {
            let response: SteerResponse = client.request("turn/steer", params)?;
            if response.turn_id != expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Codex steered a different turn",
                ));
            }
        }
        None => {
            let _: StartResponse = client.request("turn/start", params)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn image_payloads_carry_local_images_for_start_and_steer() {
        let paths = vec![
            PathBuf::from("/tmp/clipboard image.png"),
            PathBuf::from("/tmp/second.png"),
        ];
        for running in [None, Some("turn-123")] {
            let params = PromptParams::new("thread-123", "describe these", &paths, running);
            let actual = serde_json::to_value(params).unwrap();
            assert_eq!(
                actual["input"],
                json!([
                    {"type":"text", "text":"describe these"},
                    {"type":"localImage", "path":"/tmp/clipboard image.png"},
                    {"type":"localImage", "path":"/tmp/second.png"}
                ])
            );
            assert_eq!(actual["threadId"], "thread-123");
            match running {
                Some(turn) => assert_eq!(actual["expectedTurnId"], turn),
                None => assert!(actual.get("expectedTurnId").is_none()),
            }
        }
    }

    #[test]
    fn image_only_payload_has_no_empty_text_input() {
        let paths = vec![PathBuf::from("/tmp/image.png")];
        let params = PromptParams::new("thread", "", &paths, None);
        assert_eq!(
            serde_json::to_value(params).unwrap()["input"],
            json!([
                {"type":"localImage", "path":"/tmp/image.png"}
            ])
        );
    }
}
