//! Finds attached images and raster files carried by a piece of text.
use std::{collections::HashSet, path::Path};

use crate::images::{Image, ImageSource};

/// The images `text` carries: attached ones by label, then image files by path.
pub(super) fn text_images(
    text: &str,
    cwd: &Path,
    attached: &[Image],
    seen: &mut HashSet<ImageSource>,
) -> Vec<Image> {
    let attached: Vec<_> = attached
        .iter()
        .map(|image| {
            let mut image = image.clone();
            if let ImageSource::File { path, .. } = &mut image.source
                && let Ok(canonical) = path.canonicalize()
            {
                *path = canonical;
            }
            image
        })
        .collect();
    let attachments = &attached;
    let mut attached: Vec<_> = attached
        .iter()
        .filter_map(|image| text.find(&image.label).map(|at| (at, image)))
        .collect();
    attached.sort_by_key(|(at, _)| *at);
    let mut images: Vec<_> = attached
        .into_iter()
        .filter(|(_, image)| seen.insert(image.source.clone()))
        .map(|(_, image)| image.clone())
        .collect();
    for range in super::paths::text_paths(text, cwd) {
        let label = &text[range];
        let path = if let Some(rest) = label.strip_prefix("~/") {
            let Some(home) = std::env::var_os("HOME") else {
                continue;
            };
            Path::new(&home).join(rest)
        } else {
            cwd.join(label)
        };
        let source = path
            .canonicalize()
            .ok()
            .and_then(|path| {
                attachments
                    .iter()
                    .find(|image| {
                        matches!(image.source, ImageSource::Record { .. })
                            && cwd.join(&image.label).canonicalize().ok().as_ref() == Some(&path)
                    })
                    .map(|image| image.source.clone())
            })
            .or_else(|| image_file(&path));
        if let Some(source) = source
            && seen.insert(source.clone())
        {
            images.push(Image {
                label: label.into(),
                source,
            });
        }
    }
    images
}

