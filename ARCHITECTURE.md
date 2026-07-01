# Architecture

This project targets the **Elm Architecture** pattern: Model / Action / update / view, with a single side-effect port (GraphSource).

## Core Modules

- **parser.rs**: The pure domain core. Parses Logseq markdown syntax into structured data. It has NO dependencies on ratatui, walkdir, or crossterm. It is completely framework-agnostic.

- **app.rs**: The Model and update logic. Manages application state (file browser, content display) and the central `update(Action)` state transition. Calls the parser for Logseq syntax parsing but does NOT embed parsing logic itself.

- **action.rs**: The input layer. `Action` enum plus the pure `map_key` function that turns key events into actions. No domain (Logseq-notation) logic.

- **source.rs**: The single side-effect port. `GraphSource` trait (`children` / `read`) with `WalkdirGraphSource` (filesystem) and `FakeGraphSource` (in-memory, for tests). This is where `walkdir` / `std::fs` live, isolated from the core.

- **view_model.rs**: The presenter. Builds a `ViewModel` (the values needed to render, already scroll-clamped and sliced) from the app state.

- **ui.rs**: The View. Renders the `ViewModel` using ratatui widgets — read-only, no state mutation.

## Design Principles

1. **One-way dependency direction**: shell (binary) -> core (library). The library (`logseq_view`) contains all reusable/testable logic. The binary (`main.rs`) handles only binary concerns: arg parsing, terminal lifecycle (raw mode / alternate screen), and the event loop.

2. **Separation of concerns**: Parser is pure domain; update calls parser but does not contain parsing; UI only renders.

3. **Litmus test**: "Would it survive dropping the UI framework?" The core logic (parser, app state management) should remain usable if ratatui/crossterm were replaced.

## Migration Status

The TEA + port migration is complete:
- Library/binary split, with `clippy.toml` as a boundary guard (#25)
- `GraphSource` port — filesystem access isolated behind a trait (#21)
- `Action` enum + central `update()`, key handling via pure `map_key` (#22)
- `ViewModel` + presenter — rendering reads a DTO; scroll clamp moved out of the view (#23)

## Decisions & Alternatives

Recorded so future changes (and agents) keep the same shape instead of re-deriving it.

- **Chosen: The Elm Architecture + one side-effect port (`GraphSource`).** Fits a TUI's
  immediate-mode redraw and keeps the core pure and unit-testable.
- **Rejected: full Clean Architecture (4 layers, DTO mapping at each boundary).** Overkill for a
  small, read-only viewer — the layering and ceremony cost more than it returns at this scale.
- **Deferred: splitting the core into separate crates (`crates/core` + `crates/tui`).** Not worth it
  yet. The lib/bin split plus the `clippy.toml` boundary guard are enough to hold the dependency
  direction. Revisit only if the core needs to be published/reused, or if multi-author / multi-agent
  work makes the dependency direction hard to keep by convention — at which point Cargo's build graph
  would enforce it physically.

The dependency direction itself (shell → core) is enforced by `clippy.toml`'s `disallowed_*` lints in
CI, not by documentation. This file is guidance; the lint is the guard.

## Release Process

Releases are automated via `.gitea/workflows/publish.yml` on `v*` tag push.

Two equivalent paths — pick either:

**UI path (Gitea Actions tab → Publish → Run workflow):**  
Enter the version (e.g. `0.2.0`) and click Run. CI bumps `Cargo.toml`, updates `Cargo.lock`, regenerates `CHANGELOG.md`, commits, tags, then publishes.

**CLI path:**
1. Bump `version` in `Cargo.toml`.
2. Run `git cliff -o CHANGELOG.md` to regenerate the full changelog.
3. Commit: `chore(release): vX.Y.Z` (excluded from the changelog by `cliff.toml`).
4. `git tag vX.Y.Z && git push origin vX.Y.Z` — CI takes it from there.

In both cases, CI verifies tag version == `Cargo.toml` version, publishes to crates.io, and creates a Gitea release with generated release notes.

**Commit message convention (required for changelog generation):** All commits must use [Conventional Commits](https://www.conventionalcommits.org/) prefixes (`feat:`, `fix:`, `refactor:`, `chore:`, etc.); `git-cliff` reads these to build the changelog.
