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
use std::sync::Arc;

async fn finish_requested_diff(
    app: &mut App,
    tui: &mut Tui,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) {
    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.expect("diff event channel");
            if matches!(event, AppEvent::DiffResult { .. }) {
                break event;
            }
        }
    })
    .await
    .expect("diff response");
    let AppEvent::DiffResult {
        request_id,
        thread_id,
        cwd,
        result,
    } = event
    else {
        unreachable!();
    };
    app.complete_diff_view(tui, request_id, thread_id, cwd, result);
}

struct LocalGitRunner;

impl crate::workspace_command::WorkspaceCommandExecutor for LocalGitRunner {
    fn run(
        &self,
        command: crate::workspace_command::WorkspaceCommand,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = std::result::Result<
                        crate::workspace_command::WorkspaceCommandOutput,
                        crate::workspace_command::WorkspaceCommandError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            assert_eq!(command.argv[0], "git");
            let mut process = tokio::process::Command::new(&command.argv[0]);
            process
                .args(&command.argv[1..])
                .current_dir(command.cwd)
                .kill_on_drop(true);
            for (key, value) in command.env {
                match value {
                    Some(value) => {
                        process.env(key, value);
                    }
                    None => {
                        process.env_remove(key);
                    }
                }
            }
            let output = process.output().await.expect("git fixture");
            Ok(crate::workspace_command::WorkspaceCommandOutput {
                exit_code: output.status.code().expect("git exit code"),
                stdout: String::from_utf8(output.stdout).expect("git stdout"),
                stderr: String::from_utf8(output.stderr).expect("git stderr"),
            })
        })
    }
}

