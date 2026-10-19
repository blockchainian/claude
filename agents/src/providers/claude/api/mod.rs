pub(super) mod session;

use serde::Deserialize;
use std::{
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};

pub(super) struct Client {
    access_token: String,
    organization_uuid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    claude_ai_oauth: Oauth,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Oauth {
    access_token: String,
}

impl Client {
    pub(super) fn connect(config_dir: &Path) -> io::Result<Self> {
        let account = super::account::read(config_dir)?;
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-w",
            ])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                "Cannot read Claude Code credentials from Keychain",
            ));
        }
        let credentials: Credentials = serde_json::from_slice(&output.stdout)?;
        for value in [
            &credentials.claude_ai_oauth.access_token,
            &account.organization_uuid,
        ] {
            if value.is_empty() || value.chars().any(char::is_control) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid Claude API credentials",
                ));
            }
        }
        Ok(Self {
            access_token: credentials.claude_ai_oauth.access_token,
            organization_uuid: account.organization_uuid,
        })
    }

    pub(super) fn request(&mut self, path: &str) -> io::Result<(u16, Vec<u8>)> {
        let quote = |value: &str| value.replace('\\', "\\\\").replace('"', "\\\"");
        // Both the ccr-byoc beta and organization header are required; omitting either returns 404.
        let config = format!(
            "header = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\nheader = \"anthropic-version: 2023-06-01\"\nheader = \"anthropic-beta: ccr-byoc-2025-07-29\"\nheader = \"x-organization-uuid: {}\"\n",
            quote(&self.access_token),
            quote(&self.organization_uuid)
        );
        let mut child = Command::new("curl")
            .args([
                "--disable",
                "--config",
                "-",
                "--silent",
                "--show-error",
                "--connect-timeout",
                "10",
                "--max-time",
                "30",
                "--request",
                "POST",
                "--data",
                "{}",
                "--write-out",
                "\n%{http_code}",
                "--url",
                &format!("https://api.anthropic.com/v1/{path}"),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let write = child
            .stdin
            .take()
            .expect("piped curl stdin")
            .write_all(config.as_bytes());
        let output = child.wait_with_output()?;
        write?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Claude API request failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let separator = output
            .stdout
            .iter()
            .rposition(|byte| *byte == b'\n')
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Missing Claude HTTP status")
            })?;
        let status = std::str::from_utf8(&output.stdout[separator + 1..])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
            .parse::<u16>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if status == 401 {
            return Err(io::Error::other(
                "Claude sign-in expired; open claude to refresh it",
            ));
        }
        Ok((status, output.stdout[..separator].to_vec()))
    }
}
