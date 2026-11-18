//! Session-only model selections leave durable defaults untouched. Explicit Astra selections retain
//! their originating task and are checked against its effective model when the app applies them.

use super::*;
use crate::bottom_pane::SelectionSecondaryAction;
use crate::keymap::ListAction;
use crate::model_catalog::LUNA_RESERVE_MODEL;

/// A final Astra selection whose model transition must be checked while the app applies it.
#[derive(Debug)]
pub(crate) enum AstraModelPickerAction {
    ApplyModelSelection {
        effort: Option<ReasoningEffortConfig>,
    },
    SelectSessionModel {
        effort: Option<ReasoningEffortConfig>,
        service_tier: Option<String>,
    },
}

impl AstraModelPickerAction {
    pub(super) fn into_picker_event(self, thread_id: Option<ThreadId>, model: String) -> AppEvent {
        match thread_id {
            Some(thread_id) => AppEvent::AstraSelectedFromModelPicker {
                thread_id,
                model,
                action: self,
            },
            None => self.into_app_event(model),
        }
    }

    pub(crate) fn into_app_event(self, model: String) -> AppEvent {
        match self {
            Self::ApplyModelSelection { effort } => AppEvent::ApplyModelSelection { model, effort },
            Self::SelectSessionModel {
                effort,
                service_tier,
            } => AppEvent::SelectSessionModel {
                model,
                effort,
                service_tier,
            },
        }
    }
}

impl ChatWidget {
    pub(super) fn model_choice(&self, model: &str, effort: ReasoningEffortConfig) -> SelectionItem {
        let thread_id = self.thread_id();
        let sparkle_thread = self.sparkle_thread_for_picker_action(model);
        let action = |save_default: bool| -> SelectionAction {
            let scoped = self.scope_model_picker_event();
            let model = model.to_owned();
            let effort = effort.clone();
            let warning = self.ultra_reasoning_concurrency_warning(&effort);
            Box::new(move |tx| {
                let effort = Some(effort.clone());
                let event = if model == LUNA_RESERVE_MODEL {
                    let Some(thread_id) = thread_id else { return };
                    AppEvent::UpdateLunaReserveReasoning { thread_id, effort }
                } else {
                    let action = if save_default {
                        AstraModelPickerAction::ApplyModelSelection { effort }
                    } else {
                        AstraModelPickerAction::SelectSessionModel {
                            effort,
                            service_tier: None,
                        }
                    };
                    action.into_picker_event(sparkle_thread, model.clone())
                };
                tx.send(scoped(event));
                if let Some(warning) = warning.clone() {
                    tx.send(scoped(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_warning_event(warning),
                    ))));
                }
            })
        };
        let secondary_action =
            (model != LUNA_RESERVE_MODEL && self.model_catalog.can_save_defaults).then(|| {
                let key = key_hint::plain(KeyCode::Char('d'));
                let keymap = self.bottom_pane.list_keymap();
                let mut hints = Vec::new();
                if let Some(accept) = keymap.primary_hint(ListAction::Accept) {
                    hints.extend([accept.into(), " session only · ".dim()]);
                }
                hints.extend([key.into(), " save as default".dim()]);
                if let Some(cancel) = keymap.primary_hint(ListAction::Cancel) {
                    hints.extend([" · ".dim(), cancel.into(), " back".dim()]);
                }
                SelectionSecondaryAction {
                    key,
                    footer_hint: hints.into(),
                    action: action(true),
                }
            });
        SelectionItem {
            actions: vec![action(false)],
            secondary_action,
            dismiss_on_select: true,
            ..Default::default()
        }
    }
}
