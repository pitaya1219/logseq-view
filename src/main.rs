use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use logseq_view::app::{App, Focus};
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
    let mut app = App::new(graph_path)?;
    let mut pending_g = false;

    loop {
        terminal.draw(|f| logseq_view::ui::draw(f, &mut app))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            let was_pending_g = pending_g;
            pending_g = false;

            // Handle quit keys first (applies to all focus modes)
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                _ => {
                    // Dispatch to focus-specific handling
                    match app.focus {
                        Focus::Browser => match key.code {
                            KeyCode::Tab => app.toggle_focus(),
                            KeyCode::Down | KeyCode::Char('j') => app.browser_down(),
                            KeyCode::Up | KeyCode::Char('k') => app.browser_up(),
                            KeyCode::Enter | KeyCode::Char('l') => {
                                app.open_selected()?;
                            }
                            KeyCode::Char('h') => app.collapse_or_jump_parent(),
                            _ => {}
                        },
                        Focus::Content => match key.code {
                            KeyCode::Tab | KeyCode::Char('h') => app.toggle_focus(),
                            KeyCode::Down | KeyCode::Char('j') => app.content_down(1),
                            KeyCode::Up | KeyCode::Char('k') => app.content_up(1),
                            KeyCode::PageDown => app.content_down(20),
                            KeyCode::PageUp => app.content_up(20),
                            KeyCode::Char('G') => app.content_bottom(),
                            KeyCode::Char('g') => {
                                if was_pending_g {
                                    app.content_top();
                                } else {
                                    pending_g = true;
                                }
                            }
                            _ => {}
                        },
                    }
                }
            }
        }
    }

    Ok(())
}
