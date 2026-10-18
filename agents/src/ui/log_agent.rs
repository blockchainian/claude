use super::*;

impl View {
    pub(super) fn jump_log_agent(&mut self) {
        if self.mouse_focus != Panel::LogList || !self.log_list_focused {
            return;
        }
        let Some(parent) = self
            .selected_row()
            .filter(|row| row.id.provider == ProviderId::Codex)
        else {
            return;
        };
        let Some(event) = self.visible_log_event() else {
            return;
        };
        let target = event.calls.iter().find_map(|call| {
            self.rows.iter().enumerate().find_map(|(index, session)| {
                if session.id.provider != ProviderId::Codex || session.id == parent.id {
                    return None;
                }
                let matches_id = call.agent_thread_id.as_deref() == Some(session.id.id.as_str());
                let matches_path = call.agent_path.as_deref().is_some_and(|path| {
                    if !self
                        .ancestors(index)
                        .any(|ancestor| self.rows[ancestor].id == parent.id)
                    {
                        return false;
                    }
                    let mut names: Vec<_> = self
                        .ancestors(index)
                        .filter(|&ancestor| self.rows[ancestor].parent.is_some())
                        .map(|ancestor| self.rows[ancestor].title.as_str())
                        .collect();
                    names.reverse();
                    names.push(session.title.as_str());
                    if path.starts_with('/') {
                        path == format!("/root/{}", names.join("/"))
                    } else {
                        path == session.title || session.name.as_deref() == Some(path)
                    }
                });
                (matches_id || matches_path).then_some(index)
            })
        });
        if let Some(index) = target {
            self.select(index);
        }
    }
}
