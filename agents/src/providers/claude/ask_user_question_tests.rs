use super::*;
use crate::ask::QuestionAnswer;

fn ask() -> Ask {
    parse_request(
        &serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude-ask-user-question/post.json"
        ))
        .unwrap(),
    )
    .unwrap()
    .0
}

#[test]
fn answers_encode_labels_notes_and_unanswered_questions() {
    let mut ask = ask();
    let AskKind::Questions { questions, .. } = &mut ask.kind;
    let q = &mut questions[0];
    q.multi = true;
    let id = q.id.clone();
    let text = q.text.clone();
    let labels: Vec<_> = q.options.iter().map(|o| o.label.clone()).collect();
    let answer = Answer {
        rejected: false,
        answers: [(
            id,
            QuestionAnswer {
                answers: labels.clone(),
                other: Some("my own words".into()),
            },
        )]
        .into(),
    };
    assert_eq!(
        encode_answers(&ask, &answer).unwrap()[&text],
        format!("{}, my own words", labels.join(", "))
    );
    assert!(encode_answers(&ask, &Answer::default()).unwrap().is_empty());
    let AskKind::Questions { questions, .. } = &mut ask.kind;
    questions[0].multi = false;
    assert!(encode_answers(&ask, &answer).is_err());
}

#[test]
fn answer_encoding_rejects_invalid_values() {
    let mut ask = ask();
    let AskKind::Questions { questions, .. } = &ask.kind;
    let id = questions[0].id.clone();
    for value in ["missing label", "Red\u{1b}"] {
        let answer = Answer {
            rejected: false,
            answers: [(
                id.clone(),
                QuestionAnswer {
                    answers: vec![value.into()],
                    other: None,
                },
            )]
            .into(),
        };
        assert!(encode_answers(&ask, &answer).is_err());
    }
    let answer = Answer {
        rejected: false,
        answers: [(
            id.clone(),
            QuestionAnswer {
                answers: vec![],
                other: Some("pasted\nnote".into()),
            },
        )]
        .into(),
    };
    assert_eq!(
        encode_answers(&ask, &answer).unwrap()[&questions[0].text],
        "pasted\nnote"
    );
    let AskKind::Questions { questions, .. } = &mut ask.kind;
    questions[0].other = false;
    let answer = Answer {
        rejected: false,
        answers: [(
            id,
            QuestionAnswer {
                answers: vec![],
                other: Some("note".into()),
            },
        )]
        .into(),
    };
    assert!(encode_answers(&ask, &answer).is_err());
}

#[test]
fn recorded_request_preserves_questions_and_session() {
    let value: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/claude-ask-user-question/post.json"
    ))
    .unwrap();
    let (ask, questions) = parse_request(&value).unwrap();
    assert_eq!(ask.id, value["tool_use_id"]);
    assert_eq!(ask.session.id, value["session_id"]);
    assert_eq!(questions, value["tool_input"]["questions"]);
    assert!(!ask.id.is_empty());
}

fn post(server: &AskUserQuestion, body: &Value) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let body = serde_json::to_vec(body).unwrap();
    write!(
        stream,
        "POST /ask-user-question HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(&body).unwrap();
    stream
}
fn await_pending(server: &AskUserQuestion) -> Ask {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(ask) = server.pending().into_iter().next() {
            return ask;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "AskUserQuestion did not become pending"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/claude-ask-user-question/post.json"
    ))
    .unwrap()
}
#[test]
fn socket_unknown_returns_empty_background_answers_release_disconnect_and_drop() {
    let config = tempfile::tempdir().unwrap();
    let server = AskUserQuestion::bind(0, config.path().to_owned()).unwrap();
    assert!(AskUserQuestion::bind(server.port, config.path().to_owned()).is_err());
    let body = fixture();
    let mut stream = post(&server, &body);
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.ends_with("\r\n\r\n"));
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(server.pending().is_empty());
    let job = config.path().join("jobs/recorded");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(
        job.join("state.json"),
        include_str!("../../../tests/fixtures/claude-ask-user-question/background-state.json"),
    )
    .unwrap();
    let mut stream = post(&server, &body);
    let ask = await_pending(&server);
    server.answer(&ask, &Answer::default()).unwrap();
    response.clear();
    stream.read_to_string(&mut response).unwrap();
    let decoded: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        decoded["hookSpecificOutput"]["updatedInput"]["questions"],
        body["tool_input"]["questions"]
    );
    assert_eq!(
        decoded["hookSpecificOutput"]["updatedInput"]["answers"],
        json!({})
    );
    let mut stream = post(&server, &body);
    let ask = await_pending(&server);
    server.release(&ask.session.id).unwrap();
    response.clear();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.ends_with("\r\n\r\n"));
    let stream = post(&server, &body);
    await_pending(&server);
    stream.shutdown(Shutdown::Both).unwrap();
    drop(stream);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !server.pending().is_empty() {
        assert!(std::time::Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let mut stream = post(&server, &body);
    await_pending(&server);
    drop(server);
    response.clear();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.is_empty());
}

