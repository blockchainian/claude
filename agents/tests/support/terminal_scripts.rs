use super::*;

#[test]
fn terminal_write_file_colors() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("write-file");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap();
    file.write_all(include_bytes!("../fixtures/claude-write/python.jsonl"))
        .unwrap();
    file.flush().unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.wait_for(|capture| capture.contains("import json, sys") && capture.contains("\"user\""));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 50);
    let color = |needle: &str| {
        let (row, column) = capture
            .lines()
            .enumerate()
            .find_map(|(row, line)| line.find(needle).map(|byte| (row, line[..byte].width())))
            .unwrap_or_else(|| panic!("missing {needle:?}: {capture}"));
        parser
            .screen()
            .cell(row as u16, column as u16)
            .unwrap()
            .fgcolor()
    };
    assert_ne!(color("\"user\""), color("import json"), "{capture}");
    let path = std::env::temp_dir().join("agents-write-file-200x50.ansi");
    std::fs::write(&path, colored).unwrap();
    println!(
        "{{\"write_file_colors\":\"pass\",\"capture\":\"{}\"}}",
        path.display()
    );
}
