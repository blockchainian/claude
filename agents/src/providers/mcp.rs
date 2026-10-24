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

#[cfg(test)]
mod tests {
    use crate::providers::tool::*;
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
            display_tool_name("mcp__claude-client__ask_claude"),
            "claude-client ask claude"
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
