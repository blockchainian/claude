use std::ops::Range;

use roxmltree::{Document as XmlTree, Node};

use super::{Document, Format};

pub(super) struct Xml;

struct XmlDocument(String);

impl Document for XmlDocument {
    fn text(&self) -> String {
        self.0.clone()
    }

    fn syntax(&self) -> &'static str {
        "xml"
    }
}

impl Format for Xml {
    fn candidate(&self, source: &str, start: usize) -> Option<usize> {
        if source.as_bytes().get(start) != Some(&b'<') {
            return None;
        }
        for character in source.get(..start)?.chars().rev() {
            scanned(character.len_utf8());
            if character == '\n' {
                break;
            }
            if !character.is_whitespace() {
                return None;
            }
        }
        let mut cursor = start;
        let mut depth = 0usize;
        let mut opened = false;
        let mut root_name = None;
        let mut root_depth = 0usize;
        while cursor < source.len() {
            let text_end = source[cursor..]
                .find('<')
                .map_or(source.len(), |offset| cursor + offset);
            scanned(text_end - cursor);
            if depth == 0 && !source[cursor..text_end].trim().is_empty() {
                return None;
            }
            cursor = text_end;
            if cursor == source.len() {
                break;
            }
            let Some((range, kind)) = markup(source, cursor) else {
                return opened.then_some(source.len());
            };
            cursor = range.end;
            match kind {
                Markup::Open => {
                    opened = true;
                    depth += 1;
                    let name = element_name(source, range.start);
                    if name == *root_name.get_or_insert(name) {
                        root_depth += 1;
                    }
                }
                Markup::Close => {
                    depth = depth.checked_sub(1)?;
                    if Some(element_name(source, range.start)) == root_name {
                        root_depth -= 1;
                    }
                    if depth == 0 || root_depth == 0 {
                        return source[cursor..]
                            .split('\n')
                            .next()?
                            .trim()
                            .is_empty()
                            .then_some(cursor);
                    }
                }
                Markup::Empty if depth == 0 => return None,
                _ => {}
            }
        }
        opened.then_some(source.len())
    }

    fn parse(&self, source: &str) -> Option<Box<dyn Document>> {
        let tree = XmlTree::parse(source).ok()?;
        if !declaration_values(source)
            || tree.descendants().any(|node| {
                node.pi()
                    .is_some_and(|pi| pi.target.eq_ignore_ascii_case("xml"))
            })
        {
            return None;
        }
        let root = tree.root_element();
        if source[root.range()].ends_with("/>") {
            return None;
        }
        // Mixed content and explicit space preservation keep their original bytes.
        if tree.descendants().filter(Node::is_element).any(|node| {
            node.attribute((roxmltree::NS_XML_URI, "space")) == Some("preserve")
                || (node.children().any(|child| !child.is_text())
                    && node.children().any(|child| {
                        child.is_text()
                            && (!child.text().unwrap_or_default().trim().is_empty()
                                || source[child.range()].contains("<![CDATA["))
                    }))
        }) {
            return Some(Box::new(XmlDocument(source.into())));
        }
        let mut output = String::new();
        let prefix = source[..root.range().start].trim_end();
        if !prefix.is_empty() {
            output.push_str(prefix);
            output.push('\n');
        }
        format_element(root, source, 0, &mut output)?;
        output.push_str(&source[root.range().end..]);
        Some(Box::new(XmlDocument(output)))
    }
}

/// The parser validates declaration syntax but leaves its field values unchecked.
fn declaration_values(source: &str) -> bool {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let Some(declaration) = source.strip_prefix("<?xml ") else {
        return true;
    };
    let Some(end) = declaration.find("?>") else {
        return false;
    };
    let declaration = format!("<declaration {}/>", &declaration[..end]);
    let Ok(tree) = XmlTree::parse(&declaration) else {
        return false;
    };
    tree.root_element().attributes().all(|attribute| {
        let value = &declaration[attribute.range_value()];
        match attribute.name() {
            "version" => value == "1.0",
            "standalone" => matches!(value, "yes" | "no"),
            "encoding" => {
                let mut bytes = value.bytes();
                bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
                    && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            }
            _ => false,
        }
    })
}

fn format_element(
    node: Node<'_, '_>,
    source: &str,
    depth: usize,
    output: &mut String,
) -> Option<()> {
    let range = node.range();
    if !node.children().any(|child| !child.is_text()) {
        output.push_str(&source[range]);
        return Some(());
    }
    let (opening, _) = markup(source, range.start)?;
    let closing = range.start + source[range.clone()].rfind("</")?;
    output.push_str(&source[opening]);
    for child in node.children().filter(|child| !child.is_text()) {
        output.push('\n');
        output.extend(std::iter::repeat_n(' ', (depth + 1) * 2));
        if child.is_element() {
            format_element(child, source, depth + 1, output)?;
        } else {
            output.push_str(&source[child.range()]);
        }
    }
    output.push('\n');
    output.extend(std::iter::repeat_n(' ', depth * 2));
    output.push_str(&source[closing..range.end]);
    Some(())
}

#[derive(Clone, Copy)]
enum Markup {
    Open,
    Close,
    Empty,
    Other,
}

fn element_name(source: &str, start: usize) -> &str {
    source[start + 1..]
        .trim_start_matches('/')
        .split([' ', '\t', '\r', '\n', '/', '>'])
        .next()
        .expect("markup has an element name span")
}

/// Finds lexical markup boundaries; the XML parser alone decides validity.
fn markup(source: &str, start: usize) -> Option<(Range<usize>, Markup)> {
    let suffix = &source[start..];
    for (prefix, ending) in [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>")] {
        if let Some(rest) = suffix.strip_prefix(prefix) {
            let length = prefix.len() + rest.find(ending)? + ending.len();
            scanned(length);
            return Some((start..start + length, Markup::Other));
        }
    }
    let mut quote = None;
    let mut brackets = 0usize;
    for (offset, byte) in suffix.bytes().enumerate().skip(1) {
        scanned(1);
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'[' if suffix.starts_with("<!") => brackets += 1,
            b']' if brackets > 0 => brackets -= 1,
            b'>' if brackets == 0 => {
                let kind = if suffix.starts_with("<!") {
                    Markup::Other
                } else if suffix.starts_with("</") {
                    Markup::Close
                } else if suffix.as_bytes()[offset - 1] == b'/' {
                    Markup::Empty
                } else {
                    Markup::Open
                };
                return Some((start..start + offset + 1, kind));
            }
            _ => {}
        }
    }
    None
}

fn scanned(bytes: usize) {
    #[cfg(test)]
    SCANNED.with(|count| count.set(count.get() + bytes));
    #[cfg(not(test))]
    let _ = bytes;
}

#[cfg(test)]
thread_local! {
    static SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[path = "xml_tests.rs"]
mod tests;
