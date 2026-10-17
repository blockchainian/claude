use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_bash_interpreter_script_colors() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("interpreter-script");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap();
    let fixture = include_str!("../fixtures/claude-inline-script.jsonl");
    let event: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let command = event["message"]["content"][0]["input"]["command"]
        .as_str()
        .unwrap();
    // Exact Bash event from a Claude transcript; commands are displayed, never executed.
    write!(file, "{fixture}").unwrap();
    file.flush().unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.wait_for(|capture| capture.contains("python3") && capture.contains("enumerate(lines)"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let path = std::env::temp_dir().join("agents-interpreter-script-200x50.ansi");
    std::fs::write(&path, &colored).unwrap();
    let parser = captured_screen(&colored, 200, 50);
    let screen = parser.screen();
    let cell = |needle: &str| {
        let (row, column) = capture
            .lines()
            .enumerate()
            .find_map(|(row, line)| line.find(needle).map(|byte| (row, line[..byte].width())))
            .unwrap_or_else(|| panic!("missing {needle:?}: {capture}"));
        screen.cell(row as u16, column as u16).unwrap()
    };
    let keyword = cell("for i,l").fgcolor();
    let variable = cell("lines=open").fgcolor();
    let string = cell("tests/ui_snapshots").fgcolor();
    let builtin = cell("enumerate(lines)");
    let shell_command = cell("cd ").fgcolor();
    let shell_path = cell(command.split_whitespace().nth(1).unwrap()).fgcolor();
    // The bundled theme leaves Python control keywords at the default foreground.
    assert_eq!(keyword, variable, "Python keyword style changed: {capture}");
    assert!(builtin.italic(), "Python builtin style missing: {capture}");
    assert_ne!(string, variable, "Python string stayed plain: {capture}");
    assert_ne!(keyword, string, "Python syntax colors collapsed: {capture}");
    assert_ne!(
        shell_command, shell_path,
        "shell syntax colors lost: {capture}"
    );
    println!(
        "{{\"interpreter_script_colors\":\"pass\",\"capture\":\"{}\",\"keyword\":\"{keyword:?}\",\"string\":\"{string:?}\",\"shell_command\":\"{shell_command:?}\"}}",
        path.display()
    );
}
