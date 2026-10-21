use super::*;
use crate::providers::{AccountUsage, ResetCreditOffer, UsageWindow, WeeklyUsage};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;

fn offer() -> ResetCreditOffer {
    ResetCreditOffer {
        window: UsageWindow::Weekly,
        resets_at: chrono::Utc::now() + chrono::Duration::days(3),
        credit_id: "fixture-credit".into(),
        expires_at: None,
    }
}

fn screen(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[test]
fn fresh_account_usage_offers_reset_and_cancel_survives_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(Hub::new(vec![]), dir.path().into());
    let usage = AccountUsage {
        weekly: Some(WeeklyUsage::from_used_percent(100.0, Some(offer().resets_at)).unwrap()),
        reset_offers: vec![offer()],
    };
    app.complete_account_usage(ProviderId::Codex, Ok(usage.clone()));
    assert!(screen(&mut app).contains("Redeem a Codex reset credit?"));
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    app.complete_account_usage(ProviderId::Codex, Ok(usage.clone()));
    assert!(!screen(&mut app).contains("Redeem a Codex reset credit?"));
    let mut next = usage;
    next.reset_offers[0].resets_at += chrono::Duration::days(7);
    app.complete_account_usage(ProviderId::Codex, Ok(next));
    assert!(screen(&mut app).contains("Redeem a Codex reset credit?"));
}

#[test]
fn five_hour_offer_does_not_require_weekly_display() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(Hub::new(vec![]), dir.path().into());
    let mut reset = offer();
    reset.window = UsageWindow::FiveHour;
    app.complete_account_usage(
        ProviderId::Claude,
        Ok(AccountUsage {
            weekly: None,
            reset_offers: vec![reset],
        }),
    );
    assert!(screen(&mut app).contains("5-hour usage limit"));
    assert!(app.view.weekly_usage.is_empty());
}

#[test]
fn redemption_refreshes_usage_and_transport_retry_keeps_the_uuid() {
    use crate::providers::{ResetCreditOutcome, ResetCreditRedemption};
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(Hub::new(vec![]), dir.path().into());
    let limited = AccountUsage {
        weekly: Some(WeeklyUsage::from_used_percent(100.0, Some(offer().resets_at)).unwrap()),
        reset_offers: vec![offer()],
    };
    app.complete_account_usage(ProviderId::Codex, Ok(limited));
    let Some(Action::RedeemResetCredit { request, .. }) = app
        .view
        .handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
    else {
        panic!("confirmed reset action");
    };
    app.complete_reset_credit(
        ProviderId::Codex,
        request.clone(),
        Err(io::Error::new(io::ErrorKind::TimedOut, "fixture timeout")),
    );
    app.view.present_reset_credit();
    let Some(Action::RedeemResetCredit { request: retry, .. }) = app
        .view
        .handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
    else {
        panic!("retry reset action");
    };
    assert_eq!(retry, request);
    let refreshed = WeeklyUsage::from_used_percent(0.0, Some(request.offer.resets_at)).unwrap();
    app.complete_reset_credit(
        ProviderId::Codex,
        retry,
        Ok(ResetCreditRedemption {
            outcome: ResetCreditOutcome::Reset,
            reason: None,
            usage: AccountUsage {
                weekly: Some(refreshed.clone()),
                reset_offers: vec![],
            },
        }),
    );
    assert_eq!(app.view.weekly_usage[&ProviderId::Codex], refreshed);
    assert_eq!(
        Preferences::load(dir.path()).usage[&ProviderId::Codex],
        refreshed
    );
    app.view.present_reset_credit();
    assert!(!screen(&mut app).contains("Redeem a Codex reset credit?"));
}
