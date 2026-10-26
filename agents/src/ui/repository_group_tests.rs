use super::*;

#[test]
fn missing_worktree_retains_primary_repository_group() {
    let (repo, linked) = crate::tasks::tests::worktree();
    let root = std::fs::canonicalize(repo.path()).unwrap();
    let mut primary = session("primary", "Primary", SessionStatus::Inactive, 0);
    primary.cwd = root.clone();
    let mut archived = session("archived", "Archived", SessionStatus::Inactive, 0);
    archived.cwd = linked.clone();
    archived.archived = true;
    let preferences_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        preferences_dir.path().join("agents.json"),
        serde_json::to_vec(&serde_json::json!({
            "config": {},
            "state": {"tasks": {archived.id.to_string(): {"repository": {"root": root}}}}
        }))
        .unwrap(),
    )
    .unwrap();
    crate::tasks::remove_worktree(&linked).unwrap();
    assert!(!linked.exists());
    assert!(crate::repository::repository_identity(&archived.cwd).is_none());
    let mut view = View::default();
    view.set_task_repositories(&crate::state::Preferences::load(preferences_dir.path()));
    view.set_sessions(vec![primary.clone(), archived.clone()], false);
    let primary_index = view
        .rows
        .iter()
        .position(|row| row.id == primary.id)
        .unwrap();
    let archived_index = view
        .rows
        .iter()
        .position(|row| row.id == archived.id)
        .unwrap();
    assert_eq!(
        view.project_groups[primary_index],
        view.project_groups[archived_index]
    );
    let group = &view.project_groups[archived_index];
    assert_eq!(group.key, (root.join(".git"), PathBuf::new()));
    assert_eq!(group.heading, root);
    assert!(group.in_repository);
}
