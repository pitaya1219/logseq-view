# logseq-view

TUI viewer for Logseq markdown files.

## Install

```bash
cargo install logseq-view
```

## Usage

```bash
# Specify your Logseq graph path
lqview ~/logseq

# Without arguments, auto-detects ~/logseq, ~/Logseq, ~/Documents/logseq
lqview
```

## Keybindings

| Key | Action |
|-----|--------|
| `j`/`k` or `↑`/`↓` | Navigate / scroll |
| `Enter`/`l` | Open file / expand directory |
| `h` | Collapse directory / jump to parent |
| `gg` | Top of content (Content) / top of current directory scope (Browser) |
| `G` | Bottom of content (Content) / bottom of current directory scope (Browser) |
| `PgUp`/`PgDn` | Page scroll |
| `/` | In-page search with match count (Content) / filter files by title or content (Browser) |
| `n` / `N` | Next / previous search match (Content only) |
| `Esc` | Cancel search / filter |
| `Tab` | Switch focus (browser ↔ content) |
| `q` / `Ctrl+c` | Quit |

## Logseq syntax rendering

- `- ` bullets with indented nesting
- `[[page link]]` — cyan + underlined
- `#tag` — green
- `**bold**` — bold
- `*italic*` — italic
- `` `code` `` — yellow on dark background
- `TODO` / `DONE` / `LATER` / `NOW` / `WAITING` / `CANCELLED` — color-coded
- `key:: value` properties — blue key
- Code blocks (` ```lang `)
- `((block-ref))` — magenta reference

## Layout

```
┌─ Files ────────┬─ PageTitle ──────────────────────────────────────┐
│ ▼ pages        │ • [[link]] text                                   │
│   Project.md   │   • indented sub-bullet                           │
│   Meeting.md   │     • nested further                              │
│ ▶ journals     │                                                   │
│                │ • TODO task                                        │
│                │ • DONE completed task                              │
└────────────────┴───────────────────────────────────────────────────┘
 BROWSER ↑↓/jk navigate  Enter open  / filter  Tab switch pane  q quit
```

## License

MIT
