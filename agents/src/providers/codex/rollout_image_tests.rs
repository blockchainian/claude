use super::*;
use std::io::{BufReader, Write};

fn captured(name: &str) -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex-images")
        .join(name);
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn transcript(records: &[Value]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for record in records {
        writeln!(file, "{record}").unwrap();
    }
    file
}

fn images(events: &[Event]) -> Vec<&Image> {
    events
        .iter()
        .flat_map(|event| &event.calls)
        .flat_map(|call| call.output.iter().flatten())
        .filter_map(|section| match section {
            Section::Image(image) => Some(image),
            _ => None,
        })
        .collect()
}

fn assert_record(image: &Image, file: &Path, pointer: &str, value: &Value) {
    let ImageSource::Record {
        record,
        pointer: actual,
    } = &image.source
    else {
        panic!("expected transcript image: {image:?}");
    };
    assert_eq!(record.path, file);
    assert_eq!(actual, pointer);
    let mut source = File::open(file).unwrap();
    source.seek(SeekFrom::Start(record.offset)).unwrap();
    let mut line = String::new();
    BufReader::new(source).read_line(&mut line).unwrap();
    let row: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(row.pointer(actual), Some(value));
}

#[test]
fn function_image_blocks_survive_without_mcp_completion() {
    let records: Vec<_> = captured("tool-mcp.jsonl")
        .into_iter()
        .filter(|record| record["type"] == "response_item")
        .collect();
    let file = transcript(&records);
    let events = tail(file.path(), usize::MAX).unwrap();
    let output = events[0].calls[0].output.as_ref().unwrap();
    assert!(matches!(&output[0], Section::Text { text, .. }
        if text == records[1]["payload"]["output"][0]["text"].as_str().unwrap()));
    let found = images(&events);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label, "[image image/jpeg]");
    assert_record(
        found[0],
        file.path(),
        "/payload/output/1/image_url",
        &records[1]["payload"]["output"][1]["image_url"],
    );
}

#[test]
fn completed_mcp_image_wins_over_response_copy() {
    let records = captured("tool-mcp.jsonl");
    for records in [
        records.clone(),
        vec![records[0].clone(), records[2].clone(), records[1].clone()],
    ] {
        let file = transcript(&records);
        let events = tail(file.path(), usize::MAX).unwrap();
        assert_eq!(events.len(), 1);
        let found = images(&events);
        assert_eq!(found.len(), 1);
        assert_record(
            found[0],
            file.path(),
            "/payload/item/result/content/0/data",
            &records
                .iter()
                .find(|record| record["type"] == "event_msg")
                .unwrap()["payload"]["item"]["result"]["content"][0]["data"],
        );
    }
}

#[test]
fn image_view_uses_enclosing_exec_output_after_file_disappears() {
    let mut records = captured("tool-view.jsonl");
    let directory = tempfile::tempdir().unwrap();
    records[1]["payload"]["item"]["path"] =
        url::Url::from_file_path(directory.path().join("missing.png"))
            .unwrap()
            .to_string()
            .into();
    let file = transcript(&records);
    let events = tail(file.path(), usize::MAX).unwrap();
    assert_eq!(events.len(), 1);
    let found = images(&events);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label, "[image image/png]");
    assert_record(
        found[0],
        file.path(),
        "/payload/output/2/image_url",
        &records[2]["payload"]["output"][2]["image_url"],
    );
    assert_eq!(events[0].source_records.len(), 3);
}

#[test]
fn image_view_decodes_file_uri_when_no_output_is_available() {
    let mut records = captured("tool-view.jsonl");
    records.truncate(2);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image #1.png");
    let bytes = crate::images::fixture_png();
    std::fs::write(&path, &bytes).unwrap();
    records[1]["payload"]["item"]["path"] =
        url::Url::from_file_path(&path).unwrap().to_string().into();
    let file = transcript(&records);
    let events = tail(file.path(), usize::MAX).unwrap();
    let found = images(&events);
    assert_eq!(found.len(), 1);
    let ImageSource::File {
        path: actual,
        len,
        modified,
    } = &found[0].source
    else {
        panic!("expected file image");
    };
    assert_eq!(actual, &path);
    assert_eq!(*len, bytes.len() as u64);
    assert_eq!(
        *modified,
        std::fs::metadata(&path).unwrap().modified().unwrap()
    );
    assert_eq!(found[0].source.bytes().unwrap(), bytes);
}

