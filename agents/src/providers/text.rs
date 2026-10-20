//! Text bytes safe to display, retaining message whitespace.

/// Keep printable Unicode and message line breaks and tabs; discard terminal controls.
pub(crate) fn printable(text: &str) -> String {
    struct Text(String);
    impl vte::Perform for Text {
        fn print(&mut self, c: char) {
            if !c.is_control() {
                self.0.push(c);
            }
        }
        fn execute(&mut self, byte: u8) {
            if matches!(byte, b'\n' | b'\t') {
                self.0.push(char::from(byte));
            }
        }
    }
    let mut output = Text(String::with_capacity(text.len()));
    vte::Parser::new().advance(&mut output, text.as_bytes());
    output.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_terminal_output_has_no_escape_payloads() {
        let raw = include_str!("../../tests/fixtures/claude-attach/attach.raw");
        let text = printable(raw);
        assert!(raw.contains("\x1b[?2026"));
        assert!(!text.contains("[?2026"));
        assert!(
            !text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        );
        assert!(text.contains("Attaching…"));
    }
}
