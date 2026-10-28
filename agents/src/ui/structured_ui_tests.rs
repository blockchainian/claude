use super::*;

// Extracted verbatim from a recorded tool result that read a TextMate plist.
const XML: &str = include_str!("../../tests/fixtures/structured_text/real-theme.plist");

fn xml_body() -> String {
    format!("Before XML\n{}\nAfter XML", XML.trim())
}

fn assert_pretty_xml(text: &str) {
    assert!(
        text.contains("Before XML\n<plist version=\"1.0\">\n  <dict>"),
        "{text}"
    );
    assert!(
        text.contains("\n    <key>settings</key>\n    <array>"),
        "{text}"
    );
    assert!(text.contains("\n</plist>\nAfter XML"), "{text}");
}

#[test]
fn structured_xml_body_is_pretty_and_highlighted_for_every_kind() {
    let (mut view, now) = event_fixture();
    let template = view.event_panel[7].clone();
    for kind in [
        crate::event::Kind::Error,
        crate::event::Kind::Denied,
        crate::event::Kind::Assistant,
        crate::event::Kind::User,
        crate::event::Kind::Agent,
        crate::event::Kind::Session,
        crate::event::Kind::QueuedUser,
        crate::event::Kind::QueuedSystem,
    ] {
        let mut event = template.clone();
        event.kind = kind;
        event.body = xml_body();
        event.calls.clear();
        assert_pretty_xml(&super::super::event_list::open_text(&event));
        view.select_fixture_event(Some(event));
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 64));
        view.render_at(buf.area, &mut buf, now);
        let event_detail_y = view.viewport.borrow().event_detail.y;
        let detail = buf
            .content
            .chunks(120)
            .skip(usize::from(event_detail_y))
            .map(|row| {
                row.iter()
                    .map(|cell| cell_text(cell.symbol()))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(detail.contains("<key>settings</key>"), "{detail}");
        let (x, y) = rendered_cell(&detail, "<plist version");
        assert!(
            (x..x + 6).any(|column| matches!(
                buf[(column, y + event_detail_y)].fg,
                Color::Rgb(..) | Color::Indexed(..)
            )),
            "XML needs syntax colors: {detail}"
        );
    }
}

#[test]
fn structured_xml_tool_text_and_markdown_share_pretty_copy_sources() {
    for markdown in [false, true] {
        let (mut view, now) = open_tool_fixture();
        let mut event = view.event_panel[7].clone();
        event.calls.truncate(1);
        event.calls[0].input.clear();
        event.calls[0].output = Some(vec![if markdown {
            crate::event::Section::Markdown {
                label: "output".into(),
                text: xml_body(),
            }
        } else {
            crate::event::Section::Text {
                label: "stdout".into(),
                text: xml_body(),
            }
        }]);
        assert_pretty_xml(&super::super::event_list::open_text(&event));
        view.select_fixture_event(Some(event));
        let rendered = event_screen(&view, 120, 64, now);
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 64));
        view.render_at(buf.area, &mut buf, now);
        let viewport = view.viewport.borrow();
        let xml = viewport
            .event_text
            .sources
            .iter()
            .find(|source| source.text.starts_with("<plist"))
            .expect("XML has a formatted copy source");
        assert!(
            xml.text.contains("\n  <dict>\n    <key>settings</key>"),
            "{}",
            xml.text
        );
        for row in viewport
            .event_text
            .rows
            .iter()
            .filter(|row| row.segment == xml.id)
        {
            for (cells, bytes) in &row.cells {
                let drawn: String = (cells.start..cells.end)
                    .map(|offset| buf[(row.x + offset, row.y)].symbol())
                    .collect();
                assert_eq!(drawn, &xml.text[bytes.clone()]);
            }
        }
        let pretty = xml.text.to_string();
        let event_detail_y = viewport.event_detail.y;
        drop(viewport);
        let detail = rendered
            .lines()
            .skip(usize::from(event_detail_y))
            .collect::<Vec<_>>()
            .join("\n");
        let (start_x, start_y) = rendered_cell(&detail, "<plist version");
        let (end_x, end_y) = rendered_cell(&detail, "</plist>");
        view.begin_text_selection(start_x, start_y + event_detail_y);
        view.extend_text_selection(end_x + 8, end_y + event_detail_y);
        event_screen(&view, 120, 64, now);
        assert_eq!(
            view.finish_text_selection(end_x + 8, end_y + event_detail_y),
            Some(Action::Copy {
                text: pretty,
                label: "Copied message".into()
            })
        );
    }
}

