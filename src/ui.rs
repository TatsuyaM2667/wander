use std::collections::HashMap;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use terminal_pixel_animation::render_half_block;

use crate::camera::Cam;
use crate::camera::FOV;
use crate::render::{braille_to_text, decode_halfblock, draw_minimap, render_scene};
use crate::world::{Theme, Weather, WORLDS, World};

pub const SPD: f64 = 4.0;
pub const RSPD: f64 = 2.8;
pub const HOLD_MS: u128 = 200;

#[derive(PartialEq)]
enum State {
    Title,
    Select,
    Play,
}

pub struct Game {
    state: State,
    sel: usize,
    world: Option<World>,
    cam: Cam,
    pixel_buf: Vec<u8>,
    cols: u32,
    rows: u32,
    render_w: u32,
    render_h: u32,
    fps: u32,
    frames: u64,
    fps_acc: f64,
    last: Instant,
    pub held: HashMap<crossterm::event::KeyCode, Instant>,
    weather: Weather,
    weather_left: f32,
    rand_state: u64,
    anim: u64,
}

impl Game {
    pub fn new(cols: u32, rows: u32) -> Self {
        let render_w = cols;
        let render_h = rows * 2;
        Self {
            state: State::Title,
            sel: 0,
            world: None,
            cam: Cam {
                x: 40.0,
                y: 40.0,
                z: 3.0,
                dx: 1.0,
                dy: 0.0,
                px: 0.0,
                py: FOV,
            },
            pixel_buf: vec![0u8; (render_w * render_h * 3) as usize],
            cols,
            rows,
            render_w,
            render_h,
            fps: 0,
            frames: 0,
            fps_acc: 0.0,
            last: Instant::now(),
            held: HashMap::new(),
            weather: Weather::Clear,
            weather_left: 12.0,
            rand_state: 0x9E3779B97F4A7C15,
            anim: 0,
        }
    }

    fn roll_weather(&mut self) {
        self.rand_state = self
            .rand_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let n = (self.rand_state >> 33) % 100;
        let w = if n < 45 {
            Weather::Clear
        } else if n < 72 {
            Weather::Rain
        } else if n < 80 {
            Weather::Snow
        } else {
            Weather::Fog
        };
        let theme = self.world.as_ref().map(|w| w.theme).unwrap_or(Theme::Valley);
        self.weather = match theme {
            Theme::Frozen | Theme::Valley => w,
            _ => {
                if w == Weather::Snow {
                    Weather::Fog
                } else {
                    w
                }
            }
        };
        self.weather_left = 8.0 + (self.rand_state % 1200) as f32 / 100.0;
    }

