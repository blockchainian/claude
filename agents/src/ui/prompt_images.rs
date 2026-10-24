use super::*;

impl PromptBox {
    pub(in crate::ui) fn begin_send(&mut self, text: &str) {
        let mut sent = self.editor.clone();
        if sent.image_paths().is_empty() {
            sent.set_text(text);
        }
        sent.clear_selection();
        self.pending_editor = Some(sent);
        self.editor.clear();
    }

    pub(in crate::ui) fn finish_send(&mut self, success: bool) {
        let Some(mut sent) = self.pending_editor.take() else {
            return;
        };
        if success {
            sent.preserve_images();
        } else {
            let draft = std::mem::take(&mut self.editor);
            if !draft.is_empty() {
                sent.end();
                sent.insert("\n");
                sent.append(draft);
            }
            self.editor = sent;
        }
    }
}
