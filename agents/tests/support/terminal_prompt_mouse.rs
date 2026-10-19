use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_prompt_mouse_click_places_caret() {
    let (_directory, tmux, _transcript) = start_probe("prompt-mouse");
    begin_named_draft(&tmux, "Mouse probe");
    let capture = tmux.capture_when(|capture| capture.contains("Mouse probe ❯"));
    assert!(
        title_rule_position(&capture, "Mouse probe").is_some(),
        "the prompt keeps the name on the details rule: {capture}"
    );
    assert!(!capture.contains("Rename ❯"), "{capture}");
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
        .find(|(_, line)| line.contains("Mouse probe ❯"))
        .unwrap();
    let x = line[..line.find("写工具详情计划").unwrap()].width();
    tmux.mouse(0, x + 3, row, true);
    tmux.run(&["send-keys", "-t", "probe", "-l", "查"]);
    tmux.wait_for(|capture| capture.contains("写查工具详情计划"));
    // Rows after the first start at the box's left edge, not after the label.
    let edge = line[..line.find("Mouse probe").unwrap()].width();
    tmux.mouse(0, edge + 3, row + 1, true);
    tmux.run(&["send-keys", "-t", "probe", "-l", "x"]);
    tmux.wait_for(|capture| capture.contains("No xreply needed."));
    tmux.run(&["send-keys", "-t", "probe", "End", "C-u"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "写工具详情计划"]);
    tmux.wait_for(|capture| capture.contains("写工具详情计划"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let row = capture
        .lines()
        .position(|line| line.contains("Mouse probe ❯"))
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
    tmux.run(&["send-keys", "-t", "probe", "End", "C-u"]);
    tmux.run(&[
        "send-keys",
        "-t",
        "probe",
        "-l",
        // Two copies outgrow the box, which takes at most half the rows, so the wheel scrolls.
        &format!(
            "\x1b[200~{0}\n{0}\x1b[201~",
            include_str!("../fixtures/claude-peer-message.txt")
        ),
    ]);
    tmux.wait_for(|capture| capture.contains("No reply needed."));
    let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let row = before
        .lines()
        .position(|line| line.contains("No reply needed."))
        .unwrap();
    // Draft row age changes while typing; compare the prompt surface itself.
    let prompt_surface = |capture: &str| {
        capture
            .lines()
            .skip_while(|line| !(line.contains("Mouse probe") && line.contains('─')))
            .collect::<Vec<_>>()
            .join("\n")
    };
    tmux.mouse(64, x, row, false);
    tmux.wait_for(|capture| prompt_surface(capture) != prompt_surface(&before));
    tmux.mouse(65, x, row, false);
    tmux.wait_for(|capture| prompt_surface(capture) == prompt_surface(&before));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let path = std::env::temp_dir().join("agents-prompt-mouse-tmux.txt");
    std::fs::write(&path, capture).unwrap();
    println!(
        "{{\"verdict\":\"pass\",\"prompt_mouse_click\":\"pass\",\"selection\":\"pass\",\"wheel\":\"pass\",\"capture\":\"{}\"}}",
        path.display()
    );
}
