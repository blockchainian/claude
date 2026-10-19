use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::text::Line;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) type CellMap = (Range<u16>, Range<usize>);

#[derive(Clone)]
struct Token {
    text: String,
    source: Option<Range<usize>>,
}

/// Maps rendered graphemes to the original bytes, excluding generated list padding.
pub(super) fn map_lines(source: &str, lines: &[Line<'_>], markdown: bool) -> Vec<Vec<CellMap>> {
    map_lines_in(source, lines, markdown, None)
}

/// Maps cwd-relative labels back to the absolute source bytes used by links and copies.
pub(super) fn map_lines_in(
    source: &str,
    lines: &[Line<'_>],
    markdown: bool,
    cwd: Option<&std::path::Path>,
) -> Vec<Vec<CellMap>> {
    let mut tokens = source_tokens(source, markdown);
    if let Some(cwd) = cwd {
        for range in super::paths::text_paths(source, cwd) {
            if let Some(relative) = super::paths::relative_path(&source[range.clone()], cwd) {
                let path = &source[range.clone()];
                let suffix: usize = path
                    .chars()
                    .rev()
                    .zip(relative.chars().rev())
                    .take_while(|(left, right)| left == right)
                    .map(|(ch, _)| ch.len_utf8())
                    .sum();
                let end = range.end - suffix;
                let prefix = &relative[..relative.len() - suffix];
                let start = tokens.iter().position(|token| {
                    token
                        .source
                        .as_ref()
                        .is_some_and(|bytes| bytes.start == range.start)
                });
                tokens.retain(|token| {
                    !token
                        .source
                        .as_ref()
                        .is_some_and(|bytes| bytes.start >= range.start && bytes.end <= end)
                });
                if prefix.is_empty() {
                    if let Some(token) = tokens.iter_mut().find(|token| {
                        token
                            .source
                            .as_ref()
                            .is_some_and(|bytes| bytes.start == end)
                    }) {
                        token.source.as_mut().unwrap().start = range.start;
                    }
                } else if let Some(start) = start {
                    tokens.splice(
                        start..start,
                        prefix.graphemes(true).map(|text| Token {
                            text: text.to_owned(),
                            source: Some(range.start..end),
                        }),
                    );
                }
            }
        }
    }
    let path_ranges = super::paths::text_paths(source, cwd.unwrap_or(std::path::Path::new("/")));
    let mut cursor = 0;
    lines
        .iter()
        .enumerate()
        .map(|(line_index, line)| {
            while cursor < tokens.len() && matches!(tokens[cursor].text.as_str(), "\n" | "\r") {
                cursor += 1;
            }
            let mut cells = 0u16;
            let mut mapped = Vec::new();
            let text = line.to_string();
            let generated_padding = if markdown {
                line.spans
                    .first()
                    .filter(|span| {
                        !span.content.is_empty()
                            && span.content.chars().all(|ch| ch == ' ')
                            && line.spans.len() > 1
                    })
                    .map_or(0, |span| span.content.len())
            } else {
                0
            };
            for (display_offset, grapheme) in text.grapheme_indices(true) {
                let width = grapheme.width() as u16;
                let range = cells..cells.saturating_add(width);
                cells = range.end;
                if display_offset < generated_padding || cursor >= tokens.len() {
                    continue;
                }
                // Wrapping removes whitespace at row boundaries and inserts list indentation.
                if tokens[cursor].text != grapheme {
                    let mut next = cursor;
                    while next < tokens.len() && tokens[next].text.trim().is_empty() {
                        next += 1;
                    }
                    if next < tokens.len()
                        && (tokens[next].text == grapheme || !grapheme.trim().is_empty())
                    {
                        cursor = next;
                    } else if grapheme.trim().is_empty() {
                        continue;
                    }
                }
                if let Some(path) = path_ranges.iter().find(|path| {
                    tokens[cursor]
                        .source
                        .as_ref()
                        .is_some_and(|bytes| bytes.start == path.start)
                }) {
                    let end = (cursor..tokens.len())
                        .find(|&index| {
                            tokens[index]
                                .source
                                .as_ref()
                                .is_none_or(|bytes| bytes.start >= path.end)
                        })
                        .unwrap_or(tokens.len());
                    let mut remaining = text[display_offset..].to_owned();
                    for continuation in &lines[line_index + 1..] {
                        if remaining.len() >= path.len() {
                            break;
                        }
                        remaining.push_str(continuation.to_string().trim_start());
                    }
                    if let Some(displayed) = path_tokens(&tokens[cursor..end], path, &remaining) {
                        tokens.splice(cursor..end, displayed);
                    }
                }
                if tokens[cursor].text == grapheme {
                    if let Some(bytes) = &tokens[cursor].source {
                        mapped.push((range, bytes.clone()));
                    }
                    cursor += 1;
                } else if grapheme == "…" {
                    let start = cursor;
                    let mut end = cursor;
                    while end < tokens.len()
                        && tokens[end].text != "/"
                        && !tokens[end].text.trim().is_empty()
                        && !matches!(
                            tokens[end].text.as_str(),
                            "，" | "。" | "；" | "：" | "！" | "？"
                        )
                    {
                        end += 1;
                    }
                    while end > start
                        && matches!(
                            tokens[end - 1].text.as_str(),
                            "`" | ")" | "\"" | "'" | "," | "." | ";" | ":"
                        )
                    {
                        end -= 1;
                    }
                    cursor = end;
                    let first = tokens[start..cursor]
                        .iter()
                        .find_map(|token| token.source.as_ref());
                    let last = tokens[start..cursor]
                        .iter()
                        .rev()
                        .find_map(|token| token.source.as_ref());
                    if let (Some(first), Some(last)) = (first, last) {
                        mapped.push((range, first.start..last.end));
                    }
                } else if grapheme == "~"
                    && let Ok(home) = std::env::var("HOME")
                    && let Some(bytes) = &tokens[cursor].source
                    && source[bytes.start..].starts_with(&home)
                {
                    let start = bytes.start;
                    let end = start + home.len();
                    while cursor < tokens.len()
                        && tokens[cursor]
                            .source
                            .as_ref()
                            .is_some_and(|bytes| bytes.start < end)
                    {
                        cursor += 1;
                    }
                    mapped.push((range, start..end));
                }
            }
            mapped
        })
        .collect()
}

/// Gives every visible path grapheme the original bytes it represents, including omitted components.
fn path_tokens(tokens: &[Token], path: &Range<usize>, display: &str) -> Option<Vec<Token>> {
    let mut forms = vec![tokens.to_vec()];
    let text: String = tokens.iter().map(|token| token.text.as_str()).collect();
    if let Ok(home) = std::env::var("HOME")
        && text.starts_with(&format!("{home}/"))
    {
        let count = home.graphemes(true).count();
        let mut home_tokens = vec![Token {
            text: "~".into(),
            source: Some(path.start..tokens[count - 1].source.as_ref()?.end),
        }];
        home_tokens.extend_from_slice(&tokens[count..]);
        forms.push(home_tokens);
    }
    let mut best: Option<(usize, Vec<Token>)> = None;
    let mut consider = |candidate: Vec<Token>| {
        let label: String = candidate.iter().map(|token| token.text.as_str()).collect();
        if display.starts_with(&label) && best.as_ref().is_none_or(|(len, _)| label.len() > *len) {
            best = Some((label.len(), candidate));
        }
    };
    for form in forms {
        consider(form.clone());
        let anchor = if form.first()?.text == "/" {
            1
        } else if form.first()?.text == "~" && form.get(1)?.text == "/" {
            2
        } else {
            0
        };
        let starts: Vec<_> = form
            .iter()
            .enumerate()
            .filter_map(|(index, token)| {
                (token.text == "/" && index + 1 < form.len()).then_some(index + 1)
            })
            .collect();
        for &start in &starts {
            if start <= anchor {
                continue;
            }
            let mut candidate = form[..anchor].to_vec();
            let omitted_start = form[anchor].source.as_ref()?.start;
            let omitted_end = form[start - 1].source.as_ref()?.start;
            candidate.push(Token {
                text: "…".into(),
                source: Some(omitted_start..omitted_end),
            });
            candidate.extend_from_slice(&form[start - 1..]);
            consider(candidate);
        }
        let basename = starts.last().copied().unwrap_or(0);
        let mut tail = form[basename..].to_vec();
        tail.first_mut()?.source.as_mut()?.start = path.start;
        consider(tail.clone());
        for kept in 0..tail.len() {
            let mut candidate = tail[..kept].to_vec();
            let start = if kept == 0 {
                path.start
            } else {
                tail[kept].source.as_ref()?.start
            };
            candidate.push(Token {
                text: "…".into(),
                source: Some(start..path.end),
            });
            consider(candidate);
        }
    }
    best.map(|(_, tokens)| tokens)
}

fn source_tokens(source: &str, markdown: bool) -> Vec<Token> {
    let mut tokens = Vec::new();
    if !markdown {
        push_text(&mut tokens, source, 0..source.len(), source, false);
        return tokens;
    }
    let mut lists = Vec::new();
    let mut code = false;
    let mut table_cells = 0;
    for (event, range) in Parser::new_ext(
        source,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES,
    )
    .into_offset_iter()
    {
        match event {
            Event::Start(Tag::TableHead | Tag::TableRow) => table_cells = 0,
            Event::Start(Tag::TableCell) => {
                if table_cells > 0 {
                    tokens.extend(" │ ".graphemes(true).map(|text| Token {
                        text: text.to_owned(),
                        source: None,
                    }));
                }
                table_cells += 1;
            }
            Event::Start(Tag::CodeBlock(_)) => code = true,
            Event::End(TagEnd::CodeBlock) => code = false,
            Event::Start(Tag::List(start)) => lists.push(start),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let padding = lists.len().saturating_sub(1) * 4;
                let marker = match lists.last_mut() {
                    Some(Some(index)) => {
                        let marker = format!("{:>padding$}{index}. ", "");
                        *index += 1;
                        marker
                    }
                    _ => format!("{}• ", " ".repeat(padding)),
                };
                tokens.extend(marker.graphemes(true).map(|text| Token {
                    text: text.to_owned(),
                    source: None,
                }));
            }
            Event::Text(text) => push_text(&mut tokens, &text, range, source, !code),
            Event::Html(text) | Event::InlineHtml(text) | Event::Code(text) => {
                push_text(&mut tokens, &text, range, source, false);
            }
            Event::SoftBreak | Event::HardBreak => tokens.push(Token {
                text: " ".into(),
                source: Some(range),
            }),
            Event::Rule => tokens.extend((0..3).map(|_| Token {
                text: "─".into(),
                source: None,
            })),
            _ => {}
        }
    }
    tokens
}

fn push_text(
    tokens: &mut Vec<Token>,
    text: &str,
    range: Range<usize>,
    source: &str,
    decode_entities: bool,
) {
    let raw = &source[range.clone()];
    let mut offset = 0;
    for grapheme in text.graphemes(true) {
        // Offset events preserve text except inline code delimiters, entities and escapes.
        let bytes = if decode_entities
            && raw != text
            && raw[offset..].starts_with('&')
            && raw[offset..].contains(';')
        {
            let end = offset + raw[offset..].find(';').expect("entity terminator") + 1;
            let bytes = range.start + offset..range.start + end;
            offset = end;
            bytes
        } else if let Some(relative) = raw[offset..].find(grapheme) {
            let start = offset + relative;
            offset = start + grapheme.len();
            range.start + start..range.start + offset
        } else if decode_entities && raw[offset..].starts_with('&') {
            let end = raw[offset..]
                .find(';')
                .map_or(raw.len(), |end| offset + end + 1);
            let bytes = range.start + offset..range.start + end;
            offset = end;
            bytes
        } else if offset < raw.len() {
            let next = raw[offset..]
                .graphemes(true)
                .next()
                .expect("remaining source grapheme");
            let bytes = range.start + offset..range.start + offset + next.len();
            offset += next.len();
            bytes
        } else {
            range.clone()
        };
        let expanded = grapheme.replace('\t', "    ");
        let displayed: String = expanded
            .chars()
            .map(|ch| {
                if ch.is_control() && !matches!(ch, '\n' | '\r') {
                    ' '
                } else {
                    ch
                }
            })
            .collect();
        tokens.extend(displayed.graphemes(true).map(|text| Token {
            text: text.to_owned(),
            source: Some(bytes.clone()),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_fixture_paths_keep_every_cell_linked_and_copy_original_bytes() {
        let fixture: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-start.jsonl")
                .lines()
                .nth(2)
                .unwrap(),
        )
        .unwrap();
        let body = fixture["message"]["content"].as_str().unwrap();
        let path = body
            .split('"')
            .find(|text| text.starts_with("/Users/"))
            .unwrap();
        let basename = path.rsplit('/').next().unwrap();
        let parent = path.rsplitn(3, '/').nth(1).unwrap();
        let home = std::env::var("HOME").unwrap();
        let fixture_cwd = std::path::Path::new(fixture["cwd"].as_str().unwrap())
            .parent()
            .unwrap()
            .to_str()
            .unwrap();
        for cwd in ["/nonexistent", fixture_cwd] {
            for label in [
                format!("/…/{parent}/{basename}"),
                format!("~/…/{basename}"),
                format!("…/{parent}/{basename}"),
                basename.to_owned(),
                "manager…".to_owned(),
                "…".to_owned(),
            ] {
                // Each label is a legal output for one of the rooted, home or cwd-relative forms.
                if (label.starts_with("~/") && (!path.starts_with(&home) || cwd != "/nonexistent"))
                    || (label.starts_with("/…") && cwd != "/nonexistent")
                    || (label.starts_with("…/") && cwd == "/nonexistent")
                {
                    continue;
                }
                let source = format!("before {path} and {path} after");
                let display = format!("before {label} and {label} after");
                let maps = map_lines_in(
                    &source,
                    &[Line::raw(&display)],
                    false,
                    Some(std::path::Path::new(cwd)),
                );
                assert_eq!(
                    maps[0].len(),
                    display.graphemes(true).count(),
                    "{cwd}: {label}"
                );
                let ranges: Vec<_> = source
                    .match_indices(path)
                    .map(|(start, text)| start..start + text.len())
                    .collect();
                let links: Vec<_> = ranges
                    .iter()
                    .map(|range| super::super::links::Link {
                        range: range.clone(),
                        target: path.into(),
                    })
                    .collect();
                let drawn = super::super::links::row_links(&maps[0], &links);
                let first = "before ".width() as u16;
                let second = first + label.width() as u16 + " and ".width() as u16;
                assert_eq!(
                    drawn,
                    [
                        (first..first + label.width() as u16, path.into()),
                        (second..second + label.width() as u16, path.into()),
                    ],
                    "{cwd}: {label}"
                );
                assert_eq!(
                    maps[0]
                        .iter()
                        .map(|(_, range)| &source[range.clone()])
                        .collect::<String>(),
                    source,
                    "{cwd}: {label}"
                );
            }
        }
    }

    #[test]
    fn home_paths_under_a_parent_cwd_keep_links_on_the_expanded_label() {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let source = "~/project/file.rs";
        let target = home.join("project/file.rs");
        let display = target.strip_prefix("/").unwrap().to_str().unwrap();
        let maps = map_lines_in(
            source,
            &[Line::raw(display)],
            false,
            Some(std::path::Path::new("/")),
        );
        assert_eq!(maps[0].len(), display.chars().count());
        let links = [super::super::links::Link {
            range: 0..source.len(),
            target: target.clone(),
        }];
        assert_eq!(
            super::super::links::row_links(&maps[0], &links),
            [(0..display.width() as u16, target)]
        );
        assert_eq!(maps[0].first().unwrap().1.start, 0);
        assert_eq!(maps[0].last().unwrap().1.end, source.len());
    }

    #[test]
    fn relative_path_cells_retain_absolute_link_and_copy_bytes() {
        let cwd = std::path::Path::new("/a/b");
        for (source, display) in [
            (
                "See /a/b/src/lib.rs:272-274 done",
                "See src/lib.rs:272-274 done",
            ),
            ("/a/b", "."),
            ("/a/b/", "."),
            (
                "/a/b/src/lib.rs and /a/bc/other.rs",
                "src/lib.rs and /a/bc/other.rs",
            ),
        ] {
            let lines = [Line::raw(display)];
            let maps = map_lines_in(source, &lines, false, Some(cwd));
            assert_eq!(maps[0].len(), display.chars().count(), "{source}");
            assert_eq!(
                maps[0]
                    .iter()
                    .map(|(_, bytes)| &source[bytes.clone()])
                    .collect::<String>(),
                source
            );
        }
    }

    #[test]
    fn shortened_filename_preserves_following_cjk_punctuation() {
        let source = "报告已写到 /private/tmp/claude-501/-Users-satoshi-Code-blockchainian-claude/0ed4978e-9ece-4b9d-9bc5-04bf44fedacb/scratchpad/background-research/report.md，并已发给 team-lead。";
        for markdown in [false, true] {
            let shortened = super::super::line_truncation::abbreviate_paths_for_wrap(
                source,
                &crate::ui::paths::text_paths(source, std::path::Path::new("/")),
                25,
                std::path::Path::new("/nonexistent"),
            );
            let lines = if markdown {
                super::super::markdown::render_with_options(
                    &shortened,
                    super::super::line_truncation::prose_wrap_options(25),
                )
            } else {
                textwrap::wrap(
                    &shortened,
                    super::super::line_truncation::prose_wrap_options(25),
                )
                .into_iter()
                .map(|line| Line::raw(line.into_owned()))
                .collect()
            };
            let maps = map_lines(source, &lines, markdown);
            let comma = source.find('，').unwrap();
            assert!(
                maps.iter()
                    .flatten()
                    .any(|(_, bytes)| bytes == &(comma..comma + '，'.len_utf8()))
            );
            let sent = source.find('并').unwrap();
            assert!(
                maps.iter()
                    .flatten()
                    .any(|(_, bytes)| bytes == &(sent..sent + '并'.len_utf8()))
            );
            assert!(
                maps.iter()
                    .flatten()
                    .all(|(_, bytes)| !(bytes.start < comma && bytes.end > comma))
            );
        }
    }

    #[test]
    fn inline_code_entities_are_literal_characters() {
        let source = "`x &amp; y`";
        let lines = super::super::markdown::render(source, 40);
        let maps = map_lines(source, &lines, true);
        assert_eq!(maps[0][2].1, 3..4);
        assert_eq!(maps[0].last().unwrap().1, 9..10);
    }

    #[test]
    fn home_replacement_after_wrapped_whitespace_maps_original_home() {
        let home = std::env::var("HOME").expect("HOME");
        let source = format!("before {home}/workspace/report.md after");
        let shortened = super::super::line_truncation::abbreviate_paths_for_wrap(
            &source,
            &crate::ui::paths::text_paths(&source, std::path::Path::new("/")),
            15,
            std::path::Path::new("/nonexistent"),
        );
        let lines: Vec<_> = textwrap::wrap(
            &shortened,
            super::super::line_truncation::prose_wrap_options(15),
        )
        .into_iter()
        .map(|line| Line::raw(line.into_owned()))
        .collect();
        let maps = map_lines(&source, &lines, false);
        assert!(
            maps.iter()
                .flatten()
                .any(|(_, bytes)| bytes == &(7..7 + home.len()))
        );
        assert_eq!(maps.last().unwrap().last().unwrap().1.end, source.len());
    }

    #[test]
    fn wrapped_fixture_text_keeps_original_offsets() {
        let fixture: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-start.jsonl")
                .lines()
                .nth(2)
                .unwrap(),
        )
        .unwrap();
        let source = fixture["message"]["content"]
            .as_str()
            .unwrap()
            .lines()
            .find(|line| line.contains("codex thread"))
            .unwrap()
            .trim();
        let lines: Vec<_> = textwrap::wrap(source, 3)
            .into_iter()
            .map(|text| Line::raw(text.into_owned()))
            .collect();
        let maps = map_lines(source, &lines, false);
        assert_eq!(maps[0][0].1, 0..1);
        for (line, map) in lines.iter().zip(maps) {
            for (cells, bytes) in map {
                assert_eq!(
                    &line.to_string()[cells.start as usize..cells.end as usize],
                    &source[bytes]
                );
            }
        }
    }

    #[test]
    fn markdown_gutters_and_hidden_syntax_do_not_match_body() {
        let source = "- **one one** [one](https://example.com/one)\n  - 中文 👩‍💻";
        let lines = super::super::markdown::render(source, 12);
        let maps = map_lines(source, &lines, true);
        assert_eq!(maps[0][0], (2..3, 4..5));
        assert!(
            maps.iter()
                .flatten()
                .all(|(_, bytes)| !source[bytes.clone()].contains("https"))
        );
        let chinese = source.find('中').unwrap();
        assert!(
            maps.iter()
                .flatten()
                .any(|(cells, bytes)| cells.end - cells.start == 2
                    && bytes == &(chinese..chinese + 3))
        );
    }

    #[test]
    fn continuation_indentation_and_entities_keep_source_mapping() {
        let source = "- one two three &amp; one";
        let lines = super::super::markdown::render(source, 10);
        let maps = map_lines(source, &lines, true);
        assert!(maps[1].iter().all(|(cells, _)| cells.start >= 2));
        let entity = source.find("&amp;").unwrap();
        assert!(
            maps.iter()
                .flatten()
                .any(|(_, bytes)| bytes == &(entity..entity + 5))
        );
        assert_eq!(maps.last().unwrap().last().unwrap().1.end, source.len());
    }

    #[test]
    fn abbreviated_paths_map_omitted_bytes_and_preserve_following_text() {
        let source = "before /private/workspace/reports/report.md after";
        let shortened = super::super::line_truncation::abbreviate_paths_for_wrap(
            source,
            &crate::ui::paths::text_paths(source, std::path::Path::new("/")),
            20,
            std::path::Path::new("/nonexistent"),
        );
        let lines = super::super::markdown::render(&shortened, 20);
        let maps = map_lines(source, &lines, true);
        assert!(
            maps.iter()
                .flatten()
                .any(|(_, bytes)| source[bytes.clone()].contains("private/workspace"))
        );
        let last = maps.last().unwrap().last().unwrap();
        assert_eq!(&source[last.1.clone()], "r");
        assert_eq!(last.1.end, source.len());
    }
}
