// taskvim v0.2 — a vim-motions-driven terminal task / project / course manager.
//
// Model:
//   Courses hold the full backlog, organized under sections ("subtitles").
//   The main dashboard's Tasks list contains only tasks you pin/link there
//   (p inside a course) or add manually. Completing a task anywhere creates
//   a "commit" that feeds the heatmap, streak, daily goal and course stats.
//
// Data lives in $XDG_DATA_HOME/taskvim/data.json (~/.local/share/taskvim/).
// Fully local — no cloud sync.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

mod app;
mod due;
mod model;
mod parse;
mod ui;

use app::App;
use ui::ui;

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|f| ui(f, app))?;
        if event::poll(Duration::from_millis(400))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.handle_key(k.code, k.modifiers);
                }
            }
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let mut app = App::new();
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    let _ = app.data.save();
    result
}
