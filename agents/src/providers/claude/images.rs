use crate::log::Image;
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf};

/// Companions arrive before their prompts in a backwards transcript walk.
#[derive(Default)]
pub(super) struct Companions {
    prompts: HashMap<String, Vec<PathBuf>>,
}

impl Companions {
    pub(super) fn record(&mut self, record: &Value) {
        if record["type"] != "user" || record["isMeta"] != true || record["turnCompanion"] != true {
            return;
        }
        let paths = record["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| {
                block["text"]
                    .as_str()?
                    .strip_prefix("[Image: source: ")?
                    .strip_suffix(']')
                    .map(PathBuf::from)
            })
            .collect();
        if let Some(id) = record["promptId"].as_str() {
            self.prompts.insert(id.to_owned(), paths);
        }
    }

    pub(super) fn take(&mut self, record: &Value, body: &str) -> Vec<Image> {
        let paths = if let Some(id) = record["promptId"].as_str() {
            self.prompts.remove(id)
        } else {
            None
        };
        let Some(ids) = record["imagePasteIds"].as_array() else {
            return Vec::new();
        };
        ids.iter()
            .filter_map(Value::as_u64)
            .filter_map(|id| {
                let number = id.to_string();
                let label = format!("[Image #{id}]");
                if !body.contains(&label) {
                    return None;
                }
                let path = paths.as_ref()?.iter().find(|path| {
                    path.file_stem().and_then(|stem| stem.to_str()) == Some(number.as_str())
                        && path.is_file()
                })?;
                Some(Image {
                    label,
                    path: path.clone(),
                })
            })
            .collect()
    }
}
