//! Opt-in delivery through the dashboard to a real running Claude Agent.
use super::*;

fn subagent_transcript(parent: &Path, description: &str) -> io::Result<Option<PathBuf>> {
    let directory = parent.with_extension("").join("subagents");
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut found = None;
    for entry in entries {
        let path = entry?.path();
        if !path.to_string_lossy().ends_with(".meta.json") {
            continue;
        }
        let metadata: Value = serde_json::from_slice(&fs::read(&path)?)?;
        if metadata["description"] != description {
            continue;
        }
        assert!(found.is_none(), "duplicate live-test Agent description");
        let transcript = path.with_extension("").with_extension("jsonl");
        if transcript.is_file() {
            found = Some(transcript);
        }
    }
    Ok(found)
}

fn wait_subagent(parent: &Path, description: &str) -> io::Result<PathBuf> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(path) = subagent_transcript(parent, description)? {
            return Ok(path);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "Claude did not start the requested background Agent",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn wait_subagent_queue(path: &Path, message: &str) -> io::Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let text = fs::read_to_string(path)?;
        // The writer can have an unfinished final record while its turn is running.
        let complete = text.rfind('\n').map_or("", |end| &text[..=end]);
        for line in complete.lines() {
            let event: Value = serde_json::from_str(line)?;
            if event["type"] == "attachment"
                && event["attachment"]["type"] == "queued_command"
                && event["attachment"]["prompt"] == message
                && event["attachment"]["origin"]["kind"] == "human"
            {
                return Ok(event);
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "subagent transcript never recorded the exact human queued_command",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "starts a real billable Claude background Agent and restores global configuration"]
fn real_dashboard_prompt_reaches_running_subagent_and_restores_main() {
    let mut live = Live::new().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> io::Result<()> {
        let job = live.prepare()?;
        let parent_id = live.status(&job, "idle")?["sessionId"]
            .as_str()
            .ok_or_else(|| io::Error::other("registry lacks sessionId"))?
            .to_owned();
        live.wait_prompt(
            &parent_id,
            "Reply exactly READY and stop. Do not use tools or edit files.",
        )?;
        eprintln!("parent transcript ready: {job}");
        let mut app = dashboard::app(
            Box::new(ClaudeProvider::new(live.home.clone())),
            live.repo.path().to_owned(),
        );
        let parent = dashboard::session(&app, &parent_id);
        let parent_transcript = parent
            .transcript
            .as_ref()
            .ok_or_else(|| io::Error::other("parent has no transcript"))?;
        let description = "agents-input-looper";
        let request = format!(
            "Spawn exactly one background subagent with the Agent tool (subagent_type general-purpose, run_in_background true, description '{description}'). Its prompt: 'This is an explicitly authorized input delivery test. Call the Bash tool with command \"seq 1 3000000 | wc -l\" (no sleep, no background), then call it again, 300 times, one call per turn. Never stop early and never call SubagentHandback before 300 calls. If you receive a user message, answer it in one line and keep going.' After spawning it, end your turn without waiting."
        );
        dashboard::send(&mut app, parent.id.clone(), SessionInput::Prompt(request));
        eprintln!("spawn request accepted");
        let child_transcript = wait_subagent(parent_transcript, description)?;
        eprintln!("Agent transcript exists");
        let child_id = child_transcript.file_stem().unwrap().to_str().unwrap();
        dashboard::refresh(&mut app);
        let child = dashboard::session(&app, child_id);
        assert_eq!(child.parent.as_ref(), Some(&parent.id));
        assert_eq!(child.title, description);
        assert!(
            child.input_disabled_reason().is_none(),
            "running background Agent input is disabled"
        );

        let before = attach::inspect(&job, live.repo.path())?;
        assert!(
            before.lines().any(|line| line.trim() == "⏺ main"),
            "initial view must be main: {before}"
        );
        let message =
            "agents-subagent-message-✓\nReply RECEIVED and keep doing the counting calls.";
        dashboard::send(
            &mut app,
            child.id.clone(),
            SessionInput::Prompt(message.to_owned()),
        );
        let queued = wait_subagent_queue(&child_transcript, message)?;
        let screen = attach::inspect(&job, live.repo.path())?;
        assert!(
            screen.lines().any(|line| line.trim() == "⏺ main"),
            "send did not restore main: {screen}"
        );
        assert!(
            !screen.contains("Enter to view") && !screen.contains("↑/↓ to select"),
            "send left focus in the Agent list: {screen}"
        );
        assert!(
            !live.transcript(&parent_id)?.iter().any(|event| {
                event["attachment"]["type"] == "queued_command"
                    && event["attachment"]["prompt"] == message
            }),
            "subagent message reached its parent"
        );

        // Attach clients retain a shared viewed Agent. A parent message must leave
        // that view, reach the parent, then restore the same Agent with input focus.
        attach::select_view(&job, live.repo.path(), Some(description))?;
        let parent_message =
            "Reply exactly agents-parent-route-OK. Keep the background Agent running.";
        dashboard::refresh(&mut app);
        dashboard::send(
            &mut app,
            parent.id.clone(),
            SessionInput::Prompt(parent_message.into()),
        );
        live.wait_prompt(&parent_id, parent_message)?;
        let restored_child = attach::inspect(&job, live.repo.path())?;
        assert!(
            restored_child.lines().any(|line| {
                line.trim_start().starts_with("⏺ ") && line.trim_end().ends_with(description)
            }),
            "parent send did not restore the original Agent: {restored_child}"
        );
        assert!(
            !restored_child.contains("↑/↓ to select") && !restored_child.contains("Enter to view")
        );
        let child_records = fs::read_to_string(&child_transcript)?;
        for line in child_records.lines().filter(|line| line.ends_with('}')) {
            let record: Value = serde_json::from_str(line)?;
            assert_ne!(record["attachment"]["prompt"], parent_message);
        }
        attach::select_view(&job, live.repo.path(), None)?;

        let directory = std::env::temp_dir().join("agents-stage3-live");
        fs::create_dir_all(&directory)?;
        fs::write(
            directory.join("subagent-queued-command.json"),
            serde_json::to_vec_pretty(&queued)?,
        )?;
        fs::write(directory.join("subagent-restored-main.screen.txt"), screen)?;
        eprintln!(
            "exact human queued_command reached the running Agent; main view and input focus restored"
        );
        Ok(())
    }));
    let cleanup = live.cleanup();
    assert!(cleanup.is_ok(), "cleanup failed: {cleanup:?}");
    match result {
        Ok(result) => result.unwrap(),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
