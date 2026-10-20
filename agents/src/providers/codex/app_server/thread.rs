use super::Client;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
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
struct UnarchiveResponse {
    #[serde(rename = "thread")]
    _thread: serde_json::Value,
}

pub(in crate::providers::codex) fn unarchive(
    client: &mut Client,
    thread_id: &str,
) -> io::Result<()> {
    let _: UnarchiveResponse = client.request("thread/unarchive", ThreadParams { thread_id })?;
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

/// Interrupts the running turn and waits until the daemon reports it has ended.
pub(in crate::providers::codex) fn stop(client: &mut Client, thread_id: &str) -> io::Result<()> {
    let Some(turn_id) = running_turn(client, thread_id)? else {
        return Ok(());
    };
    interrupt(client, thread_id, &turn_id)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while running_turn(client, thread_id)?.is_some() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Codex turn did not stop before detach",
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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

/// Rejoins a daemon-owned question and waits for its replay on this connection.
pub(in crate::providers::codex) fn question(
    client: &mut Client,
    thread_id: &str,
    call_id: &str,
) -> io::Result<Option<super::ServerRequest>> {
    resume(client, thread_id)?;
    client.wait_question(thread_id, call_id, Duration::from_secs(2))
}

pub(in crate::providers::codex) fn answer(
    client: &mut Client,
    thread_id: &str,
    ask: &crate::ask::Ask,
    answer: &crate::ask::Answer,
    transcript: &Path,
) -> io::Result<()> {
    let result = super::super::questions::encode(ask, answer)?;
    let request = question(client, thread_id, &ask.id)?
        .ok_or_else(|| io::Error::other("Codex question is no longer pending"))?;
    let offset = std::fs::metadata(transcript)?.len();
    client.respond(&request.id, &result)?;
    super::super::questions::confirm(transcript, &ask.id, &result, offset)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsParams<'a> {
    thread_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<&'a str>,
}

#[derive(Deserialize)]
struct SettingsResponse {}

/// Loads the thread and updates its settings without starting a turn.
pub(in crate::providers::codex) fn set_settings(
    client: &mut Client,
    thread_id: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> io::Result<()> {
    resume(client, thread_id)?;
    let _: SettingsResponse = client.request(
        "thread/settings/update",
        SettingsParams {
            thread_id,
            model,
            effort,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_payload_changes_only_the_selected_field() {
        for (model, effort, expected) in [
            (
                Some("gpt-6.1-sol"),
                None,
                json!({"threadId":"thread", "model":"gpt-6.1-sol"}),
            ),
            (
                None,
                Some("high"),
                json!({"threadId":"thread", "effort":"high"}),
            ),
        ] {
            assert_eq!(
                serde_json::to_value(SettingsParams {
                    thread_id: "thread",
                    model,
                    effort
                })
                .unwrap(),
                expected
            );
        }
    }
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundParams<'a> {
    thread_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
}

pub(in crate::providers::codex) fn background_terminals(
    client: &mut Client,
    thread_id: &str,
) -> io::Result<Vec<super::super::background::Terminal>> {
    let mut terminals = Vec::new();
    let mut cursor = None;
    loop {
        let response: super::super::background::Terminals = client.request(
            "thread/backgroundTerminals/list",
            BackgroundParams { thread_id, cursor },
        )?;
        terminals.extend(response.data);
        cursor = response.next_cursor;
        if cursor.is_none() {
            return Ok(terminals);
        }
    }
}
