# nucleo-matcher 0.3.1

Source: https://crates.io/crates/nucleo-matcher/0.3.1
SHA-256: bf33f538733d1a5a3494b836ba913207f14d9d4a1d3cd67030c5061bdd2cac85
License: MPL-2.0; see LICENSE.

The only source change from the published crate is in `src/exact.rs`:
`substring_match_non_ascii` scans `haystack[start..=end]` instead of
`haystack[start..end]`. `end` is the final legal substring start, so it must
be included. This fixes missing suffix matches in logs containing Unicode.

The packaged Cargo.toml also allows `unnecessary_transmutes` and
`mismatched_lifetime_syntaxes`, diagnostics in unchanged upstream source that
were suppressed when Cargo built it as a registry dependency. No other lint
levels are changed.

As checked on 2026-10-18, crates.io has no release after 0.3.1, and upstream
`matcher/src/exact.rs` still uses the exclusive range. Its latest modifying
commit is 3d46b62546aa8fa6784d37b13942cc7b6b062134.

Regression coverage: `src/ui/log_search_tests.rs` loads a real Codex command
event and checks its trailing tool name with ASCII, Chinese, middle-dot and
Unicode-chevron body text.
