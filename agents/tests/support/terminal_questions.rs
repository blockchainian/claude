use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_ask_questions_render_real_rejection_and_answer() {
    let (_directory, tmux, transcript) = start_probe("ask-questions");
    tmux.run(&["resize-window", "-t", "probe", "-x", "220", "-y", "70"]);
    for (name, fixture, header, output) in [
        (
            "rejected",
            include_str!("../fixtures/claude-questions-rejected.jsonl"),
            "Error AskUserQuestion · 压缩摘要",
            "Questions asked:",
        ),
        (
            "answered",
            include_str!("../fixtures/claude-questions-answered.jsonl"),
            "Assistant AskUserQuestion · 子命令 · cd",
            "Your questions have been answered:",
        ),
    ] {
        std::fs::write(&transcript, fixture).unwrap();
        tmux.run(&["send-keys", "-t", "probe", "End"]);
        std::thread::sleep(Duration::from_secs(2));
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let path = std::env::temp_dir().join(format!("agents-ask-question-{name}.txt"));
        std::fs::write(&path, &capture).unwrap();
        assert!(capture.contains(header), "{capture}");
        assert!(capture.contains("• "), "{capture}");
        assert!(capture.contains("(Recommended):"), "{capture}");
        assert!(!capture.contains("questions: [{"), "{capture}");
        let output_at = capture.find(output).expect(&capture);
        if name == "rejected" {
            assert!(
                output_at
                    < capture
                        .find("The user doesn't want to proceed")
                        .expect(&capture)
            );
            assert!(capture.contains("(No answer provided)"), "{capture}");
        }
        println!(
            "{{\"case\":\"{name}\",\"verdict\":\"pass\",\"capture\":\"{}\"}}",
            path.display()
        );
    }
}