#[test]
fn image_views_with_reversed_outputs_use_files_or_labels() {
    use base64::Engine;
    let mut records = captured("tool-view.jsonl");
    let directory = tempfile::tempdir().unwrap();
    let first_path = directory.path().join("first.png");
    let second_path = directory.path().join("second.png");
    let first_bytes = crate::images::fixture_png();
    let mut second_image = image::load_from_memory(&first_bytes).unwrap();
    second_image.invert();
    let mut second_bytes = std::io::Cursor::new(Vec::new());
    second_image
        .write_to(&mut second_bytes, image::ImageFormat::Png)
        .unwrap();
    let second_bytes = second_bytes.into_inner();
    assert_ne!(first_bytes, second_bytes);
    std::fs::write(&first_path, &first_bytes).unwrap();
    std::fs::write(&second_path, &second_bytes).unwrap();
    records[1]["payload"]["item"]["path"] = url::Url::from_file_path(&first_path)
        .unwrap()
        .to_string()
        .into();
    let mut second = records[1].clone();
    second["payload"]["item"]["id"] = "second-image-view".into();
    second["payload"]["item"]["path"] = url::Url::from_file_path(&second_path)
        .unwrap()
        .to_string()
        .into();
    let mut block = records[2]["payload"]["output"][2].clone();
    let data_url = |bytes: &[u8]| {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    };
    records[2]["payload"]["output"][2]["image_url"] = data_url(&second_bytes).into();
    block["image_url"] = data_url(&first_bytes).into();
    records[2]["payload"]["output"]
        .as_array_mut()
        .unwrap()
        .push(block);
    records.insert(2, second);
    let file = transcript(&records);
    for missing in 0..=2 {
        if missing == 1 {
            std::fs::remove_file(&first_path).unwrap();
        } else if missing == 2 {
            std::fs::remove_file(&second_path).unwrap();
        }
        let events = tail(file.path(), usize::MAX).unwrap();
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        assert_eq!(calls.len(), 2);
        for (index, (path, bytes)) in [(&first_path, &first_bytes), (&second_path, &second_bytes)]
            .into_iter()
            .enumerate()
        {
            let found = images(&events);
            if index < missing {
                assert!(
                    calls[index].output.as_ref().is_some_and(Vec::is_empty),
                    "missing file keeps only its tool label"
                );
                assert!(calls[index].input.iter().any(|section| matches!(section, Section::Structured { text, .. } if serde_json::from_str::<Value>(text).unwrap()["path"] == url::Url::from_file_path(path).unwrap().as_str())));
            } else {
                let image = found
                    .iter()
                    .find(|image| {
                        matches!(&image.source,
                    ImageSource::File { path: actual, .. } if actual == path)
                    })
                    .expect("ambiguous wrapper must use each ImageView's own file");
                assert_eq!(image.source.bytes().as_ref(), Some(bytes));
            }
            assert_eq!(found.len(), 2 - missing);
        }
    }
}

#[test]
fn image_view_with_multiple_output_images_uses_file() {
    let mut records = captured("tool-view.jsonl");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fallback.png");
    std::fs::write(&path, crate::images::fixture_png()).unwrap();
    records[1]["payload"]["item"]["path"] =
        url::Url::from_file_path(&path).unwrap().to_string().into();
    let block = records[2]["payload"]["output"][2].clone();
    records[2]["payload"]["output"]
        .as_array_mut()
        .unwrap()
        .push(block);
    let file = transcript(&records);
    let events = tail(file.path(), usize::MAX).unwrap();
    let found = images(&events);
    assert_eq!(found.len(), 1);
    assert!(matches!(&found[0].source, ImageSource::File { path: actual, .. } if actual == &path));
}

#[test]
fn image_view_does_not_guess_between_calls_or_turns() {
    let mut records = captured("tool-view.jsonl");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fallback.png");
    std::fs::write(&path, crate::images::fixture_png()).unwrap();
    records[1]["payload"]["item"]["path"] =
        url::Url::from_file_path(&path).unwrap().to_string().into();
    let mut other_turn = records.clone();
    other_turn[1]["payload"]["turn_id"] = "another-turn".into();
    let mut ambiguous = records.clone();
    let mut other_call = records[0].clone();
    other_call["payload"]["call_id"] = "another-call".into();
    let mut other_output = records[2].clone();
    other_output["payload"]["call_id"] = "another-call".into();
    ambiguous.insert(1, other_call);
    ambiguous.push(other_output);
    for records in [other_turn, ambiguous] {
        let file = transcript(&records);
        let events = tail(file.path(), usize::MAX).unwrap();
        let found = images(&events);
        assert_eq!(found.len(), 1);
        assert!(
            matches!(&found[0].source, ImageSource::File { path: actual, .. } if actual == &path)
        );
    }
}

