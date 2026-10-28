use super::*;
use serde_json::{Value, json};
use std::os::unix::{ffi::OsStringExt, fs::symlink};

const ROUNDTRIP: &str =
    include_str!("../../../../tests/fixtures/claude-daemon-socket/protocol-roundtrip.json");
const RESULTS: &str = include_str!("../../../../tests/fixtures/claude-daemon-socket/results.json");
const EXTRA_RESULTS: &str =
    include_str!("../../../../tests/fixtures/claude-daemon-socket/extra-results.json");

fn roundtrip(index: usize, field: &str) -> Vec<u8> {
    serde_json::from_str::<Value>(ROUNDTRIP).unwrap()[index][field]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec()
}

fn ping() -> Value {
    serde_json::from_slice(&roundtrip(0, "response_line")).unwrap()
}

fn reply() -> Value {
    serde_json::from_str::<Value>(RESULTS).unwrap()[0]["ack"].clone()
}

fn encoded(value: &Value) -> Vec<u8> {
    // Mutated captured payloads retain the wire line delimiter.
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    bytes
}

#[test]
fn request_matches_every_captured_field_and_has_one_line_delimiter() {
    let captured = roundtrip(3, "request_line");
    let expected: Value = serde_json::from_slice(&captured).unwrap();
    let frame = reply_frame("c8b6f19b", "POC_STOPPED_JOB", "<redacted>").unwrap();
    assert_eq!(frame.last(), Some(&b'\n'));
    assert_eq!(frame.iter().filter(|&&b| b == b'\n').count(), 1);
    let actual: Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.as_object().unwrap().len(), 6);
    assert_eq!(actual["proto"], 1);
    assert_eq!(actual["op"], "reply");
    assert_eq!(actual["short"], "c8b6f19b");
    assert_eq!(actual["text"], "POC_STOPPED_JOB");
    assert_eq!(actual["auth"], "<redacted>");
    assert_eq!(actual["nextTurn"], true);
}

