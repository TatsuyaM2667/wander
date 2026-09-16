use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::sim::{Diff, EnemyState, PlayerState, Sim, Snapshot};
use crate::world::{Theme, Weather};

pub const DEFAULT_ADDR: &str = "127.0.0.1:4242";
const MAX_CLIENTS: usize = 8;

#[derive(Clone)]
pub struct Hello {
    pub seed: u64,
    pub theme: Theme,
    pub player_id: usize,
    pub x: f64,
    pub y: f64,
    pub angle: f64,
}

pub struct NetState {
    pub hello: Option<Hello>,
    pub snap: Option<Snapshot>,
    pub err: Option<String>,
}

pub struct NetClient {
    pub state: Arc<Mutex<NetState>>,
    stream: Arc<Mutex<TcpStream>>,
    last_send: std::time::Instant,
    connected: bool,
}

// ---------------------------------------------------------------------------
// wire encoding
// ---------------------------------------------------------------------------

fn le_u16(v: u16, o: &mut Vec<u8>) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn le_u32(v: u32, o: &mut Vec<u8>) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn le_u64(v: u64, o: &mut Vec<u8>) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn le_f32(v: f32, o: &mut Vec<u8>) {
    o.extend_from_slice(&v.to_le_bytes());
}

fn encode_packet(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 2);
    le_u16(payload.len() as u16, &mut out);
    out.extend_from_slice(payload);
    out
}