#[tokio::test]
async fn diff_panel_auto_refreshes_external_edits_without_stealing_focus() -> Result<()> {
    let repo = tempfile::tempdir()?;
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()?
            .success()
    );
    let file = repo.path().join("changed.txt");
    std::fs::write(&file, "original content\n")?;
    let (mut app, mut events, _ops) = crate::app::tests::make_test_app_with_channels().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    app.local_settings.tui.animations = false;
    app.workspace_command_runner = Some(Arc::new(LocalGitRunner));
    let thread = ThreadId::new();
    attach_thread(&mut app, thread);
    app.chat_widget
        .handle_thread_session_quiet(crate::app::tests::test_thread_session(
            thread,
            repo.path().to_path_buf(),
        ));
    app.chat_widget.apply_external_edit("unsent draft".into());
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    app.start_diff_view(
        &mut tui,
        Uuid::new_v4(),
        Some(thread),
        repo.path().to_path_buf(),
    );
    finish_requested_diff(&mut app, &mut tui, &mut events).await;
    let size = Size::new(140, 24);
    let first = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(first.contains("+original content"), "{first}");
    app.sidebar_mut()
        .show(SidebarContent::Diff, PaneFocus::Main);

    std::fs::write(&file, "externally updated content\n")?;
    let previous = app.sidebar().unwrap().diff.as_ref().unwrap().request_id;
    let next_refresh = app.sidebar().unwrap().diff.as_ref().unwrap().next_refresh;
    app.refresh_visible_diff(
        &tui.frame_requester(),
        next_refresh - Duration::from_millis(1),
    );
    assert_eq!(
        app.sidebar().unwrap().diff.as_ref().unwrap().request_id,
        previous
    );
    app.sidebar_mut().diff.as_mut().unwrap().next_refresh = Instant::now();
    let during = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(during.contains("+original content"), "{during}");
    assert!(!during.contains("Loading changes"));
    let refreshing = app.sidebar().unwrap().diff.as_ref().unwrap().request_id;
    assert_ne!(refreshing, previous);
    app.refresh_visible_diff(&tui.frame_requester(), next_refresh + DIFF_REFRESH_INTERVAL);
    assert_eq!(
        app.sidebar().unwrap().diff.as_ref().unwrap().request_id,
        refreshing
    );
    finish_requested_diff(&mut app, &mut tui, &mut events).await;
    let changed = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(changed.contains("+externally updated content"), "{changed}");
    assert!(!changed.contains("+original content"), "{changed}");
    assert_eq!(app.panel_focus(), Some(PaneFocus::Main));
    assert_eq!(app.chat_widget.composer_text_with_pending(), "unsent draft");

    std::fs::remove_file(file)?;
    app.sidebar_mut().diff.as_mut().unwrap().next_refresh = Instant::now();
    crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    finish_requested_diff(&mut app, &mut tui, &mut events).await;
    let clean = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(clean.contains("No changes detected."), "{clean}");
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn diff_panel_refresh_errors_keep_the_snapshot_and_manual_refresh_keeps_scroll() -> Result<()>
{
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    app.local_settings.tui.animations = false;
    let thread = ThreadId::new();
    attach_thread(&mut app, thread);
    let cwd = app.chat_widget.config_ref().cwd.to_path_buf();
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let request = Uuid::new_v4();
    let content = (0..80)
        .map(|i| format!("+line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.start_diff_view(&mut tui, request, Some(thread), cwd.clone());
    app.complete_diff_view(
        &mut tui,
        request,
        Some(thread),
        cwd.clone(),
        Err("not connected".into()),
    );
    let initial_error =
        crate::app::test_support::side_frame(&mut app, &mut tui, Size::new(140, 24))?;
    assert!(
        initial_error.contains("Failed to compute diff: not connected"),
        "{initial_error}"
    );
    app.complete_diff_view(
        &mut tui,
        request,
        Some(thread),
        cwd.clone(),
        Ok(content.clone()),
    );
    app.sidebar_mut()
        .diff
        .as_mut()
        .unwrap()
        .view
        .scroll_lines(20);
    let size = Size::new(140, 24);
    let before = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(
        before.contains("+line 20") && !before.contains("+line 0"),
        "{before}"
    );
    app.complete_diff_view(
        &mut tui,
        request,
        Some(thread),
        cwd.clone(),
        Err("connection lost".into()),
    );
    let failed = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(
        failed.contains("+line 20") && !failed.contains("+line 0"),
        "{failed}"
    );
    assert!(
        failed.contains("Refresh failed: connection lost"),
        "{failed}"
    );

    let next = app.sidebar().unwrap().diff.as_ref().unwrap().next_refresh;
    app.refresh_visible_diff(&tui.frame_requester(), next);
    let retry = app.sidebar().unwrap().diff.as_ref().unwrap().request_id;
    assert_ne!(retry, request);
    let loading = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(
        loading.contains("+line 20") && !loading.contains("Loading changes"),
        "{loading}"
    );
    app.complete_diff_view(
        &mut tui,
        request,
        Some(thread),
        cwd.clone(),
        Ok("stale".into()),
    );
    app.complete_diff_view(
        &mut tui,
        retry,
        Some(thread),
        cwd.clone(),
        Ok(content.clone()),
    );
    let recovered = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(
        recovered.contains("+line 20") && !recovered.contains("+line 0"),
        "{recovered}"
    );
    assert!(
        !recovered.contains("Refresh failed") && !recovered.contains("stale"),
        "{recovered}"
    );
    let manual = Uuid::new_v4();
    app.keymap.pager.close = vec![crate::key_hint::plain(KeyCode::F(6))];
    app.start_diff_view(&mut tui, manual, Some(thread), cwd.clone());
    app.complete_diff_view(&mut tui, manual, Some(thread), cwd, Ok(content));
    let refreshed = crate::app::test_support::side_frame(&mut app, &mut tui, size)?;
    assert!(
        refreshed.contains("+line 20") && !refreshed.contains("+line 0"),
        "{refreshed}"
    );
    assert!(app.handle_diff_panel_event(&mut tui, &TuiEvent::Key(KeyCode::F(6).into()))?);
    assert!(app.sidebar().unwrap().visible_content().is_none());
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn diff_panel_hidden_and_covered_views_do_not_poll() -> Result<()> {
    let mut app = crate::app::test_support::make_test_app().await;
    app.local_settings.transcript_mode = TranscriptMode::Owned;
    app.local_settings.tui.animations = false;
    let thread = ThreadId::new();
    attach_thread(&mut app, thread);
    let cwd = app.chat_widget.config_ref().cwd.to_path_buf();
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let request = Uuid::new_v4();
    app.start_diff_view(&mut tui, request, Some(thread), cwd.clone());
    let abort = app
        .sidebar()
        .unwrap()
        .diff
        .as_ref()
        .unwrap()
        .pending
        .as_ref()
        .unwrap()
        .abort_handle();
    app.sidebar_mut().hide();
    tokio::task::yield_now().await;
    assert!(abort.is_finished());
    crate::app::test_support::side_frame(&mut app, &mut tui, Size::new(140, 24))?;
    app.refresh_visible_diff(
        &tui.frame_requester(),
        Instant::now() + DIFF_REFRESH_INTERVAL,
    );
    assert!(
        app.sidebar()
            .unwrap()
            .diff
            .as_ref()
            .unwrap()
            .pending
            .is_none()
    );

    app.sidebar_mut()
        .show(SidebarContent::Diff, PaneFocus::Sidebar);
    app.complete_diff_view(&mut tui, request, Some(thread), cwd, Ok("+ready".into()));
    app.sidebar_mut().diff.as_mut().unwrap().next_refresh = Instant::now();
    app.chat_widget
        .show_selection_view(crate::bottom_pane::SelectionViewParams {
            title: Some("Agent command center".into()),
            items: vec![crate::bottom_pane::SelectionItem {
                name: "Task".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
    crate::app::test_support::side_frame(&mut app, &mut tui, Size::new(140, 24))?;
    assert!(app.conversation_frame.panel_area().is_none());
    assert!(
        app.sidebar()
            .unwrap()
            .diff
            .as_ref()
            .unwrap()
            .pending
            .is_none()
    );
    tui.set_owned_screen(false)?;
    Ok(())
}

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
        Ok((0..80)
            .map(|index| format!("+line {index}"))
            .collect::<Vec<_>>()
            .join("\n")),
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
        Ok("+hidden change".into()),
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
        Ok("STALE DIFF".into()),
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
        Ok("fresh diff".into()),
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
        Ok("background diff".into()),
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
        Ok("+inline change".into()),
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
    app.chat_widget
        .apply_external_edit("keep this draft".into());
    app.keymap.app.focus_pane_left = vec![crate::key_hint::plain(KeyCode::F(6))];
    app.keymap.app.focus_pane_right = vec![crate::key_hint::plain(KeyCode::F(7))];
    for (key, focus) in [
        (KeyCode::F(6), PaneFocus::Main),
        (KeyCode::F(7), PaneFocus::Sidebar),
    ] {
        assert!(
            app.handle_side_focus_key(&mut tui, &mut server, key.into())
                .await
        );
        assert_eq!(app.panel_focus(), Some(focus));
        assert!(matches!(
            app.sidebar().and_then(|pane| pane.visible_content()),
            Some(SidebarContent::Diff)
        ));
        assert_eq!(app.current_displayed_thread_id(), Some(thread));
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "keep this draft"
        );
    }
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
