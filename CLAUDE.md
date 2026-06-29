# Agent Guide

See ARCHITECTURE.md for the full architecture documentation.

## One-way Dependency Rule

When editing this repository: **maintain one-way dependency direction from shell (binary) to core (library)**.

- Library modules (`parser`, `app`, `ui` in `src/lib.rs`) contain all reusable/testable logic
- Binary (`main.rs`) handles only: arg parsing, terminal lifecycle, event loop
- `parser.rs` must have NO dependencies on ratatui, walkdir, or crossterm
- `app.rs` calls the parser but must NOT embed Logseq syntax parsing logic

Litmus test: Would the code survive dropping the UI framework? If not, reconsider the design.
