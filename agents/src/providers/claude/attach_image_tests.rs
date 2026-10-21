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
#[ignore = "requires an explicitly supplied real Claude background job and image"]
fn real_background_image_attach() {
    use serde_json::Value;
    let job = std::env::var("AGENTS_CLAUDE_IMAGE_JOB").expect("AGENTS_CLAUDE_IMAGE_JOB");
    let image = std::path::PathBuf::from(
        std::env::var_os("AGENTS_CLAUDE_IMAGE_PATH").expect("AGENTS_CLAUDE_IMAGE_PATH"),
    );
    let transcript = std::path::PathBuf::from(
        std::env::var_os("AGENTS_CLAUDE_IMAGE_TRANSCRIPT").expect("AGENTS_CLAUDE_IMAGE_TRANSCRIPT"),
    );
    for text in [
        "Describe the image color and shape only. Do not use tools.",
        "",
    ] {
        let start = std::fs::read_to_string(&transcript)
            .unwrap()
            .lines()
            .count();
        let cwd = std::env::current_dir().unwrap();
        send(
            &job,
            &cwd,
            &cwd.join("unused-settings.json"),
            SessionInput::PromptWithImages {
                text: text.to_owned(),
                images: vec![image.clone()],
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            let records: Vec<Value> = std::fs::read_to_string(&transcript)
                .unwrap()
                .lines()
                .skip(start)
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let image = records.iter().position(|record| {
                record["type"] == "user"
                    && record["message"]["content"]
                        .as_array()
                        .is_some_and(|blocks| {
                            blocks.iter().any(|block| {
                                block["type"] == "image" && block["source"]["type"] == "base64"
                            })
                        })
            });
            if let Some(index) = image
                && let Some(reply) = records[index + 1..].iter().find(|record| {
                    record["type"] == "assistant"
                        && record["message"]["content"]
                            .as_array()
                            .is_some_and(|blocks| {
                                blocks.iter().any(|block| block["type"] == "text")
                            })
                })
            {
                eprintln!(
                    "real image block and reply: {}",
                    reply["message"]["content"]
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no real image and assistant reply"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
fn image_confirmation_counts_attachments_across_turns() {
    let screen = include_str!("../../../tests/fixtures/claude-attach/image-path-second.screen.txt");
    assert!(image_visible(screen, 1));
    assert!(!image_visible(screen, 2));
}
