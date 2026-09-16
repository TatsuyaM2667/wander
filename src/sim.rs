use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::camera::free_pos;
use crate::world::{Theme, WH, Weather, World};

pub const MAX_HP: f32 = 100.0;
pub const SPD: f64 = 4.0;

pub const K_W: u16 = 1 << 0;
pub const K_A: u16 = 1 << 1;
pub const K_S: u16 = 1 << 2;
pub const K_D: u16 = 1 << 3;

/// Difficulty level, 1..=10.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Diff {
    pub level: i32,
}

impl Diff {
    pub fn new(level: i32) -> Self {
        Self {
            level: level.clamp(1, 10),
        }
    }

    pub fn name(self) -> &'static str {
        match self.level {
            1 | 2 => "Casual",
            3 | 4 => "Easy",
            5 | 6 => "Normal",
            7 | 8 => "Hard",
            _ => "Nightmare",
        }
    }

    pub fn enemy_count(self) -> usize {
        (4 + self.level) as usize
    }

    pub fn enemy_speed(self) -> f64 {
        1.0 + self.level as f64 * 0.12
    }

    pub fn enemy_hp(self) -> f32 {
        60.0 + self.level as f32 * 14.0
    }

    pub fn enemy_dmg(self) -> f32 {
        4.0 + self.level as f32 * 0.8
    }
}

#[derive(Clone, Copy)]
pub struct PlayerState {
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    pub hp: f32,
    pub score: u32,
    pub kills: u32,
    pub dead_timer: f32,
    pub dmg_dir: f64,
    pub dmg_t: f32,
}

#[derive(Clone, Copy)]
pub struct EnemyState {
    pub x: f64,
    pub y: f64,
    pub alive: bool,
}

#[derive(Clone, Copy)]
pub struct Particle {
    pub x: f64,
    pub y: f64,
    pub vz: f64,
    pub vx: f64,
    pub vy: f64,
    pub z: f64,
    pub life: f32,
    pub kind: u8,
}

#[derive(Clone)]
pub struct Snapshot {
    pub players: Vec<PlayerState>,
    pub enemies: Vec<EnemyState>,
    pub weather: Weather,
    pub tick: u64,
    pub remaining: u32,
    pub complete: bool,
}

pub struct Sim {
    pub world: World,
    pub players: Vec<PlayerState>,
    pub enemies: Vec<EnemyState>,
    enemy_hp: Vec<f32>,
    atk_cd: Vec<f32>,
    pub remaining: u32,
    pub complete: bool,
    pub particles: Vec<Particle>,
    diff: Diff,
    weather: Weather,
    wleft: f32,
    rng: u64,
    tick: u64,
    pub hit_landed: Arc<AtomicBool>,
}

impl Sim {
    pub fn new(seed: u64, theme: Theme, diff: Diff) -> Self {
        let world = World::build(seed, theme);
        let mut s = Self {
            world,
            players: Vec::new(),
            enemies: Vec::new(),
            enemy_hp: Vec::new(),
            atk_cd: Vec::new(),
            remaining: 0,
            complete: false,
            particles: Vec::new(),
            diff,
            weather: Weather::Clear,
            wleft: 12.0,
            rng: 0x9E3779B97F4A7C15,
            tick: 0,
            hit_landed: Arc::new(AtomicBool::new(false)),
        };
        s.spawn_enemies();
        s
    }

    pub fn add_player(&mut self) -> usize {
        let n = self.players.len();
        let (x, y) = self.spawn_point(n as i32);
        self.players.push(PlayerState {
            x,
            y,
            angle: 0.0,
            hp: MAX_HP,
            score: 0,
            kills: 0,
            dead_timer: 0.0,
            dmg_dir: 0.0,
            dmg_t: 0.0,
        });
        n
    }

    fn spawn_point(&self, i: i32) -> (f64, f64) {
        match i.rem_euclid(4) {
            0 => (40.5, 40.5),
            1 => (39.5, 40.5),
            2 => (40.5, 39.5),
            3 => (39.5, 39.5),
            _ => (40.5, 40.5),
        }
    }

