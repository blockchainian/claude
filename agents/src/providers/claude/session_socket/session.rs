use super::Client;
use serde::Serialize;
use std::io;

#[derive(Serialize)]
struct Rename<'a> {
    r#type: &'static str,
    action: &'static str,
    name: &'a str,
}

/// Writes the rename frame; Claude publishes the applied name asynchronously.
pub(in crate::providers::claude) fn set_name(client: &mut Client, name: &str) -> io::Result<()> {
    if name.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Claude session name must not be empty",
        ));
    }
    client.send(Rename {
        r#type: "control",
        action: "rename",
        name,
    })
}

// Socket prompts are peer messages; terminal input deliberately uses attach instead.
#[allow(dead_code)]
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(in crate::providers::claude) enum Priority {
    Now,
    Next,
    Later,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct Prompt<'a> {
    r#type: &'static str,
    message: Message<'a>,
    priority: Priority,
}

#[allow(dead_code)]
pub(in crate::providers::claude) fn prompt(
    client: &mut Client,
    text: &str,
    priority: Priority,
) -> io::Result<()> {
    if text.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Claude prompt must not be empty",
        ));
    }
    client.send(Prompt {
        r#type: "user",
        message: Message {
            role: "user",
            content: text,
        },
        priority,
    })
}
