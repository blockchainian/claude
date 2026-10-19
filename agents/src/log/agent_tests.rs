use std::path::Path;

#[test]
fn claude_agent_exchange_uses_perspective_titles() {
    let events = crate::providers::claude::log::tail(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-agent-names.jsonl"),
        100,
    )
    .unwrap();
    let spawn = events
        .iter()
        .flat_map(|e| &e.calls)
        .find(|c| c.name == "Agent")
        .unwrap();
    assert_eq!(spawn.title, "reviewer · Spawn agent");
    assert_eq!(spawn.detail_left, "Spawn agent");
}

#[test]
fn claude_peer_exchange_preserves_text_and_known_fields() {
    let events = crate::providers::claude::log::tail(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-peer-prompts.jsonl"),
        100,
    )
    .unwrap();
    for event in events {
        let call = &event.calls[0];
        let message = call.agent_message.as_ref().unwrap();
        assert_eq!(message.author, event.sender);
        assert_eq!(message.recipient, None);
        assert_eq!(message.content.text(), event.body);
        assert_eq!(message.direction, super::agent::Direction::Incoming);
        assert_eq!(call.detail_left, "Message");
        assert_eq!(call.detail_title, event.sender.as_deref().unwrap());
        assert!(event.summary().ends_with(" → message"));
    }
}

#[test]
fn claude_peer_exchange_merges_live_deliveries_and_keeps_boundaries() {
    use std::io::Write;
    let record = include_str!("../../tests/fixtures/claude-peer-prompts.jsonl")
        .lines()
        .next()
        .unwrap();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{record}").unwrap();
    let mut reader = crate::providers::claude::log::Reader::new(file.path().to_owned(), false);
    assert_eq!(reader.refresh(10).unwrap().events.len(), 1);
    // Two deliveries of the same real peer payload retain their own file positions.
    writeln!(file, "{record}").unwrap();
    let update = reader.refresh(10).unwrap();
    assert_eq!(update.events.len(), 1);
    let event = &update.events[0];
    assert_eq!(event.summary(), "translate-pdf → 2 messages");
    assert_eq!(event.calls.len(), 2);
    assert_ne!(
        event.source_records[0].offset,
        event.source_records[1].offset
    );
    assert_eq!(event.calls[0].agent_message, event.calls[1].agent_message);
    assert_eq!(
        crate::providers::claude::log::tail(file.path(), 10).unwrap(),
        update.events
    );
    assert_eq!(reader.refresh(10).unwrap().events, update.events);
}

#[test]
fn claude_outgoing_exchange_has_no_invented_sender_or_recipient() {
    for line in include_str!("../../tests/fixtures/claude-tool-details.jsonl").lines() {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        if record["message"]["content"][0]["name"] != "SendMessage" {
            continue;
        }
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), line).unwrap();
        let events = crate::providers::claude::log::tail_session(file.path(), 10, true).unwrap();
        let call = &events[0].calls[0];
        let message = call.agent_message.as_ref().unwrap();
        assert_eq!(message.author, None);
        let input = &record["message"]["content"][0]["input"];
        assert_eq!(message.recipient.as_deref(), input["to"].as_str());
        assert_eq!(message.content.text(), input["message"].as_str().unwrap());
        assert_eq!(call.detail_left, "Message");
        assert_eq!(call.detail_title, input["to"].as_str().unwrap());
        assert_eq!(
            events[0].summary(),
            format!("{} ← message", input["to"].as_str().unwrap())
        );
    }
}

#[test]
fn ordinary_claude_prompts_are_not_agent_messages() {
    let events = crate::providers::claude::log::tail_session(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl"),
        100,
        true,
    ).unwrap();
    let users: Vec<_> = events.iter().filter(|event| event.kind == super::Kind::User).collect();
    assert!(!users.is_empty());
    assert!(users.iter().all(|event| event.calls.is_empty()));
}
