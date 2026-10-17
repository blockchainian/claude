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
            .zip(paths.into_iter().flatten())
            .filter_map(|(id, path)| {
                let id = id.as_u64()?;
                let label = format!("[Image #{id}]");
                if !body.contains(&label) || !path.is_file() {
                    return None;
                }
                Some(Image { label, path })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Transcript e2364ea0, lines 3781/3785; base64 removed and source directory redacted.
    #[test]
    fn pasted_filename_links_to_its_prompt_and_requires_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-pasted-image.jsonl")
            .replace("/redacted", dir.path().to_str().unwrap())
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let path = dir.path().join("pasted-image-20261016-190234.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
        let body = records[0]["message"]["content"][0]["text"]
            .as_str()
            .unwrap();
        let mut companions = Companions::default();
        companions.record(&records[1]);
        assert_eq!(
            companions.take(&records[0], body),
            vec![Image {
                label: "[Image #18]".into(),
                path: path.clone(),
            }]
        );
        std::fs::remove_file(&path).unwrap();
        companions.record(&records[1]);
        assert!(companions.take(&records[0], body).is_empty());
    }
}
