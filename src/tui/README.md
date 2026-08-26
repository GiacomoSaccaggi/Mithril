# TUI — Minas Tirith

Full-screen terminal UI with ratatui: scrollable chat, sidebar, popups.

## Key Concept

Pure presentation layer. Delegates all logic to ChatCore and renders ChatAction results.

## Features

- `@file` popup with Tab completion
- `#agent` popup with Tab completion
- `/command` suggestions
- Multiline input (Shift+Enter)
- Sidebar toggle (Ctrl+S)
- Page scroll (PageUp/PageDown)

## Files

- `app.rs` — App state for the Mithril TUI.
- `events.rs` — Event handling for the TUI — keyboard input, resize, etc.
- `mod.rs` — Mithril TUI — full terminal user interface built with ratatui.
- `splash.rs` — Mithril startup splash — fullscreen dwarf mining animation.
- `theme.rs` — Mithril TUI theme — dark forge aesthetic.
- `ui.rs` — UI rendering — draws the TUI layout each frame.