#[test]
fn generated_image_suppresses_async_output_copy() {
    let records = captured("tool-generation.jsonl");
    let file = transcript(&records);
    let events = tail(file.path(), usize::MAX).unwrap();
    let found = images(&events);
    assert_eq!(found.len(), 1);
    assert_record(
        found[0],
        file.path(),
        "/payload/item/result",
        &records[2]["payload"]["item"]["result"],
    );
}

#[test]
fn generated_image_does_not_suppress_other_tool_returns() {
    let mut records = captured("tool-generation.jsonl");
    let call = captured("tool-mcp.jsonl").remove(0);
    records[3] = call;
    records[4]["payload"]["call_id"] = records[3]["payload"]["call_id"].clone();
    let file = transcript(&records);
    let events = tail(file.path(), usize::MAX).unwrap();
    assert_eq!(images(&events).len(), 2);
}

#[test]
fn tool_images_remain_correct_across_appends() {
    for name in ["tool-view.jsonl", "tool-generation.jsonl", "tool-mcp.jsonl"] {
        let records = captured(name);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        for record in records {
            writeln!(file, "{record}").unwrap();
            let actual = reader.refresh(usize::MAX).unwrap();
            let expected = tail(file.path(), usize::MAX).unwrap();
            assert_eq!(actual.events, expected, "{name}");
        }
        assert_eq!(
            images(&reader.refresh(usize::MAX).unwrap().events).len(),
            1,
            "{name}"
        );
    }
}

#[test]
fn tool_images_remain_correct_across_backward_pages() {
    for name in ["tool-view.jsonl", "tool-generation.jsonl", "tool-mcp.jsonl"] {
        let mut records = captured(name);
        for record in &mut records {
            record["paging_padding"] = "x".repeat(CHUNK as usize).into();
        }
        let file = transcript(&records);
        let expected = tail(file.path(), usize::MAX).unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        reader.refresh(1).unwrap();
        let actual = reader.refresh(usize::MAX).unwrap();
        assert_eq!(actual.events, expected, "{name}");
        assert_eq!(images(&actual.events).len(), 1, "{name}");
    }
}

#[test]
fn async_image_copy_is_removed_when_older_generation_is_loaded() {
    let mut records = captured("tool-generation.jsonl");
    records.extend(captured("tool-mcp.jsonl"));
    records.sort_by_key(|record| record["ordinal"].as_u64().unwrap());
    for record in &mut records {
        record["paging_padding"] = "x".repeat(CHUNK as usize).into();
    }
    let file = transcript(&records);
    let mut reader = Reader::new(file.path().to_path_buf(), false);
    reader.refresh(1).unwrap();
    let actual = reader.refresh(usize::MAX).unwrap();
    assert_eq!(actual.events, tail(file.path(), usize::MAX).unwrap());
    assert_eq!(images(&actual.events).len(), 2);
}

#[test]
fn selected_tool_images_keep_their_unselected_context() {
    for name in ["tool-view.jsonl", "tool-generation.jsonl", "tool-mcp.jsonl"] {
        let file = transcript(&captured(name));
        let text = std::fs::read_to_string(file.path()).unwrap();
        let mut at = 0;
        let records: Vec<_> = text
            .lines()
            .map(|line| {
                let offset = at;
                at += line.len() as u64 + 1;
                (offset, serde_json::from_str::<Value>(line).unwrap())
            })
            .collect();
        for (offset, mut expected) in events(file.path(), &records, usize::MAX).unwrap() {
            let actual =
                events_selected(file.path(), &records, usize::MAX, &HashSet::from([offset]))
                    .unwrap();
            expected.previous_end = None;
            assert_eq!(actual, vec![(offset, expected)], "{name}");
        }
    }
}

