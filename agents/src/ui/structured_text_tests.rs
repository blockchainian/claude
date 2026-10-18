use super::*;

#[test]
fn embedded_xml_is_indented_highlighted_and_copied_from_pretty_source() {
    let source = "before\n<plist version=\"1.0\">\n<dict><key>Label</key><string>agents</string></dict>\n</plist>\nafter";
    let parts = Part::text_parts(source.into(), false);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].source, "before");
    assert_eq!(
        parts[1].source,
        "<plist version=\"1.0\">\n  <dict>\n    <key>Label</key>\n    <string>agents</string>\n  </dict>\n</plist>"
    );
    assert_eq!(parts[2].source, "after");
    let lines = parts[1].lines(120, Style::default(), chrono::DateTime::UNIX_EPOCH);
    assert!(
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| span.style.fg.is_some())
    );
    let maps = super::super::text_selection_layout::map_lines(&parts[1].source, &lines, false);
    for (line, map) in lines.iter().zip(maps) {
        for (cells, bytes) in map {
            assert_eq!(
                &line.to_string()[cells.start as usize..cells.end as usize],
                &parts[1].source[bytes]
            );
        }
    }
}

#[test]
fn embedded_xml_keeps_inline_html_and_invalid_xml_literal() {
    for source in [
        "look at <path> in this sentence",
        "use <foo> and continue",
        "inline <em>hello</em> HTML",
        "before <root><child>value</child></root> after",
        "before\n<root>\n<child>value</root>\nafter",
        "before\n<root>\n<child>value</child>\nafter",
        "before\n<root a=\"1\" a=\"2\">value</root>\nafter",
        "before\n<root>bad &missing;</root>\nafter",
        "before\n<root>one</root><root>two</root>\nafter",
    ] {
        let parts = Part::text_parts(source.into(), false);
        assert_eq!(parts.len(), 1, "{source}");
        assert_eq!(parts[0].source, source);
    }
}

#[test]
fn markdown_code_blocks_keep_their_declared_content() {
    let xml = include_str!("../../tests/fixtures/structured_text/real-theme.plist");
    let source = format!("before\n\n```xml\n{xml}\n```\n\nafter");
    let parts = Part::prose_parts(source.clone(), true);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].source, source);
}

#[test]
fn explicit_structured_sections_use_the_format_registry() {
    let section = Section::Structured {
        label: "input".into(),
        text: include_str!("../../tests/fixtures/structured_text/real-theme.plist").into(),
    };
    let mut parts = Vec::new();
    push_section(&mut parts, &section, Style::default(), false);
    assert_eq!(parts.len(), 1);
    assert!(
        parts[0]
            .source
            .contains("\n  <dict>\n    <key>settings</key>")
    );
}
