use super::*;

#[test]
fn terminal_task_details_border_double_clicks() {
    let prompt = (0..20)
        .chain(std::iter::once(79))
        .map(|i| format!("Border prompt row {i:02}  \n"))
        .collect::<String>();
    let records = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            if record["type"] == "user" {
                record["message"]["content"] = prompt.clone().into();
            }
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (directory, tmux, _transcript) =
        start_probe_with_transcript("details-border-jump", &(records + "\n"));
    tmux.wait_for(|capture| capture.contains("Border prompt row 00"));
    for width in [100, 149, 150, 200] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "30",
        ]);
        let initial = tmux.capture_when(|capture| {
            capture.lines().count() == 30
                && capture.lines().nth(1).unwrap().width() == width
                && capture.lines().nth(2).unwrap().matches('│').count()
                    == if width >= 150 { 2 } else { 1 }
                && capture
                    .lines()
                    .nth(28)
                    .is_some_and(|line| line.width() == width && line.chars().all(|ch| ch == '─'))
                && capture
                    .lines()
                    .nth(29)
                    .is_some_and(|line| line.contains("? help"))
        });
        let rows = initial.lines().collect::<Vec<_>>();
        let top = 1;
        let separator = rows[2].chars().position(|c| c == '│').unwrap();
        let column = separator + 2;
        let bottom = rows
            .iter()
            .enumerate()
            .skip(2)
            .find(|(_, line)| matches!(line.chars().nth(separator), Some('├' | '┴')))
            .unwrap_or_else(|| panic!("missing details border at {separator}: {initial}"))
            .0;
        let task_details = |capture: &str| {
            capture
                .lines()
                .take(bottom)
                .skip(top + 1)
                .map(|line| {
                    line.chars()
                        .skip(column)
                        .take(if width >= 150 {
                            width / 4 - 3
                        } else {
                            width / 2 - 2
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let double_click = |row: usize| {
            let sequence = format!(
                "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                column + 1,
                row + 1,
                column + 1,
                row + 1
            )
            .repeat(2);
            tmux.run(&["send-keys", "-t", "probe", "-l", &sequence]);
        };
        // The title rule's folder is clickable; use a plain rule cell after it.
        let border_column = (column
            ..column
                + if width >= 150 {
                    width / 4 - 3
                } else {
                    width / 2 - 2
                })
            .find(|&x| rows[bottom].chars().nth(x) == Some('─'))
            .unwrap();
        let bottom_click = format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            border_column + 1,
            bottom + 1,
            border_column + 1,
            bottom + 1
        )
        .repeat(2);
        tmux.run(&["send-keys", "-t", "probe", "-l", &bottom_click]);
        let end =
            tmux.capture_when(|capture| task_details(capture).contains("Border prompt row 79"));
        std::fs::write(directory.path().join(format!("bottom-{width}.txt")), end).unwrap();
        double_click(top);
        let start = tmux.capture_when(|capture| {
            task_details(capture).contains("Border prompt row 00")
                && !task_details(capture).contains("Border prompt row 79")
        });
        std::fs::write(directory.path().join(format!("top-{width}.txt")), start).unwrap();
    }
    let verdict = serde_json::json!({"task_details_border_double_clicks":"pass", "widths":[100,149,150,200], "ends":["top","bottom"]});
    if let Some(path) = std::env::var_os("AGENTS_PROBE_VERDICT") {
        std::fs::write(path, verdict.to_string()).unwrap();
    }
    println!("{verdict}");
}
