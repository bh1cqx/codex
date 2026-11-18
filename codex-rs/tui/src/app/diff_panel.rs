//! Task-scoped diffs that refresh while their sidebar is visible.

use super::App;
use super::sidebar::PaneFocus;
use super::sidebar::SidebarContent;
use crate::activation_key_guard::ActivationKeySuppression;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::get_git_diff::get_git_diff;
use crate::key_hint::KeyBinding;
use crate::key_hint::KeyBindingListExt;
use crate::keymap::PagerKeymap;
use crate::keymap::bindings_for_action;
use crate::keymap::keymap_action_ids;
use crate::pager_overlay::Overlay;
use crate::pager_overlay::StaticOverlay;
use crate::session_resume::cwds_differ;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use crate::workspace_command::WorkspaceCommandRunner;
use codex_ansi_escape::ansi_escape_line;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;
use crossterm::event::KeyEventKind;
use crossterm::event::MouseButton;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Widget;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

const DIFF_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub(super) struct DiffPanel {
    request_id: Uuid,
    thread_id: Option<ThreadId>,
    cwd: PathBuf,
    panel: bool,
    view: StaticOverlay,
    keymap: PagerKeymap,
    pending: Option<AbortOnDropHandle<()>>,
    next_refresh: Instant,
    snapshot: Option<String>,
    error: Option<String>,
}

fn close_area(area: Rect) -> Rect {
    let width = area.width.min(3);
    Rect::new(area.right() - width, area.y, width, area.height.min(1))
}

impl DiffPanel {
    fn new(
        request_id: Uuid,
        thread_id: Option<ThreadId>,
        cwd: PathBuf,
        panel: bool,
        keymap: PagerKeymap,
    ) -> Self {
        Self {
            request_id,
            thread_id,
            cwd,
            panel,
            keymap: keymap.clone(),
            view: StaticOverlay::with_title(
                vec!["Loading changes…".into()],
                "Changes".into(),
                keymap,
            ),
            pending: None,
            next_refresh: Instant::now(),
            snapshot: None,
            error: None,
        }
    }

    fn refresh(
        &mut self,
        request_id: Uuid,
        runner: Option<WorkspaceCommandRunner>,
        tx: AppEventSender,
    ) {
        self.pending = None;
        self.request_id = request_id;
        let thread_id = self.thread_id;
        let cwd = self.cwd.clone();
        self.pending = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let result = match runner {
                Some(runner) => {
                    get_git_diff(runner.as_ref(), &cwd)
                        .await
                        .map(|(is_git_repo, text)| {
                            if is_git_repo {
                                text
                            } else {
                                "`/diff` — _not inside a git repository_".to_owned()
                            }
                        })
                }
                None => Err("workspace command runner unavailable".to_owned()),
            };
            tx.send(AppEvent::DiffResult {
                request_id,
                thread_id,
                cwd,
                result,
            });
        })));
    }

    pub(super) fn pause_refresh(&mut self) {
        self.pending = None;
        self.next_refresh = Instant::now();
    }

    pub(super) fn render(&mut self, area: Rect, buffer: &mut Buffer) {
        self.view.render(area, buffer);
        if self.snapshot.is_some()
            && area.height >= 6
            && let Some(error) = &self.error
        {
            let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            Line::from(format!(
                "Refresh failed: {}",
                error.replace(char::is_control, " ")
            ))
            .style(crate::style::status_style(
                crate::style::StatusTone::Attention,
            ))
            .render(footer, buffer);
        }
        Line::from(" × ")
            .style(crate::style::secondary_text_style())
            .render(close_area(area), buffer);
    }
}

impl App {
    pub(super) fn diff_panel_focused(&self) -> bool {
        self.local_settings.transcript_mode.is_owned()
            && self.panel_focus() == Some(PaneFocus::Sidebar)
            && self.sidebar().is_some_and(|pane| pane.is_diff())
    }

