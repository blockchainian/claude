use crate::images::ImageAttachment;
use crate::line_edit::{LineEdit, Placeholder};
use crate::state::StoredDraftInput;
use std::path::PathBuf;

impl Placeholder for ImageAttachment {
    fn label(ordinal: usize) -> String {
        format!("[Image #{ordinal}]")
    }
}

pub(super) type Composer = LineEdit<ImageAttachment>;

pub(super) trait ComposerImages {
    fn image_paths(&self) -> Vec<PathBuf>;
    fn preserve_images(&self);
    fn prompt_text(&self) -> String;
    fn stored_input(&self) -> StoredDraftInput;
    fn restore_input(input: &StoredDraftInput) -> Self;
}

impl ComposerImages for Composer {
    fn image_paths(&self) -> Vec<PathBuf> {
        self.placeholders()
            .map(|image| image.path().to_owned())
            .collect()
    }

    fn preserve_images(&self) {
        for image in self.placeholders() {
            image.preserve();
        }
    }

    fn prompt_text(&self) -> String {
        self.text_without_placeholders()
    }

    fn stored_input(&self) -> StoredDraftInput {
        self.preserve_images();
        StoredDraftInput {
            text: self.text().to_owned(),
            images: self.image_paths(),
        }
    }

    fn restore_input(input: &StoredDraftInput) -> Self {
        Self::from_labeled_text(
            &input.text,
            input
                .images
                .iter()
                .map(|path| ImageAttachment::from_path(path).ok()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_draft_images_keep_paths_and_restore_text_positions() {
        let mut edit = Composer::default();
        edit.insert("before ");
        edit.insert_placeholder(
            ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap(),
        );
        edit.insert(" middle ");
        edit.insert_placeholder(
            ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap(),
        );
        edit.insert(" after");
        let stored = edit.stored_input();
        let paths = stored.images.clone();
        drop(edit);
        assert!(paths.iter().all(|path| path.is_file()));
        let restored = Composer::restore_input(&stored);
        assert_eq!(restored.text(), stored.text);
        assert_eq!(restored.image_paths(), paths);
        assert_eq!(restored.prompt_text(), "before  middle  after");
        drop(restored);
        std::fs::remove_file(&paths[0]).unwrap();
        let restored = Composer::restore_input(&stored);
        assert_eq!(restored.text(), "before  middle [Image #1] after");
        assert_eq!(restored.image_paths(), vec![paths[1].clone()]);
        drop(restored);
        std::fs::remove_file(&paths[1]).unwrap();
    }
}

#[cfg(test)]
mod ordering_tests {
    use super::*;
    #[test]
    fn stored_images_restore_after_inserting_an_attachment_before_another() {
        let attach = || {
            ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap()
        };
        let mut editor = Composer::default();
        editor.insert("suffix");
        editor.insert_placeholder(attach());
        editor.move_caret(crate::line_edit::Motion::TextStart);
        editor.insert_placeholder(attach());
        let stored = editor.stored_input();
        let restored = Composer::restore_input(&stored);
        assert_eq!(restored.image_paths(), editor.image_paths());
        assert_eq!(restored.text(), editor.text());
        for path in stored.images {
            std::fs::remove_file(path).unwrap();
        }
    }
}
