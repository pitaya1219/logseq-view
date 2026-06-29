# Architecture

This project targets the **Elm Architecture** pattern: Model / Action / update / view, with a single side-effect port (GraphSource).

## Core Modules

- **parser.rs**: The pure domain core. Parses Logseq markdown syntax into structured data. It has NO dependencies on ratatui, walkdir, or crossterm. It is completely framework-agnostic.

- **app.rs**: The Model and update logic. Manages application state (file browser, content display). Calls the parser for Logseq syntax parsing but does NOT embed parsing logic itself.

- **ui.rs**: The View. Renders the Model using ratatui widgets.

## Design Principles

1. **One-way dependency direction**: shell (binary) -> core (library). The library (`logseq_view`) contains all reusable/testable logic. The binary (`main.rs`) handles only binary concerns: arg parsing, terminal lifecycle (raw mode / alternate screen), and the event loop.

2. **Separation of concerns**: Parser is pure domain; update calls parser but does not contain parsing; UI only renders.

3. **Litmus test**: "Would it survive dropping the UI framework?" The core logic (parser, app state management) should remain usable if ratatui/crossterm were replaced.

## Foundation Step

This is the foundation step. The current implementation has:
- Library/binary split (Issue #25)
- Clippy config as boundary guard (clippy.toml)

Future issues will add:
- GraphSource port (single side-effect port)
- Action type and update function
- ViewModel for cleaner view separation
