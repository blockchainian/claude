use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_group_headings_center_at_wide_and_narrow_widths() {
    let (_directory, tmux, _transcript) = start_probe("headings");
    for (grouping, heading) in [
        ("Project", "/claude"),
        ("Status", "Inactive"),
        ("Model", "claude-opus-5-5"),
    ] {
        if grouping != "Project" {
            tmux.run(&["send-keys", "-t", "probe", "g"]);
            tmux.wait_for(|capture| {
                capture
                    .lines()
                    .nth(2)
                    .is_some_and(|row| row.contains(heading))
            });
        }
        for width in [80, 112, 200, 300] {
            tmux.run(&[
                "resize-window",
                "-t",
                "probe",
                "-x",
                &width.to_string(),
                "-y",
                "50",
            ]);
            std::thread::sleep(Duration::from_millis(300));
            let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            let row = capture.lines().nth(2).unwrap();
            let separator = row.find('│').unwrap();
            let list: String = row.chars().take(separator - 1).skip(1).collect();
            let label = list.trim();
            assert!(!label.is_empty(), "{capture}");
            assert_eq!(
                list.find(label).unwrap(),
                (list.width() - label.width()) / 2,
                "{capture}"
            );
            if grouping == "Project" {
                assert!(label.ends_with(heading), "{capture}");
            } else {
                assert_eq!(label, heading, "{capture}");
            }
            assert!(!capture.lines().last().unwrap().contains("g group by"));
            println!(
                "{grouping}, {width} columns:\n{}",
                capture.lines().take(7).collect::<Vec<_>>().join("\n")
            );
        }
    }
    println!("{{\"group_headings\":\"pass\",\"widths\":[80,112,200,300]}}");
}
