use super::*;
use crate::key_hint::KeyBindingListExt;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;

impl TranscriptView {
    pub(crate) fn is_answers_only(&self) -> bool {
        self.answers_only
    }

    pub(crate) fn set_live_answer(&mut self, active: Option<&dyn HistoryCell>) {
        self.live_answer_start =
            active.is_some_and(|cell| cell.is_answer() && !cell.is_stream_continuation());
    }

    pub(crate) fn handle_conversation_key(
        &mut self,
        key: KeyEvent,
        cells: &[Arc<dyn HistoryCell>],
    ) -> Option<ViewAction> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        let keymap = &self.disclosure.keymap.app;
        if keymap.toggle_answers_only.is_pressed(key) {
            self.toggle_answers_only(cells);
        } else if keymap.previous_answer.is_pressed(key) {
            self.jump_answer(cells, /*newer*/ false);
        } else if keymap.next_answer.is_pressed(key) {
            self.jump_answer(cells, /*newer*/ true);
        } else {
            return None;
        }
        Some(ViewAction::Changed)
    }

    fn clear_conversation_interaction(&mut self) {
        let position = self.position;
        self.cancel_search();
        self.position = position;
        self.search = Search::default();
        self.selection = None;
        self.select_mode = None;
        self.copy_mode = None;
        self.disclosure.focused = None;
        self.disclosure.resume_after_approval = None;
        self.release_live_reading();
        self.cancel_beginning();
    }

    fn toggle_answers_only(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        self.clear_conversation_interaction();
        self.answers_only = !self.answers_only;
        self.cache.clear();
        self.live = None;
        self.live_key = None;
        self.live_separated = None;
        self.visible.clear();
        self.saved_position = None;
        self.suppressed_prompt_header = None;
        if self.answers_only
            && let Position::Reading(anchor) = self.position
        {
            let index = self.resolve(cells, anchor);
            if index == cells.len() {
                self.position = Position::Latest;
            } else if !cells[index].is_conversation_message()
                && let Some(previous) = cells[..index]
                    .iter()
                    .rposition(|cell| cell.is_conversation_message())
            {
                self.jump_to_entry(cells, previous);
            }
        }
    }

    fn is_answer_start(&self, cells: &[Arc<dyn HistoryCell>], index: usize) -> bool {
        cells.get(index).map_or_else(
            || index == cells.len() && self.live.is_some() && self.live_answer_start,
            |cell| cell.is_answer() && !cell.is_stream_continuation(),
        )
    }

    fn jump_answer(&mut self, cells: &[Arc<dyn HistoryCell>], newer: bool) {
        self.clear_conversation_interaction();
        let (index, include_current) = match self.position {
            Position::Latest if !newer => (cells.len() + 1, false),
            Position::Latest => return,
            Position::Reading(anchor) => (self.resolve(cells, anchor), anchor.offset > 0),
        };
        let target = if newer {
            (index.saturating_add(1)..=cells.len())
                .find(|&index| self.is_answer_start(cells, index))
        } else {
            let end = (index + usize::from(include_current)).min(cells.len() + 1);
            (0..end)
                .rev()
                .find(|&index| self.is_answer_start(cells, index))
        };
        if let Some(index) = target {
            self.jump_to_entry(cells, index);
        } else if !newer && self.history.has_unloaded_history() {
            self.pending_previous_answer = true;
        }
    }

    pub(super) fn resume_previous_answer(
        &mut self,
        cells: &[Arc<dyn HistoryCell>],
        inserted: std::ops::Range<usize>,
    ) {
        if self.pending_previous_answer
            && let Some(index) = inserted
                .rev()
                .find(|&index| self.is_answer_start(cells, index))
        {
            self.jump_to_entry(cells, index);
        }
    }
}

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;
