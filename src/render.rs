use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::camera::Cam;
use crate::sim::{EnemyState, PlayerState};
use crate::world::{I_GLASS, Palette, Weather, World};

pub fn cl(v: f64) -> u8 {
    v.max(0.0).min(255.0) as u8
}

pub fn lerp(a: (f64, f64, f64), b: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
    let t = t.max(0.0).min(1.0);
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

fn mix(c: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
    (c.0 * t, c.1 * t, c.2 * t)
}

fn hash(x: i32, y: i32, s: u64) -> f64 {
    let mut v = (x as u64)
        .wrapping_mul(374761393)
        .wrapping_add(y as u64)
        .wrapping_mul(668265263)
        .wrapping_add(s);
    v ^= v >> 13;
    v = v.wrapping_mul(1274126177);
    v ^= v >> 16;
    (v as f64) / (u64::MAX as f64)
}

fn sky_weather(c: (f64, f64, f64), weather: Weather) -> (f64, f64, f64) {
    match weather {
        Weather::Clear => c,
        Weather::Rain => lerp(c, (105.0, 115.0, 130.0), 0.55),
        Weather::Snow => lerp(c, (225.0, 232.0, 240.0), 0.55),
        Weather::Fog => lerp(c, (190.0, 200.0, 205.0), 0.45),
    }
}

// ---------------------------------------------------------------------------
// main scene
// ---------------------------------------------------------------------------

pub fn render_scene(
    buf: &mut [u8],
    rw: usize,
    rh: usize,
    cam: &Cam,
    w: &World,
    pal: &Palette,
    weather: Weather,
    frame: u64,
    players: &[PlayerState],
    enemies: &[EnemyState],
    self_id: usize,
    flash: f32,
) {
    for b in buf.iter_mut() {
        *b = 0;
    }
    let mut depth = vec![9999.0f32; rw * rh];
    let hw = (rh as f64) * 0.5;
    let proj = hw;

    let vis: f64 = match weather {
        Weather::Fog => 11.0,
        Weather::Rain => 22.0,
        _ => 34.0,
    };
    let sky_fog_floor: f64 = match weather {
        Weather::Fog => 0.45,
        _ => 0.0,
    };
    let precip = match weather {
        Weather::Rain => Some((198.0, 212.0, 232.0, 1i32, 0.5)),
        Weather::Snow => Some((238.0, 245.0, 250.0, 0i32, 0.6)),
        _ => None,
    };

    let sky_h = sky_weather(pal.sky_horizon, weather);
    let sky_z = sky_weather(pal.sky_zenith, weather);

    for sx in 0..rw {
        let cx = 2.0 * sx as f64 / rw as f64 - 1.0;
        let rdx = cam.dx + cam.px * cx;
        let rdy = cam.dy + cam.py * cx;

        let mut mx = cam.x as i32;
        let mut my = cam.y as i32;
        let ddx = if rdx.abs() < 1e-10 {
            1e30
        } else {
            (1.0 / rdx).abs()
        };
        let ddy = if rdy.abs() < 1e-10 {
            1e30
        } else {
            (1.0 / rdy).abs()
        };
        let stepx: i32;
        let stepy: i32;
        let mut sdx: f64;
        let mut sdy: f64;
        if rdx < 0.0 {
            stepx = -1;
            sdx = (cam.x - mx as f64) * ddx;
        } else {
            stepx = 1;
            sdx = (mx as f64 + 1.0 - cam.x) * ddx;
        }
        if rdy < 0.0 {
            stepy = -1;
            sdy = (cam.y - my as f64) * ddy;
        } else {
            stepy = 1;
            sdy = (my as f64 + 1.0 - cam.y) * ddy;
        }

        let mut hit = 0u8;
        let mut wall_top = 0i32;
        let mut side = 0i32;
        for _ in 0..128 {
            let top = (1..=crate::world::WD as i32)
                .rev()
                .find(|&z| w.at(mx, my, z) > 0)
                .unwrap_or(0);
            if top >= cam.z as i32 {
                hit = w.at(mx, my, top);
                wall_top = top;
                break;
            }
            if sdx < sdy {
                sdx += ddx;
                mx += stepx;
                side = 0;
            } else {
                sdy += ddy;
                my += stepy;
                side = 1;
            }
        }

        let (dstart, dend, wd);
        if hit == 0 {
            dstart = hw as usize;
            dend = hw as usize - 1;
            wd = 999.0;
        } else {
            let wdist = if side == 0 {
                (mx as f64 - cam.x + (1 - stepx) as f64 / 2.0) / rdx
            } else {
                (my as f64 - cam.y + (1 - stepy) as f64 / 2.0) / rdy
            };
            wd = wdist.max(0.001);
            let lh_top = ((wall_top as f64 + 1.0 - cam.z) / wd * proj) as i64;
            let lh_bot = ((cam.z - 1.0) / wd * proj) as i64;
            dstart = (hw as i64 - lh_top).clamp(0, rh as i64) as usize;
            dend = (hw as i64 + lh_bot).clamp(0, rh as i64 - 1) as usize;
        }

        // SKY
        for dy in 0..dstart {
            let denom = (hw - dy as f64).max(1.0);
            let rowdist = hw / denom;
            let fog = (1.0 - (1.0 / rowdist.max(0.1)).min(1.0)).max(sky_fog_floor);
            let t = (hw - dy as f64) / hw;
            let local = lerp(sky_h, sky_z, t);
            let (r, g, b) = lerp(local, sky_h, fog * 0.85);
            let idx = (dy * rw + sx) * 3;
            buf[idx] = cl(r);
            buf[idx + 1] = cl(g);
            buf[idx + 2] = cl(b);
            depth[dy * rw + sx] = 1000.0;
        }

        // WALL
        if hit > 0 {
            let wf = {
                let wp = if side == 0 {
                    cam.y + wd * rdy
                } else {
                    cam.x + wd * rdx
                };
                wp - wp.floor()
            };
            let tint = if wf < 0.33 || wf > 0.66 { 0 } else { 1 };
            let fog = (wd / vis).min(1.0).max(sky_fog_floor * 0.6);
            for dy in dstart..=dend {
                let mut base = pal.block(hit, tint);
                let hv = ((mx + my).rem_euclid(2)) as i64;
                if hit == I_GLASS {
                    let band = (dy as i64 / 2 + hv).rem_euclid(2);
                    base = if band == 0 {
                        mix(base, 0.6)
                    } else {
                        lerp(base, (255.0, 220.0, 140.0), 0.22)
                    };
                } else if (hit == crate::world::I_BRICK || hit == crate::world::I_CONCRETE)
                    && dy > dstart
                {
                    let lintel = (dy as i64 - dstart as i64) % 3 < 1;
                    if lintel {
                        base = mix(base, 0.7);
                    }
                }
                let (r, g, b) = lerp(base, sky_h, fog * 0.75);
                let idx = (dy * rw + sx) * 3;
                buf[idx] = cl(r);
                buf[idx + 1] = cl(g);
                buf[idx + 2] = cl(b);
                depth[dy * rw + sx] = wd as f32;
            }
        }

        // FLOOR
        for dy in (dend + 1)..rh {
            let denom = (dy as f64 - hw).max(1.0);
            let rowdist = hw / denom;
            let fog = (1.0 - (1.0 / rowdist.max(0.1)).min(1.0)).max(sky_fog_floor * 0.7);
            let wx = cam.x + rowdist * rdx;
            let wy = cam.y + rowdist * rdy;
            let gnd = pal.ground(w, wx as i32, wy as i32);
            let (r, g, b) = lerp(gnd, pal.ground_far, fog * 0.85);
            let idx = (dy * rw + sx) * 3;
            buf[idx] = cl(r);
            buf[idx + 1] = cl(g);
            buf[idx + 2] = cl(b);
            depth[dy * rw + sx] = rowdist as f32;
        }
    }

    // precipitation streaks over the sky region
    if let Some((cr, cg, cb, _len, prob)) = precip {
        let sky_rows = (hw as usize).max(4);
        for sx in 0..rw {
            if hash(sx as i32, 0, 7) < prob {
                let speed = 5.6;
                let base_y = hash(sx as i32, 1, 9);
                let y = ((base_y * sky_rows as f64 + frame as f64 * speed) as usize) % sky_rows;
                let (y2, _step) = if y + 1 < sky_rows { (y + 1, 1) } else { (y, 0) };
                let col_l = (cr, cg, cb);
                let col_d = mix(col_l, 0.85);
                for &(yy, col) in &[(y, col_d), (y2, col_l)] {
                    if yy < sky_rows {
                        let idx = (yy * rw + sx) * 3;
                        buf[idx] = cl(col.0);
                        buf[idx + 1] = cl(col.1);
                        buf[idx + 2] = cl(col.2);
                    }
                }
            }
        }
    }

    // sprites (enemies + other players), far first
    draw_sprites(buf, rw, rh, &mut depth, cam, players, enemies, self_id);

    // gun viewmodel + crosshair on top
    draw_crosshair(buf, rw, rh, flash);
    draw_gun(buf, rw, rh, flash);
}

// ---------------------------------------------------------------------------
// sprites
// ---------------------------------------------------------------------------

struct Sprite {
    x: f64,
    y: f64,
    dist: f64,
    friend: bool,
}

fn draw_sprites(
    buf: &mut [u8],
    rw: usize,
    rh: usize,
    depth: &mut [f32],
    cam: &Cam,
    players: &[PlayerState],
    enemies: &[EnemyState],
    self_id: usize,
) {
    let mut sprites: Vec<Sprite> = Vec::new();
    for (i, p) in players.iter().enumerate() {
        if i == self_id {
            continue;
        }
        sprites.push(Sprite {
            x: p.x,
            y: p.y,
            dist: (p.x - cam.x).powi(2) + (p.y - cam.y).powi(2),
            friend: true,
        });
    }
    for e in enemies {
        if !e.alive {
            continue;
        }
        sprites.push(Sprite {
            x: e.x,
            y: e.y,
            dist: (e.x - cam.x).powi(2) + (e.y - cam.y).powi(2),
            friend: false,
        });
    }
    sprites.sort_by(|a, b| b.dist.partial_cmp(&a.dist).unwrap_or(std::cmp::Ordering::Equal));

    let hw_px = rw as f64 / 2.0;
    let hw_row = rh as f64 / 2.0;

    for sp in &sprites {
        let rx = sp.x - cam.x;
        let ry = sp.y - cam.y;
        let t = rx * cam.dx + ry * cam.dy;
        if t < 0.3 {
            continue;
        }
        let l = rx * cam.px + ry * cam.py;
        let cx = l / t;
        let sx_c = (cx + 1.0) * hw_px;
        let half_w = (0.36 * hw_px / t).max(0.6);
        let x0 = (sx_c - half_w).floor() as i64;
        let x1 = (sx_c + half_w).ceil() as i64;

        let head = cam.z - 1.85;
        let y_top = hw_row + (head / t) * hw_row;
        let y_bot = hw_row + (cam.z / t) * hw_row;
        let y0 = y_top.max(0.0) as i64;
        let y1 = y_bot.min(rh as f64 - 1.0) as i64;
        if x1 < 0 || x0 >= rw as i64 || y1 < y0 {
            continue;
        }

        let midset = 0.55;
        let body: (u8, u8, u8) = if sp.friend {
            (70, 140, 230)
        } else {
            (210, 45, 45)
        };
        let dark: (u8, u8, u8) = if sp.friend {
            (35, 85, 150)
        } else {
            (120, 22, 22)
        };
        let headc: (u8, u8, u8) = if sp.friend {
            (150, 205, 255)
        } else {
            (255, 190, 110)
        };

        for sy in y0..=y1 {
            let fy = (sy as f64 - y_top) / (y_bot - y_top).max(0.001);
            let (r, g, b) = if fy < 0.16 {
                headc
            } else if fy < midset {
                body
            } else {
                dark
            };
            for sx in x0..=x1 {
                if sx < 0 || sx >= rw as i64 || sy < 0 || sy >= rh as i64 {
                    continue;
                }
                let pi = (sy as usize) * rw + (sx as usize);
                let t2 = t as f32;
                if t2 >= depth[pi] {
                    continue;
                }
                let idx = pi * 3;
                buf[idx] = r;
                buf[idx + 1] = g;
                buf[idx + 2] = b;
                depth[pi] = t2;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// gun + crosshair
// ---------------------------------------------------------------------------

fn draw_crosshair(buf: &mut [u8], rw: usize, rh: usize, _flash: f32) {
    let cx = rw as i64 / 2;
    let cy = rh as i64 / 2;
    for (dx, dy, r, g, b) in [
        (-2, 0, 0, 255, 220),
        (2, 0, 0, 255, 220),
        (0, -2, 0, 255, 220),
        (0, 2, 0, 255, 220),
        (-1, 0, 40, 200, 160),
        (1, 0, 40, 200, 160),
        (0, -1, 40, 200, 160),
        (0, 1, 40, 200, 160),
        (0, 0, 0, 255, 255),
    ] {
        let sx = cx + dx;
        let sy = cy + dy;
        if sx < 0 || sy < 0 || sx >= rw as i64 || sy >= rh as i64 {
            continue;
        }
        let idx = (sy as usize * rw + sx as usize) * 3;
        buf[idx] = r;
        buf[idx + 1] = g;
        buf[idx + 2] = b;
    }
}

fn draw_gun(buf: &mut [u8], rw: usize, rh: usize, flash: f32) {
    let gw = 13i64;
    let gh = 26i64;
    let gx0 = (rw as i64 / 2) - gw / 2;
    let gy0 = (rh as i64 - gh).max(0);
    let gx1 = gx0 + gw - 1;
    let gy1 = rh as i64 - 1;

    for sy in gy0..=gy1 {
        for sx in gx0..=gx1 {
            let idx = (sy as usize * rw + sx as usize) * 3;
            // slide (top, narrow)
            let in_slide = sx >= gx0 + 3 && sx <= gx0 + gw - 4 && sy <= gy0 + 8;
            // receiver / grip
            let inset = if sy >= gy1 - 9 { 3 } else { 1 };
            let in_body = !in_slide && sx >= gx0 + inset && sx <= gx1 - inset && sy > gy0 + 6;
            let (r, g, b): (u8, u8, u8) = if in_slide {
                if sy == gy0 {
                    (140, 122, 96)
                } else if sy == gy0 + 1 {
                    (110, 98, 82)
                } else {
                    (64, 58, 52)
                }
            } else if in_body {
                let edge = sx < gx0 + inset + 2 || sx > gx1 - inset - 2;
                if edge {
                    (44, 38, 34)
                } else if (sy - gy0) % 4 == 2 {
                    (74, 64, 56)
                } else {
                    (68, 60, 52)
                }
            } else {
                continue;
            };
            buf[idx] = r;
            buf[idx + 1] = g;
            buf[idx + 2] = b;
        }
    }

    // muzzle flash
    if flash > 0.02 {
        let mcx = rw as i64 / 2;
        let my = gy0;
        let rad = 1 + (flash * 3.0) as i64;
        for sy in (my - rad)..=(my + rad) {
            for sx in (mcx - rad)..=(mcx + rad) {
                if sx < 0 || sy < 0 || sx >= rw as i64 || sy >= rh as i64 {
                    continue;
                }
                let d = ((sx - mcx).pow(2) + (sy - my).pow(2)) as f64;
                if d > (rad * rad) as f64 {
                    continue;
                }
                let hot = d < (rad * rad / 3) as f64;
                let (r, g, b) = if hot {
                    (255, 244, 190)
                } else {
                    (255, 150, 60)
                };
                let idx = (sy as usize * rw + sx as usize) * 3;
                buf[idx] = r;
                buf[idx + 1] = g;
                buf[idx + 2] = b;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// minimap
// ---------------------------------------------------------------------------

pub struct MiniDot {
    pub x: f64,
    pub y: f64,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub fn draw_minimap(
    buf: &mut [u8],
    rw: usize,
    cam: &Cam,
    w: &World,
    dots: &[MiniDot],
) {
    let mw: usize = 22;
    let mh: usize = 22;
    let ox: usize = 2;
    let oy: usize = 2;
    let px = cam.x as i32;
    let py = cam.y as i32;
    let half_mw = mw / 2;
    let half_mh = mh / 2;
    for dy in 0..mh {
        for dx in 0..mw {
            let wx = px + dx as i32 - half_mw as i32;
            let wy = py + dy as i32 - half_mh as i32;
            let bx = ox + dx;
            let by = oy + dy;
            if bx >= rw || by * 3 + 2 >= buf.len() {
                continue;
            }
            let idx = (by * rw + bx) * 3;
            if dx == half_mw && dy == half_mh {
                buf[idx] = 0;
                buf[idx + 1] = 255;
                buf[idx + 2] = 0;
                continue;
            }
            if wx >= 0 && wx < crate::world::WW as i32 && wy >= 0 && wy < crate::world::WH as i32
            {
                let b = w.at(wx, wy, 1);
                let (r, g, bl): (u8, u8, u8) = match b {
                    crate::world::I_WATER => (40, 90, 170),
                    crate::world::I_GRASS => (70, 140, 50),
                    crate::world::I_TRUNK | crate::world::I_LEAVES => (30, 110, 40),
                    crate::world::I_BRICK => (170, 90, 70),
                    crate::world::I_WOOD => (115, 82, 55),
                    crate::world::I_CONCRETE => (140, 135, 128),
                    crate::world::I_GLASS => (100, 140, 190),
                    crate::world::I_STONE | crate::world::I_PLAZA => (150, 150, 148),
                    _ => (20, 20, 30),
                };
                buf[idx] = r;
                buf[idx + 1] = g;
                buf[idx + 2] = bl;
            } else {
                buf[idx] = 15;
                buf[idx + 1] = 15;
                buf[idx + 2] = 25;
            }
        }
    }
    let dir_len = ((cam.dx * cam.dx + cam.dy * cam.dy).sqrt()).max(0.001);
    let ndx = (cam.dx / dir_len * 3.0) as i32;
    let ndy = (cam.dy / dir_len * 3.0) as i32;
    let cx = ox as i32 + half_mw as i32 + ndx;
    let cy = oy as i32 + half_mh as i32 + ndy;
    if cx >= ox as i32 && cx < (ox + mw) as i32 && cy >= oy as i32 && cy < (oy + mh) as i32 {
        let idx = (cy as usize * rw + cx as usize) * 3;
        buf[idx] = 255;
        buf[idx + 1] = 255;
        buf[idx + 2] = 0;
    }

    // actor dots
    for d in dots {
        let mdx = (d.x - cam.x).round() as i32;
        let mdy = (d.y - cam.y).round() as i32;
        let bx = ox as i32 + half_mw as i32 + mdx;
        let by = oy as i32 + half_mh as i32 + mdy;
        if bx >= ox as i32 && bx < (ox + mw) as i32 && by >= oy as i32 && by < (oy + mh) as i32 {
            let idx = (by as usize * rw + bx as usize) * 3;
            buf[idx] = d.r;
            buf[idx + 1] = d.g;
            buf[idx + 2] = d.b;
        }
    }
}

pub struct HalfBlockCell {
    pub r_fg: u8,
    pub g_fg: u8,
    pub b_fg: u8,
    pub r_bg: u8,
    pub g_bg: u8,
    pub b_bg: u8,
}

pub fn decode_halfblock(cells: &[u8], cols: u32, rows: u32) -> Vec<HalfBlockCell> {
    let mut out = Vec::with_capacity((cols * rows) as usize);
    for cy in 0..rows {
        for cx in 0..cols {
            let idx = ((cy * cols + cx) * 6) as usize;
            if idx + 5 >= cells.len() {
                out.push(HalfBlockCell {
                    r_fg: 0,
                    g_fg: 0,
                    b_fg: 0,
                    r_bg: 0,
                    g_bg: 0,
                    b_bg: 0,
                });
                continue;
            }
            out.push(HalfBlockCell {
                r_fg: cells[idx],
                g_fg: cells[idx + 1],
                b_fg: cells[idx + 2],
                r_bg: cells[idx + 3],
                g_bg: cells[idx + 4],
                b_bg: cells[idx + 5],
            });
        }
    }
    out
}

pub fn braille_to_text(cells: &[HalfBlockCell], cols: u32) -> ratatui::text::Text<'static> {
    let mut lines = Vec::new();
    for row_cells in cells.chunks(cols as usize) {
        let spans: Vec<ratatui::text::Span<'static>> = row_cells
            .iter()
            .map(|c| {
                let fg = Color::Rgb(c.r_fg, c.g_fg, c.b_fg);
                let bg = Color::Rgb(c.r_bg, c.g_bg, c.b_bg);
                Span::styled("\u{2580}", Style::default().fg(fg).bg(bg))
            })
            .collect();
        lines.push(ratatui::text::Line::from(spans));
    }
    ratatui::text::Text::from(lines)
}