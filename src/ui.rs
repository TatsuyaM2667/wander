use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use terminal_pixel_animation::render_half_block;

use crate::camera::{Cam, free_pos, FOV};
use crate::net::{NetClient, spawn_host, DEFAULT_ADDR};
use crate::render::{
    braille_to_text, decode_halfblock, draw_minimap, render_scene, MiniDot,
};
use crate::sim::{Diff, EnemyState, PlayerState, Sim, K_A, K_D, K_S, K_W};
use crate::world::{Theme, Weather, WORLDS, World};

const SPD: f64 = 4.0;
const RSPD: f64 = 2.8;
const HOLD_MS: u128 = 200;

// ---------------------------------------------------------------------------
// state machine
// ---------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum PlayMode {
    Single,
    Host,
    Join,
}

#[derive(PartialEq)]
enum State {
    Title,
    Mode,
    Select,
    Diff,
    JoinAddr,
    Play,
}

pub struct Game {
    state: State,
    mode: PlayMode,
    mode_sel: usize,
    sel: usize,
    diff: i32,

    // simulation / networking
    sim: Option<Sim>,
    net: Option<NetClient>,
    my_id: usize,
    cur_name: String,
    net_world: Option<World>,

    // local player state (authoritative for display)
    px: f64,
    py: f64,
    p_angle: f64,
    p_hp: f32,
    p_score: u32,
    weather: Weather,
    enemy_snapshot: Vec<EnemyState>,
    player_snapshot: Vec<PlayerState>,

    // input
    angle: f64,
    wants_shoot: bool,
    shoot_cd: f32,

    // visual
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
    anim: u64,
    flash: f32,

    // join address input
    addr: String,
    addr_err: Option<String>,
}

impl Game {
    pub fn new(cols: u32, rows: u32) -> Self {
        let render_w = cols;
        let render_h = rows * 2;
        Self {
            state: State::Title,
            mode: PlayMode::Single,
            mode_sel: 0,
            sel: 0,
            diff: 5,
            sim: None,
            net: None,
            my_id: 0,
            cur_name: String::new(),
            net_world: None,
            px: 40.0,
            py: 40.0,
            p_angle: 0.0,
            p_hp: 100.0,
            p_score: 0,
            weather: Weather::Clear,
            enemy_snapshot: Vec::new(),
            player_snapshot: Vec::new(),
            angle: 0.0,
            wants_shoot: false,
            shoot_cd: 0.0,
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
            anim: 0,
            flash: 0.0,
            addr: DEFAULT_ADDR.to_string(),
            addr_err: None,
        }
    }

    fn local_world(&self) -> Option<&World> {
        if let Some(ref sim) = self.sim {
            Some(&sim.world)
        } else {
            self.net_world.as_ref()
        }
    }

