use super::*;
use std::io::Write;

fn pairs() -> Vec<(Value, Value)> {
    let messages: Vec<Value> = include_str!("../../../tests/fixtures/codex-rollout.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let tools: Vec<Value> = include_str!("../../../tests/fixtures/codex-tool-calls.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let raw = tools[0].clone();
    let mut completion = tools[4].clone();
    // Associate captured native and raw payloads as the same upstream call.
    completion["payload"]["item"]["id"] = raw["payload"]["call_id"].clone();
    vec![
        (messages[6].clone(), messages[5].clone()),
        (raw, completion),
    ]
}

#[test]
fn superseded_records_keep_both_sources_in_either_order() {
    for (raw, completion) in pairs() {
        for records in [[&raw, &completion], [&completion, &raw]] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let mut offsets = Vec::new();
            for record in records {
                offsets.push(file.as_file().metadata().unwrap().len());
                writeln!(file, "{record}").unwrap();
            }
            let parsed: Vec<_> = offsets
                .iter()
                .copied()
                .zip(records.into_iter().cloned())
                .collect();
            let mut expected = events(file.path(), &[(offsets[1], records[1].clone())], usize::MAX)
                .unwrap()
                .remove(0)
                .1;
            if raw["payload"]["type"] == "message" {
                // Assistant decoding always resolves to the completion, but keeps the visited timestamp.
                expected = events(file.path(), &[(offsets[1], completion.clone())], usize::MAX)
                    .unwrap()
                    .remove(0)
                    .1;
                expected.started_at = timestamp(records[1]).unwrap();
            }
            let actual = events(file.path(), &parsed, usize::MAX).unwrap();
            assert_eq!(actual.len(), 1);
            assert_eq!(actual[0].1.body, expected.body);
            assert_eq!(actual[0].1.started_at, expected.started_at);
            assert_eq!(actual[0].1.ended_at, expected.ended_at);
            assert_eq!(
                actual[0]
                    .1
                    .source_records
                    .iter()
                    .map(|r| r.offset)
                    .collect::<Vec<_>>(),
                offsets
            );
        }
    }
}

#[test]
fn superseded_records_page_raw_into_retained_completion() {
    for (raw, completion) in pairs() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{raw}").unwrap();
        // Real completion payloads with distinct ids keep the initial page bounded.
        for index in 0..6 {
            let mut filler = completion.clone();
            filler["payload"]["item"]["id"] = format!("padding-{index}").into();
            filler["paging_padding"] = "x".repeat(CHUNK as usize).into();
            writeln!(file, "{filler}").unwrap();
        }
        let completion_offset = file.as_file().metadata().unwrap().len();
        writeln!(file, "{completion}").unwrap();
        let id = source(&completion, completion_offset);
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let initial = reader.refresh(1).unwrap();
        assert!(reader.oldest > 0);
        assert!(!reader.records.iter().any(|(offset, _)| *offset == 0));
        let retained = initial
            .events
            .iter()
            .find(|event| event.source.as_deref() == Some(&id))
            .unwrap();
        assert_eq!(retained.source_records.len(), 1);
        let expanded = reader.refresh(usize::MAX).unwrap();
        let row = expanded
            .events
            .iter()
            .find(|event| event.source.as_deref() == Some(&id))
            .unwrap();
        assert_eq!(row.body, retained.body);
        assert_eq!(row.started_at, retained.started_at);
        assert_eq!(
            row.source_records
                .iter()
                .map(|record| record.offset)
                .collect::<Vec<_>>(),
            [0, completion_offset]
        );
        assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
    }
}
