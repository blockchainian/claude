use crate::event::Image;
use base64::Engine;
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf};
use std::{
    hash::{Hash, Hasher},
    io::{self, Write},
    os::unix::fs::DirBuilderExt,
};

/// The images embedded in prompts; queued prompts arrive before their deliveries in a backwards walk.
#[derive(Default)]
pub(super) struct PromptImages {
    queued: HashMap<String, Value>,
    delivered: HashMap<String, Vec<Value>>,
}

impl PromptImages {
    pub(super) fn record(&mut self, record: &Value) {
        if queued_prompt(record).is_some()
            && let Some(id) = record["attachment"]["source_uuid"].as_str()
        {
            self.queued.insert(id.to_owned(), record.clone());
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

    pub(super) fn take(&mut self, record: &Value, body: &str) -> io::Result<Vec<Image>> {
        if record["type"] == "queue-operation"
            && record["operation"] == "enqueue"
            && let Some(delivery) = record["content"]
                .as_str()
                .and_then(|text| self.delivered.get_mut(text))
                .and_then(Vec::pop)
        {
            return self.take(&delivery, body);
        }
        let (prompt, content) = if let Some(prompt) = queued_prompt(record) {
            (&record["attachment"], prompt)
        } else {
            (record, &record["message"]["content"])
        };
        let Some(ids) = prompt["imagePasteIds"].as_array() else {
            return Ok(Vec::new());
        };
        ids.iter()
            .zip(
                content
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|block| block["type"] == "image"),
            )
            .filter_map(|(id, block)| {
                let label = format!("[Image #{}]", id.as_u64()?);
                body.contains(&label).then_some((label, block))
            })
            .map(|(label, block)| {
                Ok(Image {
                    label,
                    path: embedded_image(&block["source"])?,
                })
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

fn embedded_image(source: &Value) -> io::Result<PathBuf> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid Claude pasted image");
    if source["type"] != "base64" {
        return Err(invalid());
    }
    let extension = match source["media_type"].as_str() {
        Some("image/png") => "png",
        Some("image/jpeg") => "jpg",
        Some("image/gif") => "gif",
        Some("image/webp") => "webp",
        _ => return Err(invalid()),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(source["data"].as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    let directory = std::env::temp_dir().join(format!("agents-images-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&directory)?;
    let path = directory.join(format!("{:016x}.{extension}", hash.finish()));
    if !path.exists() {
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        file.write_all(&bytes)?;
        match file.persist_noclobber(&path) {
            Ok(_) => {}
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.error),
        }
    }
    if std::fs::read(&path)? != bytes {
        return Err(invalid());
    }
    Ok(path)
}