    pub(super) fn prepare_diff_panel(&mut self) {
        let parent = self.sidebar_parent();
        let thread = self.current_displayed_thread_id();
        let cwd = self.chat_widget.config_ref().cwd.as_path();
        if let Some(pane) = self.sidebars.get_mut(&parent)
            && let Some(diff) = &pane.diff
            && (diff.thread_id != thread || cwds_differ(&diff.cwd, cwd))
        {
            pane.hide();
            pane.diff = None;
        }
    }

    pub(super) fn start_diff_view(
        &mut self,
        tui: &mut Tui,
        request_id: Uuid,
        thread_id: Option<ThreadId>,
        cwd: PathBuf,
    ) {
        if thread_id != self.current_displayed_thread_id()
            || cwds_differ(&cwd, self.chat_widget.config_ref().cwd.as_path())
            || self.overlay.is_some()
            || self.chat_widget.has_active_view()
            || self.sidebar().is_some_and(|pane| pane.close.is_some())
        {
            return;
        }
        let panel = tui.is_owned_screen()
            && self.local_settings.workspace_enabled(&self.config)
            && !self.conversation_frame.has_caller_inset();
        let mut diff = self
            .sidebar_mut()
            .diff
            .take()
            .filter(|diff| {
                panel && diff.panel && diff.thread_id == thread_id && !cwds_differ(&diff.cwd, &cwd)
            })
            .unwrap_or_else(|| {
                DiffPanel::new(request_id, thread_id, cwd, panel, self.keymap.pager.clone())
            });
        diff.keymap = self.keymap.pager.clone();
        diff.view.set_keymap(diff.keymap.clone());
        diff.refresh(
            request_id,
            self.workspace_command_runner.clone(),
            self.app_event_tx.clone(),
        );
        let pane = self.sidebar_mut();
        pane.diff = Some(diff);
        if panel {
            pane.show(SidebarContent::Diff, PaneFocus::Sidebar);
            self.zoomed_thread = None;
        }
        tui.frame_requester().schedule_frame();
    }

    pub(super) fn complete_diff_view(
        &mut self,
        tui: &mut Tui,
        request_id: Uuid,
        thread_id: Option<ThreadId>,
        cwd: PathBuf,
        result: Result<String, String>,
    ) {
        self.prepare_diff_panel();
        let Some(parent) = self.sidebars.iter().find_map(|(parent, pane)| {
            pane.diff
                .as_ref()
                .filter(|diff| {
                    diff.request_id == request_id
                        && diff.thread_id == thread_id
                        && !cwds_differ(&diff.cwd, &cwd)
                })
                .map(|_| *parent)
        }) else {
            return;
        };
        let active =
            self.sidebar_parent() == parent && self.current_displayed_thread_id() == thread_id;
        let pane = self.sidebars.get_mut(&parent).expect("matched diff parent");
        let diff = pane.diff.as_mut().expect("matched diff");
        diff.pending = None;
        diff.next_refresh = Instant::now() + DIFF_REFRESH_INTERVAL;
        match result {
            Ok(text) => {
                if diff.snapshot.as_ref() != Some(&text) {
                    let lines = if text.trim().is_empty() {
                        vec!["No changes detected.".into()]
                    } else {
                        text.lines().map(ansi_escape_line).collect()
                    };
                    diff.view.replace_lines(lines);
                    diff.snapshot = Some(text);
                }
                diff.error = None;
            }
            Err(error) => {
                if diff.snapshot.is_none() {
                    diff.view.replace_lines(
                        format!("Failed to compute diff: {error}")
                            .lines()
                            .map(ansi_escape_line)
                            .collect(),
                    );
                }
                diff.error = Some(error);
            }
        }
        if !diff.panel {
            let diff = pane.diff.take().expect("matched inline diff");
            if active {
                let _ = tui.enter_alt_screen();
                self.overlay = Some(Overlay::Static(diff.view));
            }
        }
        if active {
            self.chat_widget.on_diff_complete();
            tui.frame_requester().schedule_frame();
        }
    }

