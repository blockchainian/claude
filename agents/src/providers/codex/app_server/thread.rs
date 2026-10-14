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