#[test]
fn backward_pages_hash_each_image_record_once() {
    let mut records = captured("tool-generation.jsonl");
    records.extend(captured("tool-mcp.jsonl"));
    for record in &mut records {
        record["paging_padding"] = "x".repeat(CHUNK as usize).into();
    }
    let expected: usize = records
        .iter()
        .flat_map(|record| {
            let payload = &record["payload"];
            let generated = (payload["item"]["kind"] == "image_gen.generation")
                .then(|| payload["item"]["result"].as_str())
                .flatten();
            generated
                .into_iter()
                .chain(output_image_blocks(&payload["output"]).map(|(_, url)| url))
        })
        .map(|url| image_data(url).len())
        .sum();
    let file = transcript(&records);
    IMAGE_HASH_BYTES.with(|count| count.set(0));
    let mut reader = Reader::new(file.path().to_path_buf(), false);
    reader.refresh(1).unwrap();
    reader.refresh(usize::MAX).unwrap();
    assert_eq!(IMAGE_HASH_BYTES.with(std::cell::Cell::get), expected);
}

#[test]
fn image_view_interval_lookup_scales_with_records() {
    let fixture = captured("tool-view.jsonl");
    let mut records = Vec::new();
    for index in 0..128 {
        let mut group = fixture.clone();
        for record in &mut group {
            record["payload"]["call_id"] = format!("wrapper-{index}").into();
        }
        records.extend(group);
    }
    let records: Vec<_> = records
        .into_iter()
        .enumerate()
        .map(|(index, record)| (index as u64, record))
        .collect();
    IMAGE_INTERVAL_CHECKS.with(|count| count.set(0));
    let found = image_views(Path::new("captured.jsonl"), &records);
    assert_eq!(found.len(), 128);
    assert!(IMAGE_INTERVAL_CHECKS.with(std::cell::Cell::get) <= records.len());
}

#[test]
fn image_view_index_keeps_strict_intervals_and_record_order() {
    let fixture = captured("tool-view.jsonl");
    let mut output = fixture[2].clone();
    let block = output["payload"]["output"][2].clone();
    output["payload"]["output"]
        .as_array_mut()
        .unwrap()
        .push(block);
    let mut nested_call = fixture[0].clone();
    nested_call["payload"]["call_id"] = "nested".into();
    let mut nested_output = fixture[2].clone();
    nested_output["payload"]["call_id"] = "nested".into();
    let records = vec![
        (10, fixture[0].clone()),
        (29, fixture[1].clone()),
        (11, fixture[1].clone()),
        (10, fixture[1].clone()),
        (30, fixture[1].clone()),
        (15, nested_call),
        (20, fixture[1].clone()),
        (25, nested_output),
        (30, output),
    ];
    let found = image_views(Path::new("captured.jsonl"), &records);
    assert!(
        found.is_empty(),
        "multiple ImageViews cannot be paired by order"
    );
}

#[test]
fn pending_image_record_reuses_hash_when_completed() {
    let records = captured("tool-generation.jsonl");
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let mut reader = Reader::new(file.path().to_path_buf(), false);
    IMAGE_HASH_BYTES.with(|count| count.set(0));
    for record in records {
        write!(file, "{record}").unwrap();
        reader.refresh(usize::MAX).unwrap();
        let before = IMAGE_HASH_BYTES.with(std::cell::Cell::get);
        writeln!(file).unwrap();
        reader.refresh(usize::MAX).unwrap();
        assert_eq!(IMAGE_HASH_BYTES.with(std::cell::Cell::get), before);
    }
    assert_eq!(
        reader.refresh(usize::MAX).unwrap().events,
        tail(file.path(), usize::MAX).unwrap()
    );
}

#[test]
fn f4_prompt_paths_prefer_record_and_duplicate_output_blocks_are_removed() {
    let mut records = captured("wrapped.jsonl");
    records[0]["payload"]["content"][3]["text"] =
        Value::String("bug: [Image #1] /tmp/f4-image.png".into());
    records[1]["payload"]["item"]["content"][1]["text"] =
        records[0]["payload"]["content"][3]["text"].clone();
    records[1]["payload"]["item"]["content"][0]["path"] = Value::String("/tmp/f4-image.png".into());
    let prompt = user_prompt(&records[0]["payload"]["content"]);
    let found = prompt_images(
        &prompt,
        &records[0]["payload"],
        Some(&records[1]),
        crate::event::SourceRecord {
            path: PathBuf::from("unused"),
            offset: 0,
        },
    );
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].source, found[1].source);
    let mut records = captured("tool-mcp.jsonl");
    records.retain(|r| r["type"] == "response_item");
    let block = records[1]["payload"]["output"][1].clone();
    records[1]["payload"]["output"]
        .as_array_mut()
        .unwrap()
        .push(block);
    let file = transcript(&records);
    assert_eq!(images(&tail(file.path(), usize::MAX).unwrap()).len(), 1);
}
