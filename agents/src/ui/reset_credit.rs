use super::{Action, ConfirmAction, ConfirmDialog, ProviderId, View};
use crate::providers::{ResetCreditOffer, ResetCreditRequest, UsageWindow};
use chrono::{DateTime, Utc};

pub(super) struct ResetIncident {
    provider: ProviderId,
    request: ResetCreditRequest,
    offered: bool,
    active: bool,
}

pub(super) fn relative_time(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (at - now).num_seconds().max(0);
    let (count, unit) = if seconds < 3600 {
        ((seconds / 60).max(1), "minute")
    } else if seconds < 48 * 3600 {
        (seconds / 3600, "hour")
    } else {
        (seconds / 86400, "day")
    };
    format!("in {count} {unit}{}", if count == 1 { "" } else { "s" })
}

pub(super) fn reset_text(
    provider: ProviderId,
    offer: &ResetCreditOffer,
    now: DateTime<Utc>,
) -> (String, String) {
    let provider = match provider {
        ProviderId::Claude => "Claude",
        ProviderId::Codex => "Codex",
    };
    let window = match offer.window {
        UsageWindow::FiveHour => "5-hour",
        UsageWindow::Weekly => "weekly",
    };
    let mut description = format!(
        "{provider} reached its {window} usage limit. The limit resets {}.",
        relative_time(offer.resets_at, now)
    );
    if let Some(expires_at) = offer.expires_at {
        description.push_str(&format!(
            " This reset credit expires {}.",
            relative_time(expires_at, now)
        ));
    }
    (format!("Redeem a {provider} reset credit?"), description)
}

impl View {
    /// Offers each limited window once until its reset timestamp changes.
    pub(crate) fn offer_reset_credit(
        &mut self,
        provider: ProviderId,
        offers: Vec<ResetCreditOffer>,
    ) {
        self.reset_incidents.retain(|incident| {
            incident.provider != provider
                || !offers.iter().any(|offer| {
                    offer.window == incident.request.offer.window
                        && offer.resets_at != incident.request.offer.resets_at
                })
        });
        for incident in &mut self.reset_incidents {
            if incident.provider == provider {
                incident.active = false;
            }
        }
        for offer in offers {
            if let Some(incident) = self.reset_incidents.iter_mut().find(|incident| {
                incident.provider == provider
                    && incident.request.offer.window == offer.window
                    && incident.request.offer.resets_at == offer.resets_at
            }) {
                // Refresh credit details without changing this incident's idempotency key.
                incident.request.offer = offer;
                incident.active = true;
            } else {
                self.reset_incidents.push(ResetIncident {
                    provider,
                    request: ResetCreditRequest {
                        offer,
                        idempotency_key: uuid::Uuid::new_v4().to_string(),
                    },
                    offered: false,
                    active: true,
                });
            }
        }
        self.present_reset_credit();
    }

    pub(crate) fn reset_credit_failed(
        &mut self,
        provider: ProviderId,
        request: &ResetCreditRequest,
    ) {
        self.reset_credit_completed(provider);
        for incident in &mut self.reset_incidents {
            if incident.provider == provider
                && incident.request.idempotency_key == request.idempotency_key
            {
                incident.offered = false;
            }
        }
    }

    pub(crate) fn reset_credit_completed(&mut self, provider: ProviderId) {
        self.reset_pending.remove(&provider);
    }

    pub(crate) fn present_reset_credit(&mut self) {
        if self.confirm_dialog.is_some()
            || self.help
            || self.rename_target.is_some()
            || self.name_target.is_some()
            || self.search_focused()
            || self.selector.is_some()
        {
            return;
        }
        if let Some(incident) = self.reset_incidents.iter_mut().find(|incident| {
            incident.active && !incident.offered && !self.reset_pending.contains(&incident.provider)
        }) {
            incident.offered = true;
            self.confirm_dialog = Some(ConfirmDialog {
                action: ConfirmAction::Reset {
                    provider: incident.provider,
                    request: incident.request.clone(),
                },
                title: String::new(),
                selected: 0,
                worktree: None,
            });
        }
    }

