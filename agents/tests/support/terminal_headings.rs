use super::*;

// The full-width divider belongs to the panels; section titles below it are
// ordinary scrollable rows, without their own rules.
fn section_rows(capture: &str, width: usize) -> Vec<&str> {
    let rows: Vec<_> = capture.lines().collect();
    let Some(divider) = rows
        .iter()
        .rposition(|row| row.matches('─').count() > width / 2)
    else {
        return Vec::new();
    };
    rows[divider + 1..rows.len() - 1].to_vec()
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_event_section_titles_and_scrolling() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("section-titles");
    let capture = tmux.capture_when(|capture| {
        section_rows(capture, 200)
            .first()
            .is_some_and(|row| row.contains("Assistant"))
    });
    let rows = section_rows(&capture, 200);
    assert!(rows[0].contains("Assistant"), "{capture}");
    assert_eq!(
        rows[0][..rows[0].find("Assistant").unwrap()].width(),
        terminal_event_detail_width::event_detail_center(200, "Assistant".width()),
        "{capture}"
    );
    assert!(!rows[0].contains('─'), "{capture}");
    assert!(rows[1].trim_end_matches('│').trim().is_empty(), "{capture}");
    assert!(
        !rows[2].trim_end_matches('│').trim().is_empty(),
        "{capture}"
    );

    let recorded_agents: Vec<_> = include_str!("../fixtures/claude-agent-display.jsonl")
        .lines()
        .collect();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    writeln!(file, "{}\n{}", recorded_agents[0], recorded_agents[1]).unwrap();
    let capture = tmux.capture_when(|capture| {
        section_rows(capture, 200)
            .first()
            .is_some_and(|row| row.contains("Agent"))
    });
    let rows = section_rows(&capture, 200);
    assert!(rows[0].contains("Agent"), "{capture}");
    assert!(rows[0].contains("实测 attach 发提示词"), "{capture}");
    assert!(!rows[0].contains("sonnet"), "{capture}");
    assert!(!rows[0].contains('─'), "{capture}");
    assert!(rows[1].trim_end_matches('│').trim().is_empty(), "{capture}");
    assert!(
        !rows[2].trim_end_matches('│').trim().is_empty(),
        "{capture}"
    );
    let body = rows[2..].join("\n");
    assert!(!body.contains("general-purpose"), "{capture}");
    assert!(!body.contains("attach-probe"), "{capture}");
    assert!(!body.contains("实测 attach 发提示词"), "{capture}");
    assert!(
        body.contains("You have a hard 10-minute limit"),
        "{capture}"
    );
    std::fs::write(
        std::env::temp_dir().join("agents-log-agent-section.txt"),
        &capture,
    )
    .unwrap();

    // A long recorded prompt makes the title leave the viewport with its body.
    let calls: Vec<_> = include_str!("../fixtures/claude-tool-calls.jsonl")
        .lines()
        .collect();
    writeln!(file, "{}\n{}", calls[10], calls[11]).unwrap();
    tmux.run(&["resize-window", "-t", "probe", "-x", "90", "-y", "30"]);
    let before = tmux.capture_when(|capture| {
        capture.lines().count() == 30
            && section_rows(capture, 90)
                .first()
                .is_some_and(|row| row.contains("Agent"))
            && capture.contains("Write an implementation")
    });
    let divider = before
        .lines()
        .collect::<Vec<_>>()
        .iter()
        .rposition(|row| row.matches('─').count() > 45)
        .unwrap();
    tmux.mouse(65, 45, divider + 2, false);
    let after = tmux.capture_when(|capture| {
        let rows = section_rows(capture, 90);
        !rows.iter().any(|row| row.contains("Agent"))
            && rows.iter().any(|row| !row.trim().is_empty())
    });
    assert_ne!(before, after);
    assert!(
        !section_rows(&after, 90).iter().any(|row| row.contains('─')),
        "{after}"
    );
    let output = std::env::temp_dir().join("agents-log-section-scroll.txt");
    std::fs::write(&output, after).unwrap();
    println!(
        "{{\"event_section_titles\":\"pass\",\"message\":true,\"general_purpose\":true,\"title_scrolls\":true,\"capture\":\"{}\"}}",
        output.display()
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_parallel_tools_have_only_section_titles() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("parallel-section-titles");
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "140"]);
    let records: Vec<_> = include_str!("../fixtures/claude-tool-calls.jsonl")
        .lines()
        .skip(16)
        .take(3)
        .collect();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap();
    writeln!(file, "{}", records.join("\n")).unwrap();
    let capture = tmux.capture_when(|capture| {
        let rows = section_rows(capture, 200);
        rows.iter()
            .filter(|row| row.trim_start().starts_with("codex-manager start"))
            .count()
            == 2
            && rows.iter().any(|row| row.contains("Claude 图片链接"))
            && rows.iter().any(|row| row.contains("Codex 图片链接"))
    });
    let rows = section_rows(&capture, 200);
    assert!(rows[0].starts_with(" codex-manager start"), "{capture}");
    assert!(!rows.iter().any(|row| row.contains('─')), "{capture}");
    assert!(
        !rows.iter().any(|row| row.contains("codex-manager start ·")),
        "{capture}"
    );
    let titles: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.starts_with(" codex-manager start"))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(titles.len(), 2, "{capture}");
    for (section, name) in ["Claude 图片链接", "Codex 图片链接"]
        .into_iter()
        .enumerate()
    {
        let index = titles[section];
        let row = rows[index];
        assert!(row.contains(name), "{capture}");
        assert_eq!(row.matches("codex-manager start").count(), 1, "{capture}");
        assert_eq!(
            row[..row.find(name).unwrap()].width(),
            terminal_event_detail_width::event_detail_center(200, name.width()),
            "{capture}"
        );
        assert!(rows[index + 1].trim().is_empty(), "{capture}");
        assert!(
            rows[index + 2].contains("Implement workstream"),
            "{capture}"
        );
        assert!(
            !rows[index + 2..]
                .iter()
                .take_while(|row| row.trim() != "↓")
                .any(|row| row.contains("\"cwd\":")),
            "{capture}"
        );
    }
    let output = std::env::temp_dir().join("agents-log-parallel-section-titles.txt");
    std::fs::write(&output, capture).unwrap();
    println!(
        "{{\"parallel_tool_sections\":\"pass\",\"merged_title\":false,\"rules\":false,\"capture\":\"{}\"}}",
        output.display()
    );
}
