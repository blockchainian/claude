use crate::event::SourceRecord;
use crate::images::{Image, ImageSource};
use serde_json::Value;
use std::collections::HashMap;

/// The images embedded in prompts; queued prompts arrive before their deliveries in a backwards walk.
#[derive(Default)]
pub(super) struct PromptImages {
    queued: HashMap<String, (SourceRecord, Value)>,
    companions: HashMap<String, Vec<String>>,
    parents: HashMap<String, String>,
    delivered: HashMap<String, Vec<(SourceRecord, Value)>>,
}

impl PromptImages {
    pub(super) fn record(&mut self, source: &SourceRecord, record: &Value) {
        if let (Some(id), Some(parent)) = (record["uuid"].as_str(), record["parentUuid"].as_str()) {
            self.parents.insert(parent.to_owned(), id.to_owned());
        }
        if record["turnCompanion"] == true {
            let paths: Vec<_> = record["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|block| {
                    block["text"]
                        .as_str()?
                        .strip_prefix("[Image: source: ")?
                        .strip_suffix(']')
                        .map(str::to_owned)
                })
                .collect();
            for key in [record["promptId"].as_str(), record["uuid"].as_str()]
                .into_iter()
                .flatten()
            {
                self.companions.insert(key.to_owned(), paths.clone());
            }
        }
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
        let paths = record["promptId"]
            .as_str()
            .and_then(|id| self.companions.get(id))
            .or_else(|| {
                let mut id = record["uuid"].as_str()?;
                for _ in 0..self.parents.len() + 1 {
                    if let Some(paths) = self.companions.get(id) {
                        return Some(paths);
                    }
                    id = self.parents.get(id)?;
                }
                None
            });
        let mut seen = Vec::new();
        image_blocks(record)
            .enumerate()
            .flat_map(|(image_index, (index, label, block))| {
                let pointer = format!("{prefix}/{index}/source/data");
                let encoded = block["source"]["data"].as_str();
                let first = encoded.and_then(|encoded| {
                    seen.iter()
                        .find(|(data, _)| *data == encoded)
                        .map(|(_, pointer): &(&str, String)| pointer.clone())
                });
                let duplicate = first.is_some();
                let pointer = first.unwrap_or(pointer);
                if !duplicate && let Some(encoded) = encoded {
                    seen.push((encoded, pointer.clone()));
                }
                let source = ImageSource::Record {
                    record: source.clone(),
                    pointer,
                };
                let mut images = Vec::new();
                if !duplicate {
                    let label = label
                        .filter(|label| body.contains(label))
                        .unwrap_or_else(|| image_caption(block));
                    images.push(Image {
                        label,
                        source: source.clone(),
                    });
                }
                if let Some(path) = paths
                    .and_then(|paths| paths.get(image_index))
                    .filter(|path| body.contains(path.as_str()))
                {
                    images.push(Image {
                        label: path.clone(),
                        source,
                    });
                }
                images
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

#[cfg(test)]
mod f4_tests {
    use super::*;
    #[test]
    fn f4_companion_path_uses_record_and_duplicate_blocks_share_source() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-prompt-images.jsonl");
        let records: Vec<Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let mut prompt = records[0].clone();
        let block = prompt["message"]["content"][1].clone();
        prompt["message"]["content"]
            .as_array_mut()
            .unwrap()
            .push(block);
        let body = "/redacted/claude-session/images/6.png [Image #6]";
        let source = SourceRecord { path, offset: 0 };
        let mut images = PromptImages::default();
        for record in records[..4].iter().rev() {
            images.record(&source, record);
        }
        let found = images.take(&source, &prompt, body);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].source, found[1].source);
        assert_eq!(found[1].label, "/redacted/claude-session/images/6.png");
    }
}
