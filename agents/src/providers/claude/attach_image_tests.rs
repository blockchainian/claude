use super::*;

#[test]
fn image_path_paste_quotes_spaces() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("red square.png");
    std::fs::write(&path, b"PNG").unwrap();
    assert_eq!(
        image_path(&path).unwrap(),
        format!("'{}'", path.canonicalize().unwrap().display())
    );
}

#[test]
fn image_path_paste_rejects_missing_files_and_controls() {
    assert!(image_path(Path::new("/missing/agents-image.png")).is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image\n.png");
    std::fs::write(&path, b"PNG").unwrap();
    assert_eq!(
        image_path(&path).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn image_confirmation_requires_the_current_editor() {
    let screen = include_str!("../../../tests/fixtures/claude-attach/image-path.screen.txt");
    assert!(image_visible(screen, 1));
    assert!(!image_visible(screen, 2));
    let historical = format!(
        "❯ old prompt [Image #1]\n{}",
        screen.replace("[Image #1]", "")
    );
    assert!(!image_visible(&historical, 1));
}

#[test]
fn image_confirmation_counts_attachments_across_turns() {
    let screen = include_str!("../../../tests/fixtures/claude-attach/image-path-second.screen.txt");
    assert!(image_visible(screen, 1));
    assert!(!image_visible(screen, 2));
}
