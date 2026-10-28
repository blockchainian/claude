use super::*;

#[test]
fn settings_inputs_are_supported_for_inactive_and_active_threads() {
    let directory = tempfile::tempdir().unwrap();
    Connection::open(directory.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let mut provider = CodexProvider::new(directory.path().into());
    let mut session = provider
        .refresh(1)
        .sessions
        .into_iter()
        .find(|session| session.can_resume())
        .unwrap();
    for status in [
        SessionStatus::Inactive,
        SessionStatus::Ready,
        SessionStatus::Working,
    ] {
        session.status = status;
        for input in [
            SessionInput::SetModel("gpt-6.1-sol".into()),
            SessionInput::SetEffort("high".into()),
        ] {
            assert!(
                provider.send(&session, input).is_some(),
                "{status:?} settings must be supported"
            );
        }
    }
}
