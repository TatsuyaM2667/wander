pub const WW: usize = 80;
pub const WH: usize = 80;
pub const WD: usize = 20;

#[derive(Clone, Copy, PartialEq)]
pub enum Weather {
    Clear,
    Rain,
    Snow,
    Fog,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Theme {
    Valley,
    Desert,
    Frozen,
    Tropical,
    Forest,
}

pub const I_GRASS: u8 = 1;
pub const I_DIRT: u8 = 2;
pub const I_WATER: u8 = 3;
pub const I_TRUNK: u8 = 6;
pub const I_LEAVES: u8 = 7;
pub const I_BRICK: u8 = 8;
pub const I_WOOD: u8 = 9;
pub const I_CONCRETE: u8 = 10;
pub const I_GLASS: u8 = 11;
pub const I_STONE: u8 = 12;
pub const I_PLAZA: u8 = 14;

pub const WORLDS: &[(&str, u64, &str, Theme)] = &[
    ("Green Valley", 42, "Gentle hills, scattered trees", Theme::Valley),
    ("Desert Oasis", 1337, "Dry plains with stone formations", Theme::Desert),
    ("Frozen Peaks", 2048, "High mountains, deep snow", Theme::Frozen),
    ("Tropical Island", 9999, "Warm beaches, dense jungle", Theme::Tropical),
    ("Dark Forest", 31415, "Dense canopy, narrow paths", Theme::Forest),
];

pub struct World {
    grid: [[[u8; WD]; WH]; WW],
    pub theme: Theme,
}

impl World {
    pub fn at(&self, x: i32, y: i32, z: i32) -> u8 {
        if x >= 0
            && (x as usize) < WW
            && y >= 0
            && (y as usize) < WH
            && z >= 0
            && (z as usize) < WD
        {
            self.grid[x as usize][y as usize][z as usize]
        } else {
            0
        }
    }

