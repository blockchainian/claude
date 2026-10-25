/// Extracts message bodies from codex-manager await events.
pub fn message(text: &str) -> Option<String> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    let mut messages = Vec::new();
    let mut saw_event = false;
    while let Some(line) = lines.next() {
        if lines.peek().is_none() && is_exit_status(line) {
            break;
        }
        let event: serde_json::Value = serde_json::from_str(line).ok()?;
        let event = event.as_object()?;
        let kind = event.get("kind")?.as_str()?;
        let body = match kind {
            "completed" => event.get("lastMessage")?.as_str()?,
            "ask" | "approval" | "notify" => event.get("text")?.as_str()?,
            _ => return None,
        };
        if !body.is_empty() {
            messages.push(body.to_owned());
        }
        if kind == "completed"
            && let Some(error) = event.get("error")
        {
            let error = error.as_str()?;
            if !error.is_empty() {
                messages.push(error.to_owned());
            }
        }
        saw_event = true;
    }
    saw_event.then(|| messages.join("\n\n"))
}

fn is_exit_status(line: &str) -> bool {
    let Some(code) = line
        .strip_prefix("[exited with code ")
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return false;
    };
    let digits = code.strip_prefix('-').unwrap_or(code);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Formats await event bodies as Markdown, retaining other output as plain text.
pub fn section(text: String) -> crate::event::Section {
    match message(&text) {
        Some(text) => crate::event::Section::Markdown {
            label: String::new(),
            text,
        },
        None => crate::event::Section::Text {
            label: String::new(),
            text,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::message;

    const COMPLETED: &str =
        include_str!("../../../../tests/fixtures/background/codex-await.output");

    #[test]
    fn completed_preserves_message_markdown() {
        let event: serde_json::Value =
            serde_json::from_str(COMPLETED.lines().next().unwrap()).unwrap();
        let expected = event["lastMessage"].as_str().unwrap();
        assert!(expected.contains("`agents-panel`"));
        assert!(expected.contains("\n\n- Space / Enter"));
        assert_eq!(message(COMPLETED).as_deref(), Some(expected));
    }

    #[test]
    fn section_uses_markdown_only_for_await_messages() {
        let text = "{\"kind\":\"completed\",\"lastMessage\":\"**done**\\n\"}";
        assert_eq!(
            super::section(text.to_owned()),
            crate::event::Section::Markdown {
                label: String::new(),
                text: "**done**\n".to_owned(),
            }
        );
        let ordinary = "{\"text\":\"ordinary JSON\"}";
        assert_eq!(
            super::section(ordinary.to_owned()),
            crate::event::Section::Text {
                label: String::new(),
                text: ordinary.to_owned(),
            }
        );
    }

    #[test]
    fn event_messages_keep_their_order_and_completion_error() {
        let output = concat!(
            "{\"kind\":\"ask\",\"text\":\"Which **branch**?\"}\n",
            "\n{\"kind\":\"approval\",\"text\":\"Run `cargo test`\"}\n",
            "{\"kind\":\"notify\",\"text\":\"Tests started\"}\n",
            "{\"kind\":\"completed\",\"lastMessage\":\"## Result\",\"error\":\"Process failed\"}\n",
            "[exited with code -1]\n\n"
        );
        assert_eq!(
            message(output).as_deref(),
            Some(
                "Which **branch**?\n\nRun `cargo test`\n\nTests started\n\n## Result\n\nProcess failed"
            )
        );
    }

    #[test]
    fn rejects_non_await_json_and_incomplete_event_bodies() {
        for output in [
            "{\"text\":\"ordinary JSON\"}",
            "[]",
            "null",
            "{\"kind\":4,\"text\":\"wrong kind type\"}",
            "{\"kind\":\"unknown\",\"text\":\"unrecognized event\"}",
            "{\"kind\":\"completed\",\"lastMessage\":4}",
            "{\"kind\":\"completed\"}",
            "{\"kind\":\"completed\",\"lastMessage\":\"done\",\"error\":null}",
            "{\"kind\":\"ask\"}",
            "{\"kind\":\"notify\",\"text\":[]}",
            "{\"kind\":\"approval\",\"text\":true}",
            "{\"kind\":\"notify\",\"text\":\"hello\"}\nplain output",
        ] {
            assert_eq!(message(output), None, "{output}");
        }
    }

    #[test]
    fn accepts_only_an_integer_exit_code_on_the_last_nonempty_line() {
        let event = "{\"kind\":\"notify\",\"text\":\"hello\"}";
        for suffix in ["[exited with code 0]", "[exited with code -15]"] {
            assert_eq!(
                message(&format!("{event}\n{suffix}\n")).as_deref(),
                Some("hello")
            );
        }
        for output in [
            String::new(),
            " \n\n".to_owned(),
            "[exited with code 0]".to_owned(),
            format!("[exited with code 0]\n{event}"),
            format!("{event}\n[exited with code 0]\n[exited with code 0]"),
            format!("{event}\n[exited with code 1.0]"),
            format!("{event}\n[exited with code +1]"),
            format!("{event}\n[exited with code ]"),
            format!("{event}\n[exited with code --1]"),
            format!("{event}\n[exited with code 0] extra"),
        ] {
            assert_eq!(message(&output), None, "{output}");
        }
    }

    #[test]
    fn empty_completion_can_still_display_its_error() {
        assert_eq!(
            message("{\"kind\":\"completed\",\"lastMessage\":\"\",\"error\":\"Failed\"}")
                .as_deref(),
            Some("Failed")
        );
        assert_eq!(
            message("{\"kind\":\"completed\",\"lastMessage\":\"\"}").as_deref(),
            Some("")
        );
    }
}
