mod javascript;
mod python;
mod rust;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Language {
    JavaScript,
    TypeScript,
    Rust,
    Python,
}

impl Language {
    /// A file extension or tool language token such as "js", "mjs" or "ts".
    pub(crate) fn from_extension(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "py" | "pyi" => Some(Self::Python),
            "rs" => Some(Self::Rust),
            _ => None,
        }
    }

    /// An interpreter program such as "node" or "/usr/local/bin/node".
    pub(crate) fn from_interpreter(name: &str) -> Option<Self> {
        match std::path::Path::new(name).file_name()?.to_str()? {
            "node" => Some(Self::JavaScript),
            "python" | "python3" => Some(Self::Python),
            name if name.strip_prefix("python3.").is_some_and(|minor| {
                !minor.is_empty() && minor.bytes().all(|byte| byte.is_ascii_digit())
            }) =>
            {
                Some(Self::Python)
            }
            _ => None,
        }
    }

    /// The formatted text, or None when the formatter rejects it.
    pub(crate) fn format(self, text: &str) -> Option<String> {
        let path = match self {
            Self::JavaScript => "input.js",
            Self::TypeScript => "input.ts",
            Self::Python => return python::format(text),
            Self::Rust => return rust::format(text),
        };
        javascript::format(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::Language;

    #[test]
    fn extensions_resolve_languages() {
        for extension in ["js", "mjs", "cjs", "JS", "Mjs"] {
            assert_eq!(
                Language::from_extension(extension),
                Some(Language::JavaScript)
            );
        }
        for extension in ["ts", "mts", "cts", "TS", "Cts"] {
            assert_eq!(
                Language::from_extension(extension),
                Some(Language::TypeScript)
            );
        }
        for extension in ["", "javascript", "node", "input.js"] {
            assert_eq!(Language::from_extension(extension), None);
        }
    }

    #[test]
    fn interpreter_program_names_resolve_languages() {
        for interpreter in ["node", "/usr/local/bin/node", "./node"] {
            assert_eq!(
                Language::from_interpreter(interpreter),
                Some(Language::JavaScript)
            );
        }
        for interpreter in ["", "js", "ts", "python2", "nodejs"] {
            assert_eq!(Language::from_interpreter(interpreter), None);
        }
    }

    #[test]
    fn typescript_requires_its_language() {
        let text = "const count:number=1;";
        assert_eq!(
            Language::TypeScript.format(text).as_deref(),
            Some("const count: number = 1;\n")
        );
        assert_eq!(Language::JavaScript.format(text), None);
    }
}

#[cfg(test)]
mod rust_tests;

#[cfg(test)]
mod python_tests;
