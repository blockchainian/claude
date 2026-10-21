use super::{Event, Kind, SourceRecord};

fn raw_record(record: &str) -> std::io::Result<String> {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), record).unwrap();
    Event {
        turn: None,
        source: None,
        completed_time: false,
        background: false,
        source_records: vec![SourceRecord {
            path: file.path().to_owned(),
            offset: 0,
        }],
        kind: Kind::Tool,
        sender: None,
        body: String::new(),
        detail_body: None,
        paths: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        duration: None,
        previous_end: None,
        images: vec![],
        calls: vec![],
    }
    .raw_text()
}

#[test]
fn raw_text_preserves_record_key_order_and_scalar_spelling() {
    let record = r#"{"timestamp":"2026-10-12T02:12:55.062Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"printf \\\"hello\\\"\"}","call_id":"call_1","extra":[{"z":1e+3,"a":"\u0061"},true,null,[],{}]}}"#;
    let actual = raw_record(record).unwrap();
    assert_eq!(actual, record);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::from_str::<serde_json::Value>(record).unwrap()
    );
}

#[test]
fn raw_text_rejects_invalid_records() {
    assert_eq!(
        raw_record(r#"{"type":"assistant",}"#).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn raw_text_keeps_the_key_order_of_a_recorded_codex_call() {
    let record = include_str!("../../tests/fixtures/codex-raw-detail.jsonl")
        .lines()
        .nth(1)
        .unwrap();
    assert_eq!(raw_record(record).unwrap(), record);
}

#[test]
fn raw_text_preserves_string_whitespace_and_duplicate_keys() {
    let record = " { \"z\" : [  ], \"a\" : { }, \"z\" : \" , {} \\n\\t\" } ";
    assert_eq!(raw_record(&(record.to_owned() + "\n")).unwrap(), record);
}