    pub fn build(seed: u64, theme: Theme) -> Self {
        let mut g = [[[0u8; WD]; WH]; WW];

        fn hh(x: i32, y: i32, s: u64) -> f64 {
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

        const B: i32 = 8;
        const GRID: i32 = (WW / B as usize) as i32;

        // per-supercell flags: park / plaza / street suppression on each side
        let park = |gx: i32, gy: i32| hh(gx, gy, seed + 1) < 0.16;
        let plaza = |gx: i32, gy: i32| hh(gx, gy, seed + 2) < 0.26;
        let no_hstreet = |gx: i32, gy: i32| hh(gx, gy, seed + 3) < 0.14;
        let no_vstreet = |gx: i32, gy: i32| hh(gx, gy, seed + 4) < 0.14;

        for x in 0..WW as i32 {
            for y in 0..WH as i32 {
                // forest border ring
                if x < 2 || x >= WW as i32 - 2 || y < 2 || y >= WH as i32 - 2 {
                    g[x as usize][y as usize][1] = I_GRASS;
                    g[x as usize][y as usize][2] = I_TRUNK;
                    g[x as usize][y as usize][3] = I_LEAVES;
                    g[x as usize][y as usize][4] = I_LEAVES;
                    continue;
                }

                let gx = x.div_euclid(B);
                let gy = y.div_euclid(B);
                let sx = x.rem_euclid(B);
                let sy = y.rem_euclid(B);

                let in_hband = sy >= 6;
                let in_vband = sx >= 6;
                let h_street = in_hband
                    && !(no_hstreet(gx, gy) || no_hstreet(gx, gy - 1));
                let v_street = in_vband
                    && !(no_vstreet(gx, gy) || no_vstreet(gx - 1, gy));

                if h_street || v_street {
                    continue; // open road
                }

                if park(gx, gy) {
                    // grass lawn
                    g[x as usize][y as usize][1] = I_GRASS;
                    continue;
                }
                if plaza(gx, gy) {
                    g[x as usize][y as usize][1] = I_PLAZA;
                    continue;
                }

                // building tile: 2x2 cells
                let tx = x.div_euclid(2);
                let ty = y.div_euclid(2);
                let u = hh(tx, ty, seed + 5);
                let center_d = (((gx * B + 4 - WW as i32 / 2) as f64).abs()
                    + ((gy * B + 4 - WH as i32 / 2) as f64).abs())
                    / 60.0;
                let downtown = (1.0 - center_d).max(0.0).min(1.0);

                let (mat, base): (u8, usize) = if downtown > 0.5 && u < 0.4 {
                    (I_GLASS, 10 + (hh(tx, ty, seed + 6) * 6.0) as usize)
                } else if u < 0.58 {
                    (I_CONCRETE, 6 + (hh(tx, ty, seed + 7) * 5.0) as usize)
                } else if u < 0.82 {
                    (I_BRICK, 4 + (hh(tx, ty, seed + 8) * 4.0) as usize)
                } else {
                    (I_WOOD, 3 + (hh(tx, ty, seed + 9) * 3.0) as usize)
                };

                let (lx, ly) = (x.rem_euclid(2), y.rem_euclid(2));
                let spire = mat == I_GLASS && lx == 0 && ly == 0;
                let h = if spire {
                    base + 3
                } else {
                    base
                };
                let h = h.min(WD - 1);
                let is_open = hh(tx, ty, seed + 10) < 0.22 && mat != I_GLASS;
                if is_open {
                    g[x as usize][y as usize][1] = I_GRASS;
                    continue;
                }
                for z in 1..=h {
                    g[x as usize][y as usize][z] = mat;
                }
            }
        }

        // park features: pond + trees
        for gx in 0..GRID {
            for gy in 0..GRID {
                if !park(gx, gy) {
                    continue;
                }
                let bx = gx * B;
                let by = gy * B;
                let px = bx + (hh(gx, gy, seed + 11) * 4.0) as i32 + 1;
                let py = by + (hh(gx, gy, seed + 12) * 4.0) as i32 + 1;
                for dx in 0..2 {
                    for dy in 0..2 {
                        if px + dx < WW as i32 - 2 && py + dy < WH as i32 - 2 {
                            g[(px + dx) as usize][(py + dy) as usize][1] = I_WATER;
                        }
                    }
                }
                for i in 0..5 {
                    let tx = bx + (hh(gx + i, gy, seed + 13) * 5.0) as i32 + 1;
                    let ty = by + (hh(gx, gy + i, seed + 14) * 5.0) as i32 + 1;
                    if tx >= WW as i32 - 2 || ty >= WH as i32 - 2 {
                        continue;
                    }
                    if g[tx as usize][ty as usize][1] != I_GRASS {
                        continue;
                    }
                    g[tx as usize][ty as usize][1] = I_TRUNK;
                    g[tx as usize][ty as usize][2] = I_TRUNK;
                    g[tx as usize][ty as usize][3] = I_LEAVES;
                    for dx in -1i32..=1 {
                        for dy in -1i32..=1 {
                            let lx = tx + dx;
                            let ly = ty + dy;
                            if lx > 0
                                && lx < WW as i32 - 1
                                && ly > 0
                                && ly < WH as i32 - 1
                                && g[lx as usize][ly as usize][4] == 0
                            {
                                g[lx as usize][ly as usize][4] = I_LEAVES;
                            }
                        }
                    }
                }
            }
        }

        // guarantee a clear crossroads at spawn center
        for i in 0..6i32 {
            for z in 0..WD {
                g[40][(40 - 3 + i) as usize][z] = 0;
                g[(40 - 3 + i) as usize][40][z] = 0;
            }
        }
        Self {
            grid: g,
            theme,
        }
    }
}

pub struct Palette {
    pub sky_horizon: (f64, f64, f64),
    pub sky_zenith: (f64, f64, f64),
    pub ground_far: (f64, f64, f64),
    pub road: (f64, f64, f64),
    pub sidewalk: (f64, f64, f64),
    pub ground: (f64, f64, f64),
    pub water: (f64, f64, f64),
    pub leaf: (f64, f64, f64),
    pub trunk: (f64, f64, f64),
    pub brick: (f64, f64, f64),
    pub wood: (f64, f64, f64),
    pub concrete: (f64, f64, f64),
    pub glass: (f64, f64, f64),
    pub stone: (f64, f64, f64),
}

impl Theme {
    pub fn palette(self) -> Palette {
        match self {
            Theme::Valley => Palette {
                sky_horizon: (168.0, 208.0, 232.0),
                sky_zenith: (100.0, 150.0, 222.0),
                ground_far: (150.0, 165.0, 120.0),
                road: (52.0, 55.0, 63.0),
                sidewalk: (80.0, 138.0, 50.0),
                ground: (88.0, 146.0, 58.0),
                water: (48.0, 108.0, 192.0),
                leaf: (42.0, 154.0, 56.0),
                trunk: (116.0, 76.0, 40.0),
                brick: (172.0, 92.0, 72.0),
                wood: (108.0, 76.0, 50.0),
                concrete: (166.0, 160.0, 150.0),
                glass: (122.0, 162.0, 208.0),
                stone: (132.0, 132.0, 138.0),
            },
            Theme::Desert => Palette {
                sky_horizon: (252.0, 224.0, 170.0),
                sky_zenith: (130.0, 172.0, 220.0),
                ground_far: (214.0, 188.0, 128.0),
                road: (118.0, 100.0, 74.0),
                sidewalk: (212.0, 182.0, 120.0),
                ground: (226.0, 196.0, 128.0),
                water: (60.0, 138.0, 206.0),
                leaf: (120.0, 150.0, 60.0),
                trunk: (106.0, 82.0, 52.0),
                brick: (196.0, 122.0, 86.0),
                wood: (150.0, 100.0, 62.0),
                concrete: (208.0, 175.0, 120.0),
                glass: (108.0, 152.0, 196.0),
                stone: (206.0, 186.0, 140.0),
            },
            Theme::Frozen => Palette {
                sky_horizon: (208.0, 222.0, 236.0),
                sky_zenith: (130.0, 176.0, 222.0),
                ground_far: (170.0, 185.0, 195.0),
                road: (128.0, 142.0, 158.0),
                sidewalk: (212.0, 220.0, 230.0),
                ground: (226.0, 232.0, 238.0),
                water: (120.0, 170.0, 210.0),
                leaf: (160.0, 200.0, 200.0),
                trunk: (112.0, 92.0, 70.0),
                brick: (168.0, 180.0, 190.0),
                wood: (120.0, 96.0, 72.0),
                concrete: (196.0, 206.0, 214.0),
                glass: (148.0, 196.0, 224.0),
                stone: (186.0, 196.0, 206.0),
            },
            Theme::Tropical => Palette {
                sky_horizon: (210.0, 238.0, 244.0),
                sky_zenith: (52.0, 150.0, 215.0),
                ground_far: (216.0, 198.0, 140.0),
                road: (64.0, 80.0, 76.0),
                sidewalk: (242.0, 216.0, 168.0),
                ground: (58.0, 158.0, 72.0),
                water: (40.0, 150.0, 220.0),
                leaf: (52.0, 170.0, 72.0),
                trunk: (98.0, 78.0, 48.0),
                brick: (198.0, 138.0, 92.0),
                wood: (146.0, 104.0, 62.0),
                concrete: (238.0, 214.0, 168.0),
                glass: (116.0, 184.0, 218.0),
                stone: (222.0, 210.0, 180.0),
            },
            Theme::Forest => Palette {
                sky_horizon: (150.0, 176.0, 150.0),
                sky_zenith: (62.0, 102.0, 74.0),
                ground_far: (130.0, 145.0, 112.0),
                road: (88.0, 86.0, 74.0),
                sidewalk: (96.0, 126.0, 74.0),
                ground: (76.0, 120.0, 58.0),
                water: (70.0, 120.0, 150.0),
                leaf: (40.0, 116.0, 48.0),
                trunk: (90.0, 64.0, 38.0),
                brick: (128.0, 98.0, 82.0),
                wood: (98.0, 70.0, 48.0),
                concrete: (130.0, 130.0, 124.0),
                glass: (96.0, 150.0, 158.0),
                stone: (120.0, 120.0, 116.0),
            },
        }
    }
}

impl Palette {
    pub fn block(&self, id: u8, side: i32) -> (f64, f64, f64) {
        let (r, g, b) = match id {
            I_GRASS => self.ground,
            I_DIRT => (self.ground.0 * 0.8, self.ground.1 * 0.8, self.ground.2 * 0.8),
            I_WATER => self.water,
            I_TRUNK => self.trunk,
            I_LEAVES => self.leaf,
            I_BRICK => self.brick,
            I_WOOD => self.wood,
            I_CONCRETE => self.concrete,
            I_GLASS => self.glass,
            I_STONE => self.stone,
            I_PLAZA => self.sidewalk,
            _ => (120.0, 120.0, 120.0),
        };
        let f = if side == 1 { 0.75 } else { 1.0 };
        (r * f, g * f, b * f)
    }

    pub fn ground(&self, w: &World, x: i32, y: i32) -> (f64, f64, f64) {
        match w.at(x, y, 1) {
            0 => self.road,
            I_WATER => self.water,
            I_TRUNK | I_LEAVES => self.trunk,
            I_PLAZA => self.sidewalk,
            _ => self.ground,
        }
    }
}