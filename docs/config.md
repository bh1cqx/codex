# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Status line rows

Create `~/.codex/multiline.config.toml` and set `tui.status_line_rows` to place status items on explicit rows:

```toml
[tui]
status_line_rows = [
  ["model-with-reasoning", "git-branch"],
  ["current-dir", "thread-name"],
  ["context-used", "five-hour-limit", "weekly-limit"],
]
```

This fork automatically loads `multiline.config.toml` over the shared `config.toml`; no command-line flag is needed. Standard Codex continues to use `config.toml`, where you can keep its single-line `tui.status_line` setting. Keeping the fork-only setting in the separate file also lets standard Codex use `--strict-config`.

When the automatic file is present, status-line edits are saved there. If it is absent, the fork uses the shared config. An explicit `--profile` selection or user-config path bypasses the automatic file. Invalid automatic configuration is reported as an error.

Each inner array is one row, with items in the listed order. Rows keep the existing item colors and links. Long rows are truncated with an ellipsis; they do not wrap. Empty rows and rows whose items are all unavailable are omitted. If the terminal cannot fit every row alongside the input and required hints, Codex clips the remaining rows and marks the last visible row with an ellipsis.

`status_line_rows` takes precedence over `status_line`. Set it to `[]` to hide the status line, or remove it to use the existing `status_line` list and its defaults.

`/statusline` opens a row selector when multiple rows are configured, then edits the selected row. Theme colors apply to all rows. Add, remove, or reorder rows in the config file. An inline command such as `/statusline model git-branch` selects a single line for the current session; `/statusline --clear` hides it. Color-only commands preserve the row layout.

The "Current status line" preset supports single-line layouts only.

## Status header

Add `status_header` under `[tui]` in `~/.codex/multiline.config.toml` to pin a compact row above the conversation. This example keeps the model and context in the header and uses one footer row:

```toml
[tui]
status_header = [
  "model-with-reasoning",
  "context-used",
  "current-dir",
  "git-branch",
  "five-hour-limit",
  "weekly-limit",
]
status_line_rows = [
  ["run-state", "task-progress", "approval-mode", "thread-name"],
]
```

Fullscreen mode keeps the header at the top of chat view while the transcript scrolls. Selection views, including `/agents`, hide it. Inline mode places it above the live output. The header yields its row when the composer needs the space.

The header uses a shaded background across the full row, with label colors adjusted for contrast. It accepts the same item identifiers as the footer, with `context-used` shortened to `ctx 42%`. Its selection is independent of `status_line` and `status_line_rows`. Unavailable items are omitted and long headers are truncated with an ellipsis.

Remove `status_header` or set it to `[]` to hide it. Restart the TUI after editing the file. The setting stays local to this fork's TUI; it requires no daemon changes and uses the same session data as the official client.

## Session tabs

Pin a task with **Alt+P** to show session tabs below the status header in fullscreen chat. Tabs use the same pinned tasks as the agent command center and remain hidden in that view. The current task appears alongside pinned tasks, even when it is not pinned.

* **Alt+PageUp / Alt+PageDown:** switch between tabs.
* **Alt+backtick:** return to the last viewed session.
* **Alt+P** or the star button: pin or unpin the current task.
* Click a tab to open it. Overflow arrows reveal other tabs.

The badges show `▶` for working, `!` for needs input, `○` for ready, and `·` for inactive. An additional `*` marks unread activity. Switching uses the existing session attachment path and preserves each task's draft.

Remap the shortcuts with `session_tab_previous`, `session_tab_next`, `session_tab_last`, and `toggle_session_pin` under `[tui.keymap.chat]` in `~/.codex/multiline.config.toml`, or use `/keymap`.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
