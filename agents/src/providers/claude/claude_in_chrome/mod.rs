use crate::providers::{mcp::McpServer, tool::*};

pub(crate) const SERVER: McpServer = McpServer {
    name: "claude-in-chrome",
    tools: &[
        ToolRule {
            name: "javascript_tool",
            input: Some((Pick::Field(Source::Input, "/text"), Format::Code("js"))),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "find",
            title: &[Pick::Field(Source::Input, "/query")],
            input: Some((Pick::Field(Source::Input, "/tabId"), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "computer",
            title: &[
                Pick::Field(Source::Input, "/action_summary"),
                Pick::Field(Source::Input, "/action"),
            ],
            input: Some((Pick::Field(Source::Input, "/tabId"), Format::Text)),
            ..DEFAULT_RULE
        },
    ],
};

#[cfg(test)]
mod tests {
    use serde_json::Value;
    #[test]
    fn chrome_javascript_formats_recorded_text() {
        let records: Vec<Value> =
            include_str!("../../../../tests/fixtures/claude-chrome-javascript.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let input = &records[0]["message"]["content"][0]["input"];
        let content = records[1]["message"]["content"][0]["content"].clone();
        let result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: Value::Null,
            error: false,
        };
        for name in [
            "claude-in-chrome javascript_tool",
            "mcp__claude-in-chrome__javascript_tool",
            "mcp__plugin_claude-in-chrome_claude-in-chrome__javascript_tool",
            "claude-in-chrome.javascript_tool",
        ] {
            let shown = super::presentation(
                name,
                &super::ToolData {
                    input,
                    result: Some(&result),
                },
            );
            let [crate::event::Section::Code { label, lang, text }] = shown.input.as_slice() else {
                panic!("JavaScript must be a code section: {:?}", shown.input);
            };
            assert_eq!(label, "text");
            assert_eq!(lang, "js");
            assert!(text.lines().count() > 1, "{text}");
            assert!(text.contains(" => "), "{text}");
            assert!(text.contains("'resource'"), "{text}");
            assert!(text.contains("' '"), "{text}");
            let Some(super::Body::Sections(output)) = shown.output else {
                panic!("missing output")
            };
            assert_eq!(output, super::mcp_content(&content, None));
            assert_eq!(output.len(), 2);

            let mut formatted = input.clone();
            formatted["text"] = text.clone().into();
            assert_eq!(
                super::presentation(
                    name,
                    &super::ToolData {
                        input: &formatted,
                        result: None
                    }
                )
                .input,
                shown.input,
                "Already formatted JavaScript stays unchanged"
            );
        }
    }

    #[test]
    fn chrome_find_and_computer_title_their_fields_and_show_the_rest() {
        let tab = vec![crate::event::Section::Prose {
            text: "665979853".into(),
            markdown: false,
        }];
        for (name, input, title, rest) in [
            (
                "mcp__claude-in-chrome__find",
                serde_json::json!({
                    "query": "Refresh link near the calendar/summary panel",
                    "tabId": 665979853
                }),
                "Refresh link near the calendar/summary panel",
                tab.clone(),
            ),
            (
                "mcp__claude-in-chrome__computer",
                serde_json::json!({
                    "action": "left_click",
                    "tabId": 665979853,
                    "ref": "ref_21",
                    "action_summary": "点搜索按钮执行航班搜索"
                }),
                "点搜索按钮执行航班搜索",
                tab.clone(),
            ),
            (
                "mcp__claude-in-chrome__computer",
                serde_json::json!({"action": "wait", "tabId": 665979853, "duration": 4}),
                "wait",
                tab.clone(),
            ),
            (
                "mcp__claude-in-chrome__computer",
                serde_json::json!({"action": "wait"}),
                "wait",
                Vec::new(),
            ),
        ] {
            let shown = super::presentation(
                name,
                &super::ToolData {
                    input: &input,
                    result: None,
                },
            );
            assert_eq!(shown.title.as_deref(), Some(title));
            assert_eq!(shown.input, rest);
        }
    }

    #[test]
    fn chrome_javascript_invalid_text_stays_code_and_absent_text_uses_default_input() {
        let record: Value = serde_json::from_str(
            include_str!("../../../../tests/fixtures/claude-chrome-javascript.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let mut input = record["message"]["content"][0]["input"].clone();
        let invalid = format!("{}(", input["text"].as_str().unwrap());
        input["text"] = invalid.clone().into();
        let shown = super::presentation(
            "mcp__claude-in-chrome__javascript_tool",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        assert_eq!(shown.input[0], super::code_section("text", "js", &invalid));
        input.as_object_mut().unwrap().remove("text");
        assert_eq!(
            super::presentation(
                "mcp__claude-in-chrome__javascript_tool",
                &super::ToolData {
                    input: &input,
                    result: None
                }
            )
            .input,
            super::raw_input(&input)
        );
    }
}
