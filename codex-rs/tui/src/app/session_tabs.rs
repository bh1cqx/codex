//! Pinned-session navigation using the command center's shared metadata and attachment path.

use super::App;
use super::agents_overview_details::preview_text;
use super::agents_overview_discovery::is_overview_root;
use super::agents_overview_discovery::supports_shared_pinning;
use super::agents_overview_view::AgentsOverviewGroup;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::key_hint::KeyBindingListExt;
use crate::keymap::KeymapActionId;
use crate::keymap::KeymapContext;
use crate::line_truncation::truncate_line_with_ellipsis;
use crate::sidebar_tab_strip::TabStrip;
use crate::sidebar_tab_strip::TabTarget;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use codex_protocol::ThreadId;
use crossterm::event::KeyEventKind;
use crossterm::event::MouseButton;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;
use std::collections::HashSet;

struct SessionTab {
    id: ThreadId,
    title: String,
    group: AgentsOverviewGroup,
    unread: bool,
}

#[derive(Default)]
pub(super) struct SessionTabs {
    current: Option<ThreadId>,
    previous: Option<ThreadId>,
    items: Vec<SessionTab>,
    area: Rect,
    can_pin: bool,
    pinned: bool,
    pin_pending: bool,
}

enum TabAction {
    Select(ThreadId),
    TogglePin,
}

impl SessionTabs {
    fn observe(&mut self, current: Option<ThreadId>) {
        if let Some(current) = current
            && self.current != Some(current)
        {
            self.previous = self.current.or(self.previous);
            self.current = Some(current);
        }
    }

    pub(super) fn forget(&mut self, id: ThreadId) {
        self.items.retain(|tab| tab.id != id);
        if self.previous == Some(id) {
            self.previous = None;
        }
        if self.current == Some(id) {
            self.current = None;
        }
    }

    fn adjacent(&self, forward: bool) -> Option<ThreadId> {
        let count = self.items.len();
        if count < 2 {
            return None;
        }
        let current = self
            .items
            .iter()
            .position(|tab| Some(tab.id) == self.current)?;
        let next = if forward {
            (current + 1) % count
        } else {
            current.checked_sub(1).unwrap_or(count - 1)
        };
        Some(self.items[next].id)
    }

    fn targets(&self) -> Vec<(TabTarget<ThreadId>, Rect)> {
        let ids = self.items.iter().map(|tab| tab.id).collect::<Vec<_>>();
        TabStrip {
            ids: &ids,
            selected: self.current,
        }
        .layout_targets(
            self.area, /*preferred_width*/ 26, /*show_close*/ false,
        )
    }

    fn hit(&self, position: Position) -> Option<TabAction> {
        self.targets()
            .into_iter()
            .find(|(_, area)| area.contains(position))
            .and_then(|(target, _)| match target {
                TabTarget::Tab(id) | TabTarget::Overflow { tab: id, .. } => {
                    Some(TabAction::Select(id))
                }
                TabTarget::Add if self.can_pin && !self.pin_pending => Some(TabAction::TogglePin),
                TabTarget::Add | TabTarget::Close(_) => None,
            })
    }

    pub(super) fn height(&self, width: u16) -> u16 {
        u16::from(width > 3 && !self.items.is_empty())
    }

    pub(super) fn render(&mut self, area: Rect, buffer: &mut Buffer) {
        self.area = area;
        if area.is_empty() || self.items.is_empty() {
            return;
        }
        Clear.render(area, buffer);
        let base = crate::style::user_message_style();
        buffer.set_style(area, base);
        for (target, cell) in self.targets() {
            let style = match target {
                TabTarget::Tab(id) if Some(id) == self.current => {
                    crate::style::selection_style().not_underlined()
                }
                _ => base.fg(crate::style::readable_color_on(Color::Reset, base.bg)),
            };
            let text = match target {
                TabTarget::Tab(id) => {
                    let Some(tab) = self.items.iter().find(|tab| tab.id == id) else {
                        continue;
                    };
                    let status = match tab.group {
                        AgentsOverviewGroup::NeedsYou => "!",
                        AgentsOverviewGroup::Working => "▶",
                        AgentsOverviewGroup::Ready => "○",
                        AgentsOverviewGroup::Finished => "·",
                    };
                    let unread = if tab.unread { "*" } else { "" };
                    format!(" {status}{unread} {}", tab.title)
                }
                TabTarget::Close(_) => continue,
                TabTarget::Overflow { glyph, .. } => glyph.to_string(),
                TabTarget::Add if self.pin_pending => " … ".to_string(),
                TabTarget::Add if self.can_pin => {
                    if self.pinned { " ★ " } else { " ☆ " }.to_string()
                }
                TabTarget::Add => "   ".to_string(),
            };
            buffer.set_style(cell, style);
            truncate_line_with_ellipsis(Line::from(text).style(style), usize::from(cell.width))
                .render(cell, buffer);
        }
    }
}

impl App {
    pub(super) fn session_tabs_available(&self) -> bool {
        self.local_settings.transcript_mode.is_owned()
            && self.overlay.is_none()
            && !self.chat_widget.has_active_view()
            && !self.chat_widget.fork_in_progress
            && self
                .agents_overview
                .pins_by_backend
                .values()
                .any(Option::is_some)
    }

