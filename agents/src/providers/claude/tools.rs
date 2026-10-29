use super::transcript::notification_field;
use crate::event::Section;
use crate::providers::tool::*;
use serde_json::Value;

pub(super) const RULES: &[ToolRule] = &[
    ToolRule {
        name: "task-notification",
        label: false,
        list_subject: true,
        title: &[Pick::With(notification_title)],
        input: Some((Pick::With(notification_input), Format::Text)),
        output: Some((Pick::With(notification_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "advisor",
        label: false,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "background",
        label: false,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "SubagentHandback",
        input: Some((Pick::Field(Source::Input, "/message"), Format::Markdown)),
        output: Some((Pick::Field(Source::Result, "/message"), Format::Text)),
        list_subject: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Read",
        title: &[
            Pick::With(read_title),
            Pick::Field(Source::Result, "/file/filePath"),
            Pick::Field(Source::Input, "/file_path"),
        ],
        input: Some((Pick::With(empty_input), Format::Text)),
        output: Some((Pick::With(read_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Write",
        title: &[Pick::Field(Source::Input, "/file_path")],
        input: Some((Pick::With(write_input), Format::Text)),
        output: Some((Pick::With(write_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Edit",
        title: &[Pick::Field(Source::Input, "/file_path")],
        input: Some((Pick::With(empty_input), Format::Text)),
        output: Some((Pick::With(patch_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Bash",
        title: &[Pick::With(shell_description)],
        input: Some((Pick::With(shell_input), Format::Text)),
        output: Some((Pick::With(shell_output), Format::Text)),
        list_subject: true,
        shell_stream: Stream {
            merged_stderr: false,
            trimmed: true,
            exit_code: None,
            is_error: false,
        },
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "ToolSearch",
        title: &[Pick::With(tool_search_title)],
        input: Some((Pick::With(empty_input), Format::Text)),
        output: Some((Pick::With(matches_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "AskUserQuestion",
        title: &[Pick::With(question_title)],
        input: Some((Pick::With(questions_input), Format::Text)),
        output: Some((Pick::With(questions_output), Format::Text)),
        question: true,
        ..DEFAULT_RULE
    },
    // Message owns spawn names and prompt bodies. Other input fields, including
    // subagent_type, run_in_background and isolation, are intentionally not displayed.
    ToolRule {
        name: "Agent",
        aliases: &["Task"],
        output: Some((Pick::With(agent_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "SendMessage",
        title: &[Pick::With(recipient_title)],
        input: Some((Pick::Field(Source::Input, "/message"), Format::Markdown)),
        output: Some((Pick::Field(Source::Result, "/message"), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "WebSearch",
        input: Some((Pick::Field(Source::Input, "/query"), Format::Text)),
        output: Some((Pick::With(web_search_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "WebFetch",
        title: &[Pick::Field(Source::Input, "/url")],
        input: Some((Pick::With(fetch_input), Format::Text)),
        output: Some((Pick::Field(Source::Result, "/result"), Format::Markdown)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Skill",
        title: &[Pick::Field(Source::Input, "/skill")],
        input: Some((Pick::With(arguments_input), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "EnterWorktree",
        title: &[
            Pick::Field(Source::Input, "/name"),
            Pick::Field(Source::Input, "/path"),
        ],
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "SendUserFile",
        input: Some((Pick::Field(Source::Input, "/caption"), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "TaskCreate",
        hidden: true,
        task: Some(TaskAction::Create),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "TaskUpdate",
        hidden: true,
        task: Some(TaskAction::Update),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "TaskList",
        hidden: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "TaskGet",
        hidden: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "ListAgents",
        hidden: true,
        ..DEFAULT_RULE
    },
];

/// Decodes the entities Claude Code escapes in task notification fields.
fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn notification_summary(content: &str) -> String {
    let notification = notification_field(content, "task-notification").unwrap_or("");
    unescape_xml(
        notification_field(notification, "summary").unwrap_or_else(|| {
            notification
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("")
        }),
    )
}

/// Extracts a background command's description, status and exit code.
pub(super) fn background(content: &str) -> Option<(String, String, Option<i64>)> {
    let summary = notification_summary(content);
    // Split at the closing description quote, preserving embedded and escaped quotes.
    let (description, suffix) = summary
        .strip_prefix("Background command \"")?
        .rsplit_once("\" ")?;
    let status = unescape_xml(notification_field(content, "status")?);
    let exit_code = suffix.split_once("exit code ").and_then(|(_, code)| {
        let end = code
            .find(|ch: char| ch != '-' && !ch.is_ascii_digit())
            .unwrap_or(code.len());
        code[..end].parse::<i64>().ok()
    });
    Some((description.to_owned(), status, exit_code))
}

// Claude Code names a command run without a description by its full text;
// a chain of commands then reads like a Bash call: its command names, with the full
// text as input.
fn notification_chain(description: &str) -> Option<String> {
    let names = shell_summary::summarize_shell_commands(description);
    names.contains(", ").then_some(names)
}

fn notification_title(data: &ToolData<'_>) -> Option<String> {
    let content = data.input.as_str()?;
    Some(match background(content) {
        Some((description, status, exit_code)) => {
            let description = notification_chain(&description).unwrap_or(description);
            match exit_code {
                Some(0) => format!("{description} ✓"),
                Some(_) => description,
                None => format!("{description} {status}"),
            }
        }
        None => notification_summary(content),
    })
}

fn notification_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    let content = data.input.as_str()?;
    let mut sections = background(content)
        .filter(|(description, _, _)| notification_chain(description).is_some())
        .map(|(description, _, _)| shell_source_sections(&description))
        .unwrap_or_default();
    if let Some(reminder) = notification_field(content, "system-reminder") {
        sections.push(text_section("input", &unescape_xml(reminder)));
    }
    Some(sections)
}

fn notification_output(data: &ToolData<'_>) -> Option<Body> {
    let content = data.input.as_str()?;
    let Some(path) = notification_field(content, "output-file") else {
        return Some(Body::Hidden);
    };
    let path = unescape_xml(path);
    Some(Body::Sections(vec![Section::OutputFile {
        provider: crate::providers::ProviderId::Claude,
        path: path.into(),
    }]))
}

/// A message's named recipient; a peer session's socket address names nobody.
fn recipient(input: &Value) -> Option<&str> {
    text(input, "to").filter(|to| !to.starts_with("uds:"))
}

fn shell_description(data: &ToolData<'_>) -> Option<String> {
    text(data.input, "description")
        .map(str::to_owned)
        .or_else(|| shell_title(data))
}

/// Groups a `select:` query's tools by MCP server; keyword queries stay as typed.
fn tool_search_title(data: &ToolData<'_>) -> Option<String> {
    let query = text(data.input, "query")?;
    let Some(names) = query.strip_prefix("select:") else {
        return Some(query.to_owned());
    };
    let mut groups = crate::providers::tool::groups::Groups::default();
    for name in names
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        match crate::providers::mcp::parse(name) {
            Some((server, tool)) => groups.add(server, Some(&tool.replace('_', " "))),
            None => groups.add(name, None),
        }
    }
    groups.finish()
}

fn recipient_title(data: &ToolData<'_>) -> Option<String> {
    recipient(data.input).map(str::to_owned)
}

fn read_title(data: &ToolData<'_>) -> Option<String> {
    if data.input.get("offset").is_none() && data.input.get("limit").is_none() {
        return None;
    }
    let file = &data.result?.structured["file"];
    let path = text(file, "filePath")?;
    let start = file["startLine"].as_u64()?;
    let count = file["numLines"].as_u64()?;
    Some(format!(
        "{path}:{start}-{}",
        start.saturating_add(count.saturating_sub(1))
    ))
}

fn arguments_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(
        text(data.input, "args")
            .map(|text| vec![text_section("args", text)])
            .unwrap_or_default(),
    )
}

fn fetch_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    let mut sections = Vec::new();
    if let Some(url) = text(data.input, "url") {
        sections.push(text_section("", &format!("url: {url}")));
    }
    if let Some(prompt) = text(data.input, "prompt") {
        sections.push(Section::Prose {
            text: prompt.into(),
            markdown: false,
        });
    }
    Some(sections)
}

fn write_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(
        match (
            text(data.input, "file_path"),
            data.input["content"].as_str(),
        ) {
            (Some(path), Some(content)) => vec![file_section("input", path, content)],
            _ => data
                .input
                .get("content")
                .map(|value| picked_sections(value, "/content", Format::Text, false))
                .unwrap_or_default(),
        },
    )
}

fn agent_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let value = &result.structured;
    if value["isAsync"] == true
        || matches!(
            value["status"].as_str(),
            Some("async_launched" | "teammate_spawned")
        )
    {
        Some(Body::Hidden)
    } else {
        markdown_output(data)
    }
}

fn read_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let value = &result.structured;
    if value["type"] == "image" {
        // Use the message copy so Read and MCP images share the same record pointer.
        Some(Body::Sections(mcp_content(
            &result.content,
            result.source.as_ref(),
        )))
    } else {
        Some(Body::Sections(vec![file_section(
            "output",
            value["file"]["filePath"].as_str().unwrap_or(""),
            value["file"]["content"].as_str()?,
        )]))
    }
}

fn patch_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    Some(Body::Sections(vec![patch_section(&result.structured)?]))
}

fn write_output(data: &ToolData<'_>) -> Option<Body> {
    if data.result?.structured["type"] == "update" {
        patch_output(data)
    } else {
        None
    }
}

fn matches_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let text = result.structured["matches"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    Some(Body::Sections(vec![text_section("output", &text)]))
}

fn shell_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    let value = &result.structured;
    if !result.error
        && value["backgroundTaskId"].is_string()
        && let Some((_, path)) = result.text().split_once("Output is being written to: ")
        && let Some((path, _)) = path.split_once(". You will be notified")
        && !path.is_empty()
    {
        return Some(Body::Sections(vec![Section::OutputFile {
            provider: crate::providers::ProviderId::Claude,
            path: path.into(),
        }]));
    }
    shell_sections(data, rule("Bash").shell_stream)
}

fn web_search_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct SearchLink {
        title: String,
        url: String,
    }
    let result_text = result.text();
    let (_, links) = result_text.split_once("\n\nLinks: ")?;
    let mut stream = serde_json::Deserializer::from_str(links).into_iter::<Vec<SearchLink>>();
    let links_found = stream.next()?.ok()?;
    let mut text = String::from("| Title |\n| --- |\n");
    for link in links_found {
        let title = table_cell(&link.title);
        let url = link
            .url
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace('\n', "%0A")
            .replace('\r', "%0D")
            .replace('|', "%7C");
        text.push_str(&format!("| [{title}](<{url}>) |\n"));
    }
    text.pop();
    text.push_str(&links[stream.byte_offset()..]);
    Some(Body::Sections(vec![Section::Markdown {
        label: "output".into(),
        text,
    }]))
}

fn patch_section(value: &serde_json::Value) -> Option<Section> {
    let hunks = value["structuredPatch"].as_array()?;
    let text = hunks
        .iter()
        .map(|hunk| {
            std::iter::once(format!(
                "@@ -{},{} +{},{} @@",
                hunk["oldStart"], hunk["oldLines"], hunk["newStart"], hunk["newLines"]
            ))
            .chain(
                hunk["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|line| line.as_str().map(str::to_owned)),
            )
            .collect::<Vec<_>>()
            .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(Section::Diff {
        lang: file_language(value["filePath"].as_str()?).to_owned(),
        label: String::new(),
        text,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn captured_edit_patch_carries_file_language() {
        let record = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["toolUseResult"]["structuredPatch"].is_array())
            .unwrap();
        let section = super::patch_section(&record["toolUseResult"]).unwrap();
        assert!(
            matches!(section, crate::event::Section::Diff { lang, label, .. } if lang == "rs" && label.is_empty())
        );
    }

    #[test]
    fn hook_rejected_questions_show_chat_and_answers() {
        let records: Vec<serde_json::Value> =
            include_str!("../../../tests/fixtures/claude-questions-hook-rejected.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let questions = &records[0]["message"]["content"][0]["input"]["questions"];
        let result = records[1]["message"]["content"][0]["content"]
            .as_str()
            .unwrap();
        assert_eq!(
            super::rejected_answers(questions, result).as_deref(),
            Some("Chat about this")
        );
        assert_eq!(
            super::rejected_answers(questions, records[1]["toolUseResult"].as_str().unwrap()),
            super::rejected_answers(questions, result)
        );
        assert_eq!(
            super::rejected_answers(
                questions,
                &result.replace("(No answer provided)", "Answer: Red")
            )
            .as_deref(),
            Some("Chat about this\n\n● Color → Red")
        );
        assert_eq!(
            super::rejected_answers(
                questions,
                "PreToolUse:AskUserQuestion hook error: unrelated error"
            ),
            None
        );
    }

    #[test]
    fn recorded_native_rejected_questions_show_chat_and_answers() {
        for fixture in [
            include_str!("../../../tests/fixtures/claude-questions-rejected.jsonl"),
            include_str!("../../../tests/fixtures/claude-questions-hook-rejected.jsonl"),
        ] {
            let records: Vec<serde_json::Value> = fixture
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let questions = &records[0]["message"]["content"][0]["input"]["questions"];
            let header = questions[0]["header"].as_str().unwrap();
            for result in [
                records[1]["message"]["content"][0]["content"]
                    .as_str()
                    .unwrap(),
                records[1]["toolUseResult"].as_str().unwrap(),
            ] {
                // Keep the recorded four-line explanation and its actual question list.
                let (prefix, asked) = result
                    .split_once("The user wants to clarify these questions.")
                    .unwrap();
                let (_, list) = asked.split_once("Questions asked:").unwrap();
                let short = format!(
                    "{prefix}The user wants to clarify these questions. Start by asking them what they would like to clarify.\n\n    Questions asked:{list}"
                );
                for text in [result, short.as_str()] {
                    assert_eq!(
                        super::rejected_answers(questions, text).as_deref(),
                        Some("Chat about this")
                    );
                    assert_eq!(
                        super::rejected_answers(
                            questions,
                            &text.replace("(No answer provided)", "Answer: Red")
                        ),
                        Some(format!("Chat about this\n\n● {header} → Red"))
                    );
                }
            }
        }
    }

    #[test]
    fn agent_and_task_share_the_spawn_rule() {
        assert!(std::ptr::eq(super::rule("Agent"), super::rule("Task")));
        for name in ["Agent", "Task", "spawn_agent"] {
            let rule = super::rule(name);
            assert!(rule.title.is_empty());
            assert!(rule.input.is_none());
        }
    }

    #[test]
    fn enter_worktree_titles_its_name_or_path() {
        for input in [
            serde_json::json!({"name": "active-tab-colors"}),
            serde_json::json!({"path": "active-tab-colors"}),
        ] {
            let presentation = super::presentation(
                "EnterWorktree",
                &super::ToolData {
                    input: &input,
                    result: None,
                },
            );
            assert_eq!(presentation.title.as_deref(), Some("active-tab-colors"));
            assert!(presentation.input.is_empty());
        }
    }

    #[test]
    fn tool_search_title_groups_selected_tools_by_server() {
        let title = |query: &str| {
            super::presentation(
                "ToolSearch",
                &super::ToolData {
                    input: &serde_json::json!({"query": query, "max_results": 5}),
                    result: None,
                },
            )
            .title
        };
        assert_eq!(
            title("select:mcp__plugin_codex_codex-manager__start,mcp__plugin_codex_codex-manager__send,mcp__claude-in-chrome__javascript_tool,Monitor").as_deref(),
            Some("codex-manager {start, send}, claude-in-chrome javascript tool, Monitor")
        );
        assert_eq!(
            title("notebook jupyter").as_deref(),
            Some("notebook jupyter")
        );
    }

    #[test]
    fn send_user_file_input_shows_only_the_caption() {
        let input = serde_json::json!({
            "caption": "放大的那几列",
            "files": ["/tmp/crop.png"],
            "status": "normal",
        });
        let super::Presentation {
            title: subject,
            input: sections,
            ..
        } = super::presentation(
            "SendUserFile",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        assert_eq!(subject, None);
        assert!(matches!(
            sections.as_slice(),
            [crate::event::Section::Prose { text, .. }] if text == "放大的那几列"
        ));
    }
}