#[test]
fn request_escapes_multiline_unicode_quotes_and_backslashes() {
    let text = "第一行\nsecond\t\"quoted\" \\ ✓";
    let frame = reply_frame("c8b6f19b", text, "<redacted>").unwrap();
    assert_eq!(frame.iter().filter(|&&b| b == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(value["text"], text);
    assert_eq!(value["nextTurn"], true);
}

#[test]
fn client_allows_slash_and_bang_as_literal_text() {
    for text in ["/model opus", "!echo hello", "  /effort high", "\t!pwd"] {
        let frame = reply_frame("c8b6f19b", text, "<redacted>").unwrap();
        let value: Value = serde_json::from_slice(&frame).unwrap();
        assert_eq!(value["text"], text);
        assert_eq!(value["nextTurn"], true);
    }
}

#[test]
fn request_rejects_empty_text_and_invalid_job_ids() {
    assert_eq!(
        reply_frame("c8b6f19b", "", "<redacted>")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    for id in ["", "-option", ".", "..", "a/b", "/absolute", "job/"] {
        assert_eq!(
            reply_frame(id, "hello", "<redacted>").unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
            "{id:?}"
        );
    }
}

#[test]
fn request_rejects_every_control_character_except_newline_and_tab() {
    for ch in (0..=0x9f)
        .filter_map(char::from_u32)
        .filter(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
    {
        let text = format!("before{ch}after");
        assert_eq!(
            reply_frame("c8b6f19b", &text, "<redacted>")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
            "U+{:04X}",
            ch as u32
        );
    }
}

#[test]
fn accepts_captured_success_replies() {
    for source in [RESULTS, EXTRA_RESULTS] {
        let cases: Value = serde_json::from_str(source).unwrap();
        for case in cases
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["ack"]["ok"] == true)
        {
            validate_reply(&encoded(&case["ack"])).unwrap();
        }
    }
}

#[test]
fn captured_rejections_preserve_code_and_reason() {
    for index in [2, 3] {
        let bytes = roundtrip(index, "response_line");
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let error = validate_reply(&bytes).unwrap_err().to_string();
        assert!(error.starts_with("Claude daemon rejected input"), "{error}");
        assert!(!error.contains("Delivery unconfirmed;"), "{error}");
        assert!(error.contains(value["code"].as_str().unwrap()), "{error}");
        assert!(error.contains(value["error"].as_str().unwrap()), "{error}");
    }
    let case = &serde_json::from_str::<Value>(EXTRA_RESULTS).unwrap()[0]["ack"];
    let error = validate_reply(&encoded(case)).unwrap_err().to_string();
    assert!(error.starts_with("Claude daemon rejected input"), "{error}");
    assert!(!error.contains("Delivery unconfirmed;"), "{error}");
    assert!(error.contains("EUNKNOWN"), "{error}");
    assert!(error.contains(case["error"].as_str().unwrap()), "{error}");
}

#[test]
fn rejection_codes_are_not_lost() {
    let mut value: Value = serde_json::from_slice(&roundtrip(3, "response_line")).unwrap();
    // Code mutations of the captured ENOJOB refusal, not synthetic successes.
    for code in [
        "EAUTH",
        "ENOJOB",
        "ENOREPLY",
        "ERESPAWNING",
        "ESKEW",
        "EPROTO",
    ] {
        value["code"] = json!(code);
        let message = validate_reply(&encoded(&value)).unwrap_err().to_string();
        assert!(
            message.starts_with("Claude daemon rejected input"),
            "{message}"
        );
        assert!(!message.contains("Delivery unconfirmed;"), "{message}");
        assert!(message.contains(code), "{message}");
    }
}

fn assert_unknown(frame: &[u8]) {
    let message = validate_reply(frame).unwrap_err().to_string();
    assert!(
        message.starts_with("Delivery unconfirmed; check the session before resending."),
        "delivery uncertainty missing: {message}"
    );
}

#[test]
fn malformed_reply_reports_unknown_delivery() {
    let captured = encoded(&reply());
    assert_unknown(&captured[..captured.len() / 2]);
    assert_unknown(b"");
    assert_unknown(b"\xff\n");
    for value in [Value::Null, json!([]), json!(true)] {
        assert_unknown(&encoded(&value));
    }
}

#[test]
fn incomplete_or_wrong_success_reply_reports_unknown_delivery() {
    for field in ["ok", "op"] {
        let mut value = reply();
        value.as_object_mut().unwrap().remove(field);
        assert_unknown(&encoded(&value));
    }
    for (field, invalid) in [
        ("ok", json!("true")),
        ("ok", Value::Null),
        ("op", json!("ping")),
        ("op", json!(1)),
    ] {
        let mut value = reply();
        value[field] = invalid;
        assert_unknown(&encoded(&value));
    }
}

#[test]
fn incomplete_rejection_reports_unknown_delivery() {
    let captured: Value = serde_json::from_slice(&roundtrip(3, "response_line")).unwrap();
    for field in ["ok", "code", "error"] {
        let mut value = captured.clone();
        value.as_object_mut().unwrap().remove(field);
        assert_unknown(&encoded(&value));
    }
    for field in ["code", "error"] {
        for invalid in [Value::Null, json!(1), json!("")] {
            let mut value = captured.clone();
            value[field] = invalid;
            assert_unknown(&encoded(&value));
        }
    }
}

#[test]
fn reply_accepts_additional_fields() {
    let mut value = reply();
    value["diagnostic"] = json!({"future": true});
    validate_reply(&encoded(&value)).unwrap();
}

#[test]
fn complete_json_without_line_delimiter_is_not_an_acknowledgement() {
    let mut reply_bytes = encoded(&reply());
    reply_bytes.pop();
    assert_unknown(&reply_bytes);
    let mut ping_bytes = roundtrip(0, "response_line");
    ping_bytes.pop();
    assert!(validate_ping(&ping_bytes).is_err());
}

#[test]
fn frame_limit_includes_line_delimiter() {
    for mut value in [reply(), ping()] {
        value["padding"] = json!("");
        let overhead = encoded(&value).len();
        value["padding"] = json!("x".repeat(64 * 1024 - overhead));
        let boundary = encoded(&value);
        assert_eq!(boundary.len(), 64 * 1024);
        if value["op"] == "reply" {
            validate_reply(&boundary).unwrap();
        } else {
            validate_ping(&boundary).unwrap();
        }
        let mut oversized = boundary;
        oversized.insert(oversized.len() - 1, b' ');
        if value["op"] == "reply" {
            assert_unknown(&oversized);
        } else {
            assert!(validate_ping(&oversized).is_err());
        }
    }
}

#[test]
fn multiple_lines_are_not_a_single_acknowledgement() {
    let mut replies = encoded(&reply());
    replies.extend(encoded(&reply()));
    assert_unknown(&replies);
    let mut pings = roundtrip(0, "response_line");
    pings.extend(roundtrip(1, "response_line"));
    assert!(validate_ping(&pings).is_err());
}

#[test]
fn captured_ping_reports_protocol_even_for_different_probe_protocol() {
    for index in [0, 1] {
        validate_ping(&roundtrip(index, "response_line")).unwrap();
    }
}

#[test]
fn ping_requires_each_handshake_field() {
    for field in ["ok", "op", "proto"] {
        let mut value = ping();
        value.as_object_mut().unwrap().remove(field);
        assert!(validate_ping(&encoded(&value)).is_err(), "missing {field}");
    }
}

#[test]
fn ping_requires_exact_protocol() {
    for (field, invalid) in [
        ("ok", json!(false)),
        ("ok", json!("true")),
        ("op", json!("reply")),
        ("op", Value::Null),
        ("proto", json!(2)),
        ("proto", json!("1")),
        ("proto", Value::Null),
    ] {
        let mut value = ping();
        value[field] = invalid;
        assert!(validate_ping(&encoded(&value)).is_err(), "{value}");
    }
}

#[test]
fn ping_accepts_any_daemon_version_speaking_the_protocol() {
    for version in [json!("2.1.312"), Value::Null] {
        let mut value = ping();
        value["version"] = version;
        validate_ping(&encoded(&value)).unwrap();
    }
}

#[test]
fn ping_accepts_additional_fields_but_rejects_malformed_json() {
    let mut value = ping();
    value["future"] = json!([1, 2]);
    validate_ping(&encoded(&value)).unwrap();
    let captured = roundtrip(0, "response_line");
    assert!(validate_ping(&captured[..captured.len() / 2]).is_err());
    for bytes in [b"".as_slice(), b"null\n", b"[]\n", b"\xff\n"] {
        assert!(validate_ping(bytes).is_err());
    }
}

#[test]
fn absolute_paths_resolve_lexically_without_filesystem_access() {
    for (input, expected) in [
        ("/claude-config/./daemon/../", "/claude-config"),
        ("//claude-config///", "/claude-config"),
        ("/../../claude-config", "/claude-config"),
        ("/claude-config/../..", "/"),
        ("/", "/"),
    ] {
        assert_eq!(
            absolute_config_dir(Path::new(input)).unwrap(),
            PathBuf::from(expected)
        );
    }
}

#[test]
fn relative_paths_resolve_from_current_directory() {
    let cwd = std::env::current_dir().unwrap();
    for input in [
        "not-created/config",
        "./not-created//config/",
        "not-created/discard/../config",
    ] {
        assert_eq!(
            absolute_config_dir(Path::new(input)).unwrap(),
            cwd.join("not-created/config")
        );
    }
    assert_eq!(absolute_config_dir(Path::new(".")).unwrap(), cwd);
    assert_eq!(
        absolute_config_dir(Path::new("..")).unwrap(),
        cwd.parent().unwrap()
    );
}

#[test]
fn symlink_paths_keep_lexical_namespace() {
    let directory = tempfile::tempdir().unwrap();
    let link = directory.path().join("link");
    let target = directory.path().join("target/deeper");
    std::fs::create_dir_all(&target).unwrap();
    symlink(&target, &link).unwrap();
    let expected_link = absolute_config_dir(directory.path()).unwrap().join("link");
    assert_eq!(absolute_config_dir(&link).unwrap(), expected_link);
    assert_eq!(
        absolute_config_dir(&link.join("..")).unwrap(),
        absolute_config_dir(directory.path()).unwrap()
    );
    assert_ne!(socket_path(&link).unwrap(), socket_path(&target).unwrap());
}

#[test]
fn paths_reject_empty_and_non_utf8_before_hashing() {
    let invalid = PathBuf::from(std::ffi::OsString::from_vec(b"/config/\xff".to_vec()));
    for path in [Path::new(""), invalid.as_path()] {
        assert_eq!(
            absolute_config_dir(path).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            socket_path(path).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn socket_namespace_matches_independent_sha256_vectors() {
    // Fixed SHA-256 vectors; no hashing implementation duplicated in this test.
    let uid = unsafe { libc::getuid() };
    for (config, namespace) in [("/claude-config", "907581ce"), ("/", "8a5edab2")] {
        assert_eq!(
            socket_path(Path::new(config)).unwrap(),
            PathBuf::from(format!("/tmp/cc-daemon-{uid}/{namespace}/control.sock"))
        );
    }
    assert_eq!(
        socket_path(Path::new("//claude-config/./child/..//")).unwrap(),
        socket_path(Path::new("/claude-config")).unwrap()
    );
}
