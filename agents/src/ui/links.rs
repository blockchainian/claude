//! Clickable paths and image placeholders: where they sit in a text, and where they were drawn.
use super::text_selection::DrawnRow;
use ratatui::{buffer::Buffer, layout::Rect};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// A link inside a text: the bytes it covers and the file, directory or URL it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Link {
    pub range: Range<usize>,
    pub target: PathBuf,
}

/// A link on screen: the cells it covers and what it opens.
pub(super) type DrawnLink = (Rect, PathBuf);

/// The existing paths in `text`, and the label of each of `images`.
pub(super) fn text_links(text: &str, cwd: &Path, images: &[crate::images::Image]) -> Vec<Link> {
    let mut links = markdown_links(text, cwd);
    for link in path_links(text, &super::paths::text_paths(text, cwd), cwd) {
        if !links
            .iter()
            .any(|other| other.range.start < link.range.end && link.range.start < other.range.end)
        {
            links.push(link);
        }
    }
    for image in images {
        if !matches!(image.source, crate::images::ImageSource::File { .. }) {
            continue;
        }
        let Some(target) = image.source.local_file().ok().and_then(existing) else {
            continue;
        };
        links.extend(text.match_indices(&image.label).map(|(at, label)| Link {
            range: at..at + label.len(),
            target: target.clone(),
        }));
    }
    links.sort_by_key(|link| link.range.start);
    links
}

/// Markdown links whose destination resolves, covering their label.
fn markdown_links(text: &str, cwd: &Path) -> Vec<Link> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

    let mut links = Vec::new();
    let mut open: Option<(String, Option<Range<usize>>)> = None;
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => open = Some((dest_url.into_string(), None)),
            Event::End(TagEnd::Link) => {
                if let Some((url, Some(label))) = open.take()
                    && let Some(target) = resolve(&url, cwd)
                {
                    links.push(Link {
                        range: label,
                        target,
                    });
                }
            }
            Event::Text(_) | Event::Code(_) => {
                if let Some((_, label)) = &mut open {
                    let start = label.as_ref().map_or(range.start, |label| label.start);
                    *label = Some(start..range.end);
                }
            }
            _ => {}
        }
    }
    links
}

/// The links of a text whose paths are already known, such as a tool call's arguments.
pub(super) fn path_links(text: &str, paths: &[Range<usize>], cwd: &Path) -> Vec<Link> {
    let mut links: Vec<_> = paths
        .iter()
        .filter_map(|range| {
            let target = resolve(&text[range.clone()], cwd)?;
            // A line range after a text file opens that line in VS Code.
            if let Some((len, line)) = super::paths::line_range(&text[range.end..])
                && let Some(link) = super::vscode::file_line_link(&target, line)
            {
                return Some(Link {
                    range: range.start..range.end + len,
                    target: link.into(),
                });
            }
            Some(Link {
                range: range.clone(),
                target,
            })
        })
        .collect();
    add_commits(text, cwd, &mut links);
    links
}

/// Adds verified commits without covering an existing path, URL or image link.
pub(super) fn add_commits(text: &str, cwd: &Path, links: &mut Vec<Link>) {
    for range in super::paths::text_commits(text) {
        if links
            .iter()
            .any(|link| link.range.start < range.end && range.start < link.range.end)
        {
            continue;
        }
        if let Some(target) = super::commits::target(cwd, &text[range.clone()]) {
            links.push(Link { range, target });
        }
    }
    links.sort_by_key(|link| link.range.start);
}

/// Where `path` opens: GitHub pull requests in VS Code, other URLs as they are, and existing paths resolved absolutely,
/// under the home directory for `~/`, or under `cwd` for relative paths.
pub(super) fn resolve(path: &str, cwd: &Path) -> Option<PathBuf> {
    if path.contains("://") {
        return Some(
            super::vscode::pull_request_link(path, cwd).map_or_else(|| path.into(), Into::into),
        );
    }
    let target = if let Some(rest) = path.strip_prefix("~/") {
        Path::new(&std::env::var_os("HOME")?).join(rest)
    } else {
        cwd.join(path)
    };
    existing(target)
}