#[test]
fn provider_marks_pending_questions_and_releases_them_on_attach_or_disconnect() {
    use crate::model::SessionStatus;
    use crate::providers::Provider;
    let config = tempfile::tempdir().unwrap();
    let body = fixture();
    let id = body["session_id"].as_str().unwrap();
    let job = config.path().join("jobs/recorded");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(
        job.join("state.json"),
        include_str!("../../../tests/fixtures/claude-ask-user-question/background-state.json"),
    )
    .unwrap();
    let mut registry = super::super::registry::tests::fixture(config.path());
    registry["sessionId"] = id.into();
    registry["kind"] = "bg".into();
    registry["jobId"] = "recorded".into();
    registry["status"] = "busy".into();
    registry["cwd"] = config.path().to_string_lossy().as_ref().into();
    std::fs::write(
        config.path().join("sessions/live.json"),
        registry.to_string(),
    )
    .unwrap();
    super::super::tests::transcript(config.path(), id, 0);
    let server = Arc::new(AskUserQuestion::bind(0, config.path().to_owned()).unwrap());
    let mut provider = super::super::ClaudeProvider::new(config.path().to_owned());
    provider.ask_user_question = Some(Arc::clone(&server));
    let stream = post(&server, &body);
    let ask = await_pending(&server);
    let session = provider
        .refresh(1)
        .sessions
        .into_iter()
        .find(|s| s.id.id == id)
        .unwrap();
    assert_eq!(session.status, SessionStatus::NeedsInput);
    assert_eq!(session.ask.as_ref(), Some(&ask));
    assert_eq!(session.pending_questions, ["Hook color?"]);
    let cached = provider.snapshot_sessions(1, Some(&[]), std::slice::from_ref(&session));
    assert_eq!(cached.sessions[0].ask.as_ref(), Some(&ask));
    stream.shutdown(Shutdown::Both).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !server.pending().is_empty() {
        assert!(std::time::Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let cached = provider.snapshot_sessions(1, Some(&[]), &cached.sessions);
    assert!(cached.sessions[0].ask.is_none());
    let session = provider
        .refresh(1)
        .sessions
        .into_iter()
        .find(|s| s.id.id == id)
        .unwrap();
    assert_eq!(session.status, SessionStatus::Working);
    assert!(session.ask.is_none());
    let mut stream = post(&server, &body);
    let ask = await_pending(&server);
    let mut session = session;
    session.process = None;
    session.ask = Some(ask.clone());
    assert!(provider.terminal_cleanup(&session).unwrap().is_none());
    let terminal = provider.terminal_command(&session).unwrap();
    assert_eq!(terminal.command.get_program(), "claude");
    assert_eq!(
        terminal.command.get_args().collect::<Vec<_>>(),
        ["attach", "recorded"]
    );
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.ends_with("\r\n\r\n"));
    assert!(server.pending().is_empty());
    let input = crate::providers::SessionInput::Answer {
        ask,
        answer: Answer::default(),
        transcript: "unused-after-hook-release.jsonl".into(),
    };
    let error = provider.send(&session, input.clone()).unwrap()().unwrap_err();
    assert_eq!(
        error.to_string(),
        "Claude AskUserQuestion is no longer pending"
    );
    let without_hook = super::super::ClaudeProvider::new(config.path().to_owned());
    let error = without_hook.send(&session, input).unwrap()().unwrap_err();
    assert_eq!(
        error.to_string(),
        "Claude AskUserQuestion is no longer pending"
    );

    let mut stream = post(&server, &body);
    let ask = await_pending(&server);
    provider
        .send(
            &session,
            crate::providers::SessionInput::Answer {
                ask,
                answer: Answer::default(),
                transcript: "unused-while-hook-is-pending.jsonl".into(),
            },
        )
        .unwrap()()
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let body: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        body["hookSpecificOutput"]["updatedInput"]["answers"],
        json!({})
    );
    assert!(server.pending().is_empty());
}

