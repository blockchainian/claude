use crate::code_format::Language;

fn shown_shell(command: &str) -> String {
    let input = serde_json::json!({"command": command});
    let sections = crate::providers::tool::shell_input(&crate::providers::tool::ToolData {
        input: &input,
        result: None,
    })
    .unwrap();
    let [crate::event::Section::Shell { text, prefix: 0 }] = sections.as_slice() else {
        panic!("expected shell section: {sections:?}");
    };
    text.clone()
}

fn recorded_command() -> String {
    let record: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/claude-python-heredoc.jsonl"
    ))
    .unwrap();
    record["message"]["content"][0]["input"]["command"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn python_heredoc_formats_recorded_bash_call() {
    let command = recorded_command();
    let expected = concat!(
        "python3 <<'EOF'\n",
        "items = [\n",
        "    {\"name\": \"apple\", \"price\": 3},\n",
        "    {\"name\": \"pear\", \"price\": 5},\n",
        "    {\"name\": \"fig\", \"price\": 8},\n",
        "]\n\n\n",
        "def total(lst):\n",
        "    return sum(i[\"price\"] for i in lst)\n\n\n",
        "cheap = [i[\"name\"] for i in items if i[\"price\"] < 6]\n",
        "print({\"total\": total(items), \"cheap\": cheap})\n",
        "EOF",
    );
    assert_eq!(shown_shell(&command), expected);
    assert_eq!(shown_shell(expected), expected);
}

#[test]
fn python_heredoc_preserves_syntax_errors() {
    let command = recorded_command();
    let invalid = command.replace("def total(lst):", "def total(lst:");
    assert_ne!(invalid, command);
    assert_eq!(shown_shell(&invalid), invalid);
    assert_eq!(
        Language::from_extension("py").and_then(|l| l.format("def total(lst:\n")),
        None
    );
}

#[test]
fn python_language_names_resolve() {
    for extension in ["py", "pyi", "PY", "PyI"] {
        assert_eq!(
            Language::from_extension(extension).map(|language| format!("{language:?}")),
            Some("Python".to_owned()),
            "extension: {extension}",
        );
    }
    for interpreter in [
        "python",
        "python3",
        "python3.9",
        "python3.14",
        "python3.123",
        "/usr/bin/python",
        "/usr/bin/python3.14",
        "./python3",
    ] {
        assert_eq!(
            Language::from_interpreter(interpreter).map(|language| format!("{language:?}")),
            Some("Python".to_owned()),
            "interpreter: {interpreter}",
        );
    }
    for interpreter in [
        "python2",
        "python3.",
        "python3.x",
        "python3.14.1",
        "python3.14-config",
        "python30",
        "Python3",
        "ipython",
        "py",
        "",
    ] {
        assert_eq!(
            Language::from_interpreter(interpreter),
            None,
            "{interpreter}"
        );
    }
}
