use super::*;
use crate::app::agents_overview::tests::finish_overview_refresh;
use crate::app::agents_overview::tests::overview_thread;
use crate::app::owned_transcript::tests::attach_thread;
use crate::backend_id::BackendId;
use crate::keymap::RuntimeKeymap;
use crate::transcript_mode::TranscriptMode;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadActiveFlag;
use codex_app_server_protocol::ThreadSectionMoveParams;
use codex_app_server_protocol::ThreadSectionMoveResponse;
use codex_app_server_protocol::ThreadStatus;
use codex_config::types::TuiKeymap;
use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::layout::Size;
use std::time::Duration;

async fn set_shared_pin(server: &AppServerSession, id: ThreadId, pinned: bool) {
    server
        .request_handle()
        .request_typed::<ThreadSectionMoveResponse>(ClientRequest::ThreadSectionMove {
            request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
            params: ThreadSectionMoveParams {
                thread_id: id.to_string(),
                section_id: pinned.then(|| codex_state::PINNED_THREAD_SECTION_ID.to_string()),
                before_thread_id: None,
            },
        })
        .await
        .expect("update shared pin");
}

#[tokio::test]
async fn shared_pins_render_below_header_with_status_and_unread_badges() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    let backend = BackendId::new();
    let working = ThreadId::new();
    let waiting = ThreadId::new();
    let current = ThreadId::new();
    for (id, name, status) in [
        (
            working,
            "Build",
            ThreadStatus::Active {
                active_flags: Vec::new(),
            },
        ),
        (
            waiting,
            "Review",
            ThreadStatus::Active {
                active_flags: vec![ThreadActiveFlag::WaitingOnUserInput],
            },
        ),
        (current, "Current", ThreadStatus::Idle),
    ] {
        app.backend_registry.observe(backend, id);
        app.agents_overview
            .threads
            .insert(id, Some(overview_thread(id, None, name, status)));
    }
    app.agents_overview
        .pins_by_backend
        .insert(backend, Some(vec![working, waiting, working]));
    app.agents_overview.unread.insert(working);
    app.chat_widget.local_settings.tui.status_header = Some(vec!["model-name".into()]);
    attach_thread(&mut app, current);
    app.chat_widget
        .apply_external_edit("keep this draft".into());
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let size = Size::new(120, 24);
    let rendered = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert_eq!(rendered.lines().next(), Some("  gpt-test"));
    let tabs = rendered.lines().nth(1).expect("session tabs");
    assert!(tabs.contains("▶* Build"), "{rendered}");
    assert!(tabs.contains("! Review"), "{rendered}");
    assert!(tabs.contains("○ Current"), "{rendered}");
    assert!(tabs.contains("☆"), "{rendered}");
    assert_eq!(app.agents_overview.tabs.items.len(), 3);
    assert!(rendered.contains("keep this draft"));

    let selected = app
        .agents_overview
        .tabs
        .targets()
        .into_iter()
        .find(|(target, _)| matches!(target, TabTarget::Tab(id) if *id == waiting))
        .expect("review tab")
        .1;
    assert!(matches!(
        app.agents_overview.tabs.hit((selected.x, selected.y).into()),
        Some(TabAction::Select(id)) if id == waiting
    ));
    let rendered = crate::app::test_support::side_frame(&mut app, &mut tui, Size::new(40, 4))?;
    assert!(rendered.contains("keep this draft"), "{rendered}");
    assert!(app.agents_overview.tabs.area.is_empty());

    app.chat_widget
        .show_selection_view(crate::bottom_pane::SelectionViewParams {
            view_id: Some(crate::app::AGENTS_OVERVIEW_VIEW_ID),
            title: Some("Agent command center".into()),
            items: vec![crate::bottom_pane::SelectionItem {
                name: "Choose a session".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
    crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(app.agents_overview.tabs.area.is_empty());
    assert!(app.agents_overview.tabs.items.is_empty());
    tui.set_owned_screen(false)?;
    Ok(())
}

#[test]
fn overflow_keeps_selected_tab_visible_and_last_session_tracks_displayed_threads() {
    let mut tabs = SessionTabs::default();
    let ids = (0..8).map(|_| ThreadId::new()).collect::<Vec<_>>();
    tabs.items = ids
        .iter()
        .map(|id| SessionTab {
            id: *id,
            title: format!("Task {id}"),
            group: AgentsOverviewGroup::Ready,
            unread: false,
        })
        .collect();
    tabs.observe(Some(ids[0]));
    tabs.observe(Some(ids[7]));
    tabs.observe(None);
    assert_eq!(tabs.previous, Some(ids[0]));
    let area = Rect::new(4, 2, 40, 1);
    tabs.render(area, &mut Buffer::empty(area));
    let targets = tabs.targets();
    assert!(
        targets
            .iter()
            .any(|(target, _)| matches!(target, TabTarget::Tab(id) if *id == ids[7]))
    );
    assert!(
        targets
            .iter()
            .any(|(target, _)| matches!(target, TabTarget::Overflow { .. }))
    );
    assert_eq!(tabs.adjacent(true), Some(ids[0]));
    assert_eq!(tabs.adjacent(false), Some(ids[6]));
    tabs.observe(Some(ids[0]));
    assert_eq!(tabs.previous, Some(ids[7]));
    tabs.forget(ids[7]);
    assert_eq!(tabs.previous, None);
}

#[test]
fn session_tab_keys_are_remappable_and_yield_to_existing_custom_bindings() {
    let defaults = RuntimeKeymap::from_config(&TuiKeymap::default()).unwrap();
    assert!(
        defaults
            .chat
            .session_tab_next
            .is_pressed(KeyEvent::new(KeyCode::PageDown, KeyModifiers::ALT))
    );
    assert!(
        defaults
            .chat
            .toggle_session_pin
            .is_pressed(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT))
    );
    let config: TuiKeymap =
        toml::from_str("[chat]\nsession_tab_next = 'f12'\ntoggle_voice = 'alt-p'\n").unwrap();
    let customized = RuntimeKeymap::from_config(&config).unwrap();
    assert!(
        customized
            .chat
            .session_tab_next
            .is_pressed(KeyCode::F(12).into())
    );
    assert!(
        !customized
            .chat
            .session_tab_next
            .is_pressed(KeyEvent::new(KeyCode::PageDown, KeyModifiers::ALT))
    );
    assert!(customized.chat.toggle_session_pin.is_empty());
}

#[tokio::test]
async fn external_pin_changes_refresh_shared_tabs_without_opening_the_overview() -> Result<()> {
    let (mut app, mut events, _operations) = crate::app::tests::make_test_app_with_channels().await;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let started = Box::pin(server.start_thread(&app.config)).await?;
    let id = started.session.thread_id;
    app.agents_overview.initialized.insert(server.backend_id());
    app.agents_overview
        .pins_by_backend
        .insert(server.backend_id(), Some(Vec::new()));
    for pinned in [true, false] {
        set_shared_pin(&server, id, pinned).await;
        let event = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = server.next_event().await.expect("server event");
                if matches!(&event.event, AppServerEvent::ServerNotification(notification)
                    if matches!(notification.as_ref(), ServerNotification::ThreadSectionChanged(_)))
                {
                    break event;
                }
            }
        })
        .await
        .expect("pin change notification");
        app.handle_backend_event(&server, event).await;
        finish_overview_refresh(&mut app, &server, &mut events).await;
        assert_eq!(
            app.agents_overview.pins_by_backend[&server.backend_id()]
                .as_ref()
                .expect("shared pins supported")
                .contains(&id),
            pinned,
        );
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn session_tab_shortcuts_preserve_drafts_and_emit_native_pin_action() -> Result<()> {
    let (mut app, mut events, _operations) = crate::app::tests::make_test_app_with_channels().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    let projects = serde_json::json!({
        crate::test_support::test_path_buf("/").display().to_string(): {"trust_level": "trusted"},
        app.config.cwd.display().to_string(): {"trust_level": "trusted"},
    });
    app.cli_kv_overrides
        .push(("projects".into(), toml::Value::try_from(projects)?));
    std::fs::write(
        app.local_settings.user_config_path.as_path(),
        "[tui]\nresume_cwd = \"session\"\n",
    )?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let first = Box::pin(server.start_thread(&app.config)).await?;
    let first_id = first.session.thread_id;
    set_shared_pin(&server, first_id, true).await;
    let second = Box::pin(server.start_thread(&app.config)).await?;
    let second_id = second.session.thread_id;
    set_shared_pin(&server, second_id, true).await;
    app.enqueue_primary_thread_session(first.session, first.turns)
        .await?;
    for (id, title) in [(first_id, "First task"), (second_id, "Second task")] {
        app.backend_registry.observe(server.backend_id(), id);
        let mut thread = overview_thread(id, None, title, ThreadStatus::Idle);
        thread.model_provider = app.config.model_provider_id.clone();
        app.agents_overview.threads.insert(id, Some(thread));
    }
    app.agents_overview
        .pins_by_backend
        .insert(server.backend_id(), Some(vec![first_id, second_id]));
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    app.chat_widget.apply_external_edit("first draft".into());
    app.handle_tui_event(
        &mut tui,
        &mut server,
        TuiEvent::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::ALT)),
    )
    .await?;
    assert_eq!(app.current_displayed_thread_id(), Some(second_id));
    app.chat_widget.apply_external_edit("second draft".into());
    for (id, draft) in [(first_id, "first draft"), (second_id, "second draft")] {
        app.handle_tui_event(
            &mut tui,
            &mut server,
            TuiEvent::Key(KeyEvent::new(KeyCode::Char('`'), KeyModifiers::ALT)),
        )
        .await?;
        assert_eq!(app.current_displayed_thread_id(), Some(id));
        assert_eq!(app.chat_widget.composer_text_with_pending(), draft);
    }
    app.handle_tui_event(
        &mut tui,
        &mut server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT)),
    )
    .await?;
    assert!(
        std::iter::from_fn(|| events.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::ToggleAgentsOverviewPin { thread_id, pinned: false } if thread_id == second_id
        ))
    );
    tui.set_owned_screen(false)?;
    server.shutdown().await?;
    Ok(())
}