    pub fn handle_key(&mut self, code: crossterm::event::KeyCode) {
        match self.state {
            State::Title => match code {
                crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc => {
                    std::process::exit(0)
                }
                crossterm::event::KeyCode::Enter | crossterm::event::KeyCode::Char(' ') => {
                    self.state = State::Select;
                }
                _ => {}
            },
            State::Select => match code {
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                    self.state = State::Title;
                }
                crossterm::event::KeyCode::Up => {
                    self.sel = self.sel.wrapping_sub(1) % WORLDS.len();
                }
                crossterm::event::KeyCode::Down => {
                    self.sel = (self.sel + 1) % WORLDS.len();
                }
                crossterm::event::KeyCode::Enter => {
                    let (_, seed, _, theme) = WORLDS[self.sel];
                    self.world = Some(World::build(seed, theme));
                    self.cam = Cam {
                        x: 40.0,
                        y: 40.0,
                        z: 3.0,
                        dx: 1.0,
                        dy: 0.0,
                        px: 0.0,
                        py: FOV,
                    };
                    self.last = Instant::now();
                    self.held.clear();
                    self.weather = Weather::Clear;
                    self.weather_left = 12.0 + (self.rand_state % 600) as f32 / 100.0;
                    self.state = State::Play;
                }
                _ => {}
            },
            State::Play => match code {
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                    self.state = State::Select;
                    self.world = None;
                }
                _ => {}
            },
        }
    }

    pub fn update(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;

        if self.state != State::Play {
            return;
        }

        self.fps_acc += dt;
        self.frames += 1;
        self.anim = self.anim.wrapping_add(1);
        if self.fps_acc >= 0.5 {
            self.fps = (self.frames as f64 / self.fps_acc) as u32;
            self.fps_acc = 0.0;
            self.frames = 0;
        }

        self.held
            .retain(|_, t| now.duration_since(*t).as_millis() < HOLD_MS);

        self.weather_left -= dt as f32;
        if self.weather_left <= 0.0 {
            self.roll_weather();
        }

        let world = match self.world {
            Some(ref w) => w,
            None => return,
        };
        for (key, _) in self.held.iter() {
            match key {
                crossterm::event::KeyCode::Left => self.cam.rot(-RSPD * dt),
                crossterm::event::KeyCode::Right => self.cam.rot(RSPD * dt),
                crossterm::event::KeyCode::Up => {
                    self.cam
                        .mv(world, self.cam.dx * SPD * dt, self.cam.dy * SPD * dt);
                }
                crossterm::event::KeyCode::Down => {
                    self.cam
                        .mv(world, -self.cam.dx * SPD * dt, -self.cam.dy * SPD * dt);
                }
                crossterm::event::KeyCode::Char('a') | crossterm::event::KeyCode::Char('A') => {
                    self.cam
                        .mv(world, self.cam.dy * SPD * dt, -self.cam.dx * SPD * dt);
                }
                crossterm::event::KeyCode::Char('d') | crossterm::event::KeyCode::Char('D') => {
                    self.cam
                        .mv(world, -self.cam.dy * SPD * dt, self.cam.dx * SPD * dt);
                }
                crossterm::event::KeyCode::Char('w') | crossterm::event::KeyCode::Char('W') => {
                    self.cam
                        .mv(world, self.cam.dx * SPD * dt, self.cam.dy * SPD * dt);
                }
                crossterm::event::KeyCode::Char('s') | crossterm::event::KeyCode::Char('S') => {
                    self.cam
                        .mv(world, -self.cam.dx * SPD * dt, -self.cam.dy * SPD * dt);
                }
                _ => {}
            }
        }
    }

    fn draw_title(&self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Length(12),
                Constraint::Length(3),
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .split(area);

        let banner = vec![
            Line::from(Span::styled(
                "  __        __   _    _____                   _             ",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "  \\ \\      / /__| |__|  ___|_ _ __ __ _ _ __ | |_ ___  _ __",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                "   \\ \\ /\\ / / _ \\ '_ \\ |_ / _` / __/ _` | '_ \\| __/ _ \\| '__|",
                Style::default().fg(Color::Blue),
            )),
            Line::from(Span::styled(
                "    \\ V  V /  __/ |_) |  _| (_| | (_| | | | | || (_) | |   ",
                Style::default().fg(Color::Blue),
            )),
            Line::from(Span::styled(
                "     \\_/\\_/ \\___|_.__/|_|  \\__,_|\\__,_|_| |_|\\__\\___/|_|   ",
                Style::default().fg(Color::Magenta),
            )),
        ];
        let title = Paragraph::new(banner).alignment(Alignment::Center);
        f.render_widget(title, chunks[1]);

        let sub = Paragraph::new(Line::from(Span::styled(
            "A 3D Open World Explorer",
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center);
        f.render_widget(sub, chunks[2]);

        let hint = Paragraph::new(Line::from(Span::styled(
            "[Enter] Start   [Q] Quit",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center);
        f.render_widget(hint, chunks[3]);
    }

    fn draw_select(&self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "  Select World",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        f.render_widget(title, chunks[0]);

        let items: Vec<ListItem> = WORLDS
            .iter()
            .enumerate()
            .map(|(i, &(name, seed, desc, theme))| {
                let style = if i == self.sel {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let indicator = if i == self.sel { " > " } else { "   " };
                ListItem::new(Line::from(vec![
                    Span::styled(indicator, Style::default().fg(Color::Green)),
                    Span::styled(format!("{:<16}", name), style),
                    Span::styled(
                        format!(
                            "seed:{:<6} {} [{}]",
                            seed,
                            desc,
                            theme_name(theme)
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Worlds")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(list, chunks[2]);

        let help = Paragraph::new(Line::from(Span::styled(
            "  [Up/Down] Select   [Enter] Play   [Esc] Back",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[4]);
    }

    fn draw_play(&mut self, f: &mut Frame) {
        let area = f.area();
        if let Some(ref w) = self.world {
            let rw = self.render_w as usize;
            let rh = self.render_h as usize;
            let pal = w.theme.palette();
            render_scene(
                &mut self.pixel_buf,
                rw,
                rh,
                &self.cam,
                w,
                &pal,
                self.weather,
                self.anim,
            );
            draw_minimap(&mut self.pixel_buf, rw, &self.cam, w);
        }

        let cells = render_half_block(
            &self.pixel_buf,
            self.render_w,
            self.render_h,
            self.cols,
            self.rows,
        )
        .unwrap();
        let decoded = decode_halfblock(&cells, self.cols, self.rows);
        let text = braille_to_text(&decoded, self.cols);

        let para = Paragraph::new(text);
        f.render_widget(para, area);

        // HUD overlay (top-right): world name + weather + fps + coords
        let world_name = WORLDS.get(self.sel).map_or("Unknown", |&(n, _, _, _)| n);
        let wxstr = match self.weather {
            Weather::Clear => "SUNNY",
            Weather::Rain => "RAIN",
            Weather::Snow => "SNOW",
            Weather::Fog => "FOG ",
        };
        let hud_text = format!(
            " {} [{}] FPS:{:>2} ({:.1},{:.1}) ",
            world_name, wxstr, self.fps, self.cam.x, self.cam.y
        );
        let hud_w = hud_text.len() as u16;
        if hud_w < area.width {
            let hud_area = ratatui::layout::Rect {
                x: area.x + area.width - hud_w - 1,
                y: area.y,
                width: hud_w,
                height: 1,
            };
            let hud = Paragraph::new(Line::from(Span::styled(
                hud_text,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::Rgb(10, 10, 20)),
            )));
            f.render_widget(hud, hud_area);
        }

        // bottom controls hint
        let ctrl = " Arrows/WASD move  Esc menu ";
        let cw = ctrl.len() as u16;
        if cw < area.width && area.height > 0 {
            let ctrl_area = ratatui::layout::Rect {
                x: area.x + 1,
                y: area.y + area.height - 1,
                width: cw,
                height: 1,
            };
            let cp = Paragraph::new(Line::from(Span::styled(
                ctrl,
                Style::default()
                    .fg(Color::DarkGray)
                    .bg(Color::Rgb(10, 10, 20)),
            )));
            f.render_widget(cp, ctrl_area);
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        match self.state {
            State::Title => self.draw_title(f),
            State::Select => self.draw_select(f),
            State::Play => self.draw_play(f),
        }
    }
}

fn theme_name(t: Theme) -> &'static str {
    match t {
        Theme::Valley => "Valley",
        Theme::Desert => "Desert",
        Theme::Frozen => "Frozen",
        Theme::Tropical => "Tropical",
        Theme::Forest => "Forest",
    }
}
