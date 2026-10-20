use super::*;

#[test]
fn escape_routes_by_panel_and_closes_only_the_focused_layer() {
    for width in [120, 200] {
        let (directory, mut view) = git_draft();
        view.discard_selected_draft();
        let id = view.add_draft(setup(directory.path()));
        view.handle_paste("unsaved name");
        let area = Rect::new(0, 0, width, 50);
        view.render(area, &mut Buffer::empty(area));
        assert_eq!(key(&mut view, KeyCode::Esc), None);
        assert!(view.drafts.iter().all(|draft| draft.id != id));
        assert!(view.confirm_dialog.is_none());

        let id = named_draft(&mut view, directory.path(), "original");
        view.open_name(id);
        view.handle_paste("changed");
        key(&mut view, KeyCode::Esc);
        assert_eq!(
            view.selected_draft().unwrap().name.as_deref(),
            Some("original")
        );
        assert!(view.name_target.is_none());
        assert!(view.confirm_dialog.is_none());

        for field in [Field::Directory, Field::Branch] {
            view.open_field(field);
            key(&mut view, KeyCode::Esc);
            assert!(view.selector.is_none());
            assert!(view.confirm_dialog.is_none());
            assert_eq!(view.selected_draft, Some(id));
        }

        view.open_field(Field::Branch);
        key(&mut view, KeyCode::End);
        key(&mut view, KeyCode::Enter);
        view.handle_paste("discarded branch");
        key(&mut view, KeyCode::Esc);
        assert!(!view.selector.as_ref().unwrap().input_active());
        assert_eq!(view.selected_draft().unwrap().branch, DraftBranch::Unchosen);
        assert!(view.confirm_dialog.is_none());
        key(&mut view, KeyCode::Esc);
        assert!(view.selector.is_none());
        assert!(view.confirm_dialog.is_none());

        // A visible selector belongs to Details, even when Prompt has the keys.
        view.open_field(Field::Branch);
        view.set_active_panel(Panel::Prompt);
        assert!(view.selector.is_some());
        key(&mut view, KeyCode::Esc);
        assert!(view.confirm_dialog.is_some());
        assert!(view.selector.is_some());
        key(&mut view, KeyCode::Esc);
        assert!(view.confirm_dialog.is_none());
        view.close_selector();
        view.set_active_panel(Panel::Details);
        assert!(view.name_target.is_none());
        key(&mut view, KeyCode::Esc);
        assert!(view.confirm_dialog.is_some());
        assert_eq!(view.selected_draft, Some(id));
        assert_eq!(
            view.selected_draft().unwrap().name.as_deref(),
            Some("original")
        );
    }
}
