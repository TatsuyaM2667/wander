use crate::world::World;

pub const FOV: f64 = 0.66;

#[derive(Clone, Copy)]
pub struct Cam {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub dx: f64,
    pub dy: f64,
    pub px: f64,
    pub py: f64,
}

impl Cam {
    pub fn set_angle(&mut self, a: f64) {
        self.dx = a.cos();
        self.dy = a.sin();
        self.px = -a.sin() * FOV;
        self.py = a.cos() * FOV;
    }

    #[allow(dead_code)]
    pub fn rot(&mut self, a: f64) {
        let (c, sn) = (a.cos(), a.sin());
        let (odx, ody) = (self.dx, self.dy);
        self.dx = odx * c - ody * sn;
        self.dy = odx * sn + ody * c;
        let (opx, opy) = (self.px, self.py);
        self.px = opx * c - opy * sn;
        self.py = opx * sn + opy * c;
    }

    #[allow(dead_code)]
    pub fn mv(&mut self, w: &World, mx: f64, my: f64) {
        let nx = self.x + mx;
        let ny = self.y + my;
        if free_pos(w, nx, self.y) {
            self.x = nx;
        }
        if free_pos(w, self.x, ny) {
            self.y = ny;
        }
    }
}

pub fn free_pos(w: &World, x: f64, y: f64) -> bool {
    const SAMPLES: [(f64, f64); 9] = [
        (0.0, 0.0),
        (0.34, 0.0),
        (-0.34, 0.0),
        (0.0, 0.34),
        (0.0, -0.34),
        (0.24, 0.24),
        (-0.24, -0.24),
        (-0.24, 0.24),
        (0.24, -0.24),
    ];
    for (ox, oy) in SAMPLES {
        let ix = (x + ox) as i32;
        let iy = (y + oy) as i32;
        for z in 2..=4 {
            if w.at(ix, iy, z) != 0 {
                return false;
            }
        }
    }
    true
}