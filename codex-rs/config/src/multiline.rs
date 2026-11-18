use crate::AbsolutePathBuf;
use crate::LoaderOverrides;
use std::path::Path;

pub const MULTILINE_CONFIG_TOML_FILE: &str = "multiline.config.toml";

pub(crate) const FORK_SETTINGS: &[&str] = &[
    "tui.status_header",
    "tui.status_line_rows",
    "tui.keymap.global.previous_answer",
    "tui.keymap.global.next_answer",
    "tui.keymap.global.toggle_answers_only",
    "tui.keymap.chat.session_tab_previous",
    "tui.keymap.chat.session_tab_next",
    "tui.keymap.chat.session_tab_last",
    "tui.keymap.chat.toggle_session_pin",
];

pub fn is_multiline_setting(path: &str) -> bool {
    FORK_SETTINGS.contains(&path)
}

impl LoaderOverrides {
    pub fn multiline_config_path(&self, codex_home: &Path) -> Option<AbsolutePathBuf> {
        (self.user_config_path.is_none()
            && self.user_config_profile.is_none()
            && !self.ignore_user_config)
            .then(|| {
                AbsolutePathBuf::resolve_path_against_base(MULTILINE_CONFIG_TOML_FILE, codex_home)
            })
    }
}
