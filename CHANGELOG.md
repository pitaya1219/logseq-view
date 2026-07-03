# Changelog
## [0.2.0] - 2026-07-03

### Bug Fixes

- Fix BlockRef byte boundary panic and URL decode omission in title
- Resolve edge cases in inline and file parsing
- Use GitHub URL as flake homepage
- Prevent double CI run on PR by restricting push to main
- Keep typing 'q' in content search instead of quitting
- Make task keywords searchable and show search prompt on empty query
- Clamp browser/content scroll to inner height so bottom selection isn't clipped
- Extract git-cliff binary from archive root directly
- Use find to locate git-cliff binary regardless of archive structure
- Preserve scheme when constructing git remote URL for push
- Grant contents write permission for bump-and-tag push
- Use workflow-bot PAT secret for push instead of gitea.token
- Use workflow-bot actual email to satisfy Gitea user lookup
- Derive git author from WORKFLOW_BOT_TOKEN identity dynamically
- Unset checkout extraheader before push to avoid uid=-2 pre-receive crash
- Correct file content (previous commit wrote base64 string literally)
- Write RELEASE_NOTES.md outside working tree to avoid dirty cargo publish

### Documentation

- Update architecture for completed TEA migration
- Reorganize keybindings table to unify search rows by pane

### Features

- Add logseq-view TUI source
- Introduce GraphSource port to separate filesystem access from core
- Introduce Action enum and update() method for TEA pattern
- Introduce ViewModel for cleaner view separation
- Add Nix flake for reproducible builds and installation
- Sort journals directory in descending order
- Add gg/G navigation in browser with directory scoping
- Add in-page content search functionality
- Add match highlighting and hit count to in-page search
- Add incremental search in browser by file/dir name

### Miscellaneous Tasks

- Remove unused variable in parse_inline and rename dirs_next to home_dir
- Add CI workflow and fix formatting/clippy issues
- Lib/bin split + clippy guard foundation + architecture doc
- Add crates.io publish metadata to Cargo.toml
- Exclude dev-only files from crates.io package
- Add cargo publish workflow dispatch
- Rewrite CI to use nix devshell instead of rustup

### Refactor

- Eliminate duplication in collapse logic and walker configuration
- Move render_line to ui.rs and remove ratatui dependency from parser
- Move App state mutations out of draw functions into App methods
- Restrict clamp methods to pub(crate) and clarify intent with comment
- Extract quit key handling in event_loop
- Extract make_file_item helper to deduplicate FileItem construction
- Remove leftover debug output from bump-and-tag job

### Styling

- Apply rustfmt formatting

### Testing

- Add boundary tests for clamp_browser_scroll and clamp_content_scroll

### Debug

- Print git author config and commit details before push


