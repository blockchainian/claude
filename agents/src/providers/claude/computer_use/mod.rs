use crate::providers::{mcp::McpServer, tool::*};

pub(crate) const SERVER: McpServer = McpServer {
    name: "computer-use",
    tools: &[
        ToolRule {
            name: "open_application",
            title: &[Pick::Field(Source::Input, "/app")],
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "left_click",
            aliases: &[
                "right_click",
                "middle_click",
                "double_click",
                "triple_click",
                "left_click_drag",
                "left_mouse_down",
                "left_mouse_up",
                "type",
                "key",
                "hold_key",
            ],
            title: &[Pick::Field(Source::Input, "/action_summary")],
            ..DEFAULT_RULE
        },
    ],
};

#[cfg(test)]
mod tests {
    #[test]
    fn computer_use_open_application_titles_its_app() {
        let input = serde_json::json!({"app": "Wispr Flow"});
        let shown = super::presentation(
            "mcp__computer-use__open_application",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        assert_eq!(shown.title.as_deref(), Some("Wispr Flow"));
    }

    #[test]
    fn computer_use_actions_title_their_summary_and_show_the_rest() {
        let input = serde_json::json!({
            "coordinate": [586, 1607],
            "action_summary": "Opens Wispr Flow Settings"
        });
        let shown = super::presentation(
            "mcp__computer-use__left_click",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        assert_eq!(shown.title.as_deref(), Some("Opens Wispr Flow Settings"));
        assert_eq!(
            shown.input,
            super::raw_input(&serde_json::json!({"coordinate": [586, 1607]}))
        );
    }
}
