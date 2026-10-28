use super::*;

#[test]
fn terminal_structured_json_xml_and_literal_text() {
    let xml = include_str!("../fixtures/structured_text/real-theme.plist").trim();
    let body = format!(
        "Before XML\n{xml}\nAfter XML\nUse <path> inline.\n<plist><dict></plist>\n{{\"answer\":[1,2]}}\nStructured output complete"
    );
    // Preserve the captured provider envelope and replace only its assistant text.
    let records = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            if record["type"] == "assistant" {
                record["message"]["content"][0]["text"] = body.clone().into();
            }
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (_directory, tmux, _transcript) = start_probe_with_transcript("structured-text", &records);
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "120"]);
    let capture = tmux.capture_when(|capture| {
        capture.contains("Structured output complete")
            && capture.lines().count() == 120
            && capture.contains("Assistant")
    });
    let lines: Vec<_> = capture.lines().collect();
    let divider = lines
        .iter()
        .rposition(|line| line.matches('─').count() > 100)
        .expect("open log details have a panel divider");
    let rows = &lines[divider + 1..];
    let row = |needle: &str| {
        rows.iter()
            .enumerate()
            .find(|(_, line)| line.contains(needle))
    };
    let root = row("<plist version=\"1.0\">");
    let dict = row("<dict>");
    let key = row("<key>settings</key>");
    let pretty_xml = match (root, dict, key) {
        (Some((root_y, root)), Some((dict_y, dict)), Some((key_y, key))) => {
            dict_y == root_y + 1
                && key_y == dict_y + 1
                && dict.find("<dict>").unwrap() == root.find("<plist").unwrap() + 2
                && key.find("<key>").unwrap() == root.find("<plist").unwrap() + 4
        }
        _ => false,
    };
    let literal_inline = row("Use <path> inline.").is_some();
    let literal_invalid = row("<plist><dict></plist>").is_some();
    let json_compatible = row("\"answer\": [").is_some() && row("1,").is_some();
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 120);
    let xml_colors = root.is_some_and(|(y, text)| {
        let x = text.find("<plist").unwrap();
        (x..x + 6).any(|column| {
            parser
                .screen()
                .cell((divider + 1 + y) as u16, column as u16)
                .is_some_and(|cell| cell.fgcolor() != vt100::Color::Default)
        })
    });
    let capture_path = std::env::temp_dir().join("agents-structured-tmux.txt");
    std::fs::write(&capture_path, &capture).unwrap();
    let verdict = serde_json::json!({
        "verdict": if pretty_xml && xml_colors && literal_inline && literal_invalid && json_compatible { "pass" } else { "fail" },
        "xml_indentation": pretty_xml,
        "xml_colors": xml_colors,
        "inline_literal": literal_inline,
        "invalid_literal": literal_invalid,
        "json_compatible": json_compatible,
        "capture": capture_path,
        "binary": std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into()).to_string_lossy(),
    });
    std::fs::write(
        std::env::temp_dir().join("agents-structured-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
    assert_eq!(verdict["verdict"], "pass", "{capture}");
}
