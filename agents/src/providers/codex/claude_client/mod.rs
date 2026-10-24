use crate::providers::{mcp::McpServer, tool::*};

pub(crate) const SERVER: McpServer = McpServer {
    name: "claude-client",
    tools: &[
        ToolRule {
            name: "ask_claude",
            input: Some((Pick::Field(Source::Input, "/text"), Format::Markdown)),
            output: Some((Pick::With(mcp_markdown_output), Format::Text)),
            list_input: true,
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "notify_claude",
            input: Some((Pick::Field(Source::Input, "/text"), Format::Markdown)),
            output: Some((Pick::With(mcp_markdown_output), Format::Text)),
            list_input: true,
            ..DEFAULT_RULE
        },
    ],
};

#[cfg(test)]
mod tests {
    #[test]
    fn claude_client_text_is_markdown_and_follows_the_list_tool_name() {
        for source in [
            include_str!("../../../../tests/fixtures/claude-client/ask_claude.jsonl"),
            include_str!("../../../../tests/fixtures/claude-client/notify_claude.jsonl"),
        ] {
            let record: serde_json::Value = serde_json::from_str(source).unwrap();
            let item = &record["payload"]["item"];
            let input = &item["arguments"];
            let text = input["text"].as_str().unwrap();
            let tool = item["tool"].as_str().unwrap();
            for name in [
                format!("claude-client {tool}"),
                format!("mcp__claude-client__{tool}"),
                format!("mcp__plugin_claude_claude-client__{tool}"),
            ] {
                let super::Presentation {
                    title: subject,
                    input: sections,
                    ..
                } = super::presentation(
                    &name,
                    &super::ToolData {
                        input,
                        result: None,
                    },
                );
                assert_eq!(
                    sections,
                    vec![crate::event::Section::Markdown {
                        label: "text".into(),
                        text: text.into(),
                    }]
                );
                assert_eq!(
                    super::list_title(&name, subject.as_deref(), input, None),
                    format!("{} {text}", super::display_tool_name(&name))
                );
            }
        }
    }

    #[test]
    fn tool_detail_titles_center_the_tool_name_without_a_subject() {
        for source in [
            include_str!("../../../../tests/fixtures/claude-client/ask_claude.jsonl"),
            include_str!("../../../../tests/fixtures/claude-client/notify_claude.jsonl"),
            include_str!("../../../../tests/fixtures/tool-list/claude.jsonl"),
        ] {
            let record: serde_json::Value =
                serde_json::from_str(source.lines().next().unwrap()).unwrap();
            let name = record["payload"]["item"]["tool"]
                .as_str()
                .or_else(|| record["message"]["content"][0]["name"].as_str())
                .unwrap();
            let mut call = crate::event::ToolCall {
                model_settings: None,
                agent_message: None,
                started_at: None,
                hide_output: false,
                agent_thread_id: None,
                agent_path: None,
                title: String::new(),
                show_event_detail: true,
                id: String::new(),
                summary: String::new(),
                name: String::new(),
                input: Vec::new(),
                exit_code: None,
                output: None,
                is_error: false,
                duration: None,
            };
            call.name = name.into();
            let display = super::display_tool_name(name);
            for title in ["", " \t "] {
                call.title = title.into();
                assert_eq!(call.detail_left(), "");
            }
            call.title = "Inspect source".into();
            assert_eq!(call.detail_left(), display);
        }
    }
}
