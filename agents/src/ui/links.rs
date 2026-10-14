//! Clickable paths and image placeholders: where they sit in a text, and where they were drawn.
use super::text_selection::DrawnRow;
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
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

/// The paths and image placeholders in `text`: absolute and home paths, relative
/// paths that exist under `cwd`, and the label of each of `images`.
pub(super) fn text_links(text: &str, cwd: &Path, images: &[crate::log::Image]) -> Vec<Link> {
    let mut links = path_links(text, &super::paths::text_paths(text, cwd), cwd);
    for image in images {
        links.extend(text.match_indices(&image.label).map(|(at, label)| Link {
            range: at..at + label.len(),
            target: image.path.clone(),
        }));
    }
    links.sort_by_key(|link| link.range.start);
    links
}

/// The links of a text whose paths are already known, such as a tool call's arguments.
pub(super) fn path_links(text: &str, paths: &[Range<usize>], cwd: &Path) -> Vec<Link> {
    paths
        .iter()
        .filter_map(|range| {
            Some(Link {
                range: range.clone(),
                target: resolve(&text[range.clone()], cwd)?,
            })
        })
        .collect()
}

/// Where `path` opens: absolute paths and URLs as they are, `~/` under the home
/// directory, and relative paths under `cwd` when they exist there.
pub(super) fn resolve(path: &str, cwd: &Path) -> Option<PathBuf> {
    if path.starts_with('/') || path.contains("://") {
        return Some(PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Some(Path::new(&std::env::var_os("HOME")?).join(rest));
    }
    let target = cwd.join(path);
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
    };
    drawn_links(std::slice::from_ref(&row), links)
        .into_iter()
        .map(|(rect, target)| (rect.x..rect.right(), target))
        .collect()
}

/// Underlines the cells of each drawn link.
pub(super) fn underline(buf: &mut Buffer, links: &[DrawnLink]) {
    for (rect, _) in links {
        let rect = rect.intersection(buf.area);
        for x in rect.left()..rect.right() {
            buf[(x, rect.y)].modifier.insert(Modifier::UNDERLINED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_links_find_absolute_home_existing_relative_paths_and_image_labels() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
        std::fs::write(dir.path().join("src/ui/log.rs"), "").unwrap();
        let home = std::env::var("HOME").unwrap();
        let text =
            "see `src/ui/log.rs`, and/or ~/notes.md, /tmp/a/b.md and https://x.io/a/b [Image #1]";
        let images = [crate::log::Image {
            label: "[Image #1]".into(),
            path: "/tmp/1.png".into(),
        }];
        let links = text_links(text, dir.path(), &images);
        let targets: Vec<_> = links
            .iter()
            .map(|link| (&text[link.range.clone()], link.target.clone()))
            .collect();
        assert_eq!(
            targets,
            [
                ("src/ui/log.rs", dir.path().join("src/ui/log.rs")),
                ("~/notes.md", PathBuf::from(format!("{home}/notes.md"))),
                ("/tmp/a/b.md", PathBuf::from("/tmp/a/b.md")),
                ("[Image #1]", PathBuf::from("/tmp/1.png")),
            ]
        );
    }

    #[test]
    fn relative_paths_need_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(text_links("src/missing.rs and/or", dir.path(), &[]).is_empty());
    }
}
