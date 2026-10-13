//! Identity colors for session titles, drawn from a syntax theme's accents the way Codex does.
//! The default theme follows the terminal background; a `.tmTheme` file in the preferences replaces it.

use ratatui::style::Color;
use std::path::Path;
use std::sync::Mutex;

type Rgb = (u8, u8, u8);

/// Eight accents for light backgrounds, chosen to differ from each other as much as possible while
/// keeping a WCAG contrast of at least 4.5 on a light blue-gray terminal background.
const LIGHT: &[Rgb] = &[
    (161, 43, 1),
    (91, 59, 0),
    (23, 100, 7),
    (4, 92, 114),
    (12, 51, 160),
    (109, 55, 174),
    (89, 48, 93),
    (156, 25, 108),
];

/// Catppuccin Mocha accents at 60% chroma and at most 0.75 OKLCH lightness, plus two hues filling
/// its widest gaps, for dark backgrounds.
const MOCHA: &[Rgb] = &[
    (150, 185, 147),
    (193, 164, 185),
    (206, 164, 139),
    (184, 163, 211),
    (141, 184, 176),
    (149, 175, 217),
    (186, 173, 144),
    (205, 160, 167),
    (217, 154, 169),
    (137, 183, 192),
    (189, 168, 168),
    (136, 182, 203),
    (165, 171, 209),
    (183, 171, 168),
    (173, 178, 141),
    (145, 185, 164),
];

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

static ACCENTS: Mutex<Vec<Rgb>> = Mutex::new(Vec::new());

pub fn use_default_accents(light_background: bool) {
    set_accents(if light_background { LIGHT } else { MOCHA }.to_vec());
}

fn set_accents(accents: Vec<Rgb>) {
    *ACCENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = accents;
}

/// Loads the accents of a `.tmTheme` file; returns the reason when it cannot be used.
pub fn use_theme_file(path: &Path) -> Result<(), String> {
    let theme: plist::Value =
        plist::from_file(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let accents = theme_accents(&theme);
    if accents.is_empty() {
        return Err(format!("{}: no accent colors", path.display()));
    }
    set_accents(accents);
    Ok(())
}

/// Picks the same accent for the same id every time, like Codex's thread colors: FNV-1a over the id.
pub fn color_for_id(id: &str) -> Color {
    let accents = ACCENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let accents = if accents.is_empty() { MOCHA } else { &accents };
    let hash = id.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    let (r, g, b) = accents[(hash % accents.len() as u64) as usize];
    Color::Rgb(r, g, b)
}

/// Every scope foreground in file order, deduplicated, excluding the base text and background
/// colors, the comment and punctuation colors, and foregrounds paired with a special background.
fn scope(rule: &plist::Value) -> Option<&str> {
    rule.as_dictionary()?.get("scope")?.as_string()
}

fn theme_accents(theme: &plist::Value) -> Vec<Rgb> {
    let Some(rules) = theme
        .as_dictionary()
        .and_then(|theme| theme.get("settings"))
        .and_then(plist::Value::as_array)
    else {
        return Vec::new();
    };
    let rule_color = |rule: &plist::Value, key: &str| {
        rule.as_dictionary()?
            .get("settings")?
            .as_dictionary()?
            .get(key)?
            .as_string()
            .and_then(parse_hex_color)
    };
    let global = rules.iter().find(|rule| scope(rule).is_none());
    let base_foreground = global.and_then(|rule| rule_color(rule, "foreground"));
    let base_background = global.and_then(|rule| rule_color(rule, "background"));
    let mut excluded = vec![base_foreground, base_background];
    for name in ["comment", "punctuation"] {
        excluded.push(
            rules
                .iter()
                .filter(|rule| scope(rule).is_some_and(|scope| selects(scope, name)))
                .find_map(|rule| rule_color(rule, "foreground")),
        );
    }
    let mut colors = Vec::new();
    for rule in rules.iter().filter(|rule| scope(rule).is_some()) {
        let background = rule_color(rule, "background");
        if background.is_some() && background != base_background {
            continue;
        }
        let Some(color) = rule_color(rule, "foreground") else {
            continue;
        };
        if !excluded.contains(&Some(color)) && !colors.contains(&color) {
            colors.push(color);
        }
    }
    colors
}

/// A selector list such as `comment, comment.line` selects `name` when one entry is that scope
/// or one of its sub-scopes.
fn selects(selector: &str, name: &str) -> bool {
    selector
        .split(',')
        .map(str::trim)
        .any(|entry| entry == name || entry.starts_with(&format!("{name}.")))
}

/// `#RRGGBB` or `#RRGGBBAA`; translucent colors stand for terminal defaults and are skipped.
fn parse_hex_color(value: &str) -> Option<Rgb> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    if hex.len() == 8 && channel(6)? != 255 {
        return None;
    }
    Some((channel(0)?, channel(2)?, channel(4)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_palettes_reach_every_accent() {
        let _guard = TEST_LOCK.lock().unwrap();
        for palette in [LIGHT, MOCHA] {
            set_accents(palette.to_vec());
            let assigned: std::collections::HashSet<_> =
                (0..512).map(|id| color_for_id(&id.to_string())).collect();
            assert_eq!(assigned.len(), palette.len());
        }
    }

    #[test]
    fn theme_file_accents_skip_base_comment_and_backgrounds() {
        let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>settings</key><array>
<dict><key>settings</key><dict><key>foreground</key><string>#323232</string><key>background</key><string>#ffffff</string></dict></dict>
<dict><key>scope</key><string>comment, comment.line</string><key>settings</key><dict><key>foreground</key><string>#969896</string></dict></dict>
<dict><key>scope</key><string>keyword</string><key>settings</key><dict><key>foreground</key><string>#a71d5d</string></dict></dict>
<dict><key>scope</key><string>string</string><key>settings</key><dict><key>foreground</key><string>#183691</string></dict></dict>
<dict><key>scope</key><string>markup.inserted</string><key>settings</key><dict><key>foreground</key><string>#55a532</string><key>background</key><string>#eaffea</string></dict></dict>
<dict><key>scope</key><string>constant</string><key>settings</key><dict><key>foreground</key><string>#a71d5d</string></dict></dict>
<dict><key>scope</key><string>entity</string><key>settings</key><dict><key>foreground</key><string>#323232</string></dict></dict>
</array></dict></plist>"##;
        let theme: plist::Value = plist::from_bytes(xml.as_bytes()).unwrap();
        assert_eq!(
            theme_accents(&theme),
            vec![(0xa7, 0x1d, 0x5d), (0x18, 0x36, 0x91)]
        );
    }
}
