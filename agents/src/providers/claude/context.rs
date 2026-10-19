//! Context windows from Claude Code's built-in model catalog.

const MODEL_WINDOWS: &[(&str, u64)] = &[
    ("claude-haiku-4-5", 200_000),
    ("claude-haiku-5-5", 1_000_000),
    ("claude-sonnet-4-0", 200_000),
    ("claude-sonnet-4-5", 200_000),
    ("claude-sonnet-4-6", 200_000),
    ("claude-sonnet-5", 1_000_000),
    ("claude-sonnet-5-5", 1_000_000),
    ("claude-opus-4-0", 200_000),
    ("claude-opus-4-1", 200_000),
    ("claude-opus-4-5", 200_000),
    ("claude-opus-4-6", 200_000),
    ("claude-opus-4-7", 1_000_000),
    ("claude-opus-4-8", 1_000_000),
    ("claude-opus-5", 1_000_000),
    ("claude-opus-5-5", 1_000_000),
    ("claude-fable-5", 1_000_000),
    ("claude-fable-5-1", 1_000_000),
    ("claude-mythos-5", 1_000_000),
    ("claude-mythos-5-1", 1_000_000),
];

#[derive(Debug, Default)]
pub(crate) struct Context {
    exceeded_200k: bool,
}

impl Context {
    /// A request beyond 200k proves this session has a 1M window, even after compaction.
    pub(crate) fn window(&mut self, model: Option<&str>, prompt: u64) -> Option<u64> {
        self.exceeded_200k |= prompt > 200_000;
        let model = model?;
        let model = match model.rsplit_once('-') {
            Some((base, date)) if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) => {
                base
            }
            _ => model,
        };
        let model = match model {
            "claude-sonnet-4" => "claude-sonnet-4-0",
            "claude-opus-4" => "claude-opus-4-0",
            _ => model,
        };
        let (_, window) = MODEL_WINDOWS.iter().find(|(id, _)| *id == model)?;
        Some(if self.exceeded_200k {
            1_000_000
        } else {
            *window
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_match_bare_and_dated_models() {
        for &(model, window) in MODEL_WINDOWS {
            for id in [model.to_owned(), format!("{model}-20251001")] {
                assert_eq!(
                    Context::default().window(Some(&id), 50_000),
                    Some(window),
                    "{id}"
                );
            }
        }
        assert_eq!(
            Context::default().window(Some("claude-sonnet-4-20250514"), 50_000),
            Some(200_000)
        );
        assert_eq!(
            Context::default().window(Some("claude-opus-5-6"), 250_000),
            None
        );
    }
}