/// `target` while it is on disk: every local path is linked through here, since a deleted one opens nothing.
pub(super) fn existing(target: PathBuf) -> Option<PathBuf> {
    target.exists().then_some(target)
}

/// The screen cells of `links` on rows whose cells map back to the text's bytes.
pub(super) fn drawn_links(rows: &[DrawnRow], links: &[Link]) -> Vec<DrawnLink> {
    let mut drawn = Vec::new();
    for row in rows {
        for link in links {
            let mut columns: Option<Range<u16>> = None;
            for (cells, bytes) in &row.cells {
                if bytes.start < link.range.end && link.range.start < bytes.end {
                    columns = Some(match columns {
                        Some(columns) => columns.start.min(cells.start)..columns.end.max(cells.end),
                        None => cells.clone(),
                    });
                }
            }
            if let Some(columns) = columns.filter(|columns| !columns.is_empty()) {
                drawn.push((
                    Rect::new(row.x + columns.start, row.y, columns.end - columns.start, 1),
                    link.target.clone(),
                ));
            }
        }
    }
    drawn
}

/// The columns of `links` on one drawn row whose `cells` map back to the text's bytes.
pub(super) fn row_links(
    cells: &[(Range<u16>, Range<usize>)],
    links: &[Link],
) -> Vec<(Range<u16>, PathBuf)> {
    let row = DrawnRow {
        segment: 0,
        x: 0,
        y: 0,
        cells: cells.to_vec(),
        ..Default::default()
    };
    drawn_links(std::slice::from_ref(&row), links)
        .into_iter()
        .map(|(rect, target)| (rect.x..rect.right(), target))
        .collect()
}

