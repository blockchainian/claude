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

// Detail panels occupy the terminal interior. Their clock and two-cell gap
// reserve ten cells at the right edge; reading width is independent of that.
pub(super) fn content_width(width: usize) -> usize {
    width - 2 - 10
}

pub(super) fn detail_center(width: usize, content_width: usize) -> usize {
    (1 + (width - 2) / 2)
        .saturating_sub(content_width / 2)
        .min((width - 11).saturating_sub(content_width))
        .max(1)
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_preformatted_detail_stays_in_content_column() {
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
            .skip(rule + 2)
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
        let cap = content_width(usize::from(width));
        let column_left = 1;
        assert!(
            right - left <= cap,
            "detail width {} exceeds detail viewport {cap}: {capture}",
            right - left
        );
        assert_eq!(
            left,
            detail_center(usize::from(width), right - left),
            "{capture}"
        );
        assert!(right <= column_left + cap, "{capture}");
        println!(
            "{{\"detail_message_width\":\"pass\",\"width\":{width},\"cap\":{cap},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}
