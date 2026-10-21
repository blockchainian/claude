#[test]
fn short_pickers_align_to_bottom_with_matching_hits() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(directory.path());
    let mut view = draft_view(&repository, directory.path());
    view.input_choices
        .get_mut(&ProviderId::Claude)
        .unwrap()
        .models
        .truncate(2);
    let area = Rect::new(3, 2, 40, 12);
    for field in [Field::Directory, Field::Branch, Field::Model, Field::Effort] {
        view.open_field(field);
        let selector = view.selector.as_ref().unwrap();
        let count = selector.rows().len() as u16;
        let branch = field == Field::Branch;
        let mut buffer = Buffer::empty(area);
        view.render_controls(area, &mut buffer);
        let first = view.viewport.borrow().picker_rows[0].0;
        let header = u16::from(matches!(field, Field::Directory | Field::Model));
        assert_eq!(
            first.y,
            area.bottom() - count - if branch { 2 } else { 0 } + header,
            "{field:?}"
        );
        for (rect, _) in &view.viewport.borrow().picker_rows {
            assert!((rect.x..rect.right()).any(|x| buffer[(x, rect.y)].symbol() != " "));
        }
        if !branch {
            assert_eq!(
                view.viewport
                    .borrow()
                    .picker_rows
                    .last()
                    .unwrap()
                    .0
                    .bottom(),
                area.bottom()
            );
        }
    }
}

#[test]
fn branch_gap_survives_editing_and_yields_to_one_visible_branch() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(directory.path());
    let mut view = draft_view(&repository, directory.path());
    view.open_field(Field::Branch);
    for editing in [false, true] {
        if let SelectorKind::Branch { editing: value, .. } =
            &mut view.selector.as_mut().unwrap().kind
        {
            *value = editing;
        }
        for height in [0, 1, 2, 3, 4, 10] {
            let area = Rect::new(3, 2, 40, height);
            let mut buffer = Buffer::empty(area);
            view.render_controls(area, &mut buffer);
            let viewport = view.viewport.borrow();
            if height == 0 {
                assert!(viewport.picker_rows.is_empty());
                continue;
            }
            let create = viewport.picker_rows.last().unwrap().0;
            assert_eq!(create.y, area.bottom() - 1);
            if height >= 2 {
                let branch = viewport.picker_rows[viewport.picker_rows.len() - 2].0;
                assert_eq!(create.y - branch.y, if height >= 3 { 2 } else { 1 });
            }
            if height >= 3 {
                assert!((area.x..area.right()).all(|x| buffer[(x, create.y - 1)].symbol() == " "));
                assert!(
                    !viewport
                        .picker_rows
                        .iter()
                        .any(|(rect, _)| rect.y == create.y - 1)
                );
            }
        }
    }
}

#[test]
fn clicking_bottom_aligned_model_row_applies_its_choice() {
    let directory = tempfile::tempdir().unwrap();
    let mut view = draft_view(directory.path(), directory.path());
    view.open_field(Field::Model);
    screen(&view, Rect::new(0, 0, 160, 40));
    let (index, expected) = match &view.selector.as_ref().unwrap().kind {
        SelectorKind::Model { values, .. } => (values.len() - 1, values.last().unwrap().clone()),
        _ => unreachable!(),
    };
    let task_details = view.viewport.borrow().task_details;
    let bottom = Rect::new(task_details.x, task_details.bottom() - 1, task_details.width, 1);
    assert_eq!(row_of(&view, index), bottom);
    assert_eq!(click(&mut view, bottom), None);
    assert_eq!(
        view.drafts[0].setup.model.as_deref(),
        Some(expected.as_str())
    );
    assert!(view.selector.is_none());
}