    pub fn handle_key(&mut self, code: crossterm::event::KeyCode) {
        match self.state {
            State::Title => match code {
                crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc => {
                    std::process::exit(0)
                }
                crossterm::event::KeyCode::Enter | crossterm::event::KeyCode::Char(' ') => {
                    self.state = State::Mode;
                }
                _ => {}
            },
            State::Mode => match code {
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                    self.state = State::Title;
                }
                crossterm::event::KeyCode::Up => {
                    self.mode_sel = self.mode_sel.wrapping_sub(1) % 3;
                }
                crossterm::event::KeyCode::Down => {
                    self.mode_sel = (self.mode_sel + 1) % 3;
                }
                crossterm::event::KeyCode::Enter => {
                    self.mode = match self.mode_sel {
                        0 => PlayMode::Single,
                        1 => PlayMode::Host,
                        _ => PlayMode::Join,
                    };
                    if self.mode == PlayMode::Join {
                        self.addr_err = None;
                        self.state = State::JoinAddr;
                    } else {
                        self.state = State::Select;
                    }
                }
                _ => {}
            },
            State::Select => match code {
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                    self.state = State::Mode;
                }
                crossterm::event::KeyCode::Up => {
                    self.sel = self.sel.wrapping_sub(1) % WORLDS.len();
                }
                crossterm::event::KeyCode::Down => {
                    self.sel = (self.sel + 1) % WORLDS.len();
                }
                crossterm::event::KeyCode::Enter => {
                    self.state = State::Diff;
                }
                _ => {}
            },
            State::Diff => match code {
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                    self.state = State::Select;
                }
                crossterm::event::KeyCode::Up => {
                    self.diff = (self.diff - 1).max(1);
                }
                crossterm::event::KeyCode::Down => {
                    self.diff = (self.diff + 1).min(10);
                }
                crossterm::event::KeyCode::Left => {
                    self.diff = (self.diff - 1).max(1);
                }
                crossterm::event::KeyCode::Right => {
                    self.diff = (self.diff + 1).min(10);
                }
                crossterm::event::KeyCode::Enter => {
                    self.start_play();
                }
                _ => {}
            },
            State::JoinAddr => match code {
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                    self.state = State::Mode;
                }
                crossterm::event::KeyCode::Char(c) => {
                    if self.addr.len() < 40 {
                        self.addr.push(c);
                    }
                }
                crossterm::event::KeyCode::Backspace => {
                    self.addr.pop();
                }
                crossterm::event::KeyCode::Enter => {
                    self.join_network();
                }
                _ => {}
            },
            State::Play => match code {
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                    self.net = None;
                    self.net_world = None;
                    self.sim = None;
                    self.state = State::Mode;
                }
                crossterm::event::KeyCode::Char(' ')
                | crossterm::event::KeyCode::Enter => {
                    self.wants_shoot = true;
                }
                _ => {}
            },
        }
    }

    fn start_play(&mut self) {
        let (_, seed, _, theme) = WORLDS[self.sel];
        let diff = Diff::new(self.diff);
        self.cur_name = WORLDS[self.sel].0.to_string();
        match self.mode {
            PlayMode::Single => {
                let mut sim = Sim::new(seed, theme, diff);
                let my_id = sim.add_player();
                self.px = sim.players[my_id].x;
                self.py = sim.players[my_id].y;
                self.angle = 0.0;
                self.p_angle = 0.0;
                self.p_hp = sim.players[my_id].hp;
                self.p_score = 0;
                self.weather = sim.snapshot().weather;
                self.sim = Some(sim);
                self.my_id = my_id;
            }
            PlayMode::Host => {
                let _ = spawn_host(seed, theme, 4242, diff);
                let mut client =
                    NetClient::connect(&format!("127.0.0.1:{}", 4242)).ok();
                if let Some(ref mut c) = client {
                    c.join(seed, theme);
                    if !c.wait_hello(2000) {
                        self.addr_err = Some("connection failed".into());
                        self.state = State::Select;
                        return;
                    }
                    if let Some(h) = c.local_hello() {
                        self.my_id = h.player_id;
                        self.px = h.x;
                        self.py = h.y;
                        self.angle = h.angle;
                        self.p_angle = h.angle;
                        self.weather = Weather::Clear;
                        self.net_world = Some(World::build(h.seed, h.theme));
                    }
                    self.net = client;
                } else {
                    self.addr_err = Some("connect failed".into());
                    self.state = State::Select;
                    return;
                }
                self.sim = None;
            }
            PlayMode::Join => return,
        }
        self.held.clear();
        self.p_hp = 100.0;
        self.p_score = 0;
        self.enemy_snapshot.clear();
        self.player_snapshot.clear();
        self.last = Instant::now();
        self.flash = 0.0;
        self.cam.set_angle(self.angle);
        self.cam.x = self.px;
        self.cam.y = self.py;
        self.state = State::Play;
    }

    fn join_network(&mut self) {
        self.addr_err = None;
        match NetClient::connect(self.addr.trim()) {
            Err(e) => {
                self.addr_err = Some(e);
            }
            Ok(mut client) => {
                // we need seed/theme for join but server decides; use defaults
                // server hello arrives asynchronously; poll briefly
                if !client.wait_hello(1500) {
                    self.addr_err = Some("no hello received".into());
                    return;
                }
                let hello = client.local_hello();
                if let Some(h) = hello {
                    self.my_id = h.player_id;
                    self.px = h.x;
                    self.py = h.y;
                    self.angle = h.angle;
                    self.p_angle = h.angle;
                    self.net_world = Some(World::build(h.seed, h.theme));
                    self.weather = Weather::Clear;
                    self.cur_name = world_name_for(h.seed).to_string();
                    self.net = Some(client);
                    self.sim = None;
                    self.held.clear();
                    self.p_hp = 100.0;
                    self.p_score = 0;
                    self.enemy_snapshot.clear();
                    self.player_snapshot.clear();
                    self.last = Instant::now();
                    self.flash = 0.0;
                    self.cam.set_angle(self.angle);
                    self.cam.x = self.px;
                    self.cam.y = self.py;
                    self.state = State::Play;
                } else {
                    self.addr_err = Some("no hello".into());
                }
            }
        }
    }

    pub fn update(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.1);
        self.last = now;

        if self.state != State::Play {
            return;
        }

        self.fps_acc += dt;
        self.frames += 1;
        self.anim += 1;
        if self.fps_acc >= 0.5 {
            self.fps = (self.frames as f64 / self.fps_acc) as u32;
            self.fps_acc = 0.0;
            self.frames = 0;
        }

        self.held
            .retain(|_, t| now.duration_since(*t).as_millis() < HOLD_MS);

        self.flash = (self.flash - dt as f32 * 5.0).max(0.0);
        self.shoot_cd = (self.shoot_cd - dt as f32).max(0.0);

        // rotation
        let mut angle = self.angle;
        if self.held.contains_key(&crossterm::event::KeyCode::Left) {
            angle -= RSPD * dt;
        }
        if self.held.contains_key(&crossterm::event::KeyCode::Right) {
            angle += RSPD * dt;
        }
        self.angle = angle;

        // movement mask
        let mut mask: u16 = 0;
        if self.held.contains_key(&crossterm::event::KeyCode::Up)
            || self.held.contains_key(&crossterm::event::KeyCode::Char('w'))
            || self.held.contains_key(&crossterm::event::KeyCode::Char('W'))
        {
            mask |= K_W;
        }
        if self.held.contains_key(&crossterm::event::KeyCode::Down)
            || self.held.contains_key(&crossterm::event::KeyCode::Char('s'))
            || self.held.contains_key(&crossterm::event::KeyCode::Char('S'))
        {
            mask |= K_S;
        }
        if self.held.contains_key(&crossterm::event::KeyCode::Char('a'))
            || self.held.contains_key(&crossterm::event::KeyCode::Char('A'))
        {
            mask |= K_A;
        }
        if self.held.contains_key(&crossterm::event::KeyCode::Char('d'))
            || self.held.contains_key(&crossterm::event::KeyCode::Char('D'))
        {
            mask |= K_D;
        }

        let shoot = self.wants_shoot && self.shoot_cd <= 0.0;
        if shoot {
            self.shoot_cd = 0.25;
        }
        self.wants_shoot = false;

        match self.mode {
            PlayMode::Single => {
                if let Some(ref mut sim) = self.sim {
                    sim.apply_player(self.my_id, mask, angle, shoot, dt);
                    sim.tick(dt);
                    if shoot {
                        self.flash = 0.6;
                    }
                    if sim.hit_landed.swap(false, Ordering::Relaxed) {
                        self.flash = 1.0;
                    }
                    let p = sim.players[self.my_id];
                    self.px = p.x;
                    self.py = p.y;
                    self.p_hp = p.hp;
                    self.p_score = p.score;
                    self.weather = sim.snapshot().weather;
                    self.enemy_snapshot = sim.enemies.clone();
                }
            }
            PlayMode::Host | PlayMode::Join => {
                let wptr: *const World = match self.local_world() {
                    Some(w) => w as *const World,
                    None => std::ptr::null(),
                };
                if let Some(ref mut client) = self.net {
                    client.send_input(mask, angle, shoot);
                    if shoot {
                        self.flash = 0.6;
                    }
                    // client-side prediction
                    let w: Option<&World> = if wptr.is_null() {
                        None
                    } else {
                        Some(unsafe { &*wptr })
                    };
                    if let Some(w) = w {
                        let (co, si) = (angle.cos(), angle.sin());
                        let (mut wx, mut wy) = (0.0, 0.0);
                        if mask & K_W != 0 {
                            wx += co;
                            wy += si;
                        }
                        if mask & K_S != 0 {
                            wx -= co;
                            wy -= si;
                        }
                        if mask & K_A != 0 {
                            wx += si;
                            wy -= co;
                        }
                        if mask & K_D != 0 {
                            wx -= si;
                            wy += co;
                        }
                        let len = (wx * wx + wy * wy).sqrt();
                        if len > 0.0 {
                            let s = if len > 1.0 { 1.0 / len } else { 1.0 };
                            let nx = self.px + wx * s * SPD * dt;
                            let ny = self.py + wy * s * SPD * dt;
                            if free_pos(w, nx, self.py) {
                                self.px = nx;
                            }
                            if free_pos(w, self.px, ny) {
                                self.py = ny;
                            }
                        }
                    }
                    if let Some(snap) = client.get_snapshot() {
                        self.weather = snap.weather;
                        self.enemy_snapshot = snap.enemies.clone();
                        self.player_snapshot = snap.players.clone();
                        if let Some(my) = snap.players.get(self.my_id) {
                            let dx = my.x - self.px;
                            let dy = my.y - self.py;
                            if dx * dx + dy * dy > 0.16 {
                                self.px = my.x;
                                self.py = my.y;
                            } else {
                                self.px += dx * 0.25;
                                self.py += dy * 0.25;
                            }
                            self.p_hp = my.hp;
                            self.p_score = my.score;
                        }
                    }
                }
            }
        }

        self.cam.x = self.px;
        self.cam.y = self.py;
        self.cam.set_angle(self.angle);
    }

    // -----------------------------------------------------------------------
    // drawing helpers
    // -----------------------------------------------------------------------

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
            "A 3D FPS",
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

    fn draw_mode(&self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(4),
                Constraint::Length(1),
            ])
            .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "  Select Mode",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        f.render_widget(title, chunks[0]);

        let modes = ["Single Player", "Host Game", "Join Game"];
        let items: Vec<ListItem> = modes
            .iter()
            .enumerate()
            .map(|(i, &name)| {
                let selected = i == self.mode_sel;
                let style = if selected {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let indicator = if selected { " > " } else { "   " };
                ListItem::new(Line::from(vec![
                    Span::styled(indicator, Style::default().fg(Color::Green)),
                    Span::styled(format!("{:<18}", name), style),
                ]))
            })
            .collect();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Mode")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(list, chunks[1]);

        let help = Paragraph::new(Line::from(Span::styled(
            "  [Up/Down] Select   [Enter] OK   [Esc] Back",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[2]);
    }

    fn draw_select(&self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
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
                let selected = i == self.sel;
                let style = if selected {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let indicator = if selected { " > " } else { "   " };
                ListItem::new(Line::from(vec![
                    Span::styled(indicator, Style::default().fg(Color::Green)),
                    Span::styled(format!("{:<14}", name), style),
                    Span::styled(
                        format!("seed:{:<6} {} [{}]", seed, desc, theme_name(theme)),
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
        f.render_widget(list, chunks[1]);

        let help = Paragraph::new(Line::from(Span::styled(
            "  [Up/Down] Select   [Enter] Play   [Esc] Back",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[2]);
    }

    fn draw_diff(&self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(12),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "  Select Difficulty",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        f.render_widget(title, chunks[0]);

        let d = Diff::new(self.diff);
        let items: Vec<ListItem> = (1..=10)
            .map(|lv| {
                let d2 = Diff::new(lv);
                let selected = lv == self.diff;
                let style = if selected {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let name = d2.name();
                let bar_w: usize = lv as usize;
                let bar = format!(
                    "{}{}",
                    "\u{2588}".repeat(bar_w),
                    "\u{2591}".repeat(10 - bar_w)
                );
                ListItem::new(Line::from(vec![
                    Span::styled(
                        if selected { " > " } else { "   " },
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(format!("{:>2} {} ", lv, bar), style),
                    Span::styled(
                        format!(
                            "{}  enemies:{} spd:{:.2} hp:{} dmg:{}",
                            name,
                            d2.enemy_count(),
                            d2.enemy_speed(),
                            d2.enemy_hp(),
                            d2.enemy_dmg(),
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Difficulty: {} / 10 ({})", d.level, d.name()))
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(list, chunks[1]);

        let help = Paragraph::new(Line::from(Span::styled(
            "  [Up/Down] Adjust   [Enter] Start   [Esc] Back",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[2]);
    }

    fn draw_join_addr(&self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "  Connect to Host",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        f.render_widget(title, chunks[0]);

        let input_text = format!("  Address: {}█", self.addr);
        let input = Paragraph::new(Line::from(Span::styled(
            input_text,
            Style::default().fg(Color::Cyan),
        )));
        f.render_widget(input, chunks[1]);

        if let Some(ref err) = self.addr_err {
            let err_p = Paragraph::new(Line::from(Span::styled(
                format!("  Error: {}", err),
                Style::default().fg(Color::Red),
            )));
            f.render_widget(err_p, chunks[2]);
        }

        let help = Paragraph::new(Line::from(Span::styled(
            "  [Enter] Connect   [Esc] Back",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(help, chunks[3]);
    }

    fn draw_play(&mut self, f: &mut Frame) {
        let area = f.area();
        let rw = self.render_w as usize;
        let rh = self.render_h as usize;
        let (w, pal): (&World, crate::world::Palette) = if let Some(ref s) = self.sim {
            (&s.world, s.world.theme.palette())
        } else if let Some(ref nw) = self.net_world {
            (nw, nw.theme.palette())
        } else {
            return;
        };
        let cam = self.cam;
        let weather = self.weather;
        let anim = self.anim;
        let my_id = self.my_id;
        let flash = self.flash;

        {
            let pbuf = &mut self.pixel_buf;
            render_scene(
                pbuf,
                rw,
                rh,
                &cam,
                w,
                &pal,
                weather,
                anim,
                &self.player_snapshot,
                &self.enemy_snapshot,
                my_id,
                flash,
            );
            let mut dots = Vec::with_capacity(
                self.enemy_snapshot.len() + self.player_snapshot.len(),
            );
            for e in &self.enemy_snapshot {
                if e.alive {
                    dots.push(MiniDot {
                        x: e.x,
                        y: e.y,
                        r: 220,
                        g: 40,
                        b: 40,
                    });
                }
            }
            for (i, p) in self.player_snapshot.iter().enumerate() {
                if i == self.my_id || p.hp <= 0.0 {
                    continue;
                }
                dots.push(MiniDot {
                    x: p.x,
                    y: p.y,
                    r: 80,
                    g: 160,
                    b: 240,
                });
            }
            draw_minimap(pbuf, rw, &cam, w, &dots);
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

        f.render_widget(Paragraph::new(text), area);

        // HUD – top-right
        let world_name = self.cur_name.as_str();
        let wxstr = match self.weather {
            Weather::Clear => "SUNNY",
            Weather::Rain => "RAIN ",
            Weather::Snow => "SNOW ",
            Weather::Fog => "FOG  ",
        };
        let mode_str = match self.mode {
            PlayMode::Single => "SP",
            PlayMode::Host | PlayMode::Join => "MP",
        };
        let hud_text = format!(
            " {} [{}] {} DIF:{} FPS:{:>2} ({:.1},{:.1}) ",
            mode_str, wxstr, world_name, self.diff, self.fps, self.px, self.py
        );
        let hud_w = hud_text.len() as u16;
        if hud_w < area.width {
            let hud_area = ratatui::layout::Rect {
                x: area.x + area.width - hud_w - 1,
                y: area.y,
                width: hud_w,
                height: 1,
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    hud_text,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                        .bg(Color::Rgb(10, 10, 20)),
                ))),
                hud_area,
            );
        }

        // HUD – HP bar bottom-right
        let hp_pct = (self.p_hp / 100.0).max(0.0).min(1.0);
        let bar_full = 12;
        let filled = (hp_pct * bar_full as f32) as usize;
        let hp_color = if self.p_hp > 60.0 {
            Color::Green
        } else if self.p_hp > 30.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        let bar = format!(
            " HP:{}{} ",
            "\u{2588}".repeat(filled),
            "\u{2591}".repeat(bar_full - filled),
        );
        let hp_text = format!("{}{}", bar, self.p_score);
        let hp_w = hp_text.len() as u16;
        if hp_w < area.width {
            let hp_area = ratatui::layout::Rect {
                x: area.x + area.width - hp_w - 1,
                y: area.y + area.height - 1,
                width: hp_w,
                height: 1,
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    hp_text,
                    Style::default()
                        .fg(hp_color)
                        .bg(Color::Rgb(10, 10, 20)),
                ))),
                hp_area,
            );
        }

        // controls hint bottom-left
        let ctrl = " WASD/Arrows move  Space/Enter shoot  Esc menu ";
        let cw = ctrl.len() as u16;
        if cw < area.width && area.height > 0 {
            let ctrl_area = ratatui::layout::Rect {
                x: area.x + 1,
                y: area.y + area.height - 1,
                width: cw,
                height: 1,
            };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    ctrl,
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(Color::Rgb(10, 10, 20)),
                ))),
                ctrl_area,
            );
        }

        // death overlay
        if self.p_hp <= 0.0 {
            let msg = Paragraph::new(Line::from(Span::styled(
                "  DOWN... respawning ",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::Rgb(10, 10, 20)),
            )))
            .alignment(Alignment::Center);
            let da = ratatui::layout::Rect {
                x: area.x + area.width / 2 - 12,
                y: area.y + area.height / 2,
                width: 24,
                height: 1,
            };
            f.render_widget(msg, da);
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        match self.state {
            State::Title => self.draw_title(f),
            State::Mode => self.draw_mode(f),
            State::Select => self.draw_select(f),
            State::Diff => self.draw_diff(f),
            State::JoinAddr => self.draw_join_addr(f),
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

fn world_name_for(seed: u64) -> &'static str {
    WORLDS
        .iter()
        .find(|&&(_, s, _, _)| s == seed)
        .map_or("World", |&(n, _, _, _)| n)
}