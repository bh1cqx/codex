use super::*;
use crate::app::owned_transcript::tests::attach_thread;
use crate::app::owned_transcript::tests::user_cell;
use crate::keymap::RuntimeKeymap;
use crate::transcript_mode::TranscriptMode;
use codex_config::types::TuiKeymap;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use pretty_assertions::assert_eq;
use ratatui::layout::Size;

#[tokio::test]
async fn diff_panel_preserves_chat_draft_and_side_chat_while_scrolling_and_closing() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    app.local_settings.tui.animations = false;
    let thread = ThreadId::new();
    let side_chat = ThreadId::new();
    attach_thread(&mut app, thread);
    app.sidebar_mut()
        .show(SidebarContent::Chat(side_chat), PaneFocus::Main);
    app.transcript_cells = vec![user_cell("Conversation stays visible")];
    app.chat_widget
        .apply_external_edit("unfinished reply".into());
    let size = Size::new(140, 24);
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.terminal = crate::custom_terminal::Terminal::with_screen_size_and_cursor_position_for_test(
        ratatui::backend::CrosstermBackend::new(std::io::stdout()),
        size,
        ratatui::layout::Position::default(),
    );
    tui.set_owned_screen(true)?;
    let request = Uuid::new_v4();
    let cwd = app.chat_widget.config_ref().cwd.to_path_buf();
    app.start_diff_view(&mut tui, request, Some(thread), cwd.clone());
    app.complete_diff_view(
        &mut tui,
        request,
        Some(thread),
        cwd,
        (0..80)
            .map(|index| format!("+line {index}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let before = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(app.overlay.is_none());
    assert!(before.contains("Conversation stays visible"), "{before}");
    assert!(before.contains("unfinished reply"), "{before}");
    assert!(before.contains("+line 0"), "{before}");
    let area = app.conversation_frame.panel_area().expect("diff pane");
    assert!(area.width < size.width);
    assert!(app.handle_diff_panel_event(
        &mut tui,
        &TuiEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: area.x + 2,
            row: area.y + 3,
            modifiers: KeyModifiers::NONE,
        })
    )?);
    let after = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(!after.contains("+line 0"), "{after}");
    assert!(after.contains("+line 3"), "{after}");
    assert!(app.handle_diff_panel_event(
        &mut tui,
        &TuiEvent::Key(KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        ))
    )?);
    assert!(
        app.handle_diff_panel_event(&mut tui, &TuiEvent::Paste("do not edit the chat".into()))?
    );
    let scrolled = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(!scrolled.contains("+line 3"), "{scrolled}");
    assert!(scrolled.contains("+line 4"), "{scrolled}");
    assert!(!app.handle_diff_panel_event(
        &mut tui,
        &TuiEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: size.height - 4,
            modifiers: KeyModifiers::NONE,
        })
    )?);
    assert_eq!(app.panel_focus(), Some(PaneFocus::Main));
    assert!(!app.handle_diff_panel_event(&mut tui, &TuiEvent::Paste("chat input".into()))?);
    assert!(app.handle_diff_panel_event(
        &mut tui,
        &TuiEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 2,
            row: area.y + 3,
            modifiers: KeyModifiers::NONE,
        })
    )?);
    assert_eq!(app.panel_focus(), Some(PaneFocus::Sidebar));
    assert!(app.handle_diff_panel_event(&mut tui, &TuiEvent::Key(KeyCode::Char('q').into()))?);
    assert!(
        app.sidebar()
            .and_then(|pane| pane.visible_content())
            .is_none()
    );
    assert_eq!(
        app.sidebar().and_then(|pane| pane.selected_chat()),
        Some(side_chat)
    );
    assert_eq!(app.current_displayed_thread_id(), Some(thread));
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "unfinished reply"
    );
    app.sidebar_mut()
        .show(SidebarContent::Diff, PaneFocus::Sidebar);
    crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(app.handle_diff_panel_event(
        &mut tui,
        &TuiEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.right() - 2,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        })
    )?);
    assert!(
        app.sidebar()
            .and_then(|pane| pane.visible_content())
            .is_none()
    );
    assert_eq!(
        app.sidebar().and_then(|pane| pane.selected_chat()),
        Some(side_chat)
    );
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn diff_panel_yields_to_the_agent_command_center() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    app.local_settings.tui.animations = false;
    let thread = ThreadId::new();
    attach_thread(&mut app, thread);
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let request = Uuid::new_v4();
    let cwd = app.chat_widget.config_ref().cwd.to_path_buf();
    app.start_diff_view(&mut tui, request, Some(thread), cwd.clone());
    crate::app::test_support::side_frame(&mut app, &mut tui, Size::new(80, 24))?;
    assert_eq!(
        app.conversation_frame.panel_area(),
        Some(Rect::new(0, 0, 80, 24))
    );
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
    app.complete_diff_view(
        &mut tui,
        request,
        Some(thread),
        cwd,
        "+hidden change".into(),
    );
    let frame = crate::app::test_support::side_frame(&mut app, &mut tui, Size::new(80, 24))?;
    assert!(app.conversation_frame.panel_area().is_none());
    assert!(frame.contains("Agent command center"), "{frame}");
    assert!(!frame.contains("+hidden change"), "{frame}");
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn stale_diff_results_cannot_replace_refreshes_or_reopen_a_hidden_panel() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    app.local_settings.tui.animations = false;
    let thread = ThreadId::new();
    attach_thread(&mut app, thread);
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let cwd = app.chat_widget.config_ref().cwd.to_path_buf();
    let old = Uuid::new_v4();
    let current = Uuid::new_v4();
    app.start_diff_view(&mut tui, old, Some(thread), cwd.clone());
    app.start_diff_view(&mut tui, current, Some(thread), cwd.clone());
    app.complete_diff_view(
        &mut tui,
        old,
        Some(thread),
        cwd.clone(),
        "STALE DIFF".into(),
    );
    let loading = crate::app::test_support::side_frame(&mut app, &mut tui, Size::new(120, 24))?;
    assert!(loading.contains("Loading changes"), "{loading}");
    assert!(!loading.contains("STALE DIFF"));
    app.sidebar_mut().hide();
    app.complete_diff_view(
        &mut tui,
        current,
        Some(thread),
        cwd.clone(),
        "fresh diff".into(),
    );
    assert!(
        app.sidebar()
            .and_then(|pane| pane.visible_content())
            .is_none()
    );
    assert!(app.overlay.is_none());
    app.sidebar_mut()
        .show(SidebarContent::Diff, PaneFocus::Sidebar);
    let fresh = crate::app::test_support::side_frame(&mut app, &mut tui, Size::new(120, 24))?;
    assert!(fresh.contains("fresh diff"), "{fresh}");
    let other = ThreadId::new();
    attach_thread(&mut app, other);
    app.complete_diff_view(
        &mut tui,
        current,
        Some(thread),
        cwd,
        "background diff".into(),
    );
    assert!(app.sidebar().is_none());
    assert!(app.overlay.is_none());
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn inline_diff_keeps_the_existing_pager() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Terminal;
    let thread = ThreadId::new();
    attach_thread(&mut app, thread);
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let request = Uuid::new_v4();
    let cwd = app.chat_widget.config_ref().cwd.to_path_buf();
    app.start_diff_view(&mut tui, request, Some(thread), cwd.clone());
    app.complete_diff_view(
        &mut tui,
        request,
        Some(thread),
        cwd,
        "+inline change".into(),
    );
    assert!(matches!(app.overlay, Some(Overlay::Static(_))));
    Ok(())
}

