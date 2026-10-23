use super::*;

#[test]
fn child_box_clock_matches_its_own_prompt_for_both_providers() {
    let records: Vec<serde_json::Value> =
        include_str!("../../tests/fixtures/codex-thread-clock.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    let start = records[2]["timestamp"].as_str().unwrap().parse().unwrap();
    let end = records[3]["timestamp"].as_str().unwrap().parse().unwrap();
    let birth = chrono::DateTime::from_timestamp_millis(1792218925141).unwrap();
    for provider in [ProviderId::Codex, ProviderId::Claude] {
        for (prompt_at, now, expected) in [
            (Some(start), start + Duration::minutes(15), "15m 0s"),
            (Some(start), start - Duration::seconds(1), "0s"),
            (None, birth + Duration::minutes(15), "15m 0s"),
        ] {
            let status = SessionStatus::Working;
            let mut parent = session("parent", "Claude parent", SessionStatus::Ready, 0);
            parent.started_at = Some(birth - Duration::hours(12));
            let mut child = session("child", "中文答题卡住", status, 0);
            child.id.provider = provider;
            child.parent = Some(parent.id.clone());
            child.started_at = Some(birth);
            child.prompt_at = prompt_at;
            child.last_record_at = Some(end);
            let mut view = test_view(Grouping::Project, true);
            view.set_sessions(vec![parent, child], false);
            view.selected = view.rows.iter().position(|s| s.id.id == "parent").unwrap();
            let area = Rect::new(0, 0, 80, 40);
            let mut buf = Buffer::empty(area);
            view.render_task_details(area, &mut buf, now);
            let title = buf
                .content
                .chunks(80)
                .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
                .find(|line| line.replace(' ', "").contains("中文答题卡住"))
                .unwrap();
            assert!(
                title
                    .trim_end()
                    .ends_with(expected),
                "{provider:?} {status:?}: {title}"
            );
            if prompt_at.is_some() {
                view.selected = view.rows.iter().position(|s| s.id.id == "child").unwrap();
                let mut buf = Buffer::empty(area);
                view.render_task_details(area, &mut buf, now);
                let prompt = buf
                    .content
                    .chunks(80)
                    .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
                    .find(|line| line.contains("Prompt"))
                    .unwrap();
                assert!(prompt.trim_end().ends_with(expected), "{prompt}");
            }
        }
    }
}
