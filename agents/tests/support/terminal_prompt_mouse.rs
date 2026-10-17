use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_prompt_mouse_click_places_caret() {
    let (_directory, tmux, _transcript) = start_probe("prompt-mouse");
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    tmux.wait_for(|capture| capture.contains("New task ❯"));
    tmux.run(&[
        "send-keys",
        "-t",
        "probe",
        "-l",
        "\x1b[200~写工具详情计划\nNo reply needed.\x1b[201~",
    ]);
    tmux.wait_for(|capture| {
        capture.contains("写工具详情计划") && capture.contains("No reply needed.")
    });
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("New task ❯"))
        .unwrap();
    let x = line[..line.find("写工具详情计划").unwrap()].width();
    tmux.mouse(0, x + 3, row, true);
    tmux.run(&["send-keys", "-t", "probe", "-l", "查"]);
    tmux.wait_for(|capture| capture.contains("写查工具详情计划"));
    tmux.mouse(0, x + 3, row + 1, true);
    tmux.run(&["send-keys", "-t", "probe", "-l", "x"]);
    tmux.wait_for(|capture| capture.contains("No xreply needed."));
    tmux.run(&["send-keys", "-t", "probe", "C-c"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "写工具详情计划"]);
    tmux.wait_for(|capture| capture.contains("写工具详情计划"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let row = capture
        .lines()
        .position(|line| line.contains("New task ❯"))
        .unwrap();
    tmux.mouse(0, x + 2, row, false);
    tmux.mouse(32, x + 6, row, false);
    let highlighted = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&highlighted, 200, 50);
    for column in [x + 2, x + 4] {
        assert!(
            parser
                .screen()
                .cell(row as u16, column as u16)
                .unwrap()
                .inverse(),
            "{highlighted}"
        );
    }
    tmux.run(&[
        "send-keys",
        "-t",
        "probe",
        "-l",
        &format!("\x1b[<0;{};{}m", x + 7, row + 1),
    ]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "查"]);
    tmux.wait_for(|capture| capture.contains("写查详情计划"));
    tmux.run(&["send-keys", "-t", "probe", "C-c"]);
    tmux.run(&[
        "send-keys",
        "-t",
        "probe",
        "-l",
        &format!(
            "\x1b[200~{}\x1b[201~",
            include_str!("../fixtures/claude-peer-message.txt")
        ),
    ]);
    tmux.wait_for(|capture| capture.contains("No reply needed."));
    let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let row = before
        .lines()
        .position(|line| line.contains("New task ❯"))
        .unwrap();
    tmux.mouse(64, x, row, false);
    tmux.wait_for(|capture| !capture.contains("No reply needed."));
    tmux.mouse(65, x, row, false);
    tmux.wait_for(|capture| capture.contains("No reply needed."));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let path = std::env::temp_dir().join("agents-prompt-mouse-tmux.txt");
    std::fs::write(&path, capture).unwrap();
    println!(
        "{{\"verdict\":\"pass\",\"prompt_mouse_click\":\"pass\",\"selection\":\"pass\",\"wheel\":\"pass\",\"capture\":\"{}\"}}",
        path.display()
    );
}