    pub(super) fn has_session_tabs(&self) -> bool {
        self.session_tabs_available()
            && self
                .agents_overview
                .pins_by_backend
                .values()
                .flatten()
                .any(|pins| !pins.is_empty())
    }

    pub(super) fn prepare_session_tabs(&mut self) {
        let current = self.current_displayed_thread_id();
        let mut items = Vec::new();
        let pins = self
            .agents_overview
            .pins_by_backend
            .values()
            .flatten()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();
        if self.has_session_tabs() {
            for id in pins.iter().copied().chain(current) {
                if !seen.insert(id)
                    || self.agents_overview.removed_threads.contains(&id)
                    || (Some(id) != current && self.agents_overview.hidden_threads.contains(&id))
                {
                    continue;
                }
                let thread = self
                    .agents_overview
                    .threads
                    .get(&id)
                    .and_then(Option::as_ref)
                    .or_else(|| self.agents_overview.search_matches.get(&id));
                let title = thread
                    .map(|thread| {
                        thread
                            .name
                            .as_deref()
                            .unwrap_or(&thread.preview)
                            .to_string()
                    })
                    .or_else(|| {
                        if Some(id) == current {
                            self.chat_widget.thread_name()
                        } else {
                            None
                        }
                    })
                    .map(|title| preview_text(&title))
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| "Untitled task".to_string());
                items.push(SessionTab {
                    id,
                    title,
                    group: thread.map_or(
                        AgentsOverviewGroup::Finished,
                        AgentsOverviewGroup::for_thread,
                    ),
                    unread: Some(id) != current && self.agents_overview.unread.contains(&id),
                });
            }
        }
        let pin_supported = current
            .and_then(|id| self.backend_registry.owner(id))
            .and_then(|owner| self.agents_overview.pins_by_backend.get(&owner))
            .is_some_and(Option::is_some);
        let can_pin = pin_supported
            && !self.reconnect.offline
            && current
                .and_then(|id| self.agents_overview.threads.get(&id))
                .and_then(Option::as_ref)
                .is_some_and(|thread| {
                    is_overview_root(thread) && supports_shared_pinning(&thread.source)
                });
        let tabs = &mut self.agents_overview.tabs;
        tabs.observe(current.filter(|id| !self.agents_overview.removed_threads.contains(id)));
        tabs.items = items;
        tabs.can_pin = can_pin;
        tabs.pinned = current.is_some_and(|id| pins.contains(&id));
        tabs.pin_pending = self.agents_overview.pending_pin_change.is_some();
    }

    pub(super) async fn handle_session_tabs_event(
        &mut self,
        tui: &mut Tui,
        app_server: &mut AppServerSession,
        event: &TuiEvent,
    ) -> bool {
        if !self.session_tabs_available() || !self.chat_widget.no_modal_or_popup_active() {
            return false;
        }
        self.prepare_session_tabs();
        let action = match event {
            TuiEvent::Key(key) if key.kind == KeyEventKind::Press => {
                let contexts = self.active_keymap_contexts();
                let enabled = |action| {
                    contexts.contains_action(KeymapActionId {
                        context: KeymapContext::Chat,
                        action,
                    })
                };
                let keys = &self.keymap.chat;
                let tabs = &self.agents_overview.tabs;
                if enabled("session_tab_previous") && keys.session_tab_previous.is_pressed(*key) {
                    tabs.adjacent(/*forward*/ false).map(TabAction::Select)
                } else if enabled("session_tab_next") && keys.session_tab_next.is_pressed(*key) {
                    tabs.adjacent(/*forward*/ true).map(TabAction::Select)
                } else if enabled("session_tab_last") && keys.session_tab_last.is_pressed(*key) {
                    tabs.previous.map(TabAction::Select)
                } else if enabled("toggle_session_pin")
                    && keys.toggle_session_pin.is_pressed(*key)
                    && tabs.can_pin
                {
                    Some(TabAction::TogglePin)
                } else {
                    return false;
                }
            }
            TuiEvent::Mouse(mouse)
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && self
                        .agents_overview
                        .tabs
                        .area
                        .contains((mouse.column, mouse.row).into()) =>
            {
                if let Err(error) = self.refresh_owned_transcript(tui) {
                    self.chat_widget.add_error_message(error.to_string());
                    return true;
                }
                self.agents_overview
                    .tabs
                    .hit((mouse.column, mouse.row).into())
            }
            _ => return false,
        };
        self.routed_key_activation = None;
        match action {
            Some(TabAction::Select(id)) => {
                if let Err(error) =
                    Box::pin(self.select_agents_overview_thread(tui, app_server, id)).await
                {
                    self.chat_widget
                        .add_error_message(format!("Could not open session: {error}"));
                }
            }
            Some(TabAction::TogglePin) => {
                if let Some(thread_id) = self.current_displayed_thread_id() {
                    self.app_event_tx.send(AppEvent::ToggleAgentsOverviewPin {
                        thread_id,
                        pinned: !self.agents_overview.tabs.pinned,
                    });
                }
            }
            None => {}
        }
        true
    }
}

#[cfg(test)]
#[path = "session_tabs_tests.rs"]
mod tests;
