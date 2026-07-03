use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use logseq_view::action::map_key;
use logseq_view::app::{App, Effect, Focus};
use logseq_view::source::WalkdirGraphSource;
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
                    apply_effect(effect);
                }

                if update.quit {
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Interprets a single `Effect` produced by `App::update()`. There are no
/// variants yet, so this is a no-op hook; future effects (e.g.
/// `Effect::LaunchEditor`, see #46) that need terminal/process control are
/// handled here rather than in `app.rs`, keeping the core free of
/// process/terminal concerns.
// `Effect` has no variants yet, so this match only has a wildcard arm;
// `#[non_exhaustive]` still requires it, and clippy would otherwise flag the
// match as reducible to its body. Kept as a match (not a plain no-op) so #46
// can add `Effect::LaunchEditor { .. }` as a real arm here.
#[allow(clippy::match_single_binding)]
fn apply_effect(effect: Effect) {
    match effect {
        _ => {}
    }
}
