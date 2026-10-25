use super::tool::ToolRule;

/// An MCP server whose calls a provider's transcripts record.
pub(crate) struct McpServer {
    /// The server name with any plugin prefix removed.
    pub name: &'static str,
    /// Display rules keyed by bare tool name.
    pub tools: &'static [ToolRule],
}

/// Splits a recorded MCP name into its server and tool.
pub(crate) fn parse(name: &str) -> Option<(&str, &str)> {
    if let Some((server, tool)) = name.split_once(' ') {
        return (!server.is_empty() && !tool.is_empty()).then_some((server, tool));
    }
    if let Some(rest) = name.strip_prefix("mcp__") {
        let (server, tool) = rest.split_once("__")?;
        let server = if let Some(rest) = server.strip_prefix("plugin_") {
            let (plugin, server) = rest.split_once('_')?;
            if plugin.is_empty() {
                return None;
            }
            server
        } else {
            server
        };
        return (!server.is_empty() && !tool.is_empty() && !tool.contains("__"))
            .then_some((server, tool));
    }
    let (server, tool) = name.rsplit_once('.')?;
    (!server.is_empty() && !tool.is_empty()).then_some((server, tool))
}

/// Group MCP calls by server and shared tool-name components, preserving titles.
pub(crate) fn list_group(name: &str, label: &str) -> Vec<String> {
    if label != super::tool::display_tool_name(name) {
        return vec![label.to_owned()];
    }
    let Some((server, tool)) = parse(name) else {
        return vec![label.to_owned()];
    };
    if label != format!("{server} {}", tool.replace('_', " ")) {
        return vec![label.to_owned()];
    }
    let mut words = tool.split('_');
    let first = words.next().unwrap();
    std::iter::once(format!("{server} {first}"))
        .chain(words.map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::providers::tool::*;
    #[test]
    fn captured_mcp_tree_preserves_single_calls_nested_titles_and_path_ranges() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-appium.jsonl");
        let events = crate::providers::claude::event_reader::tail(&path, 100).unwrap();
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        let gestures: Vec<_> = calls
            .iter()
            .filter(|call| call.name.ends_with("__appium_gesture"))
            .collect();
        let screenshot = calls
            .iter()
            .find(|call| call.name.ends_with("__appium_screenshot"))
            .unwrap();
        let add = |groups: &mut groups::Groups, call: &crate::event::ToolCall| {
            groups.add_path(
                &super::list_group(&call.name, &display_tool_name(&call.name)),
                Some(&call.title),
            );
        };
        let mut groups = groups::Groups::default();
        add(&mut groups, gestures[0]);
        assert_eq!(groups.finish().unwrap(), "appium-mcp appium gesture tap");
        let mut groups = groups::Groups::default();
        for gesture in &gestures {
            add(&mut groups, gesture);
        }
        assert_eq!(groups.finish().unwrap(), "appium-mcp appium gesture tap");
        let mut groups = groups::Groups::default();
        for gesture in &gestures {
            add(&mut groups, gesture);
        }
        add(&mut groups, screenshot);
        groups.add("Read", Some("src/main.rs:10-20"));
        let (body, ranges) = groups.finish_with_ranges().unwrap();
        assert_eq!(
            body,
            "appium-mcp appium {gesture tap, screenshot 400}, Read src/main.rs:10-20"
        );
        assert_eq!(
            ranges
                .iter()
                .map(|range| &body[range.clone()])
                .collect::<Vec<_>>(),
            ["tap", "400", "src/main.rs:10-20"]
        );
        assert_eq!(super::list_group("functions.exec", "Exec"), ["Exec"]);
        assert_eq!(
            super::list_group(&gestures[0].name, "human description"),
            ["human description"]
        );
        assert_eq!(
            super::list_group("mcp__other__appium_gesture", "other appium gesture"),
            ["other appium", "gesture"]
        );
    }

    #[test]
    fn prefix_tools_and_subject_names_remain_distinct() {
        let mut groups = groups::Groups::default();
        groups.add_path(
            &super::list_group("mcp__server__appium_gesture", "server appium gesture"),
            None,
        );
        groups.add_path(
            &super::list_group(
                "mcp__server__appium_gesture_tap",
                "server appium gesture tap",
            ),
            Some("detail"),
        );
        let (body, ranges) = groups.finish_with_ranges().unwrap();
        assert_eq!(body, "server appium {gesture, gesture tap detail}");
        assert_eq!(&body[ranges[0].clone()], "detail");
        let mut groups = groups::Groups::default();
        groups.add_path(&["server appium".into()], Some("gesture"));
        groups.add_path(&["server appium".into(), "gesture".into()], Some("tap"));
        let (body, ranges) = groups.finish_with_ranges().unwrap();
        assert_eq!(body, "server appium gesture, server appium gesture tap");
        assert_eq!(
            ranges
                .iter()
                .map(|range| &body[range.clone()])
                .collect::<Vec<_>>(),
            ["gesture", "tap"]
        );
    }

    #[test]
    fn captured_mcp_names_preserve_server_and_tool_case() {
        let record: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/mcp-codex/image.json"))
                .unwrap();
        let item = &record["payload"]["item"];
        let server = item["server"].as_str().unwrap();
        let tool = item["tool"].as_str().unwrap();
        let raw = format!("mcp__{server}__{tool}");
        assert_eq!(
            display_tool_name(&raw),
            format!("{server} {}", tool.replace('_', " "))
        );
        assert_eq!(
            display_tool_name("mcp__claude-client__ask"),
            "claude-client ask"
        );
    }

    #[test]
    fn captured_tool_names_share_one_display_rule() {
        let record: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/codex-agent-names.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        let name = record["payload"]["name"].as_str().unwrap();
        assert_eq!(display_tool_name(name), "Send message");
        assert_eq!(
            display_tool_name(&format!("collaboration.{name}")),
            "Send message"
        );
        for line in include_str!("../../tests/fixtures/claude-tool-calls.jsonl").lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some(name @ ("WebFetch" | "Bash" | "Read" | "Skill")) =
                record["message"]["content"][0]["name"].as_str()
            {
                assert_eq!(display_tool_name(name), name);
            }
        }
    }

    #[test]
    fn mcp_titles_show_server_and_tool() {
        for (name, expected) in [
            (
                "mcp__plugin_codex_codex-manager__send",
                "codex-manager send",
            ),
            (
                "mcp__plugin_codex_codex-manager__start",
                "codex-manager start",
            ),
            (
                "mcp__claude-in-chrome__navigate",
                "claude-in-chrome navigate",
            ),
            ("mcp__context7__query-docs", "context7 query-docs"),
            ("mcp__tool", "mcp__tool"),
            ("mcp____send", "mcp____send"),
            ("mcp__server__", "mcp__server__"),
            ("mcp__plugin_codex__send", "mcp__plugin_codex__send"),
            ("mcp__plugin__server__send", "mcp__plugin__server__send"),
            ("mcp__plugin_codex___send", "mcp__plugin_codex___send"),
        ] {
            assert_eq!(display_tool_name(name), expected);
        }
    }
}
