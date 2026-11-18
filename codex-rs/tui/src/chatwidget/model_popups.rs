//! Model, collaboration, and reasoning popups for `ChatWidget`.
//!
//! These surfaces are tightly related because changing one often redirects
//! into another, especially while Plan mode is active.

use super::*;

const ULTRA_REASONING_CONCURRENCY_WARNING_THRESHOLD: usize = 8;
pub(super) const MODEL_OPTIONS_VIEW_ID: &str = "model-options";
pub(super) const MODEL_LOADING_VIEW_ID: &str = "model-loading";
pub(super) const MODEL_SELECTION_VIEW_ID: &str = "model-selection";
pub(super) const ALL_MODELS_SELECTION_VIEW_ID: &str = "all-models-selection";

impl ChatWidget {
    /// Open a popup to choose a quick auto model. Selecting "All models"
    /// opens the full picker with every available preset.
    pub(crate) fn open_model_popup(&mut self) {
        if !self.is_session_configured() {
            self.add_info_message(
                "Model selection is disabled until startup completes.".to_string(),
                /*hint*/ None,
            );
            return;
        }

        if let Some(message) = self.model_picker_unavailable() {
            self.add_info_message(message, /*hint*/ None);
            return;
        }

        let presets: Vec<ModelPreset> = match self.model_catalog.try_list_models() {
            Ok(models) => models,
            Err(_) => {
                self.add_info_message(
                    "Models are being updated; please try /model again in a moment.".to_string(),
                    /*hint*/ None,
                );
                return;
            }
        };
        let request_id = uuid::Uuid::new_v4();
        self.model_popup_request_id = Some(request_id);
        if presets.is_empty() {
            self.bottom_pane.show_selection_view(SelectionViewParams {
                view_id: Some(MODEL_LOADING_VIEW_ID),
                title: Some("Loading models…".into()),
                ..SelectionViewParams::picker()
            });
        } else {
            self.open_model_popup_with_presets(presets);
        }
        // Show cached choices immediately and update any still-present picker when the reply arrives.
        self.app_event_tx.send(AppEvent::FetchModels { request_id });
    }

    pub(super) fn model_menu_header(&self, title: &str, subtitle: &str) -> Box<dyn Renderable> {
        let title = title.to_string();
        let subtitle = subtitle.to_string();
        let mut header = vec![Line::from(title.bold())];
        if !subtitle.is_empty() {
            header.push(Line::from(subtitle.dim()));
        }
        if let Some(selection) = &self.cloud_picker_selection {
            let effort = selection
                .effort
                .as_ref()
                .map_or_else(|| "server effort".to_string(), Self::reasoning_effort_label);
            header.push(
                format!("Accepted for future turns: {} · {effort}", selection.model)
                    .dim()
                    .into(),
            );
        }
        if let Some(warning) = self.model_menu_warning_line() {
            header.push(warning);
        }
        Box::new(Paragraph::new(header).wrap(Wrap { trim: false }))
    }

