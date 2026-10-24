use super::*;
use crate::images::{Image, ImageSource};
use std::path::{Path, PathBuf};

fn prompt_event() -> Event {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-prompt-images.jsonl");
    crate::providers::claude::event_reader::tail(&path, 100)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == Kind::User && event.images.len() == 2)
        .unwrap()
}

fn content(event: &Event) -> EventDetailContent {
    event_detail_content(event, String::new(), Style::default())
}

fn shown(content: &EventDetailContent) -> Vec<&Image> {
    content
        .sections
        .iter()
        .flat_map(|section| &section.groups)
        .flat_map(|group| &group.parts)
        .filter_map(Part::shown_image)
        .collect()
}

#[test]
fn prompt_images_follow_their_text_in_separate_groups() {
    let event = prompt_event();
    let detail = content(&event).with_images(&event, Path::new("/"));
    assert_eq!(shown(&detail), event.images.iter().collect::<Vec<_>>());
    let groups = &detail.sections[0].groups;
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].parts[0].source, event.body);
    for group in &groups[1..] {
        assert_eq!(group.parts.len(), 1);
        assert!(matches!(
            group.parts[0].body,
            Body::Image { caption: false, .. }
        ));
    }
}

#[test]
fn each_text_part_gets_its_images_before_the_next_part() {
    let event = prompt_event();
    let mut detail = content(&event);
    detail.sections[0].groups[0].parts = event
        .images
        .iter()
        .map(|image| Part::text(image.label.clone(), false))
        .collect();
    let detail = detail.with_images(&event, Path::new("/"));
    let groups = &detail.sections[0].groups;
    assert_eq!(groups.len(), 4);
    for (index, image) in event.images.iter().enumerate() {
        assert_eq!(groups[index * 2].parts[0].source, image.label);
        assert_eq!(groups[index * 2 + 1].parts[0].shown_image(), Some(image));
    }
}

#[test]
fn unmentioned_prompt_images_get_captions_at_the_body_end() {
    let mut event = prompt_event();
    event.body = event.body.replace(&event.images[0].label, "");
    let detail = content(&event).with_images(&event, Path::new("/"));
    assert_eq!(shown(&detail), [&event.images[1], &event.images[0]]);
    let groups = &detail.sections[0].groups;
    assert!(matches!(
        groups[1].parts[0].body,
        Body::Image { caption: false, .. }
    ));
    assert!(matches!(
        groups[2].parts[0].body,
        Body::Image { caption: true, .. }
    ));
    assert_eq!(
        groups[2].parts[0].lines(80, Style::default(), chrono::DateTime::UNIX_EPOCH)[0].to_string(),
        event.images[0].label
    );
}

#[test]
fn repeated_labels_and_existing_image_parts_are_not_added_twice() {
    let mut event = prompt_event();
    event.body = format!("{}\n{}", event.body, event.body);
    let mut detail = content(&event);
    detail.sections[0].groups.push(Group {
        parts: vec![Part::image(event.images[0].clone(), true)],
    });
    let detail = detail.with_images(&event, Path::new("/"));
    let images = shown(&detail);
    assert_eq!(images.len(), 2);
    assert_eq!(
        images
            .iter()
            .filter(|image| image.source == event.images[0].source)
            .count(),
        1
    );
}

#[test]
fn raster_paths_are_discovered_in_tool_input_and_output() {
    let directory = tempfile::tempdir().unwrap();
    let paths = [
        directory.path().join("input.png"),
        directory.path().join("output.png"),
    ];
    for path in &paths {
        std::fs::write(path, crate::images::fixture_png()).unwrap();
    }
    let transcript =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-read-image.jsonl");
    let mut event = crate::providers::claude::event_reader::tail(&transcript, 100)
        .unwrap()
        .into_iter()
        .find(|event| !event.calls.is_empty())
        .unwrap();
    event.calls[0].input = vec![Section::Text {
        text: paths[0].to_str().unwrap().into(),
        label: String::new(),
    }];
    event.calls[0].output = Some(vec![Section::Text {
        text: paths[1].to_str().unwrap().into(),
        label: String::new(),
    }]);
    let detail = content(&event).with_images(&event, directory.path());
    let images = shown(&detail);
    assert_eq!(images.len(), 2);
    for (image, path) in images.into_iter().zip(paths) {
        assert!(matches!(&image.source, ImageSource::File { path: found, .. } if found == &path));
    }
}

#[test]
fn image_free_groups_keep_their_alignment() {
    let mut event = prompt_event();
    event.images.clear();
    let before = content(&event);
    let expected: Vec<_> = before
        .sections
        .iter()
        .map(|section| {
            section
                .groups
                .iter()
                .map(|group| {
                    group
                        .parts
                        .iter()
                        .map(|part| part.source.clone())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let detail = before.with_images(&event, Path::new("/"));
    let actual: Vec<_> = detail
        .sections
        .iter()
        .map(|section| {
            section
                .groups
                .iter()
                .map(|group| {
                    group
                        .parts
                        .iter()
                        .map(|part| part.source.clone())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(actual, expected);
}
