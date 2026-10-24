use super::super::{inline_images::InlineImages, tool_detail};
use super::*;
use crate::images::{Image, ImageSource};

fn image_source() -> ImageSource {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-read-image.jsonl");
    let text = std::fs::read_to_string(&path).unwrap();
    ImageSource::Record {
        record: crate::event::SourceRecord {
            path,
            offset: (text.lines().next().unwrap().len() + 1) as u64,
        },
        pointer: "/message/content/0/content/0/source/data".into(),
    }
}

fn prompt_event() -> Event {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-prompt-images.jsonl");
    let mut event = crate::providers::claude::event_reader::tail(&path, 100)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == Kind::User && event.images.len() == 1)
        .unwrap();
    event.images[0].source = image_source();
    event.body = event.images[0].label.clone();
    event
}

fn columns() -> EventDetailColumns {
    let area = Rect::new(0, 0, 100, 30);
    EventDetailColumns::new(area)
}

fn layout(event: &Event, cwd: &Path, images: &mut InlineImages) -> EventDetailLayout {
    let columns = columns();
    let sections = tool_detail::event_detail_content(event, String::new(), Style::default())
        .with_images(event, cwd)
        .sections;
    event_detail_parts_layout_with_images(
        event,
        columns,
        cwd,
        DateTime::UNIX_EPOCH,
        sections,
        |_, source| images.size(source, columns.text.width, Default::default()),
    )
}

#[test]
fn wide_text_images_keep_the_text_centered_independently() {
    let mut event = prompt_event();
    let mut images = InlineImages::new(Some((10, 20)));
    let with_image = layout(&event, Path::new("/"), &mut images);
    assert_eq!(with_image.images.len(), 1);
    assert!(usize::from(with_image.images[0].width) > event.body.len());
    let row = with_image
        .lines
        .iter()
        .position(|line| line.to_string().trim() == event.body)
        .unwrap();
    assert_eq!(with_image.images[0].rows.start, row + 2);
    assert_eq!(
        with_image
            .lines
            .iter()
            .filter(|line| line.to_string().contains(&event.body))
            .count(),
        1
    );
    event.images.clear();
    let without_image = layout(&event, Path::new("/"), &mut images);
    assert_eq!(
        with_image.lines[row].to_string(),
        without_image.lines[row].to_string()
    );
}

#[test]
fn body_paths_draw_once_and_keep_their_shortened_text() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("screens")).unwrap();
    let path = directory.path().join("screens/screen.png");
    std::fs::write(&path, image_source().bytes().unwrap()).unwrap();
    let mut event = prompt_event();
    event.images.clear();
    event.body = format!("{} screens/screen.png", path.display());
    let mut images = InlineImages::new(Some((10, 20)));
    let detail = layout(&event, directory.path(), &mut images);
    assert_eq!(detail.images.len(), 1);
    let text = detail
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains(path.to_str().unwrap()));
    assert_eq!(text.matches("screens/screen.png").count(), 2);
    let plain = event_detail_layout(&event, 100, directory.path(), DateTime::UNIX_EPOCH);
    assert!(plain.images.is_empty());
    assert_eq!(plain.lines.len(), detail.images[0].rows.start);
}

#[test]
fn unattached_body_labels_stay_single_lines_without_graphics() {
    let event = prompt_event();
    let detail = event_detail_layout(&event, 100, Path::new("/"), DateTime::UNIX_EPOCH);
    assert!(detail.images.is_empty());
    let text = detail
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.matches(&event.images[0].label).count(), 1);
}

#[test]
fn unmentioned_path_captions_use_the_existing_abbreviation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("screen.png");
    std::fs::write(&path, image_source().bytes().unwrap()).unwrap();
    let mut event = prompt_event();
    let source = super::super::text_images::text_images(
        path.to_str().unwrap(),
        directory.path(),
        &[],
        &mut Default::default(),
    )
    .remove(0)
    .source;
    event.images = vec![Image {
        label: path.to_str().unwrap().into(),
        source,
    }];
    let detail = event_detail_layout(&event, 100, directory.path(), DateTime::UNIX_EPOCH);
    let text = detail
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("screen.png"));
    assert!(!text.contains(path.to_str().unwrap()));
    assert!(detail.images.is_empty());
}

#[test]
fn drawn_images_carry_no_path_link_beside_them() {
    let dir = tempfile::tempdir().unwrap();
    let screenshot = dir.path().join("screenshot.png");
    image::RgbImage::new(2, 2).save(&screenshot).unwrap();
    let fixture = include_str!("../../tests/fixtures/claude-appium.jsonl").replace(
        "/tmp/screenshot_1792876190968.png",
        screenshot.to_str().unwrap(),
    );
    let transcript = dir.path().join("transcript.jsonl");
    std::fs::write(&transcript, fixture).unwrap();
    let event = crate::providers::claude::event_reader::tail(&transcript, 200)
        .unwrap()
        .into_iter()
        .find(|event| {
            event
                .calls
                .iter()
                .any(|call| call.name.ends_with("appium_screenshot"))
        })
        .unwrap();
    let mut images = InlineImages::new(Some((10, 20)));
    let layout = layout(&event, dir.path(), &mut images);
    assert_eq!(layout.images.len(), 1);
    let target = super::super::links::LinkTarget::Path(screenshot.canonicalize().unwrap());
    let raw = super::super::links::LinkTarget::Path(screenshot.clone());
    assert!(
        !layout
            .links
            .iter()
            .flatten()
            .any(|link| link.target == target || link.target == raw),
        "{:?}",
        layout.links
    );
}
