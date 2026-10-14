//! Identity colors for session titles, drawn from a syntax theme's accents the way Codex does and
//! ordered from the strongest contrast with the terminal background to the weakest.
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

/// Eight accents for dark backgrounds, chosen the same way with a contrast of at least 3, kept dim
/// enough to sit beside a near-black terminal's muted text.
const DARK: &[Rgb] = &[
    (23, 111, 124),
    (174, 32, 89),
    (114, 99, 10),
    (113, 67, 187),
    (14, 144, 33),
    (192, 80, 8),
    (17, 117, 222),
    (146, 102, 142),
];

/// Codex's accent for links and unthemed inline code, on light and on dark backgrounds.
const LIGHT_TEXT_ACCENT: Rgb = (28, 100, 200);
const DARK_TEXT_ACCENT: Rgb = (99, 168, 248);

/// Inline code in Codex's default syntax themes, Catppuccin Latte and Catppuccin Mocha.
const LIGHT_INLINE_CODE: Rgb = (0x40, 0xa0, 0x2b);
const DARK_INLINE_CODE: Rgb = (0xa6, 0xe3, 0xa1);

/// The scopes Codex looks up, in order, for the inline code color of a syntax theme.
const INLINE_CODE_SCOPES: [&str; 2] = [
    "markup.inline.raw.string.markdown",
    "markup.raw.inline.markdown",
];

/// The OKLab lightness the first accent keeps on a dark background, so it does not outshine the rest.
const MAX_FIRST_DARK_ACCENT_LIGHTNESS: f64 = 0.583;

/// The WCAG contrast Codex keeps between accent text and the terminal background.
const MIN_TEXT_CONTRAST: f64 = 4.5;

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

static ACCENTS: Mutex<Vec<Rgb>> = Mutex::new(Vec::new());
static TEXT_ACCENT: Mutex<Rgb> = Mutex::new(DARK_TEXT_ACCENT);
/// The theme's inline code color; `None` when the theme has none, so the text accent stands in.
static INLINE_CODE: Mutex<Option<Rgb>> = Mutex::new(Some(DARK_INLINE_CODE));

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn use_default_accents(light_background: bool) {
    set_accents(
        if light_background { LIGHT } else { DARK }.to_vec(),
        light_background,
    );
    *locked(&INLINE_CODE) = Some(if light_background {
        LIGHT_INLINE_CODE
    } else {
        DARK_INLINE_CODE
    });
}

/// Picks the text accent for the terminal background, darkening or lightening it toward black or
/// white until it reaches the minimum contrast, as Codex does.
pub fn set_background(background: Option<Rgb>, light_background: bool) {
    let preferred = if light_background {
        LIGHT_TEXT_ACCENT
    } else {
        DARK_TEXT_ACCENT
    };
    *locked(&TEXT_ACCENT) =
        background.map_or(preferred, |background| readable_on(preferred, background));
}

fn readable_on(preferred: Rgb, background: Rgb) -> Rgb {
    if contrast(preferred, background) >= MIN_TEXT_CONTRAST {
        return preferred;
    }
    let endpoint = if contrast((0, 0, 0), background) >= contrast((255, 255, 255), background) {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    };
    (1..=255_u8)
        .map(|step| blend(endpoint, preferred, f32::from(step) / 255.0))
        .find(|candidate| contrast(*candidate, background) >= MIN_TEXT_CONTRAST)
        .unwrap_or(endpoint)
}

fn blend(top: Rgb, bottom: Rgb, alpha: f32) -> Rgb {
    let channel =
        |top: u8, bottom: u8| (f32::from(top) * alpha + f32::from(bottom) * (1.0 - alpha)) as u8;
    (
        channel(top.0, bottom.0),
        channel(top.1, bottom.1),
        channel(top.2, bottom.2),
    )
}

