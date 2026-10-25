use super::*;

// Source excerpts from the reported heredoc display.
const SAMPLES: [(&str, &str); 4] = [
    (
        "js",
        "const users = [{ name: 'ada', age: 36 }];\nconst byAge = (a, b) => a.age - b.age;\nfor (const u of users.sort(byAge)) {}",
    ),
    (
        "ts",
        "interface User { name: string; age: number }\nexport function oldest(users:User[]):User|undefined{return users.reduce<User|undefined>((best,u)=>!best||u.age>best.age?u:best,undefined)}",
    ),
    (
        "python",
        "users = []\ndef oldest(lst):\n    return max(lst)\nfor u in sorted(users):\n    print(oldest(users))",
    ),
    (
        "rs",
        "fn oldest(users:&[User])->Option<&User>{users.iter().max_by_key(|u|u.age)}\nfn main() { let users = vec![]; if let Some(u) = oldest(&users) { println!(\"{}\", u); } }",
    ),
];

fn token_style(lines: &[Line<'_>], row: usize, token: &str) -> Style {
    let line = &lines[row];
    let source: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    let offset = source.find(token).unwrap();
    let mut start = 0;
    for span in &line.spans {
        let end = start + span.content.len();
        if start <= offset && offset < end {
            return span.style;
        }
        start = end;
    }
    panic!("missing token {token}");
}

fn assert_token(lines: &[Line<'_>], row: usize, token: &str, color: Color, modifiers: Modifier) {
    let style = token_style(lines, row, token);
    assert_eq!(style.fg, Some(color), "{token}");
    assert_eq!(style.add_modifier, modifiers, "{token}");
}

#[test]
fn scoped_identifiers_use_variable_property_and_function_colors() {
    for light in [false, true] {
        use_theme(light, None).unwrap();
        let normal = Color::Indexed(if light { 30 } else { 23 });
        let binding = Color::Indexed(if light { 124 } else { 24 });
        let function = if light {
            Color::Indexed(18)
        } else {
            Color::Rgb(0, 145, 102)
        };
        let constant = if light {
            Color::Indexed(238)
        } else {
            Color::Rgb(0, 101, 71)
        };
        let definition = Modifier::BOLD | Modifier::ITALIC;
        let js = code_block(SAMPLES[0].1, "js");
        for (row, token) in [(0, "users"), (2, "u of")] {
            assert_token(&js, row, token, binding, definition);
        }
        assert_token(&js, 1, "byAge", function, definition);
        for token in ["ada", "36"] {
            assert_token(&js, 0, token, constant, Modifier::ITALIC);
        }
        let ts = code_block(SAMPLES[1].1, "ts");
        for property in ["name", "age", "string", "number"] {
            assert_token(&ts, 0, property, normal, Modifier::empty());
        }
        assert_token(&ts, 1, "users", binding, definition);
        assert_token(&ts, 1, "User", normal, Modifier::empty());
        assert_token(&ts, 1, "reduce", function, Modifier::ITALIC);
        assert_token(&ts, 0, "User", binding, definition);
        let python = code_block(SAMPLES[2].1, "python");
        assert_token(&python, 1, "oldest", function, definition);
        assert_token(&python, 1, "lst", binding, definition);
        assert_token(&python, 3, "u in", binding, definition);
        assert_token(&python, 3, "users", normal, Modifier::empty());
        for (row, token) in [(2, "max"), (3, "sorted"), (4, "print"), (4, "oldest")] {
            assert_token(&python, row, token, function, Modifier::ITALIC);
        }
        let rust = code_block(SAMPLES[3].1, "rs");
        assert_token(&rust, 0, "users", binding, definition);
        for token in ["User", "Option"] {
            assert!(
                !token_style(&rust, 0, token)
                    .add_modifier
                    .contains(Modifier::ITALIC),
                "{token}"
            );
        }
        for token in ["vec!", "println!"] {
            assert_token(&rust, 1, token, function, Modifier::ITALIC);
        }
        // Assignment targets have no distinct grammar scope and keep the current style.
        assert_token(&python, 0, "users", normal, Modifier::empty());
        assert_token(&rust, 1, "users", normal, Modifier::empty());
    }
}

#[test]
fn support_scopes_keep_plain_text_style() {
    // These support scopes occur in the bundled grammars; only Python builtins
    // and Rust macros receive the function-call style.
    for light in [false, true] {
        let theme = bundled_theme(light);
        let highlighter = Highlighter::new(&theme);
        for scope in [
            "support",
            "support.constant",
            "support.variable",
            "support.function",
            "support.type",
            "support.class",
        ] {
            let stack = [Scope::new("source.js").unwrap(), Scope::new(scope).unwrap()];
            let style = convert(highlighter.style_for_stack(&stack));
            assert_eq!(
                style.fg,
                Some(Color::Indexed(if light { 30 } else { 23 })),
                "{scope}"
            );
            assert_eq!(style.add_modifier, Modifier::empty(), "{scope}");
        }
    }
}

#[test]
fn identifier_colors_render_in_both_backgrounds() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };
    let mut actual = String::new();
    for light in [false, true] {
        use_theme(light, None).unwrap();
        actual.push_str(if light { "light\n" } else { "dark\n" });
        for (language, source) in SAMPLES {
            actual.push_str(&format!("{language}\n"));
            let lines = code_block(source, language);
            let mut buffer = Buffer::empty(Rect::new(
                0,
                0,
                source.lines().map(str::len).max().unwrap() as u16,
                lines.len() as u16,
            ));
            Paragraph::new(lines).render(buffer.area, &mut buffer);
            for row in 0..buffer.area.height {
                let mut previous = None;
                for column in 0..source.lines().nth(row as usize).unwrap().len() as u16 {
                    let cell = &buffer[(column, row)];
                    let style = (cell.fg, cell.modifier);
                    if previous != Some(style) {
                        actual.push_str(&format!(" [{:?} {:?}]", cell.fg, cell.modifier));
                        previous = Some(style);
                    }
                    actual.push_str(cell.symbol());
                }
                actual.push('\n');
            }
        }
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/highlight_scopes.txt");
    if std::env::var_os("UPDATE_HIGHLIGHT_SCOPE_SNAPSHOT").is_some() {
        std::fs::write(&path, &actual).unwrap();
    }
    assert_eq!(actual, std::fs::read_to_string(path).unwrap());
}
