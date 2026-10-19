use super::*;

fn window_key(tmux: &Tmux, direction: &str) {
    tmux.run(&["send-keys", "-t", "probe", "C-w"]);
    tmux.run(&["send-keys", "-t", "probe", direction]);
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_vim_window_navigation_uses_rendered_panels() {
    let mut captures = Vec::new();
    for width in [200, 100] {
        let (_directory, tmux, _transcript) = start_probe(&format!("window-navigation-{width}"));
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        tmux.wait_for(|capture| {
            capture
                .lines()
                .nth(2)
                .is_some_and(|line| line.matches('│').count() == if width == 200 { 2 } else { 1 })
        });
        // Start in the task list, using the same mouse event as an actual click.
        tmux.mouse(0, 2, 2, true);
        window_key(&tmux, "l");
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        // Details ignores slash, so the following question mark opens help.
        // In the task list it would be inserted into the search editor instead.
        tmux.run(&["send-keys", "-t", "probe", "?"]);
        let details = tmux.capture_when(|capture| capture.contains("Task shortcuts"));
        assert!(!details.contains("Search ❯"), "details focus: {details}");
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| !capture.contains("Task shortcuts"));

        window_key(&tmux, "j");
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        let prompt = tmux.capture_when(|capture| capture.contains("❯ /"));
        assert!(!prompt.contains("Search ❯"), "prompt focus: {prompt}");
        tmux.run(&["send-keys", "-t", "probe", "-l", "alpha beta"]);
        tmux.wait_for(|capture| capture.contains("❯ /alpha beta"));
        window_key(&tmux, "x");
        tmux.wait_for(|capture| capture.contains("❯ /alpha x"));
        tmux.run(&["send-keys", "-t", "probe", "C-u"]);
        tmux.wait_for(|capture| !capture.contains("❯ /"));
        if width == 200 {
            window_key(&tmux, "k");
            window_key(&tmux, "l");
        } else {
            window_key(&tmux, "j");
        }
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "Stop"]);
        let list = tmux.capture_when(|capture| capture.contains("Search ❯ Stop"));
        let list_search_row = list
            .lines()
            .position(|line| line.contains("Search ❯ Stop"))
            .unwrap();
        // A non-direction key replays Ctrl-W's word deletion in the active editor.
        window_key(&tmux, "x");
        tmux.wait_for(|capture| capture.contains("Search ❯ x"));

        window_key(&tmux, "j");
        if width == 200 {
            // The full-width prompt lies between the top log list and log details.
            window_key(&tmux, "j");
        }
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "Stop"]);
        let detail = tmux.capture_when(|capture| {
            capture
                .lines()
                .enumerate()
                .any(|(row, line)| row > list_search_row && line.contains("Search ❯ Stop"))
        });
        let path = std::env::temp_dir().join(format!("agents-window-navigation-{width}.txt"));
        std::fs::write(&path, detail).unwrap();
        captures.push(path);
    }
    let verdict = serde_json::json!({
        "verdict": "pass",
        "widths": [200, 100],
        "task_list_to_details": true,
        "details_to_prompt": true,
        "log_list_to_detail": true,
        "non_direction_preserves_word_delete": true,
        "prompt_word_delete": true,
        "captures": captures,
    });
    let path = std::env::temp_dir().join("agents-window-navigation-verdict.json");
    std::fs::write(&path, format!("{verdict}\n")).unwrap();
    println!("{verdict}");
}
