//! JSON detection and the multiline-value printer.
use super::super::highlight;
use super::{Document, Format};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

pub(super) struct Json;
struct JsonDocument(Value);

/// Object entries retain source order, including repeated keys.
enum Value {
    Object(Vec<(String, Value)>),
    Array(Vec<Value>),
    Scalar(serde_json::Value),
}

impl<'de> serde::Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Value;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON value")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Value, M::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Value::Object(entries))
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> Result<Value, S::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(Value::Array(values))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Value, E> {
                Ok(Value::Scalar(value.into()))
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Value, E> {
                Ok(Value::Scalar(value.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Value, E> {
                Ok(Value::Scalar(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Value, E> {
                Ok(Value::Scalar(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
                Ok(Value::Scalar(value.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
                Ok(Value::Scalar(serde_json::Value::Null))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

impl Format for Json {
    fn candidate(&self, source: &str, start: usize) -> Option<usize> {
        matches!(source.as_bytes()[start], b'{' | b'[').then(|| candidate_end(source, start))
    }

    fn parse(&self, source: &str) -> Option<Box<dyn Document>> {
        let value: Value = serde_json::from_str(source).ok()?;
        matches!(value, Value::Object(_) | Value::Array(_))
            .then(|| Box::new(JsonDocument(value)) as Box<dyn Document>)
    }
}

impl Document for JsonDocument {
    fn text(&self) -> String {
        self.highlighted()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn syntax(&self) -> &'static str {
        "json"
    }
    fn is_empty(&self) -> bool {
        match &self.0 {
            Value::Object(entries) => entries.is_empty(),
            Value::Array(values) => values.is_empty(),
            Value::Scalar(_) => false,
        }
    }
    fn highlighted(&self) -> Vec<Line<'static>> {
        value_lines(&self.0)
    }
    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        use unicode_width::UnicodeWidthStr;
        if width == 0 {
            return Vec::new();
        }
        let mut lines = Vec::new();
        for line in self.highlighted() {
            let mut row = Line::default();
            let mut used = 0;
            for grapheme in line.styled_graphemes(Style::default()) {
                let cells = grapheme.symbol.width();
                if used > 0 && used + cells > width {
                    lines.push(std::mem::take(&mut row));
                    used = 0;
                }
                if let Some(span) = row
                    .spans
                    .last_mut()
                    .filter(|span| span.style == grapheme.style)
                {
                    span.content.to_mut().push_str(grapheme.symbol);
                } else {
                    row.spans
                        .push(Span::styled(grapheme.symbol.to_owned(), grapheme.style));
                }
                used += cells;
            }
            lines.push(row);
        }
        lines
    }
}

#[cfg(test)]
thread_local! {
    pub(in super::super) static JSON_SCAN_BUDGET: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(in super::super) fn record_json_scan(bytes: usize) {
    JSON_SCAN_BUDGET.with(|budget| {
        if let Some(remaining) = budget.get() {
            budget.set(Some(
                remaining
                    .checked_sub(bytes)
                    .expect("JSON detection exceeded its linear scan budget"),
            ));
        }
    });
}

/// Scan each candidate once, then parse only its balanced span. Failed spans stay prose.
fn candidate_end(source: &str, start: usize) -> usize {
    if source.as_bytes()[start] == b'['
        && source[..start]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | ']' | ')' | '"'))
    {
        return start + 1;
    }
    let mut brackets = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    for (offset, &byte) in source.as_bytes()[start..].iter().enumerate() {
        #[cfg(test)]
        record_json_scan(1);
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' => brackets.push(byte),
            b'}' | b']' => {
                let end = start + offset + 1;
                if brackets.pop() != Some(if byte == b'}' { b'{' } else { b'[' }) {
                    return end;
                }
                if brackets.is_empty() {
                    #[cfg(test)]
                    record_json_scan(end - start);
                    return end;
                }
            }
            _ => {}
        }
    }
    source.len()
}

/// Pretty JSON with string line breaks aligned to the opening content column.
#[cfg(test)]
pub(in super::super) fn json_lines(value: &serde_json::Value) -> Vec<Line<'static>> {
    Json.parse(&value.to_string())
        .expect("JSON document")
        .highlighted()
}

fn value_lines(value: &Value) -> Vec<Line<'static>> {
    use highlight::JsonToken;

    struct Printer {
        lines: Vec<Line<'static>>,
        styles: [Style; 7],
    }

    impl Printer {
        fn push(&mut self, text: impl Into<String>, token: JsonToken) {
            self.lines
                .last_mut()
                .expect("JSON line exists")
                .spans
                .push(Span::styled(text.into(), self.styles[token as usize]));
        }

        fn newline(&mut self, indent: usize) {
            self.lines.push(Line::from(" ".repeat(indent)));
        }

        fn string(&mut self, text: &str, token: JsonToken, multiline: bool, indent: usize) {
            let quote = if multiline {
                JsonToken::StringQuote
            } else {
                token
            };
            self.push("\"", quote);
            // Continuations use the value's structural indent, preserving its own whitespace.
            // Serialize each logical line so all other JSON escapes retain their spelling.
            let normalized = if multiline {
                text.replace("\r\n", "\n")
            } else {
                text.into()
            };
            for (index, line) in normalized.split_inclusive('\n').enumerate() {
                if index > 0 && multiline {
                    self.newline(indent);
                }
                let line = if multiline {
                    line.strip_suffix('\n').unwrap_or(line)
                } else {
                    line
                };
                let encoded = serde_json::to_string(line).expect("JSON string serializes");
                let content = &encoded[1..encoded.len() - 1];
                let mut start = 0;
                let mut cursor = 0;
                while cursor < content.len() {
                    if content.as_bytes()[cursor] != b'\\' {
                        cursor += 1;
                        continue;
                    }
                    self.push(&content[start..cursor], token);
                    let end = cursor
                        + if content.as_bytes()[cursor + 1] == b'u' {
                            6
                        } else {
                            2
                        };
                    self.push(&content[cursor..end], JsonToken::Escape);
                    cursor = end;
                    start = end;
                }
                self.push(&content[start..], token);
            }
            if multiline && normalized.ends_with('\n') {
                self.newline(indent);
            }
            self.push("\"", quote);
        }

        /// Render a multi-line Markdown value between its quotes, indented like its continuations.
        fn markdown(&mut self, text: &str, indent: usize) {
            self.push("\"", JsonToken::StringQuote);
            let base = self.styles[JsonToken::String as usize];
            for (index, line) in super::super::markdown::render(text, usize::MAX)
                .into_iter()
                .enumerate()
            {
                if index > 0 {
                    self.newline(indent);
                }
                let spans = &mut self.lines.last_mut().expect("JSON line exists").spans;
                spans.extend(line.spans.into_iter().map(|mut span| {
                    span.style = base.patch(span.style);
                    span
                }));
            }
            self.push("\"", JsonToken::StringQuote);
        }

        fn value(&mut self, value: &Value, indent: usize) {
            match value {
                Value::Object(values) => {
                    self.push("{", JsonToken::Punctuation);
                    for (index, (key, value)) in values.iter().enumerate() {
                        self.newline(indent + 2);
                        self.string(key, JsonToken::Key, false, indent + 2);
                        self.push(": ", JsonToken::Punctuation);
                        self.value(value, indent + 2);
                        if index + 1 < values.len() {
                            self.push(",", JsonToken::Punctuation);
                        }
                    }
                    if !values.is_empty() {
                        self.newline(indent);
                    }
                    self.push("}", JsonToken::Punctuation);
                }
                Value::Array(values) => {
                    self.push("[", JsonToken::Punctuation);
                    for (index, value) in values.iter().enumerate() {
                        self.newline(indent + 2);
                        self.value(value, indent + 2);
                        if index + 1 < values.len() {
                            self.push(",", JsonToken::Punctuation);
                        }
                    }
                    if !values.is_empty() {
                        self.newline(indent);
                    }
                    self.push("]", JsonToken::Punctuation);
                }
                Value::Scalar(serde_json::Value::String(text))
                    if text.contains('\n') && super::super::markdown::has_markers(text) =>
                {
                    self.markdown(text, indent)
                }
                Value::Scalar(serde_json::Value::String(text)) => {
                    self.string(text, JsonToken::String, true, indent)
                }
                Value::Scalar(serde_json::Value::Number(number)) => {
                    self.push(number.to_string(), JsonToken::Number)
                }
                Value::Scalar(value) => self.push(value.to_string(), JsonToken::Literal),
            }
        }
    }

    let mut printer = Printer {
        lines: vec![Line::default()],
        styles: highlight::json_styles(),
    };
    printer.value(value, 0);
    printer.lines
}