#[test]
fn structured_xml_body_remaps_provider_path_ranges() {
    let (mut view, now) = event_fixture();
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let path = directory.path().join("report.md");
    std::fs::write(&path, "report").unwrap();
    let path_text = path.to_str().unwrap();
    let mut event = view.event_panel[7].clone();
    event.kind = crate::event::Kind::QueuedSystem;
    event.body = xml_body().replace("#123456", path_text);
    event.calls.clear();
    let start = event.body.find(path_text).unwrap();
    event.paths.clear();
    event.paths.push(start..start + path_text.len());
    view.select_fixture_event(Some(event));
    let rendered = event_screen(&view, 120, 64, now);
    assert!(
        view.viewport
            .borrow()
            .event_links
            .iter()
            .any(|(_, target)| target == &LinkTarget::Path(path.clone())),
        "formatted XML lost the provider path: {rendered}"
    );
}

#[test]
fn structured_mixed_json_xml_and_literal_markup_keep_copy_order() {
    let (mut view, now) = event_fixture();
    let mut event = view.event_panel[7].clone();
    event.kind = crate::event::Kind::Error;
    event.calls.clear();
    event.body = format!(
        "# Result\n{{\"answer\":[1,2]}}\n{}\n**finished**",
        XML.trim()
    );
    let copied = super::super::event_list::open_text(&event);
    assert!(
        copied.contains(
            "# Result\n{\n  \"answer\": [\n    1,\n    2\n  ]\n}\n<plist version=\"1.0\">\n  <dict>"
        ),
        "{copied}"
    );
    assert!(copied.ends_with("</plist>\n**finished**"), "{copied}");
    view.select_fixture_event(Some(event));
    let rendered = event_screen(&view, 120, 100, now);
    let detail = rendered
        .lines()
        .skip(usize::from(view.viewport.borrow().event_detail.y))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(detail.contains("**finished**"), "{rendered}");
    assert!(detail.contains("\"answer\": ["), "{rendered}");
    assert!(detail.contains("<key>settings</key>"), "{rendered}");
    let snapshot =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/structured_xml.txt");
    if std::env::var_os("UPDATE_STRUCTURED_SNAPSHOT").is_some() {
        std::fs::write(&snapshot, &rendered).unwrap();
    }
    assert_eq!(rendered, std::fs::read_to_string(snapshot).unwrap());
}

#[test]
fn structured_inline_and_invalid_xml_stay_literal() {
    for body in [
        "Use <path> in this sentence and <foo> as a placeholder.",
        "Before XML\n<plist><dict></plist>\nAfter XML",
        "Before XML\n<plist><dict/></plist> trailing text\nAfter XML",
        "Before XML\n<div>inline <strong>HTML</strong></div> inside prose\nAfter XML",
    ] {
        let (mut view, now) = event_fixture();
        let mut event = view.event_panel[7].clone();
        event.kind = crate::event::Kind::Error;
        event.calls.clear();
        event.body = body.into();
        assert_eq!(super::super::event_list::open_text(&event), body);
        view.select_fixture_event(Some(event));
        let rendered = event_screen(&view, 120, 64, now);
        let viewport = view.viewport.borrow();
        assert!(
            viewport
                .event_text
                .sources
                .iter()
                .any(|source| source.text.as_ref() == body),
            "literal source changed: {rendered}"
        );
    }
}
