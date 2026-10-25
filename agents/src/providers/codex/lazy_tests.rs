use super::*;

#[test]
fn children_only_reads_native_codex_edges() {
    let (_dir, mut provider) = tests::fixture();
    let children = provider.children(&[key("thread-12".into())]);
    assert_eq!(children.sessions.len(), 1);
    assert_eq!(children.sessions[0].id, key("thread-13".into()));
    let children = provider.children(&[SessionKey {
        provider: ProviderId::Claude,
        id: "claude-parent".into(),
    }]);
    assert!(children.sessions.is_empty());
}
