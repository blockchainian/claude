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
