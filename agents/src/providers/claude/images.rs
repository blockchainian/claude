use crate::{event::SourceRecord, images::{Image, ImageSource}};
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

    pub(super) fn take(
        &mut self,
        source: &SourceRecord,
        record: &Value,
        body: &str,
    ) -> Vec<Image> {
        if record["type"] == "queue-operation"
            && record["operation"] == "enqueue"
            && let Some(delivery) = record["content"]
                .as_str()
                .and_then(|text| self.delivered.get_mut(text))
                .and_then(Vec::pop)
        {
            return self.take(&delivery.0, &delivery.1, body);
        }
        let (prompt, content, prefix) = if let Some(prompt) = queued_prompt(record) {
            (&record["attachment"], prompt, "/attachment/prompt")
        } else {
            (record, &record["message"]["content"], "/message/content")
        };
        let ids = prompt["imagePasteIds"].as_array();
        let mut image_number = 0;
        content
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, block)| block["type"] == "image")
            .map(|(index, block)| {
                let id = ids
                    .and_then(|ids| ids.get(image_number))
                    .and_then(Value::as_u64);
                image_number += 1;
                let tagged = id.map(|id| format!("[Image #{id}]"));
                let label = tagged
                    .filter(|label| body.contains(label))
                    .unwrap_or_else(|| {
                        let mime = block["source"]["media_type"].as_str().unwrap_or("image");
                        format!("[image {mime}]")
                    });
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