    pub(super) fn refresh_visible_diff(
        &mut self,
        frames: &crate::tui::FrameRequester,
        now: Instant,
    ) {
        if self.overlay.is_some()
            || self.chat_widget.has_active_view()
            || self
                .conversation_frame
                .chooser_area()
                .is_none_or(|area| area.is_empty())
            || self.sidebar().and_then(|pane| pane.visible_content()) != Some(SidebarContent::Diff)
        {
            return;
        }
        let parent = self.sidebar_parent();
        let Some(diff) = self
            .sidebars
            .get_mut(&parent)
            .and_then(|pane| pane.diff.as_mut())
        else {
            return;
        };
        if !diff.panel || diff.pending.is_some() {
            return;
        }
        if now < diff.next_refresh {
            frames.schedule_frame_in(diff.next_refresh - now);
        } else {
            diff.refresh(
                Uuid::new_v4(),
                self.workspace_command_runner.clone(),
                self.app_event_tx.clone(),
            );
        }
    }

    pub(super) fn handle_diff_panel_event(
        &mut self,
        tui: &mut Tui,
        event: &TuiEvent,
    ) -> Result<bool> {
        self.prepare_diff_panel();
        if !tui.is_owned_screen()
            || self.overlay.is_some()
            || !self.chat_widget.no_modal_or_popup_active()
            || self.sidebar().and_then(|pane| pane.visible_content()) != Some(SidebarContent::Diff)
        {
            return Ok(false);
        }
        match event {
            TuiEvent::Mouse(mouse) => {
                if !matches!(
                    mouse.kind,
                    MouseEventKind::Down(MouseButton::Left)
                        | MouseEventKind::ScrollUp
                        | MouseEventKind::ScrollDown
                ) {
                    return Ok(false);
                }
                self.refresh_owned_transcript(tui)?;
                let point = (mouse.column, mouse.row).into();
                let Some(area) = self
                    .conversation_frame
                    .chooser_area()
                    .filter(|area| area.contains(point))
                else {
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                        self.sidebar_mut()
                            .show(SidebarContent::Diff, PaneFocus::Main);
                    }
                    return Ok(false);
                };
                let pane = self.sidebar_mut();
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if close_area(area).contains(point) => {
                        pane.hide()
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        pane.show(SidebarContent::Diff, PaneFocus::Sidebar)
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        if let Some(diff) = &mut pane.diff {
                            diff.view
                                .scroll_lines(if mouse.kind == MouseEventKind::ScrollUp {
                                    -3
                                } else {
                                    3
                                });
                        }
                    }
                    _ => {}
                }
            }
            TuiEvent::Key(key)
                if self.diff_panel_focused()
                    && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
            {
                let close = self.keymap.app.close_sidebar_tab.is_pressed(*key)
                    || self.keymap.app.hide_pane.is_pressed(*key)
                    || self
                        .sidebar()
                        .and_then(|pane| pane.diff.as_ref())
                        .is_some_and(|diff| diff.keymap.close.is_pressed(*key));
                if close {
                    self.sidebar_mut().hide();
                    let activation = self
                        .routed_key_activation
                        .take()
                        .unwrap_or_else(|| KeyBinding::from_event(*key));
                    self.chat_widget.suppress_current_view_key_until_release(
                        activation,
                        Instant::now(),
                        ActivationKeySuppression::ViewTransition,
                    );
                } else {
                    let pane_control = keymap_action_ids()
                        .filter(|action| action.is_side_pane())
                        .any(|action| {
                            bindings_for_action(
                                &self.keymap,
                                action.context.config_name(),
                                action.action,
                            )
                            .is_some_and(|bindings| bindings.is_pressed(*key))
                        });
                    if pane_control {
                        return Ok(false);
                    }
                    if let Some(diff) = &mut self.sidebar_mut().diff {
                        diff.view.handle_event(tui, event.clone())?;
                    }
                }
                self.routed_key_activation = None;
            }
            TuiEvent::Paste(_) if self.diff_panel_focused() => {}
            _ => return Ok(false),
        }
        tui.frame_requester().schedule_frame();
        Ok(true)
    }
}

#[cfg(test)]
#[path = "diff_panel_tests.rs"]
mod tests;
