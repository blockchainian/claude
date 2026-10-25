use super::Language;

fn recorded_command() -> String {
    let record: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/claude-rust-heredoc.jsonl"
    ))
    .unwrap();
    record["message"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["name"] == "Bash")
        .unwrap()["input"]["command"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn recorded_body() -> String {
    let command = recorded_command();
    let script = crate::shell_commands::scripts(&command).remove(0);
    command[script.range].to_owned()
}

fn format(text: &str) -> Option<String> {
    Language::from_extension("rs").and_then(|language| language.format(text))
}

#[test]
fn rust_extensions_and_interpreters() {
    let language = Language::from_extension("rs");
    assert!(language.is_some());
    for extension in ["rs", "RS", "Rs", "rS"] {
        assert_eq!(Language::from_extension(extension), language);
    }
    for interpreter in ["rust", "rustc", "/usr/bin/rustc", "rs"] {
        assert_eq!(Language::from_interpreter(interpreter), None);
    }
}

#[test]
fn rust_formats_recorded_body() {
    let body = recorded_body();
    let formatted = format(&body).unwrap();
    assert_ne!(formatted, body);
    assert!(formatted.contains("fn main() {\n"));
    assert!(formatted.contains("        path: std::path::Path::new(\"a.js\"),\n"));
    assert_eq!(format(&formatted).as_deref(), Some(formatted.as_str()));
}

#[test]
fn rust_keeps_ordinary_comments() {
    let body = recorded_body();
    for comment in [
        "// Keep this comment.\n",
        "/* Keep this comment. */\n",
        "//// Not a doc comment.\n",
    ] {
        let text = format!("{comment}{body}");
        assert_eq!(format(&text), None);
        assert_eq!(format(&text).as_deref().unwrap_or(&text), text);
    }
}

#[test]
fn rust_preserves_doc_comments() {
    let body = recorded_body();
    for comment in [
        "/// Keep this documentation.\n",
        "//! Keep this documentation.\n",
    ] {
        let text = format!("{comment}{body}");
        let formatted = format(&text).unwrap();
        assert_ne!(formatted, text);
        assert!(formatted.starts_with(comment));
        assert!(formatted.contains("fn main() {\n"));
    }
}

#[test]
fn rust_keeps_invalid_source() {
    let text = format!("{}\nfn broken(\n", recorded_body());
    assert_eq!(format(&text), None);
    assert_eq!(format(&text).as_deref().unwrap_or(&text), text);
}
