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
| `gg` | Top of content (Content) / top of current directory (Browser) |
| `G` | Bottom of content (Content) / bottom of current directory (Browser) |
| `PgUp`/`PgDn` | Page scroll |
| `/` | Start in-page search |
| `n` | Next search match |
| `N` | Previous search match |
| `Esc` | Cancel search |
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
 BROWSER ↑↓/jk navigate  Enter open  Tab switch pane  q quit
```

## License

MIT
