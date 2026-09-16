mod camera;
mod render;
mod ui;
mod world;

use std::time::{Duration, Instant};

use ratatui::Terminal;

use ui::Game;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    crossterm::execute!(stdout, crossterm::cursor::Hide)?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let (cols, rows) = crossterm::terminal::size()?;
    let mut game = Game::new(cols as u32, rows as u32);

    loop {
        let mut keys_this_frame = Vec::new();
        while crossterm::event::poll(Duration::ZERO)? {
            if let crossterm::event::Event::Key(e) = crossterm::event::read()? {
                if e.kind == crossterm::event::KeyEventKind::Press {
                    if e.code == crossterm::event::KeyCode::Char('c')
                        && e.modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                    {
                        crossterm::execute!(
                            terminal.backend_mut(),
                            crossterm::cursor::Show
                        )?;
                        crossterm::execute!(
                            terminal.backend_mut(),
                            crossterm::terminal::LeaveAlternateScreen
                        )?;
                        crossterm::terminal::disable_raw_mode()?;
                        terminal.show_cursor()?;
                        return Ok(());
                    }
                    game.handle_key(e.code);
                    keys_this_frame.push(e.code);
                }
            }
        }

        let now = Instant::now();
        for &k in &keys_this_frame {
            game.held.insert(k, now);
        }

        game.update();
        terminal.draw(|f| game.draw(f))?;
        std::thread::sleep(Duration::from_millis(8));
    }
}