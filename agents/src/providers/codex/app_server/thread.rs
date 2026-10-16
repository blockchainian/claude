use super::Client;
use serde::{Deserialize, Serialize};
use std::io;

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptParams<'a> {
    thread_id: &'a str,
    input: [UserInput<'a>; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_turn_id: Option<&'a str>,
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
) -> io::Result<()> {
    resume(client, thread_id)?;
    let running = running_turn(client, thread_id)?;
    let params = PromptParams {
        thread_id,
        input: [UserInput::Text { text }],
        expected_turn_id: running.as_deref(),
    };
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