#[test]
fn http_reader_waits_for_a_body_that_arrives_after_the_headers() {
    let config = tempfile::tempdir().unwrap();
    let server = AskUserQuestion::bind(0, config.path().to_owned()).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let body = serde_json::to_vec(&fixture()).unwrap();
    write!(
        stream,
        "POST /ask-user-question HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .unwrap();
    thread::sleep(Duration::from_millis(100));
    // A real HTTP client can send its body in a later TCP packet.
    let sent = stream.write_all(&body);
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "body send: {sent:?}; response: {response}"
    );
}

#[test]
fn answers_encode_single_free_text_and_partial_question_sets() {
    let mut ask = ask();
    let AskKind::Questions { questions, .. } = &mut ask.kind;
    let first = questions[0].clone();
    let mut second = first.clone();
    second.id = "Second?".into();
    second.text = "Second?".into();
    questions.push(second.clone());
    for (labels, note, expected) in [
        (
            vec![first.options[0].label.clone()],
            None,
            first.options[0].label.clone(),
        ),
        (
            vec![],
            Some("自己写的话".to_owned()),
            "自己写的话".to_owned(),
        ),
    ] {
        let answer = Answer {
            rejected: false,
            answers: [
                (
                    first.id.clone(),
                    QuestionAnswer {
                        answers: labels,
                        other: note,
                    },
                ),
                (second.id.clone(), QuestionAnswer::default()),
            ]
            .into(),
        };
        assert_eq!(
            encode_answers(&ask, &answer).unwrap(),
            [(first.text.clone(), expected)].into()
        );
    }
    assert!(
        encode_answers(
            &ask,
            &Answer {
                rejected: false,
                answers: [("unknown".into(), QuestionAnswer::default())].into()
            }
        )
        .is_err()
    );
}

#[test]
fn http_reader_rejects_wrong_routes_and_invalid_json() {
    let config = tempfile::tempdir().unwrap();
    let server = AskUserQuestion::bind(0, config.path().to_owned()).unwrap();
    for request in [
        "GET /ask-user-question HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        "POST /other HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        "POST /ask-user-question HTTP/1.1\r\nContent-Length: 1\r\n\r\nx",
    ] {
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    }
    assert!(server.pending().is_empty());
}

#[test]
fn hook_rejected_questions_resend_message_and_questions() {
    let config = tempfile::tempdir().unwrap();
    let job = config.path().join("jobs/recorded");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(
        job.join("state.json"),
        include_str!("../../../tests/fixtures/claude-ask-user-question/background-state.json"),
    )
    .unwrap();
    let server = AskUserQuestion::bind(0, config.path().to_owned()).unwrap();
    let mut body = fixture();
    let recorded: Vec<Value> =
        include_str!("../../../tests/fixtures/claude-questions-hook-rejected.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    let first = recorded[0]["message"]["content"][0]["input"]["questions"][0].clone();
    body["tool_input"]["questions"] = json!([first]);
    let mut stream = post(&server, &body);
    let ask = await_pending(&server);
    server
        .answer(
            &ask,
            &Answer {
                rejected: true,
                ..Default::default()
            },
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let decoded: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    let reason = "The user dismissed these questions because the message you sent with them was not shown.\nResend that message verbatim, followed by the questions and their options as plain text, then end your turn. Ask nothing through AskUserQuestion until the user replies.";
    assert_eq!(
        decoded,
        json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}})
    );
    assert!(server.pending().is_empty());

    body["tool_input"]["questions"][0]["multiSelect"] = true.into();
    let mut second = body["tool_input"]["questions"][0].clone();
    second["question"] = "Second?".into();
    body["tool_input"]["questions"]
        .as_array_mut()
        .unwrap()
        .push(second);
    let mut stream = post(&server, &body);
    let ask = await_pending(&server);
    server
        .answer(
            &ask,
            &Answer {
                rejected: true,
                answers: [(
                    "Hook color?".into(),
                    QuestionAnswer {
                        answers: vec!["Red".into(), "Blue".into()],
                        other: Some("my words".into()),
                    },
                )]
                .into(),
            },
        )
        .unwrap();
    response.clear();
    stream.read_to_string(&mut response).unwrap();
    let decoded: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        decoded["hookSpecificOutput"]["permissionDecisionReason"],
        reason
    );
    let reason = decoded["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    for result in [
        format!("PreToolUse:AskUserQuestion hook error: {reason}"),
        format!("Error: PreToolUse:AskUserQuestion hook error: {reason}"),
        format!("The user doesn't want to proceed with this tool use. the user said:\n{reason}"),
    ] {
        assert_eq!(
            crate::providers::tool::rejected_answers(&body["tool_input"]["questions"], &result)
                .as_deref(),
            Some("Chat about this")
        );
    }
}
