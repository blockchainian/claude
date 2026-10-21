use super::Client;
use serde::Deserialize;
use std::io;

#[derive(Deserialize)]
struct ArchiveResponse {
    session_status: String,
}

pub(in crate::providers::claude) fn archive(
    client: &mut Client,
    bridge_id: &str,
) -> io::Result<()> {
    if !bridge_id.starts_with("session_")
        || !bridge_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid Claude bridge session id",
        ));
    }
    let (status, body) = client.request(&format!("sessions/{bridge_id}/archive"))?;
    match status {
        409 => Ok(()),
        200 => {
            let response: ArchiveResponse = serde_json::from_slice(&body)?;
            if response.session_status != "archived" {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Claude session was not archived",
                ));
            }
            Ok(())
        }
        _ => Err(io::Error::other(format!(
            "Claude archive failed (HTTP {status})"
        ))),
    }
}
