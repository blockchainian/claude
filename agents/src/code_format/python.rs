pub(super) fn format(text: &str) -> Option<String> {
    use ruff_python_formatter::{PyFormatOptions, format_module_source};

    format_module_source(text, PyFormatOptions::default())
        .ok()
        .map(|printed| printed.into_code())
}
