/// The side on which killed text joins the saved text.
pub enum Direction {
    /// Prepend text removed before the caret.
    Backward,
    /// Append text removed after the caret.
    Forward,
}

/// Text shared by editors for consecutive kills and Ctrl+Y.
#[derive(Default)]
pub struct KillBuffer {
    text: String,
    stroke: u64,
    last_kill_stroke: Option<u64>,
}

impl KillBuffer {
    /// Starts one key or paste; mouse events do not start a stroke.
    pub fn begin_stroke(&mut self) {
        self.stroke = self.stroke.wrapping_add(1);
    }

    /// Saves killed text, joining in the given direction within or between consecutive strokes.
    /// After an intervening non-kill stroke, nonempty text replaces the saved text.
    /// An empty kill keeps the saved text and continues the chain.
    pub fn push(&mut self, fragment: &str, direction: Direction) {
        let joins = self.last_kill_stroke.is_some_and(|last| {
            last == self.stroke || last.wrapping_add(1) == self.stroke
        });
        if !fragment.is_empty() {
            if !joins {
                self.text = fragment.to_owned();
            } else {
                match direction {
                    Direction::Backward => self.text.insert_str(0, fragment),
                    Direction::Forward => self.text.push_str(fragment),
                }
            }
        }
        self.last_kill_stroke = Some(self.stroke);
    }

    /// Returns the text Ctrl+Y inserts.
    pub fn text(&self) -> &str {
        &self.text
    }
}