    pub(super) fn accept_confirmation(&mut self, dialog: ConfirmDialog) -> Option<Action> {
        match dialog.action {
            ConfirmAction::Archive(id) => Some(if dialog.selected == 1 {
                Action::ArchiveWorktree {
                    id,
                    path: dialog
                        .worktree
                        .expect("archive second option has a linked worktree"),
                }
            } else {
                Action::Archive(id)
            }),
            ConfirmAction::Delete(id) => {
                if dialog.selected == 2 {
                    Some(Action::DeleteWorktree {
                        id,
                        path: dialog.worktree.expect("third option has a linked worktree"),
                    })
                } else {
                    self.confirm_delete(id)
                }
            }
            ConfirmAction::Reset { provider, request } => {
                self.reset_pending.insert(provider);
                Some(Action::RedeemResetCredit { provider, request })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    mod tmux {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/tmux.rs"
        ));
    }
    use super::*;
    use crate::ui::{Action, Grouping, View};
    use crossterm::event::{KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::{buffer::Buffer, layout::Rect};

    fn fixture() -> View {
        View::new(Grouping::Project, false)
    }
    fn key(view: &mut View, code: KeyCode) -> Option<Action> {
        view.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn screen(view: &View, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        buf.content
            .chunks(usize::from(width))
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    use crate::providers::{ResetCreditOffer, UsageWindow};
    use chrono::{Duration, TimeZone, Utc};
    use crossterm::event::KeyCode;

    fn offer() -> ResetCreditOffer {
        let now = Utc::now();
        ResetCreditOffer {
            window: UsageWindow::Weekly,
            resets_at: now + Duration::days(3) + Duration::minutes(1),
            credit_id: "grant-1".into(),
            expires_at: Some(now + Duration::days(2) + Duration::minutes(1)),
        }
    }

    #[test]
    fn relative_times_floor_and_pluralize() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        for (seconds, expected) in [
            (0, "in 1 minute"),
            (59, "in 1 minute"),
            (119, "in 1 minute"),
            (120, "in 2 minutes"),
            (3600, "in 1 hour"),
            (86400, "in 24 hours"),
            (172799, "in 47 hours"),
            (172800, "in 2 days"),
            (259200, "in 3 days"),
        ] {
            assert_eq!(
                relative_time(now + Duration::seconds(seconds), now),
                expected
            );
        }
    }

    #[test]
    fn copy_matches_approved_text_and_omits_absent_expiry() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let mut offer = offer();
        offer.resets_at = now + Duration::days(3);
        offer.expires_at = Some(now + Duration::days(2));
        assert_eq!(reset_text(crate::providers::ProviderId::Claude, &offer, now), ("Redeem a Claude reset credit?".into(), "Claude reached its weekly usage limit. The limit resets in 3 days. This reset credit expires in 2 days.".into()));
        offer.window = UsageWindow::FiveHour;
        offer.expires_at = None;
        assert_eq!(
            reset_text(crate::providers::ProviderId::Codex, &offer, now).1,
            "Codex reached its 5-hour usage limit. The limit resets in 3 days."
        );
    }

    #[test]
    fn cancel_suppresses_incident_but_changed_reset_time_reopens() {
        let mut view = fixture();
        let mut offer = offer();
        view.offer_reset_credit(crate::providers::ProviderId::Claude, vec![offer.clone()]);
        assert_eq!(view.confirm_dialog.as_ref().unwrap().selected, 0);
        let rendered = screen(&view, 120, 48);
        assert!(rendered.contains("Redeem a Claude reset credit?"));
        assert!(rendered.contains("2. Redeem reset credit"));
        assert!(key(&mut view, KeyCode::Esc).is_none());
        view.offer_reset_credit(crate::providers::ProviderId::Claude, vec![offer.clone()]);
        assert!(view.confirm_dialog.is_none());
        view.offer_reset_credit(crate::providers::ProviderId::Claude, vec![]);
        view.offer_reset_credit(crate::providers::ProviderId::Claude, vec![offer.clone()]);
        assert!(view.confirm_dialog.is_none());
        offer.resets_at += Duration::weeks(1);
        view.offer_reset_credit(crate::providers::ProviderId::Claude, vec![offer]);
        assert!(view.confirm_dialog.is_some());
    }

    #[test]
    fn queued_offer_waits_for_other_modal_and_keeps_uuid() {
        let mut view = fixture();
        view.help = true;
        let offer = offer();
        view.offer_reset_credit(ProviderId::Claude, vec![offer.clone()]);
        assert!(view.confirm_dialog.is_none());
        let uuid = view.reset_incidents[0].request.idempotency_key.clone();
        assert!(uuid::Uuid::parse_str(&uuid).is_ok());
        view.help = false;
        view.offer_reset_credit(ProviderId::Claude, vec![offer.clone()]);
        key(&mut view, KeyCode::Down);
        assert_eq!(
            key(&mut view, KeyCode::Enter),
            Some(Action::RedeemResetCredit {
                provider: ProviderId::Claude,
                request: ResetCreditRequest {
                    offer: offer.clone(),
                    idempotency_key: uuid.clone()
                }
            })
        );
        let request = view.reset_incidents[0].request.clone();
        view.reset_credit_failed(ProviderId::Claude, &request);
        view.present_reset_credit();
        assert_eq!(view.reset_incidents[0].request.idempotency_key, uuid);
        let retry = key(&mut view, KeyCode::Char('2')).unwrap();
        assert!(
            matches!(retry, Action::RedeemResetCredit { request, .. } if request.idempotency_key == uuid)
        );
        view.offer_reset_credit(ProviderId::Claude, vec![offer]);
        assert_eq!(view.reset_incidents[0].request.idempotency_key, uuid);
        assert!(view.confirm_dialog.is_none());
    }

    #[test]
    fn mouse_redeems_and_cancel_number_suppresses() {
        let mut view = fixture();
        let offer = offer();
        view.offer_reset_credit(ProviderId::Codex, vec![offer.clone()]);
        screen(&view, 120, 48);
        let rect = view
            .viewport
            .borrow()
            .confirm_options
            .iter()
            .find(|(_, selected)| *selected == 1)
            .unwrap()
            .0;
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            let result = view.handle_mouse(MouseEvent {
                kind,
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            });
            if matches!(kind, MouseEventKind::Up(_)) {
                assert!(matches!(
                    result,
                    Some(Action::RedeemResetCredit {
                        provider: ProviderId::Codex,
                        ..
                    })
                ));
            } else {
                assert!(result.is_none());
            }
        }
        let mut view = fixture();
        view.offer_reset_credit(ProviderId::Claude, vec![offer.clone()]);
        key(&mut view, KeyCode::Char('1'));
        view.offer_reset_credit(ProviderId::Claude, vec![offer]);
        assert!(view.confirm_dialog.is_none());
    }

    #[test]
    fn dialog_snapshot_and_upper_panel_position() {
        let mut view = fixture();
        view.offer_reset_credit(ProviderId::Claude, vec![offer()]);
        let area = Rect::new(0, 0, 100, 24);
        let mut buf = Buffer::empty(area);
        view.render_confirm_dialog(view.confirm_dialog.as_ref().unwrap(), area, &mut buf);
        let text = buf
            .content
            .chunks(100)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/ui_snapshots/reset_credit_dialog.txt");
        if std::env::var_os("UPDATE_RESET_SNAPSHOT").is_some() {
            std::fs::write(&path, format!("{}\n", text.trim_end())).unwrap();
        }
        assert_eq!(
            text.trim_end(),
            std::fs::read_to_string(path).unwrap().trim_end()
        );
        assert!(
            view.viewport
                .borrow()
                .confirm_options
                .iter()
                .all(|(rect, _)| rect.y < area.bottom())
        );
    }

    #[test]
    #[ignore = "fixture surface for the isolated tmux probe"]
    fn terminal_surface_fixture() {
        let Ok(verdict) = std::env::var("AGENTS_RESET_VERDICT") else {
            return;
        };
        use crossterm::{
            event::{Event, read},
            execute,
            terminal::{
                EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
            },
        };
        use ratatui::{Terminal, backend::CrosstermBackend};
        enable_raw_mode().unwrap();
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen).unwrap();
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout)).unwrap();
        let mut view = fixture();
        view.offer_reset_credit(ProviderId::Claude, vec![offer()]);
        loop {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    view.render(area, frame.buffer_mut());
                })
                .unwrap();
            if let Event::Key(event) = read().unwrap()
                && let Some(action) = view.handle_key(event)
            {
                assert!(matches!(
                    action,
                    Action::RedeemResetCredit {
                        provider: ProviderId::Claude,
                        ..
                    }
                ));
                std::fs::write(&verdict, r#"{"passed":true,"surface":"reset confirmation","assertions":["default Cancel","keyboard redeem","real View renderer"],"network_requests":0}"#).unwrap();
                break;
            }
        }
        disable_raw_mode().unwrap();
        execute!(terminal.backend_mut(), LeaveAlternateScreen).unwrap();
    }

    #[test]
    #[ignore = "requires tmux; isolated real terminal surface, never consumes credits"]
    fn reset_credit_terminal_probe() {
        use std::time::{Duration as StdDuration, Instant};
        let directory = tempfile::tempdir().unwrap();
        let verdict = directory.path().join("verdict.json");
        let server = tmux::Tmux::new(format!("agents-reset-credit-{}", std::process::id()));
        let run = |args: &[&str]| server.run(args);
        let executable = std::env::current_exe().unwrap();
        run(&[
            "new-session",
            "-d",
            "-s",
            "probe",
            "-x",
            "120",
            "-y",
            "48",
            "-e",
            &format!("AGENTS_RESET_VERDICT={}", verdict.display()),
            executable.to_str().unwrap(),
            "ui::reset_credit::tests::terminal_surface_fixture",
            "--exact",
            "--ignored",
            "--nocapture",
        ]);
        let deadline = Instant::now() + StdDuration::from_secs(15);
        let capture = loop {
            let capture = run(&["capture-pane", "-t", "probe", "-p"]);
            if capture.contains("Redeem a Claude reset credit?") {
                break capture;
            }
            assert!(
                Instant::now() < deadline,
                "reset dialog did not render: {capture}"
            );
            std::thread::sleep(StdDuration::from_millis(50));
        };
        assert!(capture.contains("❯ 1. Cancel"), "{capture}");
        assert!(capture.contains("2. Redeem reset credit"), "{capture}");
        run(&["send-keys", "-t", "probe", "Down"]);
        run(&["send-keys", "-t", "probe", "Enter"]);
        while !verdict.exists() {
            assert!(Instant::now() < deadline, "redeem action did not arrive");
            std::thread::sleep(StdDuration::from_millis(50));
        }
        let raw = std::fs::read_to_string(&verdict).unwrap();
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(json["passed"], true);
        let report =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scratchpad/reset-dialog");
        std::fs::create_dir_all(&report).unwrap();
        std::fs::write(report.join("terminal-verdict.json"), raw).unwrap();
        std::fs::write(report.join("terminal-screen.txt"), capture).unwrap();
    }
    #[test]
    fn reset_confirmation_owns_input_over_an_open_terminal() {
        use crate::providers::{Provider, claude::ClaudeProvider};
        use crate::session_terminal::TerminalScreen;
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/probe");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_str!("../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut provider = ClaudeProvider::new(dir.path().into());
        let sessions = provider.refresh(1).sessions;
        assert!(!sessions.is_empty());
        let id = sessions[0].id.clone();
        let mut view = fixture();
        view.set_sessions(sessions, false);
        if view.selected_id().is_none() {
            key(&mut view, KeyCode::Tab);
        }
        assert_eq!(view.selected_id(), Some(&id));
        let screen = std::sync::Arc::new(std::sync::Mutex::new(TerminalScreen::new(20, 80)));
        view.show_terminal(id, screen);
        view.offer_reset_credit(ProviderId::Claude, vec![offer()]);
        assert!(
            view.terminal_paste("2").is_none(),
            "paste cannot reach a covered terminal"
        );
        assert!(matches!(
            key(&mut view, KeyCode::Char('2')),
            Some(Action::RedeemResetCredit { .. })
        ));
    }

    #[test]
    fn confirmed_credit_blocks_the_other_window_until_usage_refresh() {
        let mut view = fixture();
        let weekly = offer();
        let mut hourly = weekly.clone();
        hourly.window = UsageWindow::FiveHour;
        view.offer_reset_credit(ProviderId::Claude, vec![weekly, hourly]);
        assert!(matches!(
            key(&mut view, KeyCode::Char('2')),
            Some(Action::RedeemResetCredit { .. })
        ));
        view.present_reset_credit();
        assert!(
            view.confirm_dialog.is_none(),
            "a pending reset may clear both windows"
        );
    }

    #[test]
    fn stale_queue_is_inactive_and_retry_does_not_revive_cancelled_window() {
        let mut view = fixture();
        let weekly = offer();
        let mut hourly = weekly.clone();
        hourly.window = UsageWindow::FiveHour;
        view.offer_reset_credit(ProviderId::Claude, vec![weekly.clone(), hourly.clone()]);
        key(&mut view, KeyCode::Esc);
        view.present_reset_credit();
        let request = view.reset_incidents[1].request.clone();
        key(&mut view, KeyCode::Char('2'));
        view.reset_credit_failed(ProviderId::Claude, &request);
        view.present_reset_credit();
        assert!(
            matches!(&view.confirm_dialog.as_ref().unwrap().action, ConfirmAction::Reset { request, .. } if request.offer.window == UsageWindow::FiveHour)
        );
        key(&mut view, KeyCode::Esc);
        view.offer_reset_credit(ProviderId::Claude, vec![]);
        view.present_reset_credit();
        assert!(view.confirm_dialog.is_none());
        view.offer_reset_credit(ProviderId::Claude, vec![weekly, hourly]);
        assert!(view.confirm_dialog.is_none());

        let mut view = fixture();
        view.help = true;
        view.offer_reset_credit(ProviderId::Claude, vec![offer()]);
        view.offer_reset_credit(ProviderId::Claude, vec![]);
        view.help = false;
        view.present_reset_credit();
        assert!(view.confirm_dialog.is_none());
    }
}
