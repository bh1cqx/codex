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

Codex and `ccx` share `~/.codex/config.toml` for presets, model defaults, `tui.status_line`, colors, and other upstream settings. `ccx` automatically reads `tui.status_line_rows`, `tui.status_header`, and the answer-navigation and session-tab key bindings from `~/.codex/multiline.config.toml`. Edits to these fork settings go in that file. An explicit `--profile` or user-config path selects the usual profile configuration.

If an older multiline file contains presets or other shared settings, `ccx` ignores those entries. Copy any values you want to keep into `config.toml`.

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

The header uses a purple tint across the full row: deep purple on dark terminals and light lavender on light terminals, preserving the item colors. Labels are adjusted for contrast where needed. It accepts the same item identifiers as the footer, with `context-used` shown as `ctx 42% ▰▰▱▱▱`. Each filled block represents 20% used. The meter turns yellow at 80% and bold red at 95% when status colors are enabled; it stays on the same row. Its selection is independent of `status_line` and `status_line_rows`. Unavailable items are omitted and long headers are truncated with an ellipsis.

Remove `status_header` or set it to `[]` to hide it. Restart the TUI after editing the file. The setting stays local to this fork's TUI; it requires no daemon changes and uses the same session data as the official client.

## Message appearance

Answers and partial answers have a teal `▌` along the left edge. Progress messages use a small `·` and softer text; expanded thinking summaries use an italic `∴`. Tool calls retain their action labels and success/error colors.

This styling applies automatically to live output and restored history, with no extra rows or configuration. Messages without phase metadata keep their original appearance. Raw output and copied Markdown retain the source text.

In fullscreen chat and the **Ctrl+T** transcript:

* **Alt+K / Alt+J:** jump to the previous or next teal answer. A streamed answer counts as one message. Previous-answer navigation loads older history when needed.
* **Alt+A:** toggle between the normal view and prompts plus answers. The filtered view also keeps proposed plans and questions. It hides progress, reasoning, and tool output, including from transcript search and selection.

Filtering changes only the view. Drafts, saved history, and approval controls stay available. Older messages without phase metadata remain visible because their role cannot be determined reliably. The footer shows “Answers only” while filtering is enabled.

Remap these keys with `previous_answer`, `next_answer`, and `toggle_answers_only` under `[tui.keymap.global]` in `~/.codex/multiline.config.toml`, or use `/keymap`. Existing custom bindings take precedence over these defaults. Native terminal scrollback is unchanged; use **Ctrl+T** when running inline.

## Session tabs

Pin a task with **Alt+P** to show session tabs below the status header in fullscreen chat. Tabs use the same pinned tasks as the agent command center and remain hidden in that view. The current task appears alongside pinned tasks, even when it is not pinned.

* **Alt+H / Alt+L:** switch to the previous or next tab. Alt+PageUp / Alt+PageDown remain aliases.
* **Alt+backtick:** return to the last viewed session.
* **Alt+P** or the star button: pin or unpin the current task.
* Click a tab to open it. Overflow arrows reveal other tabs.

The badges show `▶` for working, `!` for needs input, `○` for ready, and `·` for inactive. An additional `*` marks unread activity. Switching uses the existing session attachment path and preserves each task's draft.

Remap the shortcuts with `session_tab_previous`, `session_tab_next`, `session_tab_last`, and `toggle_session_pin` under `[tui.keymap.chat]` in `~/.codex/multiline.config.toml`, or use `/keymap`.

## Diff side panel

`/diff` opens a read-only Changes panel beside fullscreen chat. It includes tracked and untracked changes and uses the existing workspace command runner.

While visible, the panel refreshes about every two seconds, including edits made outside Codex. Refreshes keep the current diff, scroll position, and keyboard focus. Automatic refreshes run one at a time. Run `/diff` again for an immediate refresh. A failed refresh keeps the last successful diff and shows an error while retrying. Hiding the panel stops further refreshes.

Click the conversation to keep typing, or click the panel to use the usual pager shortcuts. The mouse wheel scrolls the panel. Press **q** or **Ctrl+C** while the panel has focus, or click **×**, to close it. Closing the diff leaves side conversations intact.

On narrow terminals, the focused panel fills the screen. Inline mode keeps the existing full-screen diff pager. Delayed results cannot replace a newer request or reopen a hidden panel.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