fn image_file(path: &Path) -> Option<ImageSource> {
    image::ImageFormat::from_path(path).ok()?;
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some(ImageSource::File {
        path: path.canonicalize().ok()?,
        len: metadata.len(),
        modified: metadata.modified().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::SourceRecord;
    use std::path::PathBuf;

    fn prompt_images() -> (String, Vec<Image>) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-prompt-images.jsonl");
        let transcript = std::fs::read_to_string(&path).unwrap();
        let mut offset = 0;
        for line in transcript.split_inclusive('\n') {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            if value["imagePasteIds"] == serde_json::json!([8, 9]) {
                let content = value["message"]["content"].as_array().unwrap();
                let images = value["imagePasteIds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(
                        content
                            .iter()
                            .enumerate()
                            .filter(|(_, block)| block["type"] == "image"),
                    )
                    .map(|(id, (index, _))| Image {
                        label: format!("[Image #{}]", id.as_u64().unwrap()),
                        source: ImageSource::Record {
                            record: SourceRecord {
                                path: path.clone(),
                                offset,
                            },
                            pointer: format!("/message/content/{index}/source/data"),
                        },
                    })
                    .collect();
                return (content[0]["text"].as_str().unwrap().into(), images);
            }
            offset += line.len() as u64;
        }
        panic!("the prompt image fixture must contain images 8 and 9");
    }

    fn write_image(path: &Path) {
        std::fs::write(path, crate::images::fixture_png()).unwrap();
    }

    #[test]
    fn f4_symlink_paths_are_one_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        let alias = dir.path().join("alias.png");
        write_image(&path);
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let text = format!("{} {}", path.display(), alias.display());
        assert_eq!(
            text_images(&text, dir.path(), &[], &mut HashSet::new()).len(),
            1
        );
    }

    #[test]
    fn attached_images_follow_label_order_and_are_seen_across_texts() {
        let (text, mut attached) = prompt_images();
        let expected = attached.clone();
        attached.reverse();
        let mut seen = HashSet::new();
        assert_eq!(
            text_images(&text, Path::new("/"), &attached, &mut seen),
            expected
        );
        assert!(text_images(&text, Path::new("/"), &attached, &mut seen).is_empty());
        for image in expected {
            assert!(image.source.bytes().unwrap().starts_with(b"\x89PNG\r\n"));
        }
    }

    #[test]
    fn attached_images_precede_paths_and_share_source_deduplication() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        write_image(&path);
        let metadata = path.metadata().unwrap();
        let image = Image {
            label: "[Image #8]".into(),
            source: ImageSource::File {
                path: path.canonicalize().unwrap(),
                len: metadata.len(),
                modified: metadata.modified().unwrap(),
            },
        };
        let text = format!("{} [Image #8] [Image #8]", path.display());
        assert_eq!(
            text_images(
                &text,
                dir.path(),
                std::slice::from_ref(&image),
                &mut HashSet::new()
            ),
            [image]
        );
    }

    #[test]
    fn paths_keep_their_labels_and_capture_file_metadata() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("screens")).unwrap();
        let path = dir.path().join("screens/screen.PNG");
        write_image(&path);
        let text = format!("`screens/screen.PNG`，再看 {}:12", path.display());
        let images = text_images(&text, dir.path(), &[], &mut HashSet::new());
        let metadata = path.metadata().unwrap();
        assert_eq!(
            images,
            [Image {
                label: "screens/screen.PNG".into(),
                source: ImageSource::File {
                    path: path.canonicalize().unwrap(),
                    len: metadata.len(),
                    modified: metadata.modified().unwrap(),
                },
            }]
        );
        assert_eq!(
            images[0].source.bytes().unwrap(),
            crate::images::fixture_png()
        );
    }

    #[test]
    fn only_existing_raster_files_are_selected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("directory.png")).unwrap();
        let mut labels = Vec::new();
        for extension in [
            "png", "apng", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "avif", "ico", "tga",
        ] {
            let path = dir.path().join(format!("image.{extension}"));
            write_image(&path);
            labels.push(path.to_str().unwrap().to_owned());
        }
        for name in ["image.svg", "image.txt", "image"] {
            write_image(&dir.path().join(name));
        }
        let text = format!(
            "{} {}/directory.png {}/missing.png {}/image.svg {}/image.txt {}/image",
            labels.join(" "),
            dir.path().display(),
            dir.path().display(),
            dir.path().display(),
            dir.path().display(),
            dir.path().display()
        );
        let images = text_images(&text, dir.path(), &[], &mut HashSet::new());
        assert_eq!(
            images.iter().map(|image| &image.label).collect::<Vec<_>>(),
            labels.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn unmentioned_attachments_are_left_for_the_caller() {
        let (_, attached) = prompt_images();
        let mut seen = HashSet::new();
        assert!(text_images("no image labels", Path::new("/"), &attached, &mut seen).is_empty());
        assert!(seen.is_empty());
        assert_eq!(
            text_images(&attached[1].label, Path::new("/"), &attached, &mut seen),
            [attached[1].clone()]
        );
        assert!(!seen.contains(&attached[0].source));
    }

    #[test]
    fn attached_images_are_selected_before_distinct_text_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("screen.png");
        write_image(&path);
        let (_, attached) = prompt_images();
        let text = format!("{} {}", path.display(), attached[1].label);
        let images = text_images(&text, dir.path(), &attached, &mut HashSet::new());
        assert_eq!(images.len(), 2);
        assert_eq!(images[0], attached[1]);
        assert_eq!(images[1].label, path.to_str().unwrap());
    }

    #[test]
    fn home_paths_keep_their_labels() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("screen.png");
        write_image(&path);
        let rest = format!(
            "{}{}",
            "../".repeat(home.components().count() - 1),
            path.strip_prefix("/").unwrap().display()
        );
        let label = format!("~/{rest}");
        let images = text_images(&label, Path::new("/"), &[], &mut HashSet::new());
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].label, label);
        assert!(
            matches!(&images[0].source, ImageSource::File { path: found, .. } if found == &path.canonicalize().unwrap())
        );
    }

    #[test]
    fn modified_files_of_the_same_length_have_distinct_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("screen.png");
        write_image(&path);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        let time =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        file.set_modified(time).unwrap();
        let mut seen = HashSet::new();
        let first = text_images(path.to_str().unwrap(), dir.path(), &[], &mut seen);
        file.set_modified(time + std::time::Duration::from_secs(1))
            .unwrap();
        let second = text_images(path.to_str().unwrap(), dir.path(), &[], &mut seen);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0].source, second[0].source);
    }

    #[test]
    fn overwritten_paths_have_distinct_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("screen.png");
        write_image(&path);
        let text = path.to_str().unwrap();
        let mut seen = HashSet::new();
        let first = text_images(text, dir.path(), &[], &mut seen);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .set_len(path.metadata().unwrap().len() + 1)
            .unwrap();
        let second = text_images(text, dir.path(), &[], &mut seen);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0].source, second[0].source);
    }
}