/// Underlines the cells of each drawn link with dots. Ratatui has no dotted underline, so
/// each cell carries its own escape codes; call it last, after the panel's text is read.
pub(super) fn underline(buf: &mut Buffer, links: &[DrawnLink]) {
    use ratatui::buffer::CellDiffOption;
    use std::num::NonZeroU16;
    use unicode_width::UnicodeWidthStr;

    for (rect, _) in links {
        let rect = rect.intersection(buf.area);
        let mut x = rect.left();
        while x < rect.right() {
            let cell = &mut buf[(x, rect.y)];
            // A cell that already carries escape codes keeps its forced width.
            let width = match cell.diff_option {
                CellDiffOption::ForcedWidth(width) => usize::from(width.get()),
                _ => cell.symbol().width().max(1),
            };
            cell.set_symbol(&format!("\x1b[4:4m{}\x1b[24m", cell.symbol()));
            cell.set_diff_option(CellDiffOption::ForcedWidth(
                NonZeroU16::new(width as u16).expect("width is at least one"),
            ));
            x += width as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_links_open_their_destination_from_their_label() {
        let text = "see [the docs](https://example.com/a) and [/tmp](/no/such/path)";
        assert_eq!(
            text_links(text, Path::new("/"), &[]),
            [Link {
                range: 5..13,
                target: "https://example.com/a".into(),
            }]
        );
    }

    #[test]
    fn links_get_a_dotted_underline_that_keeps_cell_widths() {
        let area = Rect::new(0, 0, 8, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "a界b c", ratatui::style::Style::default());
        let before = buf.clone();
        let links = [(Rect::new(0, 0, 4, 1), PathBuf::from("/a"))];
        underline(&mut buf, &links);
        let symbols: Vec<_> = (0..5).map(|x| buf[(x, 0)].symbol().to_owned()).collect();
        assert_eq!(
            symbols,
            [
                "\x1b[4:4ma\x1b[24m",
                "\x1b[4:4m界\x1b[24m",
                " ",
                "\x1b[4:4mb\x1b[24m",
                " "
            ]
        );
        assert_eq!(before.diff(&buf).len(), 3);
        assert!(
            before
                .diff(&buf)
                .iter()
                .all(|(x, _, cell)| *x != 2 && cell.symbol().contains("\x1b[4:4m"))
        );
    }

    #[test]
    fn a_line_range_after_a_text_file_opens_its_first_line_in_vs_code() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("log.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let text = format!("{}:10-40", file.display());
        assert_eq!(
            text_links(&text, dir.path(), &[]),
            [Link {
                range: 0..text.len(),
                target: format!("{}:10", super::super::vscode::open_target(&file).unwrap()).into(),
            }]
        );
    }

    #[test]
    fn rooted_paths_link_only_when_the_target_exists() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plan.md");
        let absolute = file.to_str().unwrap();
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let home_path = format!(
            "~/{}{}",
            "../".repeat(home.components().count() - 1),
            file.strip_prefix("/").unwrap().display()
        );
        for path in [absolute, &home_path] {
            assert!(text_links(path, dir.path(), &[]).is_empty());
            assert!(
                path_links(path, std::slice::from_ref(&(0..path.len())), dir.path()).is_empty()
            );
            assert!(!super::super::paths::text_paths(path, dir.path()).is_empty());
        }
        std::fs::write(&file, "").unwrap();
        for path in [absolute, &home_path] {
            let target = resolve(path, dir.path()).unwrap();
            assert_eq!(target.canonicalize().unwrap(), file.canonicalize().unwrap());
            assert_eq!(text_links(path, dir.path(), &[]).len(), 1);
            assert_eq!(
                path_links(path, std::slice::from_ref(&(0..path.len())), dir.path()).len(),
                1
            );
        }
        assert!(resolve("~/C…/.w…/a…/s…/l…/plan.md", dir.path()).is_none());
        assert_eq!(
            resolve("https://github.com/example/project/pull/1", dir.path()),
            super::super::vscode::pull_request_link(
                "https://github.com/example/project/pull/1",
                dir.path()
            )
            .map(PathBuf::from)
        );
    }

    #[test]
    fn text_links_find_absolute_home_existing_relative_paths_and_image_labels() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
        std::fs::write(dir.path().join("src/ui/log.rs"), "").unwrap();
        let absolute = dir.path().join("a/b.md");
        std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        std::fs::write(&absolute, "").unwrap();
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let text = format!(
            "see `src/ui/log.rs`, and/or ~/., {} and https://x.io/a/b [Image #1]",
            absolute.display()
        );
        let image = dir.path().join("1.png");
        std::fs::write(&image, "").unwrap();
        let images = [crate::images::Image {
            label: "[Image #1]".into(),
            source: file_source(image.clone()),
        }];
        let links = text_links(&text, dir.path(), &images);
        let targets: Vec<_> = links
            .iter()
            .map(|link| (&text[link.range.clone()], link.target.clone()))
            .collect();
        assert_eq!(
            targets,
            [
                ("src/ui/log.rs", dir.path().join("src/ui/log.rs")),
                ("~/", home),
                (absolute.to_str().unwrap(), absolute.clone()),
                ("[Image #1]", image),
            ]
        );
    }

    #[test]
    fn deleted_images_are_not_links() {
        let images = [crate::images::Image {
            label: "[Image #1]".into(),
            source: file_source("/nonexistent/1.png".into()),
        }];
        assert!(text_links("see [Image #1]", Path::new("/"), &images).is_empty());
    }

    fn file_source(path: PathBuf) -> crate::images::ImageSource {
        let metadata = std::fs::metadata(&path).ok();
        crate::images::ImageSource::File {
            path,
            len: metadata.as_ref().map_or(0, std::fs::Metadata::len),
            modified: metadata
                .and_then(|metadata| metadata.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH),
        }
    }

    #[test]
    fn relative_paths_need_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(text_links("src/missing.rs and/or", dir.path(), &[]).is_empty());
    }
}
