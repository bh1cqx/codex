//! Save status line settings to the active local user configuration.

use super::App;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use crate::statusline_provider::StatuslineProvider;
use anyhow::Context;
use codex_config::types::BuiltinStatusLine;
use codex_config::types::StatusLineSelection;
use codex_config::types::StatusLineSelection::Builtin;
use codex_config::types::StatusLineSelection::Mcp;

use crate::bottom_pane::StatusLineItem;
use crate::legacy_core::config::edit::status_line_items_edit;
use crate::legacy_core::config::edit::status_line_rows_edit;
use crate::legacy_core::config::edit::status_line_use_colors_edit;

impl App {
    pub(super) async fn save_status_line(
        &mut self,
        items: Vec<StatusLineItem>,
        use_theme_colors: bool,
        row: Option<usize>,
    ) -> anyhow::Result<()> {
        let ids = items.iter().map(ToString::to_string).collect::<Vec<_>>();
        let rows = if let Some(row) = row {
            let mut rows = self
                .chat_widget
                .local_settings
                .tui
                .status_line_rows
                .clone()
                .context("Status line rows are no longer configured")?;
            *rows
                .get_mut(row)
                .context("Status line row no longer exists")? = ids.clone();
            Some(rows)
        } else {
            None
        };
        let items_edit = match &rows {
            Some(rows) => status_line_rows_edit(rows),
            None => status_line_items_edit(&ids),
        };
        let mut edits = vec![items_edit, status_line_use_colors_edit(use_theme_colors)];
        if rows.is_none() {
            edits.push(ConfigEdit::ClearPath {
                segments: vec!["tui".to_string(), "status_line_rows".to_string()],
            });
        }
        ConfigEditsBuilder::for_config_path(self.local_settings.user_config_path.as_path())
            .with_edits(edits)
            .apply()
            .await?;

        self.local_settings.tui.status_line_rows = rows.clone();
        self.local_settings.tui.status_line_use_colors = use_theme_colors;
        if let Some(rows) = rows {
            self.chat_widget
                .setup_status_line_rows(rows, use_theme_colors);
        } else {
            self.local_settings.tui.status_line = Some(ids);
            self.chat_widget.setup_status_line(items, use_theme_colors);
        }
        Ok(())
    }

    pub(super) async fn save_status_line_provider(
        &mut self,
        provider: StatusLineSelection,
        provider_from_picker: Option<StatuslineProvider>,
    ) -> anyhow::Result<()> {
        let value = match &provider {
            Builtin(BuiltinStatusLine::Default) => toml_edit::value("default"),
            Mcp(id) => {
                let table = toml_edit::InlineTable::from_iter([
                    ("server", id.server.as_str()),
                    ("tool", id.tool.as_str()),
                ]);
                toml_edit::value(table)
            }
        };
        ConfigEditsBuilder::for_config_path(self.local_settings.user_config_path.as_path())
            .with_edits([ConfigEdit::SetPath {
                segments: vec!["tui".into(), "status_line_provider".into()],
                value,
            }])
            .apply()
            .await
            .context("Failed to save status line provider")?;

        let previous = self
            .local_settings
            .tui
            .status_line_provider
            .as_deref()
            .unwrap_or(&Builtin(BuiltinStatusLine::Default));
        let change = match (previous, &provider) {
            (Builtin(_), Mcp(_)) => Some("enabled_custom"),
            (Mcp(_), Builtin(_)) => Some("returned_to_default"),
            (Mcp(old), Mcp(new)) if old != new => Some("switched_custom"),
            (Builtin(_), Builtin(_)) | (Mcp(_), Mcp(_)) => None,
        };
        if let Some(change) = change {
            self.statusline_metrics.record(
                &self.config,
                "codex.statusline.provider_changed",
                &[("change", change)],
            );
        }
        self.local_settings.tui.status_line_provider = Some(Box::new(provider.clone()));
        self.chat_widget.local_settings.tui.status_line_provider = Some(Box::new(provider));
        self.chat_widget
            .reselect_statusline_provider(provider_from_picker);
        Ok(())
    }
}

#[cfg(test)]
#[path = "status_line_settings_tests.rs"]
mod tests;
