# WANDER

**A real-time 3D FPS that runs entirely inside your terminal.**

![status](https://img.shields.io/badge/status-active-brightgreen)

WANDER is a raycasted 3D first-person shooter rendered with Unicode half-block
pixels (mosaic-style "Zig" rendering). No window, no GPU — just a terminal.
Explore procedurally generated cities, fight hunting bot enemies, survive
dynamic weather, and even play together with friends over TCP.

---

## Features

- **True 3D in a terminal** — DDA raycasting + per-cell half-block rasterization
  (`terminal-pixel-animation`), with a depth buffer for correct sprite occlusion.
- **5 themed worlds** — Green Valley, Desert Oasis, Frozen Peaks, Tropical
  Island, Dark Forest. Each has its own color palette and block set.
- **Procedural cities** — roads, sidewalks, parks, ponds, plazas and varied
  buildings (glass towers, brick, concrete, timber).
- **Live weather** — Clear / Rain / Snow / Fog roll in randomly and change
  visibility, with animated precipitation.
- **Real FPS gameplay** — shoot with a gun viewmodel, muzzle flash, crosshair,
  HP bar, score, hit feedback. Bots chase and attack you.
- **Difficulty 1–10** — affects enemy count, speed, HP and damage.
- **Single player & multiplayer** — host a game (TCP) or join a friend's; bots
  and weather are simulated server-side.

## Screenshot

```
 ████████                  ████████████████           ████████
  ████████    (in-game)     ████████████████            ████████
   ████████    FPS view      ████████████████             ████████
```

(Real rendering is richly colored — try it!)

## Requirements

- **Terminal**: run with TrueColor (24-bit) support.
  Recommended: kitty, alacritty, Windows Terminal, GNOME Terminal.
- **Platform**: Linux / macOS / Windows (tested primarily on Linux).
- **Rust toolchain** (edition 2024).

## Getting Started

```sh
git clone https://github.com/<your-username>/wander.git
cd wander
cargo run --release
```

> **Note**: WANDER takes over the terminal (alternate screen + raw mode).
> Press <kbd>Esc</kbd> (or <kbd>Q</kbd>) to leave menus; <kbd>Ctrl+C</kbd> to quit.

### Controls

| Key | Action |
| --- | --- |
| `W` / `↑` | Move forward |
| `S` / `↓` | Move backward |
| `A` / `D` | Strafe |
| `←` `→` | Turn |
| `Space` / `Enter` | Shoot |
| `Esc` / `Q` | Back / Quit to menu |

### Menu Flow

```
Title ──▶ Mode ──┬─ Single Player ─▶ World ─▶ Difficulty ─▶ Play
                 ├─ Host Game    ─▶ World ─▶ Difficulty ─▶ Play (LAN)
                 └─ Join Game    ─▶ Address ─▶ Play
```

## Difficulty

| Level | Enemies | Speed | Enemy HP | Damage | Tier |
| ----- | ------- | ----- | -------- | ------ | ---- |
| 1–2   | 5–6     | ~1.1  | 74–88    | ~5–6   | Casual |
| 3–4   | 7–8     | ~1.4  | 102–116  | ~6–7   | Easy |
| 5–6   | 9–10    | ~1.6  | 130–144  | ~8–9   | Normal |
| 7–8   | 11–12   | ~1.8  | 158–172  | ~10    | Hard |
| 9–10  | 13–14   | ~2.1  | 186–200  | ~11–12 | Nightmare |

Difficulty is only selectable for **Single Player** and **Host** (the host's
difficulty governs all connected clients).

## Multiplayer

- **Host**: choose *Host Game* and pick a world + difficulty. WANDER listens on
  port `4242`.
- **Join**: choose *Join Game*, enter the host's `ip:port` (e.g.
  `192.168.1.20:4242`), and play.

The host is authoritative: it simulates bots/weather/shooting and streams
snapshots to clients. Clients use simple prediction with server correction.

## How It Works

- `world.rs` — procedural terrain: grids, roads, buildings, parks, water.
- `camera.rs` — camera state + cylinder collision (`free_pos`).
- `render.rs` — DDA raycasting, sky/fog, depth buffer, sprites, gun overlay,
  minimap with enemy/player dots.
- `sim.rs` — authoritative simulation: movement, bot AI, combat, weather,
  difficulty, respawns.
- `net.rs` — TCP server/client wire protocol (Hello handshake + `Snapshot`
  stream).

## Project Layout

```
src/
├── main.rs        # raw-mode loop, event polling
├── ui.rs          # menu state machine, HUD, game screen
├── world.rs       # themes, palettes, world generation
├── camera.rs      # camera + collision
├── render.rs      # raycaster + sprites + overlay + minimap
├── sim.rs         # game simulation + difficulty
└── net.rs         # multiplayer host/client
```

## Roadmap

- [ ] Sound effects (terminal beeps / OSC)
- [ ] More weapons
- [ ] Day / night cycle
- [ ] Save & load
- [ ] WebAssembly build (browser terminal)

## License

MIT