use super::*;
use std::io::Write;

pub(super) fn message_width(capture: &str, width: usize) -> usize {
    if width < 150 {
        return width - 2 - 10;
    }
    let row = capture.lines().nth(2).unwrap();
    let separator = row.rfind('│').expect("actual log-list separator");
    let list_left = row[..separator].width() + 2;
    width - 1 - list_left - 10
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_preformatted_detail_uses_viewport_width() {
    let calls: Vec<_> = include_str!("../fixtures/claude-tool-calls.jsonl")
        .lines()
        .collect();
    let (_directory, tmux, transcript) = start_probe("detail-message-width");
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(transcript)
            .unwrap(),
        "{}\n{}",
        calls[2],
        calls[3]
    )
    .unwrap();
    for width in [200_u16, 120, 150, 149] {
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
            capture.lines().count() == 50
                && capture.lines().nth(1).is_some_and(|line| {
                    line.matches('┬').count() == if width >= 150 { 2 } else { 1 }
                })
                && capture.lines().any(|line| {
                    line.width() == usize::from(width)
                        && line.matches('─').count() > usize::from(width / 2)
                })
                && capture.contains("default-terminal")
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let path = std::env::temp_dir().join(format!("agents-detail-message-width-{width}.txt"));
        std::fs::write(&path, &capture).unwrap();
        let rows: Vec<_> = capture.lines().collect();
        let rule = rows
            .iter()
            .rposition(|line| line.matches('─').count() > usize::from(width / 2))
            .unwrap();
        let body: Vec<_> = rows
            .iter()
            .skip(rule + 3)
            .take_while(|line| !line.starts_with(" ? help"))
            .map(|line| line.trim_end().trim_end_matches('│').trim_end())
            .filter(|line| !line.trim().is_empty() && line.trim() != "↓")
            .collect();
        assert!(!body.is_empty(), "{capture}");
        let left = body
            .iter()
            .map(|line| line.width() - line.trim_start().width())
            .min()
            .unwrap();
        let right = body.iter().map(|line| line.width()).max().unwrap();
        let cap = usize::from(width) - 2;
        assert!(
            right - left <= cap,
            "detail width {} exceeds detail viewport {cap}: {capture}",
            right - left
        );
        if width >= 150 {
            assert!(left.abs_diff(usize::from(width) - right) <= 1, "{capture}");
        } else {
            assert_eq!(left, 1, "narrow detail must be left aligned: {capture}");
        }
        println!(
            "{{\"detail_message_width\":\"pass\",\"width\":{width},\"cap\":{cap},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_raw_detail_uses_viewport_width() {
    let (_directory, tmux, _transcript) = start_probe("raw-message-width");
    tmux.wait_for(|capture| capture.contains("对，必须"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let rows: Vec<_> = capture.lines().collect();
    let rule = rows
        .iter()
        .rposition(|line| line.matches('─').count() > 100)
        .unwrap();
    tmux.mouse(2, 100, rule + 3, true);
    tmux.wait_for(|capture| capture.contains("\"message\""));
    for width in [200_u16, 120, 150, 149] {
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
            capture.lines().count() == 50
                && capture.lines().nth(1).is_some_and(|line| {
                    line.matches('┬').count() == if width >= 150 { 2 } else { 1 }
                })
                && capture.lines().any(|line| {
                    line.width() == usize::from(width)
                        && line.matches('─').count() > usize::from(width / 2)
                })
                && capture.contains("\"message\"")
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let path = std::env::temp_dir().join(format!("agents-raw-message-width-{width}.txt"));
        std::fs::write(&path, &capture).unwrap();
        let rows: Vec<_> = capture.lines().collect();
        let rule = rows
            .iter()
            .rposition(|line| line.matches('─').count() > usize::from(width / 2))
            .unwrap();
        let body: Vec<_> = rows
            .iter()
            .skip(rule + 3)
            .take_while(|line| !line.starts_with(" ? help"))
            .map(|line| line.trim_end().trim_end_matches('│').trim_end())
            .filter(|line| !line.trim().is_empty())
            .collect();
        let left = body
            .iter()
            .map(|line| line.width() - line.trim_start().width())
            .min()
            .unwrap();
        let right = body.iter().map(|line| line.width()).max().unwrap();
        let cap = usize::from(width) - 2;
        assert!(
            right - left <= cap,
            "raw width {} exceeds detail viewport {cap}: {capture}",
            right - left
        );
        // This record's long JSON string exceeds every tested viewport. Its visible
        // prefix starts at the left edge; clipped wide cells need not fill the last column.
        assert_eq!(left, 1, "{capture}");
        println!(
            "{{\"raw_message_width\":\"pass\",\"width\":{width},\"cap\":{cap},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}
