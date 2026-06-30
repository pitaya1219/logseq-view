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
use logseq_view::app::App;
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
            let (action, next_pending_g) = map_key(app.focus, key, pending_g);
            pending_g = next_pending_g;

            if let Some(action) = action {
                let should_quit = app.update(action)?;
                if should_quit {
                    break;
                }
            }
        }
    }

    Ok(())
}
