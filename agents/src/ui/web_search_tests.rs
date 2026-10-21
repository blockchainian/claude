use super::*;
use crate::event::Section;

fn recorded_record(index: usize) -> serde_json::Value {
    serde_json::from_str(
        include_str!("../../tests/fixtures/claude-web-search.jsonl")
            .lines()
            .nth(index)
            .unwrap(),
    )
    .unwrap()
}

fn recorded_search() -> Vec<crate::event::Event> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-web-search.jsonl");
    crate::providers::claude::event_reader::tail(&path, 10).unwrap()
}

#[test]
fn web_search_input_is_only_the_recorded_query() {
    let events = recorded_search();
    let record = recorded_record(0);
    assert_eq!(
        events[0].calls[0].input,
        vec![Section::Prose {
            text: record["message"]["content"][0]["input"]["query"]
                .as_str()
                .unwrap()
                .into(),
            markdown: false,
        }]
    );
}

#[test]
fn web_search_table_preserves_summary_and_clicks_recorded_urls() {
    let events = recorded_search();
    let call = &events[0].calls[0];
    let output = call.output.as_ref().unwrap();
    let [Section::Markdown { text, .. }] = output.as_slice() else {
        panic!("WebSearch output must be Markdown: {output:?}");
    };
    let record = recorded_record(1);
    let original = record["message"]["content"][0]["content"].as_str().unwrap();
    let source = original.split_once("Links: ").unwrap().1;
    let mut stream = serde_json::Deserializer::from_str(source).into_iter::<serde_json::Value>();
    let links = stream.next().unwrap().unwrap();
    let summary = &source[stream.byte_offset()..];
    assert!(text.ends_with(summary));
    assert!(text.starts_with("| Title |\n| --- |\n"));
    assert!(!text.contains("Web search results for query:"));
    let now = events[0].ended_at.unwrap();
    for width in [120, 180] {
        let (mut view, _) = event_fixture();
        view.set_event(events.clone());
        view.select_fixture_event(Some(events[0].clone()));
        let rendered = event_screen(&view, width, 90, now);
        assert!(rendered.contains("Title"), "{rendered}");
        assert!(!rendered.contains("Links:"), "{rendered}");
        let drawn = view.viewport.borrow().event_links.clone();
        for link in links.as_array().unwrap() {
            let url = PathBuf::from(link["url"].as_str().unwrap());
            let (rect, _) = drawn
                .iter()
                .find(|(_, target)| *target == url)
                .unwrap_or_else(|| panic!("missing {} in {drawn:?}\n{rendered}", url.display()));
            assert_eq!(
                click_mouse(
                    &mut view,
                    mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y,)
                )
                .or_else(|| view.flush_pending_click(Instant::now() + view.double_click_interval)),
                Some(Action::OpenPath(url))
            );
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/ui_snapshots/web_search_{width}.txt"));
        if std::env::var_os("UPDATE_WEB_SEARCH_SNAPSHOTS").is_some() {
            std::fs::write(&path, &rendered).unwrap();
        }
        assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
    }
}

#[test]
fn web_search_without_summary_still_renders_the_table() {
    let record = recorded_record(1);
    let original = record["message"]["content"][0]["content"].as_str().unwrap();
    let (header, source) = original.split_once("Links: ").unwrap();
    let mut stream = serde_json::Deserializer::from_str(source).into_iter::<serde_json::Value>();
    let links = stream.next().unwrap().unwrap();
    let text = format!("{header}Links: {}", &source[..stream.byte_offset()]);
    let output = crate::providers::tool::text_output(
        &text,
        crate::providers::tool::rule("WebSearch").output,
        &serde_json::Value::Null,
    );
    let [Section::Markdown { text, .. }] = output.as_slice() else {
        panic!("{output:?}");
    };
    assert_eq!(text.lines().count(), links.as_array().unwrap().len() + 2);
}

#[test]
fn web_search_unrecognized_result_keeps_its_source() {
    let record = recorded_record(1);
    let original = record["message"]["content"][0]["content"].as_str().unwrap();
    // An interrupted capture must remain readable instead of losing its raw result.
    for source in [
        original.split_once("Links: ").unwrap().0,
        &original[..original.find("}]").unwrap() + 1],
    ] {
        assert_eq!(
            crate::providers::tool::text_output(
                source,
                crate::providers::tool::rule("WebSearch").output,
                &serde_json::Value::Null,
            ),
            vec![Section::Text {
                label: "output".into(),
                text: source.into()
            }]
        );
    }
}
