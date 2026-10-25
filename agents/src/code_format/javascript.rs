pub(super) fn format(path: &str, text: &str) -> Option<String> {
    use dprint_plugin_typescript::configuration::{
        Configuration, ConfigurationBuilder, QuoteStyle,
    };
    use dprint_plugin_typescript::{FormatTextOptions, format_text};

    static CONFIG: std::sync::OnceLock<Configuration> = std::sync::OnceLock::new();
    let config = CONFIG.get_or_init(|| {
        ConfigurationBuilder::new()
            .quote_style(QuoteStyle::PreferSingle)
            .line_width(80)
            .build()
    });
    format_text(FormatTextOptions {
        path: std::path::Path::new(path),
        extension: None,
        text: text.to_owned(),
        config,
        external_formatter: None,
    })
    .ok()
    .flatten()
}
