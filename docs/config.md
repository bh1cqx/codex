# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Status line rows

Set `tui.status_line_rows` to place status items on explicit rows:

```toml
[tui]
status_line_rows = [
  ["model-with-reasoning", "git-branch"],
  ["current-dir", "thread-name"],
  ["context-used", "five-hour-limit", "weekly-limit"],
]
```

Each inner array is one row, with items in the listed order. Rows keep the existing item colors and links. Long rows are truncated with an ellipsis; they do not wrap. Empty rows and rows whose items are all unavailable are omitted. If the terminal cannot fit every row alongside the input and required hints, Codex clips the remaining rows and marks the last visible row with an ellipsis.

`status_line_rows` takes precedence over `status_line`. Set it to `[]` to hide the status line, or remove it to use the existing `status_line` list and its defaults.

`/statusline` opens a row selector when multiple rows are configured, then edits the selected row. Theme colors apply to all rows. Add, remove, or reorder rows in the config file. An inline command such as `/statusline model git-branch` selects a single line for the current session; `/statusline --clear` hides it. Color-only commands preserve the row layout.

The "Current status line" preset supports single-line layouts only.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
