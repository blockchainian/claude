use crate::event::SourceRecord;
use crate::images::{Image, ImageSource};
use serde_json::Value;
use std::collections::HashMap;

/// The images embedded in prompts; queued prompts arrive before their deliveries in a backwards walk.
#[derive(Default)]
pub(super) struct PromptImages {
    queued: HashMap<String, (SourceRecord, Value)>,
    delivered: HashMap<String, Vec<(SourceRecord, Value)>>,
}

impl PromptImages {
    pub(super) fn record(&mut self, source: &SourceRecord, record: &Value) {
        if queued_prompt(record).is_some()
            && let Some(id) = record["attachment"]["source_uuid"].as_str()
        {
            self.queued
                .insert(id.to_owned(), (source.clone(), record.clone()));
        }
        if record["type"] == "queue-operation"
            && record["operation"] == "remove"
            && let Some(delivery) = record["commandUuid"]
                .as_str()
                .and_then(|id| self.queued.remove(id))
            && let Some(text) = record["content"].as_str()
        {
            self.delivered
                .entry(text.to_owned())
                .or_default()
                .push(delivery);
        }
    }

    pub(super) fn take(&mut self, source: &SourceRecord, record: &Value, body: &str) -> Vec<Image> {
        if record["type"] == "queue-operation"
            && record["operation"] == "enqueue"
            && let Some((source, delivery)) = record["content"]
                .as_str()
                .and_then(|text| self.delivered.get_mut(text))
                .and_then(Vec::pop)
        {
            return self.take(&source, &delivery, body);
        }
        let prefix = if queued_prompt(record).is_some() {
            "/attachment/prompt"
        } else {
            "/message/content"
        };
        image_blocks(record)
            .map(|(index, label, block)| {
                let label = label
                    .filter(|label| body.contains(label))
                    .unwrap_or_else(|| image_caption(block));
                Image {
                    label,
                    source: ImageSource::Record {
                        record: source.clone(),
                        pointer: format!("{prefix}/{index}/source/data"),
                    },
                }
            })
            .collect()
    }
}

/// A queued human prompt may be plain text or a sequence of text and image blocks.
pub(super) fn queued_prompt(record: &Value) -> Option<&Value> {
    let attachment = &record["attachment"];
    (record["type"] == "attachment"
        && attachment["type"] == "queued_command"
        && attachment["commandMode"] == "prompt")
        .then_some(&attachment["prompt"])
}

/// Prompt text, or image labels when a prompt contains only images.
pub(super) fn prompt_text(record: &Value) -> Option<String> {
    let content = queued_prompt(record).unwrap_or(&record["message"]["content"]);
    let text = if let Some(text) = content.as_str() {
        text.to_owned()
    } else {
        content
            .as_array()?
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    if text.trim().is_empty() {
        let labels = image_blocks(record)
            .map(|(_, label, block)| label.unwrap_or_else(|| image_caption(block)))
            .collect::<Vec<_>>();
        Some(if labels.is_empty() {
            text
        } else {
            labels.join(" ")
        })
    } else {
        Some(text)
    }
}

fn image_blocks(record: &Value) -> impl Iterator<Item = (usize, Option<String>, &Value)> {
    let (prompt, content) = if let Some(prompt) = queued_prompt(record) {
        (&record["attachment"], prompt)
    } else {
        (record, &record["message"]["content"])
    };
    content
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, block)| block["type"] == "image")
        .enumerate()
        .map(move |(image_index, (index, block))| {
            let label = prompt["imagePasteIds"][image_index]
                .as_u64()
                .map(|number| format!("[Image #{number}]"));
            (index, label, block)
        })
}

fn image_caption(block: &Value) -> String {
    let mime = block["source"]["media_type"].as_str().filter(|mime| {
        mime.strip_prefix("image/").is_some_and(|subtype| {
            !subtype.is_empty()
                && subtype
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        })
    });
    match mime {
        Some(mime) => format!("[image {mime}]"),
        None => "[image]".to_owned(),
    }
}
