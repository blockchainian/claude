use crate::event::Section;
use crate::providers::tool::*;
use serde_json::Value;

pub(super) const RULES: &[ToolRule] = &[
    ToolRule {
        name: "attach",
        title: &[Pick::Field(Source::Input, "/thread")],
        output: Some((Pick::With(thread_id_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "review",
        title: &[Pick::Field(Source::Input, "/name")],
        output: Some((Pick::With(thread_id_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "start",
        title: &[Pick::Field(Source::Input, "/name")],
        input: Some((Pick::Field(Source::Input, "/prompt"), Format::Markdown)),
        output: Some((Pick::With(thread_id_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "send",
        output: Some((Pick::With(note_output), Format::Text)),
        title: &[Pick::Field(Source::Input, "/threadId")],
        input: Some((Pick::Field(Source::Input, "/prompt"), Format::Markdown)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "reply",
        title: &[Pick::Field(Source::Input, "/threadId")],
        input: Some((Pick::Field(Source::Input, "/ask"), Format::Text)),
        output: Some((Pick::With(reply_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "list",
        input: Some((Pick::Field(Source::Input, "/state"), Format::Table)),
        output: Some((Pick::With(thread_table), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "interrupt",
        output: Some((Pick::With(thread_id_output), Format::Text)),
        title: &[Pick::Field(Source::Input, "/threadId")],
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "detach",
        title: &[
            Pick::Field(Source::Result, "/name"),
            Pick::Field(Source::Input, "/threadId"),
        ],
        output: Some((Pick::With(thread_id_output), Format::Text)),
        ..DEFAULT_RULE
    },
];

fn thread_id_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result.filter(|result| !result.error)?;
    let mut sections = picked_sections(
        result.structured.get("threadId")?,
        "/threadId",
        Format::Text,
        true,
    );
    sections.extend(attention_sections(data));
    Some(Body::Sections(sections))
}

/// The note codex-manager attaches to a sent prompt, such as a steer into a running turn.
fn note_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result.filter(|result| !result.error)?;
    let mut sections: Vec<Section> = text(&result.structured, "note")
        .map(|note| Section::Prose {
            text: note.to_owned(),
            markdown: false,
        })
        .into_iter()
        .collect();
    sections.extend(attention_sections(data));
    Some(Body::Sections(sections))
}

fn thread_table(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result.filter(|result| !result.error)?;
    let threads: Vec<Value> = result.structured["threads"]
        .as_array()?
        .iter()
        .map(|thread| serde_json::json!({ "Name": thread["name"], "State": thread["state"] }))
        .collect();
    let mut sections = vec![Section::Markdown {
        label: "output".into(),
        text: markdown_table(&Value::Array(threads), "threads")?,
    }];
    sections.extend(attention_sections(data));
    Some(Body::Sections(sections))
}

/// The answer Claude sent, shown below the request it answers.
fn reply_output(data: &ToolData<'_>) -> Option<Body> {
    data.result.filter(|result| !result.error)?;
    let mut sections = vec![Section::Prose {
        text: text(data.input, "reply")?.to_owned(),
        markdown: false,
    }];
    sections.extend(attention_sections(data));
    Some(Body::Sections(sections))
}

/// Separates the requests waiting for Claude into their own Attention table.
fn attention_sections(data: &ToolData<'_>) -> Vec<Section> {
    let Some(result) = data.result.filter(|result| !result.error) else {
        return Vec::new();
    };
    let Some(attention) = result.structured["attention"].as_array() else {
        return Vec::new();
    };
    let texts: Vec<Value> = attention
        .iter()
        .map(|entry| entry["text"].clone())
        .collect();
    let Some(text) = markdown_table(&Value::Array(texts), "Attention") else {
        return Vec::new();
    };
    vec![
        Section::GroupBreak,
        Section::Markdown {
            label: "attention".into(),
            text,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_manager_list_shows_a_thread_table() {
        let records: Vec<serde_json::Value> =
            include_str!("../../../../tests/fixtures/claude-manager-list.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let name = records[0]["message"]["content"][0]["name"]
            .as_str()
            .unwrap();
        let content = &records[1]["message"]["content"][0]["content"];
        let result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        let super::Body::Sections(output) = super::output(
            super::rule(name).output,
            &super::ToolData {
                input: &serde_json::Value::Null,
                result: Some(&result),
            },
        ) else {
            panic!("expected visible body")
        };
        let [crate::event::Section::Markdown { text, .. }] = output.as_slice() else {
            panic!("expected one Markdown table, got {output:?}");
        };
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows[..2], ["| Name | State |", "| --- | --- |"]);
        assert_eq!(rows[2], r"| 规则\:拆逗号 | completed |");
        assert_eq!(rows.last().unwrap(), &"| 框架补三处 | inProgress |");
        assert_eq!(rows.len(), 11);
    }

    #[test]
    fn codex_manager_reply_shows_ask_reply_and_attention() {
        let records: Vec<serde_json::Value> =
            include_str!("../../../../tests/fixtures/claude-manager-reply.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let call = &records[0]["message"]["content"][0];
        let content = &records[1]["message"]["content"][0]["content"];
        let result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        let presentation = super::presentation(
            call["name"].as_str().unwrap(),
            &super::ToolData {
                input: &call["input"],
                result: Some(&result),
            },
        );
        assert_eq!(
            presentation.title.as_deref(),
            call["input"]["threadId"].as_str()
        );
        let [crate::event::Section::Prose { text: ask, .. }] = presentation.input.as_slice() else {
            panic!("expected the ask, got {:?}", presentation.input);
        };
        assert_eq!(ask, call["input"]["ask"].as_str().unwrap());
        let Some(super::Body::Sections(output)) = presentation.output else {
            panic!("expected visible body")
        };
        let [
            crate::event::Section::Prose { text: reply, .. },
            crate::event::Section::GroupBreak,
            crate::event::Section::Markdown { text, .. },
        ] = output.as_slice()
        else {
            panic!("expected the reply and the attention section, got {output:?}");
        };
        assert_eq!(reply, call["input"]["reply"].as_str().unwrap());
        let attention = result.structured["attention"].as_array().unwrap();
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows[..2], ["| Attention |", "| --- |"]);
        assert_eq!(rows.len(), 2 + attention.len());
        for (row, entry) in rows[2..].iter().zip(attention) {
            assert_eq!(
                *row,
                format!("| {} |", super::table_cell(entry["text"].as_str().unwrap()))
            );
        }
    }

    #[test]
    fn codex_manager_list_input_shows_its_states_as_a_table() {
        let input = serde_json::json!({ "state": ["waiting", "inProgress"] });
        let presentation = super::presentation(
            "mcp__plugin_codex_codex-manager__list",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        let [crate::event::Section::Markdown { text, .. }] = presentation.input.as_slice() else {
            panic!("expected one Markdown table, got {:?}", presentation.input);
        };
        assert_eq!(text, "| state |\n| --- |\n| waiting |\n| inProgress |");
    }

    #[test]
    fn codex_manager_start_shows_only_the_thread_id() {
        let records: Vec<serde_json::Value> =
            include_str!("../../../../tests/fixtures/claude-manager-start-result.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let call = &records[0]["message"]["content"][0];
        let content = &records[1]["message"]["content"][0]["content"];
        let result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        let presentation = super::presentation(
            call["name"].as_str().unwrap(),
            &super::ToolData {
                input: &call["input"],
                result: Some(&result),
            },
        );
        let Some(super::Body::Sections(output)) = presentation.output else {
            panic!("expected visible body")
        };
        let [crate::event::Section::Text { text, .. }] = output.as_slice() else {
            panic!("expected only the thread id, got {output:?}");
        };
        assert_eq!(text, result.structured["threadId"].as_str().unwrap());
    }

    #[test]
    fn mcp_presentation_uses_shared_tables_and_removes_title_fields() {
        let record: serde_json::Value =
            include_str!("../../../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .find(|record: &serde_json::Value| {
                    record["message"]["content"][0]["name"]
                        == "mcp__plugin_codex_codex-manager__start"
                })
                .unwrap();
        let call = &record["message"]["content"][0];
        for name in [call["name"].as_str().unwrap(), "codex-manager.start"] {
            let super::Presentation {
                title: subject,
                input,
                ..
            } = super::presentation(
                name,
                &super::ToolData {
                    input: &call["input"],
                    result: None,
                },
            );
            let name = super::display_tool_name(name);
            assert_eq!(name, "codex-manager start");
            assert_eq!(subject.as_deref(), Some("tool-calls"));
            assert_eq!(
                input,
                vec![crate::event::Section::Markdown {
                    label: "prompt".into(),
                    text: call["input"]["prompt"].as_str().unwrap().into(),
                }]
            );
        }
    }
    #[test]
    fn attach_and_review_show_the_recorded_thread_id() {
        for source in [
            include_str!("../../../../tests/fixtures/claude-manager-attach-result.jsonl"),
            include_str!("../../../../tests/fixtures/claude-manager-review-result.jsonl"),
        ] {
            let records: Vec<Value> = source
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let call = &records[0]["message"]["content"][0];
            let content = &records[1]["message"]["content"][0]["content"];
            let result = ToolResult {
                source: None,
                content: content.clone(),
                structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
                error: false,
            };
            let shown = presentation(
                call["name"].as_str().unwrap(),
                &ToolData {
                    input: &call["input"],
                    result: Some(&result),
                },
            );
            let Some(Body::Sections(sections)) = shown.output else {
                panic!("expected output")
            };
            assert_eq!(
                sections,
                vec![text_section(
                    "output",
                    result.structured["threadId"].as_str().unwrap()
                )]
            );
            let subject = call["input"]
                .get("thread")
                .or_else(|| call["input"].get("name"))
                .unwrap()
                .as_str();
            assert_eq!(shown.title.as_deref(), subject);
        }
    }

    #[test]
    fn interrupt_shows_only_the_thread_id() {
        let records: Vec<Value> =
            include_str!("../../../../tests/fixtures/claude-manager-interrupt-result.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let call = &records[0]["message"]["content"][0];
        let content = &records[1]["message"]["content"][0]["content"];
        let result = ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        let data = ToolData {
            input: &call["input"],
            result: Some(&result),
        };
        let Body::Sections(sections) = output(rule("codex-manager.interrupt").output, &data) else {
            panic!("expected interrupt output")
        };
        assert_eq!(
            sections[0],
            text_section("output", "01a15fdd-d240-79e2-bb1e-a7d1f31b35c2")
        );
        assert_eq!(&sections[1..], attention_sections(&data));
    }

    #[test]
    fn codex_manager_send_shows_only_the_note() {
        let records: Vec<Value> =
            include_str!("../../../../tests/fixtures/claude-manager-send-result.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let content = &records[1]["message"]["content"][0]["content"];
        let result = ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        let data = ToolData {
            input: &records[0]["message"]["content"][0]["input"],
            result: Some(&result),
        };
        let Body::Sections(sections) = output(rule("codex-manager.send").output, &data) else {
            panic!("expected send output")
        };
        assert_eq!(
            sections,
            [Section::Prose {
                text: result.structured["note"].as_str().unwrap().to_owned(),
                markdown: false,
            }]
        );
    }

    #[test]
    fn result_title_follows_the_declared_fields() {
        let records: Vec<serde_json::Value> =
            include_str!("../../../../tests/fixtures/claude-manager-detach-name.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let input = &records[0]["message"]["content"][0]["input"];
        let content = &records[1]["message"]["content"][0]["content"];
        let mut result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        assert_eq!(
            super::presentation(
                "mcp__codex-manager__detach",
                &super::ToolData {
                    input,
                    result: Some(&result)
                }
            )
            .title
            .as_deref(),
            Some("模型和new挪位")
        );
        assert_eq!(
            super::presentation(
                "mcp__codex-manager__detach",
                &super::ToolData {
                    input,
                    result: None
                }
            )
            .title
            .as_deref(),
            input["threadId"].as_str()
        );
        for (name, expected) in [
            (serde_json::Value::Null, input["threadId"].as_str()),
            (serde_json::json!(" "), input["threadId"].as_str()),
            (serde_json::json!(42), Some("42")),
        ] {
            result.structured["name"] = name;
            assert_eq!(
                super::presentation(
                    "mcp__codex-manager__detach",
                    &super::ToolData {
                        input,
                        result: Some(&result)
                    }
                )
                .title
                .as_deref(),
                expected
            );
        }
    }
    #[test]
    fn every_manager_output_keeps_running_and_failed_fallbacks() {
        let records: Vec<Value> =
            include_str!("../../../../tests/fixtures/claude-manager-reply.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let input = &records[0]["message"]["content"][0]["input"];
        let content = &records[1]["message"]["content"][0]["content"];
        let failed = ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: true,
        };
        for name in [
            "start",
            "attach",
            "send",
            "reply",
            "list",
            "interrupt",
            "detach",
            "review",
        ] {
            let Some((Pick::With(render), _)) = rule(&format!("codex-manager.{name}")).output
            else {
                panic!("missing output builder: {name}")
            };
            assert!(
                render(&ToolData {
                    input,
                    result: None
                })
                .is_none(),
                "running {name}"
            );
            assert!(
                render(&ToolData {
                    input,
                    result: Some(&failed)
                })
                .is_none(),
                "failed {name}"
            );
        }
    }
}
