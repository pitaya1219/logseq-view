use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use logseq_view::action::map_key;
use logseq_view::app::{App, Effect, Focus};
use logseq_view::parser::splice_raw_lines;
use logseq_view::source::{GraphSource, WalkdirGraphSource};
use ratatui::{backend::CrosstermBackend, Terminal};

fn main() -> Result<()> {
    let graph_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // Try common Logseq locations
            let home = home_dir();
            for candidate in &[
                home.join("logseq"),
                home.join("Logseq"),
                home.join("Documents/logseq"),
                home.join("Documents/Logseq"),
            ] {
                if candidate.exists() {
                    return candidate.clone();
                }
            }
            PathBuf::from(".")
        });

    if !graph_path.exists() {
        eprintln!("error: path does not exist: {}", graph_path.display());
        std::process::exit(1);
    }

    run(graph_path)
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn run(graph_path: PathBuf) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, graph_path);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    graph_path: PathBuf,
) -> Result<()> {
    let source = WalkdirGraphSource::new();
    let mut app = App::new(graph_path, source)?;
    let mut pending_g = false;

    loop {
        terminal.draw(|f| logseq_view::ui::draw(f, &mut app))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            let search_input_active = match app.focus {
                Focus::Content => app.content_search_active,
                Focus::Browser => app.browser_search_active,
            };
            let browser_has_committed = app.browser_has_committed_search();

            let (action, next_pending_g) = map_key(
                app.focus,
                key,
                pending_g,
                search_input_active,
                browser_has_committed,
            );
            pending_g = next_pending_g;

            if let Some(action) = action {
                let update = app.update(action)?;

                for effect in update.effects {
                    apply_effect(effect, terminal, &mut app)?;
                }

                if update.quit {
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Interprets a single `Effect` produced by `App::update()`. Effects that
/// need terminal/process control are handled here rather than in `app.rs`,
/// keeping the core free of process/terminal concerns; `terminal` and `app`
/// are threaded through because acting on an effect (e.g. suspending the TUI
/// to launch an editor, then re-reading the edited file) needs both.
fn apply_effect<S: GraphSource>(
    effect: Effect,
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App<S>,
) -> Result<()> {
    match effect {
        Effect::LaunchEditor { path } => launch_editor(terminal, app, &path)?,
        Effect::EditBlock {
            path,
            raw_start,
            raw_end,
        } => launch_block_editor(terminal, app, &path, raw_start, raw_end)?,
    }
    Ok(())
}

/// Suspends the TUI (leaves raw mode + the alternate screen), runs `f`, then
/// restores the TUI (raw mode + alternate screen + `terminal.clear()`)
/// regardless of whether `f` succeeded. Shared by page-edit (`launch_editor`)
/// and block-edit (`launch_block_editor`) so both go through the exact same
/// suspend/resume dance around `$EDITOR`.
fn with_suspended_terminal(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    f: impl FnOnce() -> Result<()>,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let result = f();

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    result
}

/// Suspends the TUI, runs `$EDITOR` (falling back to `vi`, then `nano`) on
/// `path`, restores the TUI, and re-reads/re-parses the file so any edits
/// made in the external editor are reflected. Errors launching the editor
/// (e.g. none of the candidates are on `PATH`) are reported but do not crash
/// the event loop -- the TUI is always restored.
fn launch_editor<S: GraphSource>(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App<S>,
    path: &Path,
) -> Result<()> {
    let launch_result = with_suspended_terminal(terminal, || run_editor(path));

    if let Err(err) = launch_result {
        eprintln!("failed to launch editor: {err}");
        return Ok(());
    }

    app.reload_current_file()
}

/// Handles `Effect::EditBlock`: reads the current file through the
/// `GraphSource` port (via `App::read_file`), extracts just the block's raw
/// lines `[raw_start, raw_end)`, writes them to a scratch temp file, suspends
/// the TUI to edit that temp file in `$EDITOR` (reusing
/// `with_suspended_terminal`/`run_editor`, exactly like page-edit), splices
/// the edited block back into the full original file with the pure
/// `splice_raw_lines`, writes the result back through the `GraphSource` port
/// (`App::write_file`), and reloads. Temp-file creation is a legitimate
/// shell-side concern kept here rather than in the pure core; the temp file
/// is best-effort cleaned up afterwards. Errors at any step are reported but
/// never crash the event loop -- the TUI is always restored first.
fn launch_block_editor<S: GraphSource>(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App<S>,
    path: &Path,
    raw_start: usize,
    raw_end: usize,
) -> Result<()> {
    let original = match app.read_file(path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("failed to read file for block edit: {err}");
            return Ok(());
        }
    };

    let lines: Vec<&str> = original.lines().collect();
    let clamped_start = raw_start.min(lines.len());
    let clamped_end = raw_end.clamp(clamped_start, lines.len());
    let mut block_text = lines[clamped_start..clamped_end].join("\n");
    if !block_text.is_empty() {
        block_text.push('\n');
    }

    let temp_path = block_temp_path(path);
    if let Err(err) = std::fs::write(&temp_path, &block_text) {
        eprintln!("failed to create temp file for block edit: {err}");
        return Ok(());
    }

    let launch_result = with_suspended_terminal(terminal, || run_editor(&temp_path));

    let result = finish_block_edit(
        app,
        path,
        &original,
        clamped_start,
        clamped_end,
        &temp_path,
        launch_result,
    );

    // Best-effort cleanup -- a leftover temp file is harmless, so ignore errors here.
    let _ = std::fs::remove_file(&temp_path);

    if let Err(err) = result {
        eprintln!("failed to edit block: {err}");
    }

    Ok(())
}

/// The post-editor part of the block-edit flow: read back the temp file,
/// splice it into `original`, write the result, and reload. Split out of
/// `launch_block_editor` purely so temp-file cleanup there can run
/// unconditionally via a single `?`-free `result` binding.
fn finish_block_edit<S: GraphSource>(
    app: &mut App<S>,
    path: &Path,
    original: &str,
    raw_start: usize,
    raw_end: usize,
    temp_path: &Path,
    launch_result: Result<()>,
) -> Result<()> {
    launch_result?;
    let edited = std::fs::read_to_string(temp_path)?;
    let new_content = splice_raw_lines(original, raw_start, raw_end, &edited);
    app.write_file(path, &new_content)?;
    app.reload_current_file()
}

/// A scratch temp-file path for editing just one block of `path`, derived
/// from its file stem/extension plus the current process id (cheap
/// uniqueness -- avoids collisions between concurrent instances without
/// pulling in a random/uuid dependency).
fn block_temp_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("block");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("md");
    std::env::temp_dir().join(format!(
        "logseq-view-block-{stem}-{}.{ext}",
        std::process::id()
    ))
}

/// Runs the editor candidates in order (see `editor_candidates`) on `path`,
/// waiting for the process to exit. Returns an error only if none of the
/// candidates could be spawned.
fn run_editor(path: &Path) -> Result<()> {
    let candidates = editor_candidates();
    let mut last_err = None;

    for editor in &candidates {
        match Command::new(editor).arg(path).status() {
            Ok(_status) => return Ok(()),
            Err(err) => last_err = Some(err),
        }
    }

    Err(anyhow::anyhow!(
        "no editor could be launched (tried: {}): {}",
        candidates.join(", "),
        last_err.map(|e| e.to_string()).unwrap_or_default()
    ))
}

/// `$EDITOR` if set to a non-empty value, otherwise the fallback chain
/// `vi`, `nano` (tried in that order).
fn editor_candidates() -> Vec<String> {
    match std::env::var("EDITOR") {
        Ok(editor) if !editor.trim().is_empty() => vec![editor],
        _ => vec!["vi".to_string(), "nano".to_string()],
    }
}
