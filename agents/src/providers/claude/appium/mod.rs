use crate::event::Section;
use crate::images::Image;
use crate::providers::{mcp::McpServer, tool::*};
use serde_json::Value;

pub(crate) const SERVER: McpServer = McpServer {
    name: "appium-mcp",
    tools: &[
        ToolRule {
            name: "select_device",
            title: &[Pick::With(device_title)],
            input: Some((Pick::With(device_input), Format::Table)),
            output: Some((Pick::Field(Source::Result, "/message"), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_prepare_ios_real_device",
            title: &[Pick::Field(Source::Result, "/mode")],
            input: Some((Pick::Field(Source::Input, "/udid"), Format::Text)),
            output: Some((Pick::With(recommended_profiles), Format::Table)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_session_management",
            title: &[Pick::Field(Source::Input, "/action")],
            input: Some((Pick::With(session_input), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_screenshot",
            input: Some((Pick::With(screenshot_input), Format::Table)),
            output: Some((Pick::With(screenshot), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_find_element",
            aliases: &["appium_find_elements"],
            title: &[Pick::Field(Source::Input, "/strategy")],
            input: Some((Pick::With(selector_input), Format::Code(""))),
            output: Some((Pick::With(element_ids), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_gesture",
            title: &[Pick::Field(Source::Input, "/action")],
            input: Some((Pick::With(gesture_input), Format::Table)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_get_page_source",
            title: &[Pick::Field(Source::Input, "/sessionId")],
            input: Some((Pick::With(empty_input), Format::Text)),
            output: Some((Pick::Field(Source::Result, "/filepath"), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_get_element_attribute",
            title: &[Pick::Field(Source::Input, "/attribute")],
            input: Some((Pick::With(attribute_input), Format::Table)),
            output: Some((Pick::With(attribute_value), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_get_text",
            aliases: &["appium_get_active_element"],
            input: Some((Pick::With(element_input), Format::Table)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_set_value",
            title: &[Pick::Field(Source::Input, "/text")],
            input: Some((Pick::With(set_value_input), Format::Table)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_app_lifecycle",
            title: &[Pick::Field(Source::Input, "/action")],
            input: Some((Pick::With(app_input), Format::Text)),
            ..DEFAULT_RULE
        },
        ToolRule {
            name: "appium_orientation",
            title: &[Pick::Field(Source::Input, "/action")],
            input: Some((Pick::With(orientation_input), Format::Table)),
            ..DEFAULT_RULE
        },
    ],
};

/// The input fields no title or other pick shows, without the session every call carries;
/// a lone element id is shown bare.
fn other_fields(data: &ToolData<'_>, shown: &[&str]) -> Option<Vec<Section>> {
    let mut rest = data.input.as_object()?.clone();
    for key in shown.iter().chain(&["sessionId"]) {
        rest.shift_remove(*key);
    }
    if rest.is_empty() {
        return Some(Vec::new());
    }
    if rest.len() == 1
        && let Some(element) = rest.get("elementUUID").and_then(Value::as_str)
    {
        return Some(vec![Section::Prose {
            text: element.into(),
            markdown: false,
        }]);
    }
    Some(picked_sections(
        &Value::Object(rest),
        "/input",
        Format::Table,
        false,
    ))
}

fn device_title(data: &ToolData<'_>) -> Option<String> {
    let parts: Vec<_> = ["platform", "iosDeviceType"]
        .into_iter()
        .filter_map(|field| text(data.input, field))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn device_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    other_fields(data, &["platform", "iosDeviceType"])
}

/// The recommended provisioning profiles, by name, team and expiry.
fn recommended_profiles(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result.filter(|result| !result.error)?;
    let profiles: Vec<_> = result.structured["recommendedProfiles"]
        .as_array()?
        .iter()
        .map(|profile| {
            serde_json::json!({
                "name": profile["name"],
                "teamName": profile["teamName"],
                "expiresAt": profile["expiresAt"],
            })
        })
        .collect();
    Some(Body::Sections(picked_sections(
        &Value::Array(profiles),
        "/recommendedProfiles",
        Format::Table,
        true,
    )))
}

/// The app a new session launches; other session calls show their remaining fields.
fn session_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    let capabilities: Value = data.input["capabilities"]
        .as_str()
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_else(|| data.input["capabilities"].clone());
    match capabilities["appium:bundleId"].as_str() {
        Some(bundle) => Some(vec![Section::Prose {
            text: bundle.into(),
            markdown: false,
        }]),
        None => other_fields(data, &["action"]),
    }
}

fn screenshot_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    other_fields(data, &[])
}

/// The saved screenshot itself in place of the sentence naming its file.
fn screenshot(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result.filter(|result| !result.error)?;
    let path = result
        .text()
        .trim()
        .strip_prefix("Screenshot saved successfully to: ")?
        .to_owned();
    let image = Image::file(path.into())?;
    Some(Body::Sections(vec![Section::Image(image)]))
}

/// The found elements' ids, one per line.
fn element_ids(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result.filter(|result| !result.error)?;
    let ids: Vec<_> = result.structured["elements"]
        .as_array()?
        .iter()
        .filter_map(|element| element["elementId"].as_str())
        .collect();
    Some(Body::Sections(vec![text_section("output", &ids.join("\n"))]))
}

fn gesture_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    let shown = ["action", "sessionId"];
    if let Some(fields) = data.input.as_object() {
        let remaining: Vec<_> = fields
            .iter()
            .filter(|(key, _)| !shown.contains(&key.as_str()))
            .collect();
        if let [(key, Value::String(id))] = remaining.as_slice()
            && key.as_str() == "elementUUID"
            && !id.is_empty()
            && data.result.is_some_and(|result| result.text().contains(id))
        {
            return empty_input(data);
        }
    }
    other_fields(data, &["action"])
}

/// The selector as code, since predicates and XPath filters run long.
fn selector_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    let selector = text(data.input, "selector")?;
    let mut sections = vec![code_section("selector", "", selector)];
    sections.extend(other_fields(data, &["selector", "strategy"])?);
    Some(sections)
}

fn attribute_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    other_fields(data, &["attribute"])
}

/// The attribute's value without the sentences naming the element.
fn attribute_value(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result.filter(|result| !result.error)?;
    let text = result.text();
    let value = text
        .lines()
        .find(|line| line.starts_with("Attribute \""))?
        .split_once(": ")?
        .1;
    Some(Body::Sections(vec![text_section("output", value)]))
}

fn element_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    other_fields(data, &[])
}

fn set_value_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    other_fields(data, &["text"])
}

/// The app's bundle id, followed by any other fields.
fn app_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    let mut sections: Vec<_> = text(data.input, "id")
        .map(|id| Section::Prose {
            text: id.into(),
            markdown: false,
        })
        .into_iter()
        .collect();
    sections.extend(other_fields(data, &["action", "id"])?);
    Some(sections)
}

fn orientation_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    other_fields(data, &["action"])
}

#[cfg(test)]
mod tests {
    use crate::event::{Section, ToolCall};
    use serde_json::Value;

    /// The recorded calls, with the screenshot pointed at a real image file.
    fn calls(screenshot: &std::path::Path) -> Vec<ToolCall> {
        let fixture = include_str!("../../../../tests/fixtures/claude-appium.jsonl")
            .replace("/tmp/screenshot_1792876190968.png", screenshot.to_str().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        std::fs::write(&transcript, fixture).unwrap();
        crate::providers::claude::event_reader::tail(&transcript, 200)
            .unwrap()
            .into_iter()
            .flat_map(|event| event.calls)
            .collect()
    }

    fn prose(text: &str) -> Vec<Section> {
        vec![Section::Prose {
            text: text.into(),
            markdown: false,
        }]
    }

    fn output_text(call: &ToolCall) -> String {
        call.output
            .iter()
            .flatten()
            .map(|section| match section {
                Section::Text { text, .. } | Section::Markdown { text, .. } => text.clone(),
                Section::Image(image) => format!("[image {}]", image.label),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn appium_calls_title_their_key_field_and_show_only_what_matters() {
        let dir = tempfile::tempdir().unwrap();
        let screenshot = dir.path().join("screenshot.png");
        image::RgbImage::new(2, 2).save(&screenshot).unwrap();
        let calls = calls(&screenshot);
        assert_eq!(
            calls.iter().map(|call| call.title.as_str()).collect::<Vec<_>>(),
            [
                "ios real",
                "discovery",
                "create",
                "",
                "accessibility id",
                "tap",
                "-ios predicate string",
                "fc1c5393-ccb0-466a-ab9c-c0f2e70a1a9d",
                "label",
                "tap",
                "create",
            ]
        );
        let input = |index: usize| calls[index].input.clone();
        assert_eq!(input(0), Vec::new());
        assert_eq!(input(1), prose("00000000-0000000000000000"));
        assert_eq!(input(2), prose("com.amazon.mp3.AmazonCloudPlayer"));
        // A session without an app lists its remaining fields, the capabilities as their JSON text.
        assert!(matches!(
            &input(10)[..],
            [Section::Markdown { label, text }]
                if label == "input"
                    && text.starts_with(
                        "| platform | capabilities |\n| --- | --- |\n| ios | \\{\\\"appium\\:udid\\\"\\:\\\"00000000\\-0000000000000000\\\"\\,"
                    )
        ));
        assert_eq!(input(5), Vec::new());
        assert_eq!(input(7), Vec::new());
        assert_eq!(input(8), prose("42010000-0000-0000-BE14-000000000000"));
        assert_eq!(
            input(9),
            vec![Section::Markdown {
                label: "input".into(),
                text: "| x | y |\n| --- | --- |\n| 200 | 85 |".into(),
            }]
        );
        assert_eq!(
            output_text(&calls[9]),
            "Successfully tapped at coordinates (200, 85)."
        );
        assert_eq!(
            input(3),
            vec![Section::Markdown {
                label: "input".into(),
                text: "| maxWidth |\n| --- |\n| 400 |".into(),
            }]
        );
        for (index, selector) in [(4, "Playlists"), (6, "label CONTAINS[c] 'classic'")] {
            assert_eq!(
                input(index),
                vec![super::code_section("selector", "", selector)]
            );
        }

        assert_eq!(
            output_text(&calls[0]),
            "✅ Device selected: 00000000-0000000000000000 (00000000-0000000000000000)"
        );
        assert_eq!(
            output_text(&calls[1]),
            "| name | teamName | expiresAt |\n\
             | --- | --- | --- |\n\
             | iOS Team Provisioning Profile\\: \\* | Example Team | 2027\\-09\\-06T21\\:27\\:06\\.000Z |"
        );
        assert_eq!(
            output_text(&calls[3]),
            format!("[image {}]", screenshot.display())
        );
        assert_eq!(
            output_text(&calls[6]),
            "42010000-0000-0000-BE14-000000000000\n\
             B3010000-0000-0000-BE14-000000000000"
        );
        assert_eq!(
            output_text(&calls[8]),
            "ClassicalPrivate  •  51 songs  •  4 hrs 41 min"
        );
    }

    #[test]
    fn gesture_coordinates_and_remaining_fields_use_a_table() {
        let records: Vec<Value> = include_str!("../../../../tests/fixtures/claude-appium.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let mut input = records
            .iter()
            .flat_map(|record| record["message"]["content"].as_array().unwrap())
            .find(|block| block["input"]["x"].is_number())
            .unwrap()["input"]
            .clone();
        input["duration"] = 100.into();
        assert_eq!(
            super::gesture_input(&super::ToolData {
                input: &input,
                result: None
            }),
            Some(vec![Section::Markdown {
                label: "input".into(),
                text: "| x | y | duration |\n| --- | --- | --- |\n| 200 | 85 | 100 |".into(),
            }])
        );
    }

    #[test]
    fn gesture_keeps_input_when_the_result_does_not_cover_it() {
        let records: Vec<Value> = include_str!("../../../../tests/fixtures/claude-appium.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let blocks: Vec<_> = records
            .iter()
            .flat_map(|record| record["message"]["content"].as_array().unwrap())
            .collect();
        let call = blocks
            .iter()
            .find(|block| {
                block["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("appium_gesture"))
            })
            .unwrap();
        let input = &call["input"];
        let result_block = blocks
            .iter()
            .find(|block| block["tool_use_id"] == call["id"])
            .unwrap();
        let mut result = super::ToolResult {
            source: None,
            content: result_block["content"].clone(),
            structured: Value::Null,
            error: false,
        };
        fn data<'a>(
            input: &'a Value,
            result: Option<&'a super::ToolResult>,
        ) -> super::ToolData<'a> {
            super::ToolData { input, result }
        }
        assert_eq!(
            super::gesture_input(&data(input, Some(&result))),
            Some(Vec::new())
        );
        assert!(!super::gesture_input(&data(input, None)).unwrap().is_empty());
        let mut extra = input.clone();
        extra["duration"] = 100.into();
        assert!(
            !super::gesture_input(&data(&extra, Some(&result)))
                .unwrap()
                .is_empty()
        );
        result.content = Value::String("Tap failed".into());
        assert!(
            !super::gesture_input(&data(input, Some(&result)))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn appium_app_lifecycle_titles_its_action_and_shows_the_app_id() {
        let input = serde_json::json!({
            "action": "activate",
            "id": "com.amazon.mp3.AmazonCloudPlayer",
            "sessionId": "85f36cd8-4ade-476f-b3f1-814d9ac36275"
        });
        let shown = super::presentation(
            "mcp__plugin_mobile_appium-mcp__appium_app_lifecycle",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        assert_eq!(shown.title.as_deref(), Some("activate"));
        assert_eq!(shown.input, prose("com.amazon.mp3.AmazonCloudPlayer"));
    }

    #[test]
    fn page_source_titles_the_session_and_outputs_only_the_file_path() {
        let fixture = include_str!("../../../../tests/fixtures/claude-appium-page-source.jsonl");
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("transcript.jsonl");
        std::fs::write(&transcript, fixture).unwrap();
        let events = crate::providers::claude::event_reader::tail(&transcript, 10).unwrap();
        let call = &events[0].calls[0];
        assert_eq!(call.title, "fc1c5393-ccb0-466a-ab9c-c0f2e70a1a9d");
        assert!(call.input.is_empty());
        assert_eq!(
            call.output,
            Some(vec![super::text_section(
                "output",
                "/tmp/page-source-b8d54b54-36f0-415d-99f5-20d6bcb9b36e.xml"
            )])
        );
    }

    #[test]
    fn appium_screenshot_without_its_file_keeps_the_recorded_text() {
        let calls = calls(std::path::Path::new("/nonexistent/screenshot.png"));
        assert!(output_text(&calls[3]).starts_with("Screenshot saved successfully to: "));
    }
}