    fn rnd(&mut self) -> f64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.rng >> 33) as f64) / (u64::MAX >> 33) as f64
    }

    fn clear_at(&self, x: f64, y: f64) -> bool {
        let b = self.world.at(x as i32, y as i32, 1);
        b == 0 || b == crate::world::I_GRASS || b == crate::world::I_PLAZA
    }

    fn enemy_pass(&self, x: f64, y: f64) -> bool {
        if !free_pos(&self.world, x, y) {
            return false;
        }
        let b = self.world.at(x as i32, y as i32, 1);
        b == 0 || b == crate::world::I_GRASS || b == crate::world::I_PLAZA
    }

    fn spawn_enemies(&mut self) {
        let mut count: u32 = 0;
        for _ in 0..self.diff.enemy_count() {
            self.spawn_one(self.diff.enemy_hp());
            count += 1;
        }
        self.remaining = count;
    }

    fn spawn_one(&mut self, hp: f32) {
        for _ in 0..500 {
            let x = 5.0 + self.rnd() * (WH as f64 - 10.0);
            let y = 5.0 + self.rnd() * (WH as f64 - 10.0);
            if ((x - 40.0).powi(2) + (y - 40.0).powi(2)).sqrt() < 7.0 || !self.clear_at(x, y) {
                continue;
            }
            self.enemies.push(EnemyState { x, y, alive: true });
            self.enemy_hp.push(hp);
            self.atk_cd.push(0.0);
            return;
        }
    }

    fn clear_line(&self, x: f64, y: f64, a: f64, d: f64) -> bool {
        let (co, si) = (a.cos(), a.sin());
        let mut t = 0.3;
        while t < d {
            let ix = (x + co * t) as i32;
            let iy = (y + si * t) as i32;
            if self.world.at(ix, iy, 3) != 0 {
                return false;
            }
            t += 0.3;
        }
        true
    }

    fn burst_particles(&mut self, x: f64, y: f64, n: usize, speed: f64, kind: u8) {
        for _ in 0..n {
            if self.particles.len() >= 512 {
                return;
            }
            let r = self.rnd();
            let r2 = self.rnd();
            let angle = r * std::f64::consts::TAU;
            let v = speed * (0.2 + r2);
            let vz = 1.0 + self.rnd() * 2.5;
            let z = 0.3 + self.rnd() * 0.7;
            let life = 0.35 + self.rnd() as f32 * 0.6;
            let vx = angle.cos() * v;
            let vy = angle.sin() * v;
            self.particles.push(Particle {
                x,
                y,
                vz,
                vx,
                vy,
                z,
                life,
                kind,
            });
        }
    }

    fn shoot(&mut self, shooter: usize) {
        let p = self.players[shooter];
        let (co, si) = (p.angle.cos(), p.angle.sin());
        let mut best: Option<(f64, usize)> = None;
        for (i, e) in self.enemies.iter().enumerate() {
            if !e.alive {
                continue;
            }
            let dx = e.x - p.x;
            let dy = e.y - p.y;
            let along = dx * co + dy * si;
            let lat = (dx * si - dy * co).abs();
            if along < 0.3 || along > 13.0 || lat > 0.9 {
                continue;
            }
            if !self.clear_line(p.x, p.y, p.angle, along - 0.3) {
                continue;
            }
            if best.map_or(true, |(b, _)| along < b) {
                best = Some((along, i));
            }
        }
        if let Some((_, i)) = best {
            self.enemy_hp[i] -= 34.0;
            let ex = self.enemies[i].x;
            let ey = self.enemies[i].y;
            self.burst_particles(ex, ey, 3, 90.0, 0);
            if self.enemy_hp[i] <= 0.0 {
                self.enemies[i].alive = false;
                self.players[shooter].score += 100;
                self.players[shooter].kills += 1;
                self.burst_particles(ex, ey, 18, 240.0, 0);
                self.burst_particles(ex, ey, 6, 320.0, 1);
                self.remaining = self.remaining.saturating_sub(1);
                if self.remaining == 0 {
                    self.complete = true;
                }
            }
            self.hit_landed.store(true, Ordering::Relaxed);
        }
    }

    pub fn apply_player(&mut self, i: usize, mask: u16, angle: f64, shoot: bool, dt: f64) {
        self.players[i].angle = angle;
        let p = self.players[i];
        if p.dead_timer > 0.0 {
            self.players[i].dead_timer -= dt as f32;
            if self.players[i].dead_timer <= 0.0 {
                let (x, y) = self.spawn_point(i as i32);
                self.players[i].x = x;
                self.players[i].y = y;
                self.players[i].hp = MAX_HP;
            }
            return;
        }

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
            let nx = p.x + wx * s * SPD * dt;
            let ny = p.y + wy * s * SPD * dt;
            if free_pos(&self.world, nx, p.y) {
                self.players[i].x = nx;
            }
            if free_pos(&self.world, p.x, ny) {
                self.players[i].y = ny;
            }
        }

        if shoot {
            self.shoot(i);
        }
    }

    pub fn tick(&mut self, dt: f64) {
        self.tick += 1;

        for p in self.players.iter_mut() {
            if p.dmg_t > 0.0 {
                p.dmg_t -= dt as f32;
            }
            if p.dmg_t < 0.0 {
                p.dmg_t = 0.0;
            }
        }

        // particles (blood/gib spray)
        for pt in self.particles.iter_mut() {
            pt.life -= dt as f32;
            pt.x += pt.vx * dt;
            pt.y += pt.vy * dt;
            pt.z += pt.vz * dt;
            pt.vx *= 0.86;
            pt.vy *= 0.86;
            pt.vz -= 6.0 * dt;
            if pt.z < 0.15 {
                pt.z = 0.15;
                pt.vz = -pt.vz * 0.3;
            }
        }
        self.particles.retain(|p| p.life > 0.0);

        // weather
        self.wleft -= dt as f32;
        if self.wleft <= 0.0 {
            let n = (self.rnd() * 100.0) as u32;
            let w = if n < 45 {
                Weather::Clear
            } else if n < 72 {
                Weather::Rain
            } else if n < 80 {
                Weather::Snow
            } else {
                Weather::Fog
            };
            self.weather = match self.world.theme {
                Theme::Frozen | Theme::Valley => w,
                _ => {
                    if w == Weather::Snow {
                        Weather::Fog
                    } else {
                        w
                    }
                }
            };
            self.wleft = 8.0 + (self.rnd() * 12.0) as f32;
        }

        // enemies: always hunt the nearest living player
        for i in 0..self.enemies.len() {
            if !self.enemies[i].alive {
                continue;
            }
            // nearest living player
            let mut tgt = 0usize;
            let mut bd = f64::MAX;
            for (pi, p) in self.players.iter().enumerate() {
                if p.dead_timer > 0.0 {
                    continue;
                }
                let d = (p.x - self.enemies[i].x).powi(2) + (p.y - self.enemies[i].y).powi(2);
                if d < bd {
                    bd = d;
                    tgt = pi;
                }
            }
            let p = self.players[tgt];
            let (ex, ey) = (self.enemies[i].x, self.enemies[i].y);
            let dist = (bd.sqrt()).max(0.001);
            let (dx, dy) = (p.x - ex, p.y - ey);
            let (ux, uy) = (dx / dist, dy / dist);

            // attack when in melee range
            if dist <= 1.1 && p.dead_timer == 0.0 {
                self.atk_cd[i] -= dt as f32;
                if self.atk_cd[i] <= 0.0 {
                    self.atk_cd[i] = 1.0;
                    if self.players[tgt].hp > 0.0 {
                        self.players[tgt].hp -= self.diff.enemy_dmg();
                        self.players[tgt].dmg_dir = (ey - self.players[tgt].y).atan2(ex - self.players[tgt].x);
                        self.players[tgt].dmg_t = 1.5;
                        if self.players[tgt].hp <= 0.0 {
                            self.players[tgt].hp = 0.0;
                            self.players[tgt].dead_timer = 2.0;
                        }
                    }
                }
                continue;
            }

            let sp = self.diff.enemy_speed() * dt;
            let (nx, ny) = (ex + ux * sp, ey + uy * sp);
            if self.enemy_pass(nx, ey) {
                self.enemies[i].x = nx;
            } else if self.enemy_pass(ex, ny) {
                self.enemies[i].y = ny;
            } else if self.enemy_pass(nx, ny) {
                self.enemies[i].x = nx;
                self.enemies[i].y = ny;
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            players: self.players.clone(),
            enemies: self.enemies.clone(),
            weather: self.weather,
            tick: self.tick,
            remaining: self.remaining,
            complete: self.complete,
        }
    }
}