#[test]
fn embedded_diff_pager_handles_small_and_offset_rectangles() {
    let keymap = RuntimeKeymap::from_config(&TuiKeymap::default())
        .unwrap()
        .pager;
    for width in [0, 1, 5, 8, 40] {
        for height in [0, 1, 2, 4, 6, 12] {
            let mut diff =
                DiffPanel::new(Uuid::new_v4(), None, PathBuf::new(), true, keymap.clone());
            let area = Rect::new(3, 4, width, height);
            diff.render(area, &mut Buffer::empty(area));
        }
    }
    let mut diff = DiffPanel::new(Uuid::new_v4(), None, PathBuf::new(), true, keymap);
    let area = Rect::new(3, 4, 40, 12);
    let mut buffer = Buffer::empty(area);
    diff.render(area, &mut buffer);
    let text = crate::app::owned_transcript::tests::buffer_text(&buffer);
    assert!(text.contains("Loading changes"), "{text}");
}

#[tokio::test]
async fn ctrl_c_closes_the_diff_without_exiting_when_disconnected() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    let thread = ThreadId::new();
    attach_thread(&mut app, thread);
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    app.start_diff_view(
        &mut tui,
        Uuid::new_v4(),
        Some(thread),
        app.chat_widget.config_ref().cwd.to_path_buf(),
    );
    app.reconnect.offline = true;
    let control = Box::pin(app.handle_tui_event(
        &mut tui,
        &mut server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    ))
    .await?;
    assert!(matches!(control, crate::app::AppRunControl::Continue));
    assert!(
        app.sidebar()
            .and_then(|pane| pane.visible_content())
            .is_none()
    );
    assert_eq!(app.current_displayed_thread_id(), Some(thread));
    tui.set_owned_screen(false)?;
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn close_sidebar_shortcut_only_hides_the_diff() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    let thread = ThreadId::new();
    let side_chat = ThreadId::new();
    attach_thread(&mut app, thread);
    app.sidebar_mut()
        .show(SidebarContent::Chat(side_chat), PaneFocus::Main);
    app.keymap.app.close_sidebar_tab = vec![crate::key_hint::plain(KeyCode::F(12))];
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    app.start_diff_view(
        &mut tui,
        Uuid::new_v4(),
        Some(thread),
        app.chat_widget.config_ref().cwd.to_path_buf(),
    );
    assert!(app.handle_diff_panel_event(&mut tui, &TuiEvent::Key(KeyEvent::from(KeyCode::F(12))))?);
    assert!(
        app.sidebar()
            .and_then(|pane| pane.visible_content())
            .is_none()
    );
    assert_eq!(
        app.sidebar().and_then(|pane| pane.selected_chat()),
        Some(side_chat)
    );
    tui.set_owned_screen(false)?;
    Ok(())
}
