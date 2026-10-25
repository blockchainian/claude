use crate::providers::{mcp::McpServer, tool::*};

pub(crate) const SERVER: McpServer = McpServer {
    name: "claude-in-chrome",
    tools: &[
        ToolRule {
            name: "browser_batch",
            input: Some((Pick::With(batch_input), Format::Table)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "navigate",
            title: &[Pick::Field(Source::Input, "/url")],
            ..DEFAULT_RULE
        },
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
            name: "tabs_context_mcp",
            title: &[Pick::Field(Source::Input, "/createIfEmpty")],
            output: Some((Pick::With(tabs_context_output), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "tabs_close_mcp",
            title: &[Pick::Field(Source::Input, "/tabId")],
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

fn tabs_context_output(data: &ToolData<'_>) -> Option<Body> {
    use crate::event::Section;

    let result = data.result.filter(|result| !result.error)?;
    if result.content.as_array()?.len() != 3 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(content_text(&result.content, 0)?).ok()?;
    let tabs = value.get("availableTabs")?;
    tabs.as_array()?;
    Some(Body::Sections(vec![
        Section::Markdown {
            label: "output".into(),
            text: markdown_table(tabs, "availableTabs")?,
        },
        Section::Text {
            label: "output".into(),
            text: content_text(&result.content, 1)?.to_owned(),
        },
        system_reminder_section(content_text(&result.content, 2)?)?,
    ]))
}

fn batch_input(data: &ToolData<'_>) -> Option<Vec<crate::event::Section>> {
    use crate::event::Section;

    let actions = data.input.get("actions")?.as_array()?;
    let rows = actions
        .iter()
        .map(|action| {
            let name = action.get("name")?.as_str()?;
            let input = action.get("input")?;
            let shown = presentation(
                &format!("mcp__claude-in-chrome__{name}"),
                &ToolData {
                    input,
                    result: None,
                },
            );
            let cell = if let Some(text) = shown.input.iter().find_map(|section| match section {
                Section::Code { text, .. } => Some(text),
                _ => None,
            }) {
                inline_code_cell(text)
            } else if let Some(title) = shown.title {
                table_cell(&title)
            } else {
                table_cell(&input.to_string())
            };
            Some(vec![table_cell(name), cell])
        })
        .collect::<Option<Vec<_>>>()?;
    Some(vec![Section::Markdown {
        label: "input".into(),
        text: markdown_table_rows(&["Action", "Input"], rows),
    }])
}

fn inline_code_cell(text: &str) -> String {
    let text = text.replace(['\r', '\n'], " ");
    let longest = text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
    let delimiter = "`".repeat(longest + 1);
    let padding = if text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && text.chars().any(|ch| ch != ' '))
    {
        " "
    } else {
        ""
    };
    format!(
        "{delimiter}{padding}{}{padding}{delimiter}",
        text.replace('|', "\\|")
    )
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    fn tabs_context_fixture() -> (Value, super::ToolResult) {
        let records: Vec<Value> =
            include_str!("../../../../tests/fixtures/claude-chrome-tabs-context.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let call = &records[0]["message"]["content"][0];
        let result = &records[1]["message"]["content"][0];
        assert_eq!(call["id"], result["tool_use_id"]);
        (
            call["input"].clone(),
            super::ToolResult {
                source: None,
                content: result["content"].clone(),
                structured: Value::Null,
                error: false,
            },
        )
    }

    #[test]
    fn tabs_context_output_shows_recorded_table_text_and_reminder() {
        let (input, result) = tabs_context_fixture();
        let super::Body::Sections(sections) = super::output(
            super::rule("mcp__claude-in-chrome__tabs_context_mcp").output,
            &super::ToolData {
                input: &input,
                result: Some(&result),
            },
        ) else {
            panic!("expected sections")
        };
        let [
            crate::event::Section::Markdown { text: table, .. },
            crate::event::Section::Text { text, .. },
            crate::event::Section::Error { text: reminder },
        ] = sections.as_slice()
        else {
            panic!("expected table, text and reminder: {sections:?}")
        };
        assert_eq!(
            table,
            "| tabId | title | url |\n| --- | --- | --- |\n| 1001 | New Tab | chrome\\:\\/\\/newtab\\/ |"
        );
        assert_eq!(text, result.content[1]["text"].as_str().unwrap());
        assert_eq!(
            format!("<system-reminder>{reminder}</system-reminder>"),
            result.content[2]["text"].as_str().unwrap()
        );
        assert!(!reminder.contains("<system-reminder>"));
        assert!(!reminder.contains("</system-reminder>"));
    }

    #[test]
    fn tabs_context_output_preserves_unrecognized_results() {
        let (input, original) = tabs_context_fixture();
        let mut cases = Vec::new();
        let mut missing = original.content.clone();
        missing.as_array_mut().unwrap().pop();
        cases.push(missing);
        let mut extra = original.content.clone();
        extra
            .as_array_mut()
            .unwrap()
            .push(original.content[1].clone());
        cases.push(extra);
        for (index, replacement) in [
            (0, serde_json::json!({"type":"text", "text":"invalid json"})),
            (0, serde_json::json!({"type":"text", "text":"{}"})),
            (1, serde_json::json!({"type":"image"})),
            (
                2,
                serde_json::json!({"type":"text", "text":"<system-reminder>incomplete"}),
            ),
        ] {
            let mut content = original.content.clone();
            content[index] = replacement;
            cases.push(content);
        }
        for content in cases {
            let result = super::ToolResult {
                content,
                source: None,
                structured: Value::Null,
                error: false,
            };
            let super::Body::Sections(sections) = super::output(
                super::rule("mcp__claude-in-chrome__tabs_context_mcp").output,
                &super::ToolData {
                    input: &input,
                    result: Some(&result),
                },
            ) else {
                panic!("expected raw sections")
            };
            assert_eq!(sections, super::raw_output(&result));
        }
        let result = super::ToolResult {
            error: true,
            ..original
        };
        let super::Body::Sections(sections) = super::output(
            super::rule("mcp__claude-in-chrome__tabs_context_mcp").output,
            &super::ToolData {
                input: &input,
                result: Some(&result),
            },
        ) else {
            panic!("expected raw error")
        };
        assert_eq!(sections, super::raw_output(&result));
    }

    fn batch_table(input: &Value, result: Option<&super::ToolResult>) -> String {
        let shown = super::presentation(
            "mcp__claude-in-chrome__browser_batch",
            &super::ToolData { input, result },
        );
        let [crate::event::Section::Markdown { text, .. }] = shown.input.as_slice() else {
            panic!("expected a Markdown table: {:?}", shown.input);
        };
        text.clone()
    }

    #[test]
    fn chrome_browser_batch_formats_recorded_actions() {
        let records: Vec<Value> =
            include_str!("../../../../tests/fixtures/claude-chrome-browser-batch.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let input = &records[0]["message"]["content"][0]["input"];
        let result = super::ToolResult {
            source: None,
            content: records[1]["message"]["content"][0]["content"].clone(),
            structured: Value::Null,
            error: false,
        };
        let table = batch_table(input, Some(&result));
        assert_eq!(table.lines().next(), Some("| Action | Input |"));
        let actions = input["actions"].as_array().unwrap();
        let url = actions[0]["input"]["url"].as_str().unwrap();
        assert!(table.contains(&format!("| navigate | {} |", super::table_cell(url))));
        assert!(table.contains("| computer | wait |"));
        let shown = super::presentation(
            "mcp__claude-in-chrome__javascript_tool",
            &super::ToolData {
                input: &actions[2]["input"],
                result: None,
            },
        );
        let crate::event::Section::Code { text, .. } = &shown.input[0] else {
            panic!("missing code")
        };
        let code: Vec<_> =
            pulldown_cmark::Parser::new_ext(&table, pulldown_cmark::Options::ENABLE_TABLES)
                .filter_map(|event| match event {
                    pulldown_cmark::Event::Code(text) => Some(text.into_string()),
                    _ => None,
                })
                .collect();
        assert_eq!(code, [text.replace(['\r', '\n'], " ")]);
        assert_eq!(
            super::presentation(
                "mcp__claude-in-chrome__navigate",
                &super::ToolData {
                    input: &actions[0]["input"],
                    result: None
                }
            )
            .title
            .as_deref(),
            Some(url)
        );
    }

    #[test]
    fn chrome_browser_batch_code_keeps_pipes_and_backticks_in_one_cell() {
        let record: Value = serde_json::from_str(
            include_str!("../../../../tests/fixtures/claude-chrome-browser-batch.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let mut input = record["message"]["content"][0]["input"].clone();
        let action = &mut input["actions"][2];
        let code = "`a|b` || ``c`` | d\\|e";
        action["input"]["text"] = code.into();
        input["actions"] = serde_json::json!([action.clone()]);
        let table = batch_table(&input, None);
        let events: Vec<_> =
            pulldown_cmark::Parser::new_ext(&table, pulldown_cmark::Options::ENABLE_TABLES)
                .collect();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    pulldown_cmark::Event::Start(pulldown_cmark::Tag::TableCell)
                ))
                .count(),
            4
        );
        let codes: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                pulldown_cmark::Event::Code(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(codes, [code]);
    }

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
            text: "1001".into(),
            markdown: false,
        }];
        for (name, input, title, rest) in [
            (
                "mcp__claude-in-chrome__find",
                serde_json::json!({
                    "query": "Refresh link near the calendar/summary panel",
                    "tabId": 1001
                }),
                "Refresh link near the calendar/summary panel",
                tab.clone(),
            ),
            (
                "mcp__claude-in-chrome__computer",
                serde_json::json!({
                    "action": "left_click",
                    "tabId": 1001,
                    "ref": "ref_21",
                    "action_summary": "点搜索按钮执行航班搜索"
                }),
                "点搜索按钮执行航班搜索",
                tab.clone(),
            ),
            (
                "mcp__claude-in-chrome__computer",
                serde_json::json!({"action": "wait", "tabId": 1001, "duration": 4}),
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
    fn chrome_tabs_context_titles_a_true_flag_by_its_name() {
        for (flag, title) in [(true, Some("createIfEmpty")), (false, None)] {
            let input = serde_json::json!({"createIfEmpty": flag});
            let shown = super::presentation(
                "mcp__claude-in-chrome__tabs_context_mcp",
                &super::ToolData {
                    input: &input,
                    result: None,
                },
            );
            assert_eq!(shown.title.as_deref(), title);
            assert_eq!(shown.input, Vec::new());
        }
    }

    #[test]
    fn chrome_tabs_close_titles_recorded_tab_id_without_input() {
        let records: Vec<Value> =
            include_str!("../../../../tests/fixtures/claude-chrome-tabs-close.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let call = &records[0]["message"]["content"][0];
        let reply = &records[1]["message"]["content"][0];
        assert_eq!(call["id"], reply["tool_use_id"]);
        let result = super::ToolResult {
            source: None,
            content: reply["content"].clone(),
            structured: Value::Null,
            error: false,
        };
        let shown = super::presentation(
            call["name"].as_str().unwrap(),
            &super::ToolData {
                input: &call["input"],
                result: Some(&result),
            },
        );
        assert_eq!(shown.title.as_deref(), Some("1002"));
        assert!(
            shown.input.is_empty(),
            "unexpected input: {:?}",
            shown.input
        );
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