/// WCAG contrast ratio.
fn contrast(a: Rgb, b: Rgb) -> f64 {
    let luminance = |(r, g, b): Rgb| {
        0.2126 * linear_channel(r) + 0.7152 * linear_channel(g) + 0.0722 * linear_channel(b)
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

pub fn link_color() -> Color {
    let (r, g, b) = *locked(&TEXT_ACCENT);
    Color::Rgb(r, g, b)
}

pub fn inline_code_color() -> Color {
    match *locked(&INLINE_CODE) {
        Some((r, g, b)) => Color::Rgb(r, g, b),
        None => link_color(),
    }
}

/// Darkest first on a light background and lightest first on a dark one, where the lightest is
/// darkened to `MAX_FIRST_DARK_ACCENT_LIGHTNESS`.
fn set_accents(mut accents: Vec<Rgb>, light_background: bool) {
    accents.sort_by(|a, b| lightness(*a).total_cmp(&lightness(*b)));
    if !light_background {
        accents.reverse();
        if let Some(first) = accents.first_mut() {
            *first = dimmed(*first, MAX_FIRST_DARK_ACCENT_LIGHTNESS);
        }
    }
    *ACCENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = accents;
}

/// `color` blended toward black until its lightness is at most `max_lightness`.
fn dimmed(color: Rgb, max_lightness: f64) -> Rgb {
    (0..=255_u8)
        .rev()
        .map(|step| blend(color, (0, 0, 0), f32::from(step) / 255.0))
        .find(|candidate| lightness(*candidate) <= max_lightness)
        .unwrap_or((0, 0, 0))
}

/// Loads the accents of a `.tmTheme` file; returns the reason when it cannot be used.
pub fn use_theme_file(path: &Path, light_background: bool) -> Result<(), String> {
    let theme: plist::Value =
        plist::from_file(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let accents = theme_accents(&theme);
    if accents.is_empty() {
        return Err(format!("{}: no accent colors", path.display()));
    }
    set_accents(accents, light_background);
    *locked(&INLINE_CODE) = theme_inline_code(&theme);
    Ok(())
}

/// The accent in `slot`, counting from the strongest contrast and wrapping around the palette.
pub fn accent(slot: usize) -> Color {
    let accents = ACCENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let accents = if accents.is_empty() { DARK } else { &accents };
    let (r, g, b) = accents[slot % accents.len()];
    Color::Rgb(r, g, b)
}

/// How many accents the palette holds.
pub fn accent_count() -> usize {
    let accents = ACCENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if accents.is_empty() {
        DARK.len()
    } else {
        accents.len()
    }
}

/// OKLab lightness, which follows how light a color looks to people.
fn lightness((r, g, b): Rgb) -> f64 {
    let (r, g, b) = (linear_channel(r), linear_channel(g), linear_channel(b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s
}

/// An sRGB channel in linear light.
fn linear_channel(channel: u8) -> f64 {
    let channel = f64::from(channel) / 255.0;
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
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

/// The foreground the theme gives the first of `INLINE_CODE_SCOPES` it styles, matched the way
/// Codex matches a lone scope: a selector entry applies when it names that scope or one of its
/// parents, the entry with the most parts wins, and a later rule wins a tie.
fn theme_inline_code(theme: &plist::Value) -> Option<Rgb> {
    let rules = theme.as_dictionary()?.get("settings")?.as_array()?;
    INLINE_CODE_SCOPES.iter().find_map(|name| {
        rules
            .iter()
            .filter_map(|rule| {
                let foreground = rule
                    .as_dictionary()?
                    .get("settings")?
                    .as_dictionary()?
                    .get("foreground")?
                    .as_string()
                    .and_then(parse_hex_color)?;
                let parts = scope(rule)?
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| *name == *entry || name.starts_with(&format!("{entry}.")))
                    .map(|entry| entry.split('.').count())
                    .max()?;
                Some((parts, foreground))
            })
            .max_by_key(|(parts, _)| *parts)
            .map(|(_, foreground)| foreground)
    })
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
    fn accents_start_with_the_strongest_contrast_and_wrap() {
        let _guard = TEST_LOCK.lock().unwrap();
        let (black, gray, white) = ((0, 0, 0), (128, 128, 128), (255, 255, 255));
        let dimmed_white = (123, 123, 123);
        for (light_background, order) in [
            (true, [black, gray, white]),
            (false, [dimmed_white, gray, black]),
        ] {
            set_accents(vec![white, black, gray], light_background);
            let slots: Vec<_> = (0..4).map(accent).collect();
            let expected: Vec<_> = order
                .iter()
                .chain(&order[..1])
                .map(|&(r, g, b)| Color::Rgb(r, g, b))
                .collect();
            assert_eq!(slots, expected);
        }
        for light_background in [true, false] {
            use_default_accents(light_background);
            let mut lightness: Vec<_> = (0..8)
                .map(|slot| match accent(slot) {
                    Color::Rgb(r, g, b) => lightness((r, g, b)),
                    other => panic!("{other:?}"),
                })
                .collect();
            if !light_background {
                lightness.reverse();
            }
            assert!(lightness.is_sorted(), "{lightness:?}");
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

    #[test]
    fn inline_code_takes_a_single_scope_rule_as_codex_does() {
        let theme = |rules: &str| -> plist::Value {
            let xml = format!(
                r##"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>settings</key><array>
<dict><key>settings</key><dict><key>foreground</key><string>#323232</string></dict></dict>
{rules}</array></dict></plist>"##
            );
            plist::from_bytes(xml.as_bytes()).unwrap()
        };
        let rule = |scope: &str, color: &str| {
            format!(
                "<dict><key>scope</key><string>{scope}</string><key>settings</key><dict><key>foreground</key><string>{color}</string></dict></dict>\n"
            )
        };
        let nested = rule("text.html.markdown markup.raw.inline", "#111111");
        assert_eq!(theme_inline_code(&theme(&nested)), None);
        let rules = [
            nested.clone(),
            rule("markup", "#222222"),
            rule(
                "punctuation.definition.raw.markdown, markup.inline.raw.string.markdown",
                "#40a02b",
            ),
            rule("markup.raw", "#333333"),
        ]
        .concat();
        assert_eq!(theme_inline_code(&theme(&rules)), Some((0x40, 0xa0, 0x2b)));
        assert_eq!(
            theme_inline_code(&theme(&[nested, rule("markup.raw", "#333333")].concat())),
            Some((0x33, 0x33, 0x33))
        );
    }

    #[test]
    fn text_accent_darkens_until_it_reads_on_the_background() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_background(Some((0xa1, 0xca, 0xdc)), true);
        assert_eq!(link_color(), Color::Rgb(0x16, 0x4f, 0x9f));
        set_background(Some((0x00, 0x04, 0x09)), false);
        assert_eq!(link_color(), Color::Rgb(99, 168, 248));
        set_background(None, false);
        assert_eq!(link_color(), Color::Rgb(99, 168, 248));
        set_background(None, true);
        assert_eq!(link_color(), Color::Rgb(28, 100, 200));
    }

    #[test]
    fn inline_code_follows_the_theme_or_the_text_accent() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_background(None, false);
        use_default_accents(true);
        assert_eq!(inline_code_color(), Color::Rgb(0x40, 0xa0, 0x2b));
        use_default_accents(false);
        assert_eq!(inline_code_color(), Color::Rgb(0xa6, 0xe3, 0xa1));
        *INLINE_CODE.lock().unwrap() = None;
        assert_eq!(inline_code_color(), link_color());
    }
}
