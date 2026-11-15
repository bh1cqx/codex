//! Task-scoped diff snapshots rendered with the existing pager and sidebar layout.

use super::App;
use super::sidebar::PaneFocus;
use super::sidebar::SidebarContent;
use crate::activation_key_guard::ActivationKeySuppression;
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
use std::time::Instant;
use uuid::Uuid;

pub(super) struct DiffPanel {
    request_id: Uuid,
    thread_id: Option<ThreadId>,
    cwd: PathBuf,
    panel: bool,
    view: StaticOverlay,
    keymap: PagerKeymap,
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
        }
    }

    pub(super) fn render(&mut self, area: Rect, buffer: &mut Buffer) {
        self.view.render(area, buffer);
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
        let diff = DiffPanel::new(request_id, thread_id, cwd, panel, self.keymap.pager.clone());
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
        text: String,
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
        let lines = if text.trim().is_empty() {
            vec!["No changes detected.".into()]
        } else {
            text.lines().map(ansi_escape_line).collect()
        };
        let pane = self.sidebars.get_mut(&parent).expect("matched diff parent");
        let diff = pane.diff.as_mut().expect("matched diff");
        let view = StaticOverlay::with_title(lines, "Changes".into(), diff.keymap.clone());
        if diff.panel {
            diff.view = view;
        } else {
            pane.diff = None;
            if active {
                let _ = tui.enter_alt_screen();
                self.overlay = Some(Overlay::Static(view));
            }
        }
        if active {
            self.chat_widget.on_diff_complete();
            tui.frame_requester().schedule_frame();
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
                    .panel_area()
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