fn parse_u16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn parse_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn parse_u64(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}
fn parse_f32(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

// ---------------------------------------------------------------------------
// client
// ---------------------------------------------------------------------------

impl NetClient {
    pub fn connect(addr: &str) -> Result<Self, String> {
        let stream = TcpStream::connect(addr).map_err(|e| e.to_string())?;
        let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
        let state = Arc::new(Mutex::new(NetState {
            hello: None,
            snap: None,
            err: None,
        }));
        let arc = Arc::clone(&state);
        let mut stream_r = stream.try_clone().map_err(|e| e.to_string())?;
        thread::spawn(move || {
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                match stream_r.read(&mut tmp) {
                    Ok(0) | Err(_) => {
                        arc.lock().unwrap().err = Some("disconnected".into());
                        return;
                    }
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        loop {
                            if buf.len() < 2 {
                                break;
                            }
                            let plen = parse_u16(&buf) as usize;
                            if buf.len() < 2 + plen {
                                break;
                            }
                            let payload = buf[2..2 + plen].to_vec();
                            buf.drain(..2 + plen);
                            Self::handle_packet(&arc, &payload);
                        }
                    }
                }
            }
        });
        let stream = Arc::new(Mutex::new(stream));
        Ok(Self {
            state,
            stream,
            last_send: std::time::Instant::now(),
            connected: true,
        })
    }

    fn handle_packet(state: &Arc<Mutex<NetState>>, p: &[u8]) {
        if p.is_empty() {
            return;
        }
        match p[0] {
            10 if p.len() >= 26 => {
                let seed = parse_u64(&p[1..9]);
                let theme = match p[9] {
                    0 => Theme::Valley,
                    1 => Theme::Desert,
                    2 => Theme::Frozen,
                    3 => Theme::Tropical,
                    _ => Theme::Forest,
                };
                let id = parse_u32(&p[10..14]) as usize;
                let x = parse_f32(&p[14..18]) as f64;
                let y = parse_f32(&p[18..22]) as f64;
                let angle = parse_f32(&p[22..26]) as f64;
                state.lock().unwrap().hello = Some(Hello {
                    seed,
                    theme,
                    player_id: id,
                    x,
                    y,
                    angle,
                });
            }
            11 => {
                // Snapshot
                if p.len() < 9 {
                    return;
                }
                let weather = match p[6] {
                    1 => Weather::Rain,
                    2 => Weather::Snow,
                    3 => Weather::Fog,
                    _ => Weather::Clear,
                };
                let npl = p[7] as usize;
                let nen = p[8] as usize;
                // per-player: x,y,angle,hp (4 f32 = 16) + score,kills (2 u32 = 8)
                //            + dmg_t,dmg_dir (2 f32 = 8)  -> 32 bytes
                // per-enemy: x,y (8) + alive (1) = 9 bytes
                // trailing: remaining u32 + complete u8
                let need = 9 + npl * 32 + nen * 9 + 5;
                if p.len() < need {
                    return;
                }
                let mut players = Vec::with_capacity(npl);
                for i in 0..npl {
                    let o = 9 + i * 32;
                    players.push(PlayerState {
                        x: parse_f32(&p[o..o + 4]) as f64,
                        y: parse_f32(&p[o + 4..o + 8]) as f64,
                        angle: parse_f32(&p[o + 8..o + 12]) as f64,
                        hp: parse_f32(&p[o + 12..o + 16]),
                        score: parse_u32(&p[o + 16..o + 20]),
                        kills: parse_u32(&p[o + 20..o + 24]),
                        dmg_t: parse_f32(&p[o + 24..o + 28]),
                        dmg_dir: parse_f32(&p[o + 28..o + 32]) as f64,
                        dead_timer: 0.0,
                    });
                }
                let mut enemies = Vec::with_capacity(nen);
                for i in 0..nen {
                    let o = 9 + npl * 32 + i * 9;
                    enemies.push(EnemyState {
                        x: parse_f32(&p[o..o + 4]) as f64,
                        y: parse_f32(&p[o + 4..o + 8]) as f64,
                        alive: p[o + 8] != 0,
                    });
                }
                let to = 9 + npl * 32 + nen * 9;
                let remaining = parse_u32(&p[to..to + 4]);
                let complete = p[to + 4] != 0;
                state.lock().unwrap().snap = Some(Snapshot {
                    players,
                    enemies,
                    weather,
                    tick: parse_u32(&p[1..5]) as u64,
                    remaining,
                    complete,
                });
            }
            _ => {}
        }
    }

    pub fn join(&mut self, seed: u64, theme: Theme) {
        let mut p = vec![3u8];
        le_u64(seed, &mut p);
        p.push(match theme {
            Theme::Valley => 0,
            Theme::Desert => 1,
            Theme::Frozen => 2,
            Theme::Tropical => 3,
            Theme::Forest => 4,
        });
        let _ = self.send(&p);
    }

    pub fn wait_hello(&mut self, timeout_ms: u128) -> bool {
        let start = std::time::Instant::now();
        while std::time::Instant::now().duration_since(start).as_millis() < timeout_ms {
            if let Some(h) = self.state.lock().unwrap().hello.clone() {
                let _ = h;
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    pub fn local_hello(&self) -> Option<Hello> {
        self.state.lock().unwrap().hello.clone()
    }

    pub fn get_snapshot(&self) -> Option<Snapshot> {
        self.state.lock().unwrap().snap.clone()
    }

    pub fn send_input(&mut self, mask: u16, angle: f64, shoot: bool) {
        if !self.connected {
            return;
        }
        if self.last_send.elapsed() < Duration::from_millis(33) && !shoot {
            return;
        }
        self.last_send = std::time::Instant::now();
        let mut p = vec![1u8];
        le_u16(mask, &mut p);
        le_f32(angle as f32, &mut p);
        p.push(shoot as u8);
        let _ = self.send(&p);
    }

    fn send(&mut self, payload: &[u8]) -> std::io::Result<()> {
        let enc = encode_packet(payload);
        let mut s = self.stream.lock().unwrap();
        s.write_all(&enc)
    }
}

// ---------------------------------------------------------------------------
// server
// ---------------------------------------------------------------------------

struct HostConn {
    stream: TcpStream,
    input: Mutex<Option<(u16, f64, bool)>>,
}

pub fn spawn_host(seed: u64, theme: Theme, port: u16, diff: Diff) -> Result<SocketAddr, String> {
    let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;

    thread::spawn(move || {
        let mut sim = Sim::new(seed, theme, diff);
        let conns: Arc<Mutex<HashMap<usize, HostConn>>> = Arc::new(Mutex::new(HashMap::new()));
        let mut last_tick = std::time::Instant::now();
        loop {
            // accept
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let mut locked = conns.lock().unwrap();
                        let idx = if locked.len() < MAX_CLIENTS {
                            Some(sim.add_player())
                        } else {
                            None
                        };
                        let Some(idx) = idx else {
                            continue;
                        };
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(40)));
                        let mut hc = HostConn {
                            stream: stream.try_clone().unwrap(),
                            input: Mutex::new(None),
                        };
                        // send Hello
                        let p0 = sim.players[idx];
                        let mut p = vec![10u8];
                        le_u64(seed, &mut p);
                        p.push(match theme {
                            Theme::Valley => 0,
                            Theme::Desert => 1,
                            Theme::Frozen => 2,
                            Theme::Tropical => 3,
                            Theme::Forest => 4,
                        });
                        le_u32(idx as u32, &mut p);
                        le_f32(p0.x as f32, &mut p);
                        le_f32(p0.y as f32, &mut p);
                        le_f32(p0.angle as f32, &mut p);
                        let _ = hc.stream.write_all(&encode_packet(&p));
                        locked.insert(idx, hc);
                        drop(locked);

                        // reader thread
                        let mut stream_r = stream;
                        let shared = Arc::clone(&conns);
                        std::thread::spawn(move || {
                            let mut buf = Vec::new();
                            let mut tmp = [0u8; 4096];
                            loop {
                                match stream_r.read(&mut tmp) {
                                    Ok(0) | Err(_) => return,
                                    Ok(n) => {
                                        buf.extend_from_slice(&tmp[..n]);
                                        loop {
                                            if buf.len() < 2 {
                                                break;
                                            }
                                            let plen = parse_u16(&buf) as usize;
                                            if buf.len() < 2 + plen {
                                                break;
                                            }
                                            let payload = buf[2..2 + plen].to_vec();
                                            buf.drain(..2 + plen);
                                            if payload[0] == 1 && payload.len() >= 7 {
                                                let mask = parse_u16(&payload[1..3]);
                                                let angle = parse_f32(&payload[3..7]) as f64;
                                                let shoot =
                                                    payload.get(7).copied().unwrap_or(0) != 0;
                                                if let Some(c) =
                                                    shared.lock().unwrap().get_mut(&idx)
                                                {
                                                    *c.input.lock().unwrap() =
                                                        Some((mask, angle, shoot));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Err(_) => break,
                }
            }

            let now = std::time::Instant::now();
            let elapsed = (now - last_tick).as_secs_f64();
            if elapsed >= 1.0 / 30.0 {
                let dt = elapsed.min(0.1);
                last_tick = now;
                {
                    let locked = conns.lock().unwrap();
                    let mut keys: Vec<usize> = locked.keys().copied().collect();
                    keys.sort_unstable();
                    for idx in &keys {
                        let inp = locked.get(idx).unwrap().input.lock().unwrap().take();
                        let (mask, angle, shoot) = inp.unwrap_or((0, 0.0, false));
                        sim.apply_player(*idx, mask, angle, shoot, dt);
                    }
                }
                sim.tick(dt);
                let snap = sim.snapshot();
                let mut payload = vec![11u8];
                le_u32(snap.tick as u32, &mut payload);
                payload.push(match snap.weather {
                    Weather::Clear => 0,
                    Weather::Rain => 1,
                    Weather::Snow => 2,
                    Weather::Fog => 3,
                });
                payload.push(snap.players.len() as u8);
                payload.push(snap.enemies.len() as u8);
                for pl in &snap.players {
                    le_f32(pl.x as f32, &mut payload);
                    le_f32(pl.y as f32, &mut payload);
                    le_f32(pl.angle as f32, &mut payload);
                    le_f32(pl.hp, &mut payload);
                    le_u32(pl.score, &mut payload);
                    le_u32(pl.kills, &mut payload);
                    le_f32(pl.dmg_t, &mut payload);
                    le_f32(pl.dmg_dir as f32, &mut payload);
                }
                for e in &snap.enemies {
                    le_f32(e.x as f32, &mut payload);
                    le_f32(e.y as f32, &mut payload);
                    payload.push(e.alive as u8);
                }
                le_u32(snap.remaining, &mut payload);
                payload.push(snap.complete as u8);
                let enc = encode_packet(&payload);
                let mut locked = conns.lock().unwrap();
                locked.retain(|_, c| c.stream.write_all(&enc).is_ok());
            } else {
                thread::sleep(Duration::from_millis(2));
            }
        }
    });
    Ok(addr)
}
