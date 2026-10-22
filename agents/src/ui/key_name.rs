pub(super) fn display(key: &str) -> String {
    let mut result = String::with_capacity(key.len());
    for part in key.split_inclusive(|ch: char| !ch.is_ascii_alphanumeric()) {
        let end = part
            .find(|ch: char| !ch.is_ascii_alphanumeric())
            .unwrap_or(part.len());
        let name = &part[..end];
        let lower = name.to_ascii_lowercase();
        let displayed = match lower.as_str() {
            "esc" => "Esc",
            "enter" => "Enter",
            "tab" => "Tab",
            "space" => "Space",
            "backspace" => "Backspace",
            "pgup" => "PgUp",
            "pgdn" => "PgDn",
            "delete" => "Delete",
            "home" => "Home",
            "end" => "End",
            "insert" => "Insert",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "ctrl" => "Ctrl",
            "shift" => "Shift",
            "alt" => "Alt",
            "super" => "Super",
            "meta" => "Meta",
            _ => name,
        };
        result.push_str(displayed);
        result.push_str(&part[end..]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::display;

    #[test]
    fn special_key_names_use_their_display_case() {
        for (key, expected) in [
            ("esc", "Esc"),
            ("enter", "Enter"),
            ("tab", "Tab"),
            ("space", "Space"),
            ("backspace", "Backspace"),
            ("pgup", "PgUp"),
            ("pgdn", "PgDn"),
            ("delete", "Delete"),
            ("home", "Home"),
            ("end", "End"),
            ("insert", "Insert"),
        ] {
            assert_eq!(display(key), expected);
        }
    }

    #[test]
    fn combined_special_keys_preserve_their_separators() {
        for (key, expected) in [
            ("tab/⇧tab", "Tab/⇧Tab"),
            ("ctrl+enter", "Ctrl+Enter"),
            ("⌃enter", "⌃Enter"),
            ("shift+pgup", "Shift+PgUp"),
            ("Esc/Enter", "Esc/Enter"),
        ] {
            assert_eq!(display(key), expected);
        }
    }

    #[test]
    fn character_keys_and_gestures_keep_their_spelling() {
        for key in [
            "n",
            "o",
            "f",
            "?",
            "^c",
            "^w",
            "⌃c",
            "⌃w",
            "⌃w h/j/k/l",
            "↑/↓",
        ] {
            assert_eq!(display(key), key);
        }
    }
}
