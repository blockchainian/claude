pub(crate) mod await_output;
pub(crate) mod supervision;

mod tools;
pub(crate) const SERVER: crate::providers::mcp::McpServer = crate::providers::mcp::McpServer {
    name: "codex-manager",
    tools: tools::RULES,
};
