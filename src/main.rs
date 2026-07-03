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
    }
    Ok(())
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
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let launch_result = run_editor(path);

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    if let Err(err) = launch_result {
        eprintln!("failed to launch editor: {err}");
        return Ok(());
    }

    app.reload_current_file()
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
