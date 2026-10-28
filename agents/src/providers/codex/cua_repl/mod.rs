use crate::providers::{mcp::McpServer, tool::*};

pub(crate) const SERVER: McpServer = McpServer {
    name: "cua_repl",
    tools: &[ToolRule {
        name: "js",
        title: &[Pick::Field(Source::Input, "/title")],
        input: Some((Pick::Field(Source::Input, "/code"), Format::Code("js"))),
        ..DEFAULT_RULE
    }],
};

#[cfg(test)]
mod tests {
    #[test]
    fn js_title_is_the_call_title_and_input_is_its_code() {
        let source = include_str!("../../../../tests/fixtures/codex-images/tool-mcp.jsonl");
        let record: serde_json::Value =
            serde_json::from_str(source.lines().next().unwrap()).unwrap();
        let input: serde_json::Value =
            serde_json::from_str(record["payload"]["arguments"].as_str().unwrap()).unwrap();
        for name in ["cua_repl js", "mcp__cua_repl__js"] {
            let shown = super::presentation(
                name,
                &super::ToolData {
                    input: &input,
                    result: None,
                },
            );
            assert_eq!(shown.title.as_deref(), input["title"].as_str());
            let [crate::event::Section::Code { text, .. }] = shown.input.as_slice() else {
                panic!("missing code: {:?}", shown.input)
            };
            assert_eq!(text.trim_end(), input["code"].as_str().unwrap());
        }
    }
}
