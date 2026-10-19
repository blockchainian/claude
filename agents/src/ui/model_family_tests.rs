fn family_view(provider: ProviderId) -> (tempfile::TempDir, View) {
    use crate::providers::Provider;
    let directory = tempfile::tempdir().unwrap();
    let mut view = draft_view(directory.path(), directory.path());
    let choices = match provider {
        ProviderId::Claude => {
            let cache = fs::read_dir(directory.path().join("cache/model-catalog"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            fs::write(
                cache,
                include_str!("../../tests/fixtures/claude-model-families.json"),
            )
            .unwrap();
            crate::providers::claude::ClaudeProvider::new(directory.path().into()).input_choices()
        }
        ProviderId::Codex => {
            fs::write(
                directory.path().join("models_cache.json"),
                include_str!("../../tests/fixtures/codex-model-families.json"),
            )
            .unwrap();
            crate::providers::codex::CodexProvider::new(
                directory.path().into(),
                directory.path().into(),
            )
            .input_choices()
        }
    };
    view.input_choices.insert(provider, choices);
    view.drafts[0].setup.provider = provider;
    (directory, view)
}

#[test]
fn model_families_preserve_first_appearance_and_model_order() {
    for (provider, current, expected) in [
        (
            ProviderId::Claude,
            "claude-sonnet-5",
            vec![
                "#Opus",
                "Opus 5.5",
                "Opus 5",
                "",
                "#Fable",
                "Fable 5.1",
                "",
                "#Sonnet",
                "Sonnet 5.5",
                "Sonnet 5",
                "",
                "#Haiku",
                "Haiku 5.5",
            ],
        ),
        (
            ProviderId::Codex,
            "gpt-5.6-sol",
            vec![
                "#Sol",
                "GPT-6.1-Sol",
                "GPT-6-Sol",
                "GPT-5.6-Sol",
                "",
                "#Astra",
                "GPT-6-Astra",
                "",
                "#Luna",
                "GPT-6-Luna",
                "GPT-5.6-Luna",
                "",
                "#Terra",
                "GPT-5.6-Terra",
            ],
        ),
    ] {
        let (_directory, mut view) = family_view(provider);
        view.drafts[0].setup.model = Some(current.into());
        view.open_field(Field::Model);
        let selector = view.selector.as_ref().unwrap();
        let SelectorKind::Model { values, .. } = &selector.kind else {
            panic!("model selector")
        };
        assert_eq!(values[selector.selected], current);
        assert_eq!(
            selector.selected,
            if provider == ProviderId::Claude { 4 } else { 2 }
        );
        let actual: Vec<_> = selector
            .rows()
            .iter()
            .map(|row| match row {
                ListRow::Text(line) if line.width() == 0 => String::new(),
                ListRow::Text(line) => {
                    assert!(line.spans.iter().all(|span| {
                        span.style
                            .add_modifier
                            .contains(ratatui::style::Modifier::DIM)
                    }));
                    format!("#{line}")
                }
                ListRow::Choice(_, line) => line.to_string(),
            })
            .collect();
        assert_eq!(actual, expected);
        // Choice indices exclude every heading and gap, including when stepping across groups.
        let indices: Vec<_> = selector
            .rows()
            .into_iter()
            .filter_map(|row| match row {
                ListRow::Choice(index, _) => Some(index),
                _ => None,
            })
            .collect();
        assert_eq!(indices, (0..values.len()).collect::<Vec<_>>());
        let len = values.len();
        let original = selector.selected;
        screen(&view, Rect::new(0, 0, 160, 50));
        assert_eq!(view.active_panel, Panel::Details);
        for index in (original + 1..len).chain(0..=original) {
            view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            assert_eq!(view.selector.as_ref().unwrap().selected, index);
        }
        for _ in 0..len {
            view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        }
        assert_eq!(view.selector.as_ref().unwrap().selected, original);
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);
        view.render_controls(area, &mut buffer);
        let rows = view.selector.as_ref().unwrap().rows();
        let top = area.bottom() - rows.len() as u16;
        for (offset, row) in rows.iter().enumerate() {
            if matches!(row, ListRow::Text(_)) {
                let y = top + offset as u16;
                assert!(
                    !view
                        .viewport
                        .borrow()
                        .picker_rows
                        .iter()
                        .any(|(rect, _)| rect.y == y)
                );
            }
        }
        screen(&view, Rect::new(0, 0, 160, 50));
        let details = view.viewport.borrow().details;
        let top = details.bottom() - rows.len() as u16;
        for (offset, row) in rows.iter().enumerate() {
            if matches!(row, ListRow::Text(_)) {
                assert_eq!(
                    click(&mut view, Rect::new(details.x, top + offset as u16, 1, 1)),
                    None
                );
                assert_eq!(view.selector.as_ref().unwrap().selected, original);
            }
        }
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(view.selector.is_none());
        assert_eq!(view.drafts[0].setup.model.as_deref(), Some(current));
    }
}

#[test]
fn codex_models_without_a_tier_keep_their_display_name_as_a_family() {
    use crate::providers::Provider;
    let directory = tempfile::tempdir().unwrap();
    let mut cache: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/codex-model-families.json"
    ))
    .unwrap();
    // Expose recorded hidden models to exercise their non-tier slugs in the picker.
    let models = cache["models"].as_array_mut().unwrap();
    for model in models.iter_mut() {
        model["visibility"] = "list".into();
    }
    models.reverse();
    fs::write(
        directory.path().join("models_cache.json"),
        cache.to_string(),
    )
    .unwrap();
    let provider = crate::providers::codex::CodexProvider::new(
        directory.path().into(),
        directory.path().into(),
    );
    let choices = provider.input_choices();
    assert_eq!(choices.models.first().unwrap().id, "gpt-6.1-sol");
    assert_eq!(choices.model("gpt-reserve").unwrap().family, "GPT-Reserve");
    assert_eq!(
        choices.model("codex-auto-review").unwrap().family,
        "Codex Auto Review"
    );
}

#[test]
fn model_family_picker_snapshot() {
    let mut snapshots = Vec::new();
    for (provider, current) in [
        (ProviderId::Claude, "claude-sonnet-5"),
        (ProviderId::Codex, "gpt-5.6-sol"),
    ] {
        let (_directory, mut view) = family_view(provider);
        view.drafts[0].setup.model = Some(current.into());
        view.open_field(Field::Model);
        let area = Rect::new(0, 0, 32, 16);
        let mut buffer = Buffer::empty(area);
        view.render_controls(area, &mut buffer);
        let text = buffer
            .content
            .chunks(32)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        snapshots.push(format!("{provider:?}\n{text}"));
    }
    let actual = snapshots.join("\n---\n") + "\n";
    let path = Path::new("tests/ui_snapshots/model_family_picker.txt");
    if std::env::var_os("UPDATE_MODEL_FAMILY_SNAPSHOT").is_some() {
        fs::write(path, &actual).unwrap();
    } else {
        assert_eq!(actual, fs::read_to_string(path).unwrap());
    }
}