    fn model_menu_warning_line(&self) -> Option<Line<'static>> {
        let base_url = self.custom_openai_base_url()?;
        let warning = format!(
            "Warning: OpenAI base URL is overridden to {base_url}. Selecting models may not be supported or work properly."
        );
        Some(Line::from(warning.red()))
    }

    fn custom_openai_base_url(&self) -> Option<String> {
        if !self.config.model_provider.is_openai() {
            return None;
        }

        let base_url = self.config.model_provider.base_url.as_ref()?;
        let trimmed = base_url.trim();
        if trimmed.is_empty() {
            return None;
        }

        let normalized = trimmed.trim_end_matches('/');
        if normalized == DEFAULT_OPENAI_BASE_URL {
            return None;
        }

        Some(trimmed.to_string())
    }

    pub(crate) fn open_model_popup_with_presets(&mut self, presets: Vec<ModelPreset>) {
        if self.restrict_model_picker_to_luna_reserve() {
            self.open_luna_reserve_model_popup(presets, MODEL_SELECTION_VIEW_ID);
            return;
        }
        let presets: Vec<ModelPreset> = presets
            .into_iter()
            .filter(|preset| preset.show_in_picker)
            .collect();

        let current_model = self.current_model();
        let current_label = presets
            .iter()
            .find(|preset| preset.model.as_str() == current_model)
            .map(|preset| preset.display_name.clone())
            .unwrap_or_else(|| self.model_display_name().to_string());

        let (mut auto_presets, other_presets): (Vec<ModelPreset>, Vec<ModelPreset>) = presets
            .into_iter()
            .partition(|preset| Self::is_auto_model(&preset.model));

        if auto_presets.is_empty() {
            self.open_all_models_popup_with_view_id(other_presets, MODEL_SELECTION_VIEW_ID);
            return;
        }

        auto_presets.sort_by_key(|preset| Self::auto_model_order(&preset.model));
        let mut model_ids: Vec<String> = auto_presets
            .iter()
            .map(|preset| preset.model.clone())
            .collect();
        let mut items: Vec<SelectionItem> = auto_presets
            .into_iter()
            .map(|preset| {
                let description =
                    (!preset.description.is_empty()).then_some(preset.description.clone());
                let model = preset.model.clone();
                let requires_advanced_selection =
                    Self::is_advanced_reasoning_effort(&preset.default_reasoning_effort)
                        || preset
                            .supported_reasoning_efforts
                            .iter()
                            .any(|option| Self::is_advanced_reasoning_effort(&option.effort));
                let choice = if requires_advanced_selection {
                    let scoped = self.scope_model_picker_event();
                    let preset = preset.clone();
                    SelectionItem {
                        actions: vec![Box::new(move |tx| {
                            tx.send(scoped(AppEvent::OpenReasoningPopup {
                                model: preset.clone(),
                            }));
                        })],
                        dismiss_parent_on_child_accept: true,
                        ..Default::default()
                    }
                } else {
                    self.model_choice(&model, preset.default_reasoning_effort.clone())
                };
                SelectionItem {
                    name: preset.display_name,
                    description,
                    is_current: model == current_model,
                    ..choice
                }
            })
            .collect();

        if !other_presets.is_empty() {
            model_ids.push("All models".to_string());
            let scoped = self.scope_model_picker_event();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(scoped(AppEvent::OpenAllModelsPopup));
            })];

            let is_current = !items.iter().any(|item| item.is_current);
            let description = Some(format!(
                "Choose a specific model and reasoning level (current: {current_label})"
            ));

            items.push(SelectionItem {
                name: "All models".to_string(),
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        let header = self.model_menu_header(
            "Select Model",
            "Pick a quick auto mode or browse all models.",
        );
        self.show_model_selection_view(
            model_ids,
            SelectionViewParams {
                view_id: Some(MODEL_SELECTION_VIEW_ID),
                items,
                header,
                ..SelectionViewParams::picker()
            },
        );
    }

    pub(super) fn is_auto_model(model: &str) -> bool {
        model.starts_with("codex-auto-")
    }

    fn auto_model_order(model: &str) -> usize {
        match model {
            "codex-auto-fast" => 0,
            "codex-auto-balanced" => 1,
            "codex-auto-thorough" => 2,
            _ => 3,
        }
    }

    pub(crate) fn open_all_models_popup(&mut self) {
        if self.restrict_model_picker_to_luna_reserve() {
            self.open_luna_reserve_model_popup(
                self.model_catalog.try_list_models().unwrap_or_default(),
                ALL_MODELS_SELECTION_VIEW_ID,
            );
            return;
        }
        let presets = self
            .model_catalog
            .try_list_models()
            .unwrap_or_default()
            .into_iter()
            .filter(|preset| preset.show_in_picker && !Self::is_auto_model(&preset.model))
            .collect();
        self.open_all_models_popup_with_view_id(presets, ALL_MODELS_SELECTION_VIEW_ID);
    }

    fn open_all_models_popup_with_view_id(
        &mut self,
        presets: Vec<ModelPreset>,
        view_id: &'static str,
    ) {
        if presets.is_empty() {
            self.bottom_pane.dismiss_view_by_id(view_id);
            self.add_info_message(
                "No additional models are available right now.".to_string(),
                /*hint*/ None,
            );
            return;
        }

        let mut items: Vec<SelectionItem> = Vec::new();
        let model_ids = presets.iter().map(|preset| preset.model.clone()).collect();
        for preset in presets.into_iter() {
            let description =
                (!preset.description.is_empty()).then_some(preset.description.to_string());
            let is_current = preset.model.as_str() == self.current_model();
            let direct_effort = match preset.supported_reasoning_efforts.as_slice() {
                [] => Some(preset.default_reasoning_effort.clone()),
                [option] => Some(option.effort.clone()),
                _ => None,
            }
            .filter(|effort| !Self::is_advanced_reasoning_effort(effort));
            let name = preset.display_name.clone();
            let choice = if let Some(effort) = direct_effort {
                self.model_choice(&preset.model, effort)
            } else {
                let scoped = self.scope_model_picker_event();
                SelectionItem {
                    actions: vec![Box::new(move |tx| {
                        tx.send(scoped(AppEvent::OpenReasoningPopup {
                            model: preset.clone(),
                        }));
                    })],
                    dismiss_parent_on_child_accept: true,
                    ..Default::default()
                }
            };
            items.push(SelectionItem {
                name,
                description,
                is_current,
                ..choice
            });
        }

        let header = self.model_menu_header("Select Model and Effort", "");
        self.show_model_selection_view(
            model_ids,
            SelectionViewParams {
                view_id: Some(view_id),
                items,
                header,
                ..SelectionViewParams::picker()
            },
        );
    }

    /// Open a popup to choose the standard reasoning effort for the given model.
    ///
    /// Max and Ultra require an explicit second step so expensive efforts cannot
    /// be selected accidentally while moving through the normal effort scale.
    pub(crate) fn open_reasoning_popup(&mut self, preset: ModelPreset) {
        let default_effort = preset.default_reasoning_effort.clone();
        let supported = &preset.supported_reasoning_efforts;
        let in_plan_mode =
            self.collaboration_modes_enabled() && self.active_mode_kind() == ModeKind::Plan;

        let warn_effort = if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::XHigh)
        {
            Some(ReasoningEffortConfig::XHigh)
        } else if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::High)
        {
            Some(ReasoningEffortConfig::High)
        } else {
            None
        };
        let warning_text = warn_effort.as_ref().map(|effort| {
            let effort_label = Self::reasoning_effort_label(effort);
            format!("⚠ {effort_label} reasoning effort can quickly consume Plus plan rate limits")
        });
        let warn_for_model = preset.model.starts_with("gpt-5.1-codex")
            || preset.model.starts_with("gpt-5.1-codex-max")
            || preset.model.starts_with("gpt-5.2");

        let mut all_choices: Vec<ReasoningEffortConfig> = supported
            .iter()
            .map(|option| option.effort.clone())
            .collect();
        if all_choices.is_empty() {
            all_choices.push(default_effort.clone());
        }
        let (choices, advanced_choices): (Vec<_>, Vec<_>) = all_choices
            .into_iter()
            .partition(|effort| !Self::is_advanced_reasoning_effort(effort));

        if let [effort] = choices.as_slice()
            && advanced_choices.is_empty()
        {
            for action in self.model_choice(&preset.model, effort.clone()).actions {
                action(&self.app_event_tx);
            }
            return;
        }

        let default_choice = choices
            .contains(&default_effort)
            .then(|| default_effort.clone());

        let model_slug = preset.model.to_string();
        let model_label = preset.display_name.clone();
        let is_current_model = self.current_model() == preset.model.as_str();
        let highlight_choice = if is_current_model {
            if in_plan_mode {
                self.config
                    .plan_mode_reasoning_effort
                    .clone()
                    .or_else(|| self.effective_reasoning_effort())
            } else {
                self.effective_reasoning_effort()
            }
        } else {
            default_choice.clone().or_else(|| choices.first().cloned())
        };
        let selection_choice = highlight_choice.clone().or_else(|| default_choice.clone());
        let initial_selected_idx = choices
            .iter()
            .position(|choice| Some(choice) == selection_choice.as_ref());
        let mut items: Vec<SelectionItem> = Vec::new();
        for choice in choices.iter() {
            let effort = choice.clone();
            let mut effort_label = Self::reasoning_effort_label(&effort);
            if Some(choice) == default_choice.as_ref() {
                effort_label.push_str(" (default)");
            }

            let description = supported
                .iter()
                .find(|option| option.effort == effort)
                .map(|option| option.description.to_string())
                .filter(|text| !text.is_empty());

            let show_warning = warn_for_model && warn_effort.as_ref() == Some(&effort);
            let selected_description = if show_warning {
                warning_text.as_ref().map(|warning_message| {
                    description.as_ref().map_or_else(
                        || warning_message.clone(),
                        |d| format!("{d}\n{warning_message}"),
                    )
                })
            } else {
                None
            };

            items.push(SelectionItem {
                name: effort_label,
                description,
                selected_description,
                is_current: is_current_model && Some(choice) == highlight_choice.as_ref(),
                ..self.model_choice(&model_slug, effort)
            });
        }

        if !advanced_choices.is_empty() {
            let advanced_label = advanced_choices
                .iter()
                .map(Self::reasoning_effort_label)
                .collect::<Vec<_>>()
                .join(" and ");
            let verb = if advanced_choices.len() == 1 {
                "consumes"
            } else {
                "consume"
            };
            let preset_for_action = preset;
            let scoped = self.scope_model_picker_event();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(scoped(AppEvent::OpenAdvancedReasoningPopup {
                    model: preset_for_action.clone(),
                }));
            })];
            items.push(SelectionItem {
                name: "More reasoning…".to_string(),
                description: Some(format!("{advanced_label} {verb} usage limits faster")),
                is_current: is_current_model
                    && highlight_choice
                        .as_ref()
                        .is_some_and(Self::is_advanced_reasoning_effort),
                actions,
                dismiss_parent_on_child_accept: true,
                ..Default::default()
            });
        }

        let header = Paragraph::new(Line::from(
            format!("Select Reasoning Level for {model_label}").bold(),
        ))
        .wrap(Wrap { trim: false });

        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(MODEL_OPTIONS_VIEW_ID),
            header: Box::new(header),
            items,
            initial_selected_idx,
            ..SelectionViewParams::picker()
        });
    }

    /// Open the explicit Max/Ultra effort picker for the given model.
    pub(crate) fn open_advanced_reasoning_popup(&mut self, preset: ModelPreset) {
        let mut choices = preset
            .supported_reasoning_efforts
            .iter()
            .map(|option| option.effort.clone())
            .filter(Self::is_advanced_reasoning_effort)
            .collect::<Vec<_>>();
        if choices.is_empty()
            && Self::is_advanced_reasoning_effort(&preset.default_reasoning_effort)
        {
            choices.push(preset.default_reasoning_effort.clone());
        }
        choices.sort_by_key(|effort| matches!(effort, ReasoningEffortConfig::Ultra));
        if choices.is_empty() {
            return;
        }

        let model_slug = preset.model.to_string();
        let is_current_model = self.current_model() == preset.model.as_str();
        let highlight_choice = is_current_model
            .then(|| self.effective_reasoning_effort())
            .flatten();
        let mut items = Vec::new();
        for effort in choices {
            let description = match &effort {
                ReasoningEffortConfig::Max => {
                    "For difficult problems when quality matters more than speed · higher usage"
                }
                ReasoningEffortConfig::Ultra => {
                    "For demanding work using multiple agents · highest usage"
                }
                _ => unreachable!("advanced choices are limited to Max and Ultra"),
            };
            items.push(SelectionItem {
                name: Self::reasoning_effort_label(&effort),
                description: Some(description.to_string()),
                is_current: is_current_model && Some(&effort) == highlight_choice.as_ref(),
                ..self.model_choice(&model_slug, effort)
            });
        }

        let header = Paragraph::new(vec![
            Line::from("Advanced Reasoning".bold()),
            Line::from("⚠ Consumes usage limits faster".cyan()),
        ])
        .wrap(Wrap { trim: false });
        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(MODEL_OPTIONS_VIEW_ID),
            header: Box::new(header),
            items,
            ..SelectionViewParams::picker()
        });
    }

    pub(super) fn is_advanced_reasoning_effort(effort: &ReasoningEffortConfig) -> bool {
        matches!(
            effort,
            ReasoningEffortConfig::Max | ReasoningEffortConfig::Ultra
        )
    }

    pub(super) fn reasoning_effort_label(effort: &ReasoningEffortConfig) -> String {
        match effort {
            ReasoningEffortConfig::None => "None".to_string(),
            ReasoningEffortConfig::Minimal => "Minimal".to_string(),
            ReasoningEffortConfig::Low => "Low".to_string(),
            ReasoningEffortConfig::Medium => "Medium".to_string(),
            ReasoningEffortConfig::High => "High".to_string(),
            ReasoningEffortConfig::XHigh => "Extra high".to_string(),
            ReasoningEffortConfig::Max => "Max".to_string(),
            ReasoningEffortConfig::Ultra => "Ultra".to_string(),
            ReasoningEffortConfig::Persistent => "Persistent".to_string(),
            ReasoningEffortConfig::Custom(value) => value.clone(),
        }
    }

    pub(super) fn reasoning_effort_sentence_label(effort: &ReasoningEffortConfig) -> String {
        match effort {
            ReasoningEffortConfig::Custom(value) => value.clone(),
            effort => Self::reasoning_effort_label(effort).to_lowercase(),
        }
    }

    pub(super) fn ultra_reasoning_concurrency_warning(
        &self,
        effort: &ReasoningEffortConfig,
    ) -> Option<String> {
        if effort != &ReasoningEffortConfig::Ultra {
            return None;
        }

        let max_threads = self
            .config
            .multi_agent_v2
            .max_concurrent_threads_per_session;
        if max_threads < ULTRA_REASONING_CONCURRENCY_WARNING_THRESHOLD {
            return None;
        }

        let max_subagents = max_threads.saturating_sub(1);
        Some(format!(
            "Ultra reasoning may proactively use multiple agents. This session is configured for \
             {max_threads} concurrent threads with up to {max_subagents} subagents which can \
             increase usage quickly. Consider setting \
             features.multi_agent_v2.max_concurrent_threads_per_session below 8."
        ))
    }
}
