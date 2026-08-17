//! Voxel world: chunk storage, deterministic terrain generation, meshing, raycasting.
//! Faithful port of js/world.js per SPEC_world.md.

use crate::data::{self, ids, Biome, CHUNK, SEA, WORLD_H};
use crate::rng::{hash2, vnoise3, Noise2, Rng};
use bevy::math::Vec3;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;

pub const CHUNK_CELLS: usize = (CHUNK * CHUNK * WORLD_H) as usize; // 24576
pub const GEN_R: i32 = 17;
pub const MESH_R: i32 = 16;
pub const UNLOAD_R: i32 = 19;
pub const GEN_BUDGET: usize = 4;
pub const MESH_BUDGET: usize = 2;

pub fn cf(v: f32) -> i32 {
    (v / CHUNK as f32).floor() as i32
}
pub fn ckey(cx: i32, cz: i32) -> i64 {
    cx as i64 * 65536 + cz as i64
}
pub fn strkey(cx: i32, cz: i32) -> String {
    format!("{cx},{cz}")
}
/// Index into chunk data: (lx, y, lz) -> y*256 + lz*16 + lx
#[inline]
pub fn lidx(lx: i32, y: i32, lz: i32) -> usize {
    (y * 256 + lz * 16 + lx) as usize
}

/// Character axes (planet personality) — computed from seed.
#[derive(Clone, Copy, Debug)]
pub struct CharAxes {
    pub rugged: f32,
    pub temp: f32,
    pub wet: f32,
}

#[derive(Clone, Debug)]
pub enum Structure {
    Village {
        x: i32,
        z: i32,
        h: i32,
        huts: Vec<(i32, i32, i32)>, // (hut x, hut z, hut ground h)
    },
    Ruin {
        x: i32,
        z: i32,
        kind: u32,
        h: i32,
        seed: u32,
    },
}

/// Immutable per-world generation context.
pub struct WorldGen {
    pub seed: u32,
    pub biome: &'static Biome,
    pub noise: Noise2,
    pub axes: CharAxes,
    pub structures: Vec<Structure>,
}

impl WorldGen {
    pub fn new(seed: u32, biome: &'static Biome) -> Self {
        let mut rnd = Rng::new(seed ^ 0xA45C1);
        let axes = CharAxes {
            rugged: 0.72 + rnd.next() * 0.56,
            temp: rnd.next(),
            wet: rnd.next(),
        };
        let noise = Noise2::new(seed);
        let mut g = Self {
            seed,
            biome,
            noise,
            axes,
            structures: Vec::new(),
        };
        g.structures = g.gen_structures();
        g
    }

    pub fn sea(&self) -> i32 {
        SEA + self.biome.sea_lift
    }

    // ---------- terrain operators ----------

    fn warp_xz(&self, wx: f32, wz: f32, amount: f32) -> (f32, f32) {
        let n = &self.noise;
        (
            wx + n.fbm2(wx * 0.0021 + 7.3, wz * 0.0021 - 2.1, 3, 2.0, 0.5) * amount,
            wz + n.fbm2(wx * 0.0021 - 3.7, wz * 0.0021 + 9.1, 3, 2.0, 0.5) * amount,
        )
    }

    fn crater_field(&self, wx: f32, wz: f32, cell: i32, chance: f32, r0: f32, r1: f32, rim: f32, floor: f32) -> f32 {
        let cx = (wx / cell as f32).floor();
        let cz = (wz / cell as f32).floor();
        let mut rnd = hash2(cx as i32, cz as i32, 0xCEA7, self.seed);
        if rnd.next() > chance {
            return 0.0;
        }
        let dx = (wx - (cx + 0.15 + rnd.next() * 0.7) * cell as f32) / cell as f32;
        let dz = (wz - (cz + 0.15 + rnd.next() * 0.7) * cell as f32) / cell as f32;
        let r0 = r0 * (0.6 + rnd.next() * 0.8);
        let r1 = r1 * (0.6 + rnd.next() * 0.8);
        let d = (dx * dx + dz * dz).sqrt();
        if d > r1 {
            return 0.0;
        }
        if d < r0 {
            return -floor * (1.0 - (d / r0) * 0.25);
        }
        let t = (d - r0) / (r1 - r0).max(1e-4);
        rim * (t * std::f32::consts::PI).sin()
    }

    fn spire_field(&self, wx: f32, wz: f32, freq: f32, th: f32, gain: f32, cap: f32) -> f32 {
        let b = self.noise.fbm2(wx * freq, wz * freq, 4, 2.0, 0.5);
        let m = (b - th).max(0.0);
        (m * m * gain).min(cap)
    }

    fn hex_dome(&self, wx: f32, wz: f32, cell: f32) -> f32 {
        let q = (0.577350269 * wx - wz / 3.0) / cell;
        let r = (0.666666667 * wz) / cell;
        let hq = q.round();
        let hr = r.round();
        let d = (q - hq).abs().max((r - hr).abs()).max(((q - hq) + (r - hr)).abs());
        let b = (1.0 - d * 1.35).max(0.0);
        b * b * 18.0
    }

    fn float_island_at(&self, wx: f32, wz: f32) -> Option<(i32, i32)> {
        let n = &self.noise;
        let mask = n.fbm2(wx * 0.0045, wz * 0.0045, 4, 2.0, 0.5) * 0.5 + 0.5;
        if mask < 0.62 || mask > 0.78 {
            return None;
        }
        let body = n.fbm2(wx * 0.011 + 31.0, wz * 0.011 - 17.0, 4, 2.0, 0.5) * 0.5 + 0.5;
        let thick = ((body - 0.35).max(0.0) * 14.0 + 3.0) as i32;
        Some(((54.0 + mask * 20.0) as i32, thick))
    }

    /// Terrain height (blocks) at (wx, wz), clamped 3..=88.
    pub fn height_at(&self, wx: f32, wz: f32) -> i32 {
        let ch = self.axes;
        let rugged = ch.rugged;
        let t = self.biome.terrain;
        let n = &self.noise;
        let h = match t {
            "dunes" => {
                let (q0, q1) = self.warp_xz(wx, wz, 210.0);
                let base = n.fbm2(q0 * 0.0052, q1 * 0.0052, 5, 2.0, 0.5) * 0.5 + 0.5;
                let ripple = (wx * 0.016 + n.fbm2(wx * 0.004, wz * 0.004, 3, 2.0, 0.5) * 2.6).sin();
                SEA as f32 - 2.0 + base * 24.0 * rugged + ripple * ripple * 7.0 * rugged
                    + self.crater_field(wx, wz, 90, 0.12, 0.16, 0.38, 10.0, 10.0)
            }
            "mesa" => {
                let (q0, q1) = self.warp_xz(wx, wz, 150.0);
                let steps = (3.0 + (ch.temp * 2.0).floor()) as f32;
                let mut v = n.fbm2(q0 * 0.0042, q1 * 0.0042, 5, 2.0, 0.5) * 0.5 + 0.5;
                v = (v * steps).round() / steps;
                SEA as f32 - 8.0 + v * 44.0 * rugged + n.fbm2(wx * 0.05, wz * 0.05, 3, 2.0, 0.5) * 2.0
            }
            "volcanic" => {
                let (q0, q1) = self.warp_xz(wx, wz, 130.0);
                let b = n.fbm2(q0 * 0.0065, q1 * 0.0065, 5, 2.0, 0.5);
                let ridge = (1.0 - n.fbm2(q0 * 0.0105 + 40.0, q1 * 0.0105, 4, 2.0, 0.5).abs() * 1.7 - 0.18).max(0.0);
                let basin = n.fbm2(wx * 0.006 + 55.0, wz * 0.006 - 21.0, 2, 2.0, 0.5) * 0.5 + 0.5;
                SEA as f32 - 10.0
                    + ridge * 52.0 * rugged
                    + b * 10.0
                    + (basin - 0.5) * 20.0
                    + self.spire_field(wx, wz, 0.008, 0.58, 160.0, 24.0) * rugged
                    + self.crater_field(wx, wz, 110, 0.3, 0.14, 0.34, 13.0, 16.0)
            }
            "archipelago" => {
                let (q0, q1) = self.warp_xz(wx, wz, 240.0);
                let v = n.fbm2(q0 * 0.0065, q1 * 0.0065, 3, 2.0, 0.5) * 0.5 + 0.5;
                let m = ((v - 0.47) / 0.17).max(0.0);
                SEA as f32 - 12.0 + m.powf(1.5) * 60.0 * rugged + n.fbm2(wx * 0.03, wz * 0.03, 3, 2.0, 0.5) * 2.5
            }
            "glacial" => {
                let b = n.fbm2(wx * 0.0042, wz * 0.0042, 4, 2.0, 0.5) * 0.5 + 0.5;
                let ridge = 1.0 - n.fbm2(wx * 0.008 + 9.0, wz * 0.008, 4, 2.0, 0.5).abs();
                SEA as f32 - 2.0 + b * 14.0 * rugged + ridge.powf(3.0) * 26.0 * rugged
            }
            "flats" => {
                let b = n.fbm2(wx * 0.004, wz * 0.004, 4, 2.0, 0.5) * 0.5 + 0.5;
                SEA as f32 - 1.0 + (b - 0.5) * 10.0 * rugged
                    + self.crater_field(wx, wz, 120, 0.22, 0.12, 0.3, 7.0, 9.0)
            }
            "swamp" => {
                let (q0, q1) = self.warp_xz(wx, wz, 180.0);
                let b = n.fbm2(q0 * 0.0038, q1 * 0.0038, 4, 2.0, 0.5) * 0.5 + 0.5;
                let v = n.fbm2(wx * 0.004 + 9.0, wz * 0.004, 2, 2.0, 0.5) * 0.5 + 0.5;
                let m = ((v - 0.48) / 0.19).max(0.0);
                SEA as f32 - 1.0
                    + (b - 0.5) * 10.0 * rugged
                    + (wx * 0.013 + wz * 0.021).sin() * 1.6
                    + m.powf(1.5) * 20.0 * rugged
            }
            "shatter" => {
                let (q0, q1) = self.warp_xz(wx, wz, 90.0);
                let ridge = 1.0 - n.fbm2(q0 * 0.009 + 17.0, q1 * 0.009, 4, 2.0, 0.5).abs();
                let mut v = ridge.powf(1.4) * 40.0 * rugged;
                v = (v / 7.0).round() * 7.0;
                SEA as f32 - 6.0 + v + n.fbm2(wx * 0.05, wz * 0.05, 3, 2.0, 0.5) * 2.0
            }
            "hive" => {
                let b = n.fbm2(wx * 0.004, wz * 0.004, 4, 2.0, 0.5) * 0.5 + 0.5;
                SEA as f32 - 2.0 + (b - 0.5) * 12.0 * rugged + self.hex_dome(wx, wz, 34.0) * (0.8 + ch.wet * 0.5)
            }
            "alien" => {
                let (q0, q1) = self.warp_xz(wx, wz, 160.0);
                let b = n.fbm2(q0 * 0.0055, q1 * 0.0055, 5, 2.0, 0.5);
                let spire = (n.fbm2(q0 * 0.012, q1 * 0.012, 4, 2.0, 0.5) - 0.45).max(0.0).powf(1.6);
                SEA as f32 - 6.0 + (b * 0.5 + 0.5) * 18.0 * rugged + spire * 44.0 * rugged
            }
            _ => {
                // continental
                let (q0, q1) = self.warp_xz(wx, wz, 190.0);
                let b = n.fbm2(q0 * 0.005, q1 * 0.005, 5, 2.0, 0.5);
                SEA as f32 - 5.0 + (b * 0.5 + 0.5) * 30.0 * rugged + n.fbm2(wx * 0.05, wz * 0.05, 3, 2.0, 0.5) * 3.5
            }
        };
        let amp: f32 = match t {
            "continental" => 12.0,
            "dunes" => 8.0,
            "mesa" => 8.0,
            "volcanic" => 4.0,
            "glacial" => 8.0,
            "flats" => 4.0,
            "shatter" => 6.0,
            "hive" => 6.0,
            "alien" => 6.0,
            "archipelago" => 0.0,
            "swamp" => 3.0,
            _ => 12.0,
        };
        let h = h + n.fbm2(wx * 0.0028, wz * 0.0028, 2, 2.0, 0.5) * amp * rugged;
        (h as i32).clamp(3, WORLD_H - 8)
    }

    /// Sub-biome index + definition for a column.
    pub fn sub_at(&self, wx: f32, wz: f32) -> Option<(&'static str, f32, f32)> {
        let sub = self.biome.sub;
        if sub.is_empty() {
            return None;
        }
        let ch = self.axes;
        let m = self.noise.fbm2(wx * 0.0016 + ch.temp * 91.0, wz * 0.0016 - ch.wet * 57.0, 3, 2.0, 0.5) * 0.5 + 0.5;
        let idx = ((m * sub.len() as f32) as usize).clamp(0, sub.len() - 1);
        let s = sub[idx];
        if s.0.is_empty() && s.1 == 0.0 && s.2 == 0.0 {
            return None;
        }
        Some(s)
    }

    /// Cave test. `cave_type` = biome.caves or default "standard".
    pub fn is_cave(&self, wx: f32, y: f32, wz: f32) -> bool {
        let t = self.biome.caves.unwrap_or("standard");
        match t {
            "lava_tubes" => {
                let a = vnoise3(wx * 0.07, y * 0.11, wz * 0.07, 0xCAFE11, self.seed);
                let b = vnoise3(wx * 0.07, y * 0.11, wz * 0.07, 0xCAFE12, self.seed);
                (a - 0.5).abs() < 0.07 && (b - 0.5).abs() < 0.07
                    || vnoise3(wx * 0.03, y * 0.06, wz * 0.03, 0xCAFE13, self.seed) > 0.88
            }
            "ice" => {
                let a = vnoise3(wx * 0.04, y * 0.06, wz * 0.04, 0xCAFE21, self.seed);
                let b = vnoise3(wx * 0.04, y * 0.06, wz * 0.04, 0xCAFE22, self.seed);
                (a - 0.5).abs() < 0.055 && (b - 0.5).abs() < 0.055
            }
            "geodes" => {
                let cell = 26.0;
                let gx = (wx / cell).floor();
                let gz = (wz / cell).floor();
                let mut rnd = hash2(gx as i32, gz as i32, 0x6E0D, self.seed);
                if rnd.next() > 0.55 {
                    return false;
                }
                let cx = gx * cell + 13.0;
                let cz = gz * cell + 13.0;
                let cy = 18.0 + rnd.next() * 26.0;
                let rad = 3.0 + rnd.next() * 4.0;
                let dx = wx - cx;
                let dy = y - cy;
                let dz = wz - cz;
                dx * dx + dy * dy + dz * dz < rad * rad
            }
            "swamp_caves" => vnoise3(wx * 0.05, y * 0.09, wz * 0.05, 0xCAFE31, self.seed) > 0.8,
            _ => {
                let a = vnoise3(wx * 0.045, y * 0.075, wz * 0.045, 0xCAFE01, self.seed);
                let b = vnoise3(wx * 0.045, y * 0.075, wz * 0.045, 0xCAFE02, self.seed);
                (a - 0.5).abs() < 0.05 && (b - 0.5).abs() < 0.05
                    || vnoise3(wx * 0.024, y * 0.045, wz * 0.024, 0xCAFE03, self.seed) > 0.855
            }
        }
    }

    /// Tree test for a column. Returns (ground h, trunk height, rng) if a tree grows here.
    pub fn tree_at(&self, wx: i32, wz: i32, tree_mul: f32) -> Option<(i32, i32, Rng)> {
        let mut r = hash2(wx, wz, 0xABCD, self.seed);
        if r.next() >= self.biome.trees * tree_mul {
            return None;
        }
        let h = self.height_at(wx as f32, wz as f32);
        if h <= SEA + self.biome.sea_lift {
            return None;
        }
        let th = 4 + (r.next() * 3.0) as i32; // 4..6
        Some((h, th, r))
    }

    /// Generate one chunk of block data (deterministic).
    pub fn gen_chunk_data(&self, cx: i32, cz: i32) -> Box<[u8; CHUNK_CELLS]> {
        let mut data = Box::new([0u8; CHUNK_CELLS]);
        let b = self.biome;
        let grass_id = data::block_by_key(b.grass).id;
        let dirt_id = data::block_by_key(b.dirt).id;
        let deep_id = data::block_by_key(b.deep).id;
        let stone_id = ids::STONE;
        let x0 = cx * CHUNK;
        let z0 = cz * CHUNK;
        let seab = self.sea();
        let no_beach = matches!(b.grass, "sand" | "basalt" | "ash" | "salt" | "obsidian" | "rust" | "hive" | "amber");
        let flora_list: &[&str] = match b.key {
            "murk" => &["glow_shroom", "glow_shroom", "oxygen_plant"],
            "salt" => &["sodium_plant", "sodium_plant", "fern"],
            _ => &["sodium_plant", "oxygen_plant", "fern"],
        };

        for lz in 0..CHUNK {
            for lx in 0..CHUNK {
                let wx = x0 + lx;
                let wz = z0 + lz;
                let h = self.height_at(wx as f32, wz as f32);
                let sd = self.sub_at(wx as f32, wz as f32);
                let surf_id = match sd {
                    Some((g, _, _)) if !g.is_empty() => data::block_by_key(g).id,
                    _ => grass_id,
                };
                let can_cave = h > seab + 1;
                let mut cr = hash2(wx, wz, 0x51CA, self.seed);
                for y in 0..=h {
                    let mut id = if y == 0 {
                        ids::BARRIER
                    } else if y == h {
                        if h < seab + 1 && !no_beach { ids::SAND } else { surf_id }
                    } else if y > h - 3 {
                        dirt_id
                    } else if y < 10 {
                        deep_id
                    } else {
                        stone_id
                    };
                    if can_cave && y >= 3 && y <= h - 3 && self.is_cave(wx as f32, y as f32, wz as f32) {
                        id = if b.key == "crystal" && cr.next() < 0.12 { ids::CRYSTAL } else { ids::AIR };
                    }
                    data[lidx(lx, y, lz)] = id;
                }
                // water / lava fill
                if h < seab && (!b.dry || b.lava) {
                    for y in h + 1..=seab.min(WORLD_H - 1) {
                        data[lidx(lx, y, lz)] = ids::WATER;
                    }
                }
                if h > seab {
                    let rv = cr.next();
                    if rv < 0.0015 {
                        let oid = if cr.next() < 0.5 { ids::IRON_ORE } else { ids::COPPER_ORE };
                        data[lidx(lx, h, lz)] = oid;
                        if cr.next() < 0.6 && h > 1 {
                            data[lidx(lx, h - 1, lz)] = oid;
                        }
                    } else if b.crystals > 0.0 && rv < 0.0015 + b.crystals {
                        let ch = 1 + (cr.next() * 3.0) as i32;
                        for dy in 1..=ch {
                            if h + dy < WORLD_H {
                                data[lidx(lx, h + dy, lz)] = ids::CRYSTAL;
                            }
                        }
                    } else {
                        let flower_mul = sd.map(|s| s.2).unwrap_or(1.0);
                        let has_tree = self.tree_at(wx, wz, sd.map(|s| s.1).unwrap_or(1.0)).is_some();
                        if rv < 0.0015 + b.flowers * flower_mul
                            && !has_tree
                            && data[lidx(lx, h, lz)] == surf_id
                        {
                            let pick = flora_list[((cr.next() * flora_list.len() as f32) as usize).min(flora_list.len() - 1)];
                            data[lidx(lx, h + 1, lz)] = data::block_by_key(pick).id;
                        }
                    }
                    decor_column(self, &mut data, lx, h, lz, &mut cr);
                } else if b.key == "ocean" {
                    if cr.next() < 0.045 && h + 1 < WORLD_H {
                        let pick = if cr.next() < 0.5 {
                            "glow_shroom"
                        } else if cr.next() < 0.5 {
                            "sodium_plant"
                        } else {
                            "fern"
                        };
                        data[lidx(lx, h + 1, lz)] = data::block_by_key(pick).id;
                    }
                }
                // floating islands (alien only)
                if b.key == "alien" {
                    if let Some((base, thick)) = self.float_island_at(wx as f32, wz as f32) {
                        let gh = self.height_at(wx as f32, wz as f32);
                        if gh + 6 <= base {
                            for y in base..base + thick {
                                if y >= WORLD_H {
                                    break;
                                }
                                let id = if y == base {
                                    ids::ALIEN
                                } else if y < base + 3 {
                                    dirt_id
                                } else {
                                    stone_id
                                };
                                data[lidx(lx, y, lz)] = id;
                            }
                            if base + thick < WORLD_H && hash2(wx, wz, 0xF10A, self.seed).next() < 0.4 {
                                data[lidx(lx, base + thick, lz)] = ids::ALIEN;
                            }
                        }
                    }
                }
            }
        }

        // ore veins
        let mut rng = hash2(cx, cz, 0x0DE5, self.seed);
        let ores: [(u8, f32, f32, i32, i32); 6] = [
            (ids::COAL_ORE, 0.7, 8.0, 4, 40),
            (ids::IRON_ORE, 0.62, 7.0, 3, 34),
            (ids::COPPER_ORE, 0.62, 7.0, 3, 34),
            (ids::TITANIUM_ORE, 0.26, 5.0, 2, 20),
            (ids::GOLD_ORE, 0.17, 4.0, 2, 16),
            (ids::URANIUM_ORE, 0.11, 4.0, 2, 12),
        ];
        for (oid, exp, size, y_min, y_max) in ores {
            let expc = exp * b.ore_mul;
            let mut n = expc.floor() as i32;
            if rng.next() < expc.fract() {
                n += 1;
            }
            while n > 0 {
                n -= 1;
                let mut lx = (rng.next() * 16.0) as i32;
                let mut lz = (rng.next() * 16.0) as i32;
                let mut y = y_min + (rng.next() * (y_max - y_min) as f32) as i32;
                let vein = 3 + (rng.next() * size) as i32;
                for _ in 0..vein {
                    if (0..CHUNK).contains(&lx) && (0..CHUNK).contains(&lz) && (0..WORLD_H).contains(&y) {
                        let idx = lidx(lx, y, lz);
                        let cur = data[idx];
                        if cur == stone_id || cur == deep_id {
                            data[idx] = oid;
                        }
                    }
                    lx += (rng.next() * 3.0 - 1.0) as i32; // JS: (rng()*3-1)|0 向零截断
                    y += (rng.next() * 3.0 - 1.0) as i32;
                    lz += (rng.next() * 3.0 - 1.0) as i32;
                }
            }
        }

        // trees / giant mushrooms (extended range so cross-chunk canopies write into existing neighbors)
        for lz in -2..CHUNK + 2 {
            for lx in -2..CHUNK + 2 {
                let wx = x0 + lx;
                let wz = z0 + lz;
                let sd = self.sub_at(wx as f32, wz as f32);
                let mul = sd.map(|s| s.1).unwrap_or(1.0);
                if let Some((h, th, mut tr)) = self.tree_at(wx, wz, mul) {
                    if self.biome.mushroom {
                        for dy in 1..=th {
                            set_local(&mut data, lx, h + dy, lz, ids::MUSH_STEM);
                        }
                        let ty = h + th + 1;
                        for oz in -2i32..=2 {
                            for ox in -2i32..=2 {
                                if ox.abs() == 2 && oz.abs() == 2 {
                                    continue;
                                }
                                set_local_if_air(&mut data, lx + ox, ty, lz + oz, ids::MUSH_CAP);
                            }
                        }
                        set_local(&mut data, lx, h + th + 2, lz, ids::MUSH_CAP); // JS 无条件覆盖
                    } else {
                        for dy in 1..=th {
                            set_local(&mut data, lx, h + dy, lz, ids::LOG);
                        }
                        set_local(&mut data, lx, h + th + 2, lz, ids::LEAVES);
                        for ly in th - 1..=th + 1 {
                            for oz in -2i32..=2 {
                                for ox in -2i32..=2 {
                                    let dist = ox.abs() + oz.abs() + (ly - th).abs();
                                    if dist > 3 || tr.next() < 0.15 {
                                        continue;
                                    }
                                    set_local_if_air(&mut data, lx + ox, h + ly, lz + oz, ids::LEAVES);
                                }
                            }
                        }
                    }
                }
            }
        }

        // structures
        for st in &self.structures {
            stamp_structure(self, st, cx, cz, &mut data);
        }
        data
    }

    /// RLE encode chunk data (flat [run, id] pairs).
    pub fn rle_encode(data: &[u8]) -> Vec<u16> {
        let mut out = Vec::new();
        if data.is_empty() {
            return out;
        }
        let mut cur = data[0];
        let mut run: u16 = 1;
        for &v in &data[1..] {
            if v == cur && run < 65535 {
                run += 1;
            } else {
                out.push(run);
                out.push(cur as u16);
                cur = v;
                run = 1;
            }
        }
        out.push(run);
        out.push(cur as u16);
        out
    }

    /// RLE decode into data; returns false on corruption/length mismatch.
    pub fn rle_decode(data: &mut [u8], pairs: &[u16]) -> bool {
        let total: u64 = pairs.iter().step_by(2).map(|&r| r as u64).sum();
        if total != data.len() as u64 {
            return false;
        }
        let mut i = 0usize;
        for p in pairs.chunks_exact(2) {
            data[i..i + p[0] as usize].fill(p[1] as u8);
            i += p[0] as usize;
        }
        true
    }

    /// Deterministic structure placement (villages for habitable biomes, ruins for hazardous).
    fn gen_structures(&self) -> Vec<Structure> {
        let mut rnd = Rng::new(self.seed ^ 0x57A7C7);
        let mut out: Vec<Structure> = Vec::new();
        let sea = SEA + self.biome.sea_lift;
        let on_land = |x: f32, z: f32| self.height_at(x, z) > sea + 1;
        let separated = |out: &Vec<Structure>, x: i32, z: i32, d: i32| {
            out.iter().all(|s| {
                let (sx, sz) = match s {
                    Structure::Village { x, z, .. } | Structure::Ruin { x, z, .. } => (*x, *z),
                };
                let dx = sx - x;
                let dz = sz - z;
                dx * dx + dz * dz >= d * d
            })
        };
        if self.biome.haz.is_none() {
            let want = 3;
            let mut tries = 0;
            while out.len() < want && tries < 70 {
                tries += 1;
                let x = (rnd.next() * 1300.0) as i32 - 650;
                let z = (rnd.next() * 440.0) as i32 - 220;
                if !on_land(x as f32, z as f32) || !separated(&out, x, z, 240) {
                    continue;
                }
                let n = 4 + (rnd.next() * 3.0) as usize;
                let mut huts = Vec::new();
                for i in 0..n {
                    let ang = i as f32 / n as f32 * std::f32::consts::TAU + rnd.next() * 0.7;
                    let dist = 8.0 + rnd.next() * 7.0;
                    let hx = x + (ang.cos() * dist).round() as i32;
                    let hz = z + (ang.sin() * dist).round() as i32;
                    if on_land(hx as f32, hz as f32) {
                        huts.push((hx, hz, self.height_at(hx as f32, hz as f32)));
                    }
                }
                if huts.len() >= 3 {
                    out.push(Structure::Village {
                        x,
                        z,
                        h: self.height_at(x as f32, z as f32),
                        huts,
                    });
                }
            }
        } else {
            let want = 3;
            let mut tries = 0;
            while out.len() < want && tries < 70 {
                tries += 1;
                let x = (rnd.next() * 1300.0) as i32 - 650;
                let z = (rnd.next() * 440.0) as i32 - 220;
                if !on_land(x as f32, z as f32) || !separated(&out, x, z, 220) {
                    continue;
                }
                let kind = (rnd.next() * 3.0) as u32;
                out.push(Structure::Ruin {
                    x,
                    z,
                    kind,
                    h: self.height_at(x as f32, z as f32),
                    seed: (rnd.next() * 0xFFFF as f32) as u32,
                });
            }
        }
        out
    }
}

/// Write a block into chunk-local coords if in-bounds and y in range (clamped semantics of g code).
fn set_local(data: &mut [u8; CHUNK_CELLS], lx: i32, y: i32, lz: i32, id: u8) {
    if (0..CHUNK).contains(&lx) && (0..CHUNK).contains(&lz) && (0..WORLD_H).contains(&y) {
        data[lidx(lx, y, lz)] = id;
    }
}

fn set_local_if_air(data: &mut [u8; CHUNK_CELLS], lx: i32, y: i32, lz: i32, id: u8) {
    if (0..CHUNK).contains(&lx) && (0..CHUNK).contains(&lz) && (0..WORLD_H).contains(&y) {
        let idx = lidx(lx, y, lz);
        if data[idx] == 0 {
            data[idx] = id;
        }
    }
}

fn decor_column(g: &WorldGen, data: &mut [u8; CHUNK_CELLS], lx: i32, h: i32, lz: i32, cr: &mut Rng) {
    let b = g.biome;
    let place = |data: &mut [u8; CHUNK_CELLS], dy: i32, id: u8| set_local(data, lx, h + dy, lz, id);
    match b.key {
        "desert" | "amber" => {
            if cr.next() < 0.006 {
                let n = 1 + (cr.next() * 3.0) as i32;
                for i in 1..=n {
                    place(data, i, if b.key == "amber" && i == 1 { ids::AMBER } else { ids::STONE });
                }
            }
        }
        "frozen" => {
            if cr.next() < 0.007 {
                let n = 1 + (cr.next() * 3.0) as i32;
                for i in 1..=n {
                    place(data, i, if i == n { ids::CRYSTAL } else { ids::ICE });
                }
            }
        }
        "volcanic" | "obsidian" | "ferrous" => {
            if cr.next() < 0.008 {
                let n = 1 + (cr.next() * 3.0) as i32;
                for i in 1..=n {
                    place(data, i, ids::BASALT);
                }
            }
        }
        "ashen" => {
            if cr.next() < 0.01 {
                place(data, 1, ids::LOG);
                if cr.next() < 0.4 {
                    place(data, 2, ids::LOG);
                }
            }
        }
        "salt" => {
            if cr.next() < 0.006 {
                let n = 1 + (cr.next() * 2.0) as i32;
                for i in 1..=n {
                    place(data, i, ids::SALT);
                }
            }
        }
        "murk" => {
            if cr.next() < 0.05 {
                place(data, 1, ids::GLOW_SHROOM);
            }
        }
        "redmoss" => {
            if cr.next() < 0.005 {
                let n = 1 + (cr.next() * 2.0) as i32;
                for i in 1..=n {
                    place(data, i, ids::STONE);
                }
            }
        }
        "crystal" => {
            if cr.next() < 0.008 {
                let n = 2 + (cr.next() * 4.0) as i32;
                for i in 1..=n {
                    place(data, i, ids::CRYSTAL);
                }
            }
        }
        _ => {}
    }
}

fn stamp_structure(g: &WorldGen, st: &Structure, cx: i32, cz: i32, data: &mut [u8; CHUNK_CELLS]) {
    let x0 = cx * CHUNK;
    let z0 = cz * CHUNK;
    let b = g.biome;
    match st {
        Structure::Village { x, z, h: _, huts } => {
            // central beacon
            if in_chunk_abs(*x, *z, x0, z0) {
                let lx = *x - x0;
                let lz = *z - z0;
                let gh = g.height_at(*x as f32, *z as f32);
                set_local(data, lx, gh + 1, lz, ids::LOG);
                set_local(data, lx, gh + 2, lz, ids::LOG);
                set_local(data, lx, gh + 3, lz, ids::LAMP);
            }
            for &(hx, hz, hh) in huts {
                stamp_hut(g, data, x0, z0, hx, hz, hh, b);
            }
        }
        Structure::Ruin { x, z, kind, h, seed } => {
            for wx in x - 10..=x + 10 {
                for wz in z - 10..=z + 10 {
                    if !in_chunk_abs(wx, wz, x0, z0) {
                        continue;
                    }
                    let lx = wx - x0;
                    let lz = wz - z0;
                    let gh = g.height_at(wx as f32, wz as f32);
                    let dx = wx - x;
                    let dz = wz - z;
                    let mut hr = hash2(wx, wz, *seed, g.seed);
                    match kind {
                        0 => {
                            let dist = ((dx * dx + dz * dz) as f32).sqrt();
                            if (dist - 7.0).abs() < 0.7 && hr.next() < 0.7 {
                                // JS: hash2(wx, wz, st.seed + 7) —— 结构种子派生柱高
                                let hh2 = 2 + (hash2(wx, wz, seed.wrapping_add(7), g.seed).next() * 3.0) as i32;
                                for dy in 1..=hh2 {
                                    let id = if dy == hh2 { data::block_by_key(b.deep).id } else { ids::STONE };
                                    set_local(data, lx, gh + dy, lz, id);
                                }
                            }
                            if dist < 1.6 {
                                set_local(data, lx, gh + 1, lz, ids::STONE);
                                if dx == 0 && dz == 0 {
                                    set_local(data, lx, gh + 2, lz, ids::LAMP);
                                }
                            }
                        }
                        1 => {
                            let t0 = *h;
                            if dx.abs() <= 1 && dz.abs() <= 1 {
                                for dy in 1..=3 {
                                    set_local(data, lx, t0 + dy, lz, data::block_by_key(b.deep).id);
                                }
                            }
                            if dx.abs() + dz.abs() <= 1 {
                                for dy in 4..=8 {
                                    set_local(data, lx, t0 + dy, lz, ids::STONE);
                                }
                            }
                            if dx == 0 && dz == 0 {
                                for dy in 9..=12 {
                                    set_local(data, lx, t0 + dy, lz, data::block_by_key(b.deep).id);
                                }
                                set_local(data, lx, t0 + 13, lz, ids::LAMP);
                            }
                        }
                        _ => {
                            let interior = dx.abs() <= 8 && dz.abs() <= 6;
                            if interior {
                                let edge = (dx.abs() == 8 && dz.abs() <= 6) || (dz.abs() == 6 && dx.abs() <= 8);
                                if edge {
                                    let hh2 = (hr.next() * 4.0) as i32;
                                    // JS: for y = 1; y <= hh; y++（含端点）
                                    for dy in 1..=hh2 {
                                        let id = if hr.next() < 0.25 { data::block_by_key(b.deep).id } else { ids::STONE };
                                        set_local(data, lx, gh + dy, lz, id);
                                    }
                                } else if hr.next() < 0.3 {
                                    set_local(data, lx, gh, lz, ids::STONE);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn in_chunk_abs(wx: i32, wz: i32, x0: i32, z0: i32) -> bool {
    (x0..x0 + CHUNK).contains(&wx) && (z0..z0 + CHUNK).contains(&wz)
}

/// 5×5 hut. `s` = 2, floor `f = hut_h + 1`.
fn stamp_hut(g: &WorldGen, data: &mut [u8; CHUNK_CELLS], x0: i32, z0: i32, hx: i32, hz: i32, hut_h: i32, b: &Biome) {
    let s = 2;
    let f = hut_h + 1;
    let dirt_id = data::block_by_key(b.dirt).id;
    let set_abs = |data: &mut [u8; CHUNK_CELLS], x: i32, y: i32, z: i32, id: u8| {
        if in_chunk_abs(x, z, x0, z0) {
            set_local(data, x - x0, y, z - z0, id);
        }
    };
    for dx in -s..=s {
        for dz in -s..=s {
            let wx = hx + dx;
            let wz = hz + dz;
            let gh = g.height_at(wx as f32, wz as f32);
            for y in gh.min(f - 1)..f - 1 {
                set_abs(data, wx, y, wz, dirt_id);
            }
            set_abs(data, wx, f - 1, wz, ids::PLANKS);
            // interior cleared（JS: f..=f+4 五层）
            for y in f..=f + 4 {
                set_abs(data, wx, y, wz, ids::AIR);
            }
            let edge = dx.abs() == s || dz.abs() == s;
            if edge {
                // 外墙（JS: f..=f+2 三层）
                for y in f..=f + 2 {
                    let id = if dx.abs() == s && dz.abs() == s { ids::LOG } else { ids::PLANKS };
                    set_abs(data, wx, y, wz, id);
                }
                // door
                if dx == 0 && dz == s {
                    set_abs(data, wx, f, wz, ids::AIR);
                    set_abs(data, wx, f + 1, wz, ids::AIR);
                }
                // windows
                if (dx.abs() == s && dz == 0) || (dx == 0 && dz == -s) {
                    set_abs(data, wx, f + 1, wz, ids::GLASS);
                }
            }
            // roof
            set_abs(data, wx, f + 3, wz, ids::PLANKS);
        }
    }
}

// ===================== Live world (chunks + streaming) =====================

#[derive(Clone)]
pub struct Chunk {
    pub cx: i32,
    pub cz: i32,
    pub data: Box<[u8; CHUNK_CELLS]>,
    pub dirty: bool,
    pub modified: bool,
    pub mesh: Option<bevy::prelude::Entity>,
    pub water_mesh: Option<bevy::prelude::Entity>,
    pub from_save: bool,
    pub need_save: bool,
}

#[derive(Resource)]
pub struct World {
    pub seed: u32,
    pub g: WorldGen,
    pub chunks: HashMap<i64, Chunk>,
    pub saved_mods: HashMap<String, Vec<u16>>,
    pub stream_dirty: bool,
    pub last_pcx: i32,
    pub last_pcz: i32,
    pub view_dist: i32,
    pub gen_count: u32,
}

impl World {
    pub fn new(seed: u32, biome_key: &str, view_dist: i32) -> Self {
        let biome = data::biome_by_key(biome_key);
        let g = WorldGen::new(seed, biome);
        Self {
            seed,
            g,
            chunks: HashMap::new(),
            saved_mods: HashMap::new(),
            stream_dirty: true,
            last_pcx: i32::MAX,
            last_pcz: i32::MAX,
            view_dist: view_dist.clamp(3, 32),
            gen_count: 0,
        }
    }

    pub fn biome(&self) -> &'static Biome {
        self.g.biome
    }

    pub fn get_chunk(&self, cx: i32, cz: i32) -> Option<&Chunk> {
        self.chunks.get(&ckey(cx, cz))
    }

    pub fn ensure_chunk(&mut self, cx: i32, cz: i32) -> &Chunk {
        let key = ckey(cx, cz);
        if !self.chunks.contains_key(&key) {
            self.gen_count += 1;
            let mut data = self.g.gen_chunk_data(cx, cz);
            let mut from_save = false;
            if let Some(mods) = self.saved_mods.get(&strkey(cx, cz)) {
                if WorldGen::rle_decode(data.as_mut_slice(), mods) {
                    from_save = true;
                }
            }
            self.chunks.insert(
                key,
                Chunk {
                    cx,
                    cz,
                    data,
                    dirty: true,
                    modified: false,
                    mesh: None,
                    water_mesh: None,
                    from_save,
                    need_save: false,
                },
            );
            // JS markNeighborsDirty：新块生成后，已网格化的 4 邻需重算边界面
            for (nx, nz) in [(cx - 1, cz), (cx + 1, cz), (cx, cz - 1), (cx, cz + 1)] {
                if let Some(c) = self.chunks.get_mut(&ckey(nx, nz)) {
                    if c.mesh.is_some() || c.water_mesh.is_some() {
                        c.dirty = true;
                        self.stream_dirty = true;
                    }
                }
            }
        }
        self.chunks.get(&key).unwrap()
    }

    /// Block id at world coords (out-of-range → air).
    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..WORLD_H).contains(&y) {
            return ids::AIR;
        }
        let cx = x.div_euclid(CHUNK);
        let cz = z.div_euclid(CHUNK);
        let Some(c) = self.chunks.get(&ckey(cx, cz)) else {
            return ids::AIR;
        };
        c.data[lidx(x.rem_euclid(CHUNK), y, z.rem_euclid(CHUNK))]
    }

    /// Set block; generates the chunk if needed; marks dirty + neighbors.
    pub fn set(&mut self, x: i32, y: i32, z: i32, id: u8) {
        if !(0..WORLD_H).contains(&y) {
            return;
        }
        let cx = x.div_euclid(CHUNK);
        let cz = z.div_euclid(CHUNK);
        self.ensure_chunk(cx, cz);
        let key = ckey(cx, cz);
        let lx = x.rem_euclid(CHUNK);
        let lz = z.rem_euclid(CHUNK);
        let c = self.chunks.get_mut(&key).unwrap();
        c.data[lidx(lx, y, lz)] = id;
        c.modified = true;
        c.need_save = true;
        c.dirty = true;
        self.stream_dirty = true;
        if lx == 0 {
            self.mark_dirty(cx - 1, cz);
        }
        if lx == CHUNK - 1 {
            self.mark_dirty(cx + 1, cz);
        }
        if lz == 0 {
            self.mark_dirty(cx, cz - 1);
        }
        if lz == CHUNK - 1 {
            self.mark_dirty(cx, cz + 1);
        }
    }

    pub fn mark_dirty(&mut self, cx: i32, cz: i32) {
        if let Some(c) = self.chunks.get_mut(&ckey(cx, cz)) {
            c.dirty = true;
            self.stream_dirty = true;
        }
    }

    /// Highest solid-or-liquid block y at column (JS topAt：solid‖liquid，兜底 0)。
    pub fn top_at(&self, x: i32, z: i32) -> i32 {
        for y in (0..WORLD_H).rev() {
            let id = self.get(x, y, z);
            let def = data::block_by_id(id);
            if def.solid || def.liquid {
                return y;
            }
        }
        0
    }

    /// Find a spawn position（JS findSpawn 移植：种子派生、400 次随机逐步扩大搜索、网格兜底）。
    /// 锚定在传入列附近以保证区块已生成（JS 为绝对坐标，此处取相对偏移保持流式中心一致）。
    pub fn find_spawn(&self, x: i32, z: i32) -> Vec3 {
        let seab = data::SEA + self.biome().sea_lift;
        let mut rng = crate::rng::Rng::new(self.seed ^ 0xB00B5);
        let valid = |wx: i32, wz: i32| -> bool {
            let y = self.top_at(wx, wz);
            if y <= seab {
                return false;
            }
            let d = data::block_by_id(self.get(wx, y, wz));
            d.solid && !d.liquid && d.key != "leaves" && d.key != "log"
        };
        for r in 0..400 {
            let range = (20 + r) as f32;
            let wx = x + ((rng.next() * range * 2.0 - range) as i32);
            let wz = z + ((rng.next() * range * 2.0 - range) as i32);
            if valid(wx, wz) {
                return Vec3::new(
                    wx as f32 + 0.5,
                    self.top_at(wx, wz) as f32 + 2.0,
                    wz as f32 + 0.5,
                );
            }
        }
        for gx in (-256..=256).step_by(8) {
            for gz in (-256..=256).step_by(8) {
                let wx = x + gx;
                let wz = z + gz;
                if valid(wx, wz) {
                    return Vec3::new(
                        wx as f32 + 0.5,
                        self.top_at(wx, wz) as f32 + 2.0,
                        wz as f32 + 0.5,
                    );
                }
            }
        }
        Vec3::new(
            x as f32 + 0.5,
            self.top_at(x, z) as f32 + 2.0,
            z as f32 + 0.5,
        )
    }

    /// Voxel DDA raycast（JS 语义：命中=非空气且非液体，穿水、中植物；零分量轴不移动）。
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<([i32; 3], [i32; 3], f32)> {
        let mut x = origin.x.floor() as i32;
        let mut y = origin.y.floor() as i32;
        let mut z = origin.z.floor() as i32;
        let step_x = if dir.x > 0.0 { 1 } else if dir.x < 0.0 { -1 } else { 0 };
        let step_y = if dir.y > 0.0 { 1 } else if dir.y < 0.0 { -1 } else { 0 };
        let step_z = if dir.z > 0.0 { 1 } else if dir.z < 0.0 { -1 } else { 0 };
        let t_delta_x = if step_x != 0 { (1.0 / dir.x).abs() } else { f32::INFINITY };
        let t_delta_y = if step_y != 0 { (1.0 / dir.y).abs() } else { f32::INFINITY };
        let t_delta_z = if step_z != 0 { (1.0 / dir.z).abs() } else { f32::INFINITY };
        let mut t_max_x = if step_x != 0 {
            if step_x > 0 { (x as f32 + 1.0 - origin.x) * t_delta_x } else { (origin.x - x as f32) * t_delta_x }
        } else {
            f32::INFINITY
        };
        let mut t_max_y = if step_y != 0 {
            if step_y > 0 { (y as f32 + 1.0 - origin.y) * t_delta_y } else { (origin.y - y as f32) * t_delta_y }
        } else {
            f32::INFINITY
        };
        let mut t_max_z = if step_z != 0 {
            if step_z > 0 { (z as f32 + 1.0 - origin.z) * t_delta_z } else { (origin.z - z as f32) * t_delta_z }
        } else {
            f32::INFINITY
        };
        let mut normal = [0, 0, 0];
        let mut t = 0.0f32;
        for _ in 0..256 {
            let def = data::block_by_id(self.get(x, y, z));
            if def.id != ids::AIR && !def.liquid {
                return Some(([x, y, z], normal, t));
            }
            if t_max_x < t_max_y && t_max_x < t_max_z {
                x += step_x;
                t = t_max_x;
                t_max_x += t_delta_x;
                normal = [-step_x, 0, 0];
            } else if t_max_y < t_max_z {
                y += step_y;
                t = t_max_y;
                t_max_y += t_delta_y;
                normal = [0, -step_y, 0];
            } else {
                z += step_z;
                t = t_max_z;
                t_max_z += t_delta_z;
                normal = [0, 0, -step_z];
            }
            if t > max_dist {
                return None;
            }
        }
        None
    }

    /// Serialize modified chunks: { "cx,cz": rle }
    pub fn serialize_mods(&self) -> HashMap<String, Vec<u16>> {
        let mut out = HashMap::new();
        for c in self.chunks.values() {
            if c.modified {
                out.insert(strkey(c.cx, c.cz), WorldGen::rle_encode(c.data.as_slice()));
            }
        }
        out
    }

    /// Chebyshev distance between two chunk coords.
    pub fn cheb(cx: i32, cz: i32, pcx: i32, pcz: i32) -> i32 {
        (cx - pcx).abs().max((cz - pcz).abs())
    }
}

// ===================== Mesh building =====================

pub struct VoxelMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl VoxelMesh {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// Face table: dir, shade. Order: +X, -X, +Y, -Y, +Z, -Z.
const FACE_DIRS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
const FACE_SHADE: [f32; 6] = [0.80, 0.80, 1.00, 0.50, 0.65, 0.65];

/// Build solid + water meshes for one chunk. Neighbors must exist.
pub fn build_chunk_meshes(world: &World, c: &Chunk, atlas: &crate::textures::Atlas) -> (Option<VoxelMesh>, Option<VoxelMesh>) {
    let mut solid = VoxelMesh::new();
    let mut water = VoxelMesh::new();
    let cx = c.cx;
    let cz = c.cz;
    let x0 = cx * CHUNK;
    let z0 = cz * CHUNK;
    let water_tint = world.biome().water_tint;
    let tint = [
        ((water_tint >> 16) & 0xFF) as f32 / 255.0,
        ((water_tint >> 8) & 0xFF) as f32 / 255.0,
        (water_tint & 0xFF) as f32 / 255.0,
    ];

    for y in 0..WORLD_H {
        for lz in 0..CHUNK {
            for lx in 0..CHUNK {
                let id = c.data[lidx(lx, y, lz)];
                if id == ids::AIR {
                    continue;
                }
                let def = data::block_by_id(id);
                let wx = x0 + lx;
                let wz = z0 + lz;
                if def.cross {
                    // two diagonal quads
                    let tile = def.tiles.side.or(def.tiles.all).unwrap_or("grass_top");
                    let [u0, v0, u1, v1] = atlas.uv_rect(tile);
                    let bright = if def.glow { 1.7 } else { 1.0 };
                    let (x, z) = (wx as f32, wz as f32);
                    let (y0, y1) = (y as f32, y as f32 + 1.0);
                    let quads = [
                        [(x, y0, z + 1.0), (x + 1.0, y0, z), (x, y1, z + 1.0), (x + 1.0, y1, z)],
                        [(x, y0, z), (x + 1.0, y0, z + 1.0), (x, y1, z), (x + 1.0, y1, z + 1.0)],
                    ];
                    let uv = [[u0, v1], [u1, v1], [u0, v0], [u1, v0]];
                    for q in quads {
                        emit_quad(&mut solid, q, uv, bright, [0.0, 1.0, 0.0]);
                    }
                    continue;
                }
                let lb = def.lowbox.unwrap_or(1.0);
                let (y0, y1) = (y as f32, y as f32 + lb);
                for f in 0..6 {
                    let dir = FACE_DIRS[f];
                    let nx = wx + dir[0];
                    let ny = y + dir[1];
                    let nz = wz + dir[2];
                    let n_id = world.get(nx, ny, nz);
                    let n_def = data::block_by_id(n_id);
                    let emit = if def.liquid {
                        !(n_id == id || (n_def.solid && !n_def.transparent))
                    } else if def.lowbox.is_some() && f == 3 {
                        !(n_def.solid && !n_def.transparent)
                    } else {
                        !(n_def.solid && !n_def.transparent && !n_def.cross && n_def.machine.is_none())
                            && !(n_id == id && def.transparent && !def.fancy)
                    };
                    if !emit {
                        continue;
                    }
                    let tile = def.tiles.for_face(f);
                    let [u0, v0, u1, v1] = atlas.uv_rect(tile);
                    let shade = if def.liquid {
                        0.72 + FACE_SHADE[f] * 0.28
                    } else if def.glow {
                        FACE_SHADE[f] * 2.2
                    } else {
                        FACE_SHADE[f]
                    };
                    let corners = face_corners(wx as f32, wz as f32, y0, y1, f);
                    // UV mapping: u across horizontal, v follows y (v0 = tile top, v1 = tile bottom)
                    let uv = face_uvs(f, [u0, v0, u1, v1]);
                    if def.liquid {
                        let mut cols = [[0f32; 4]; 4];
                        for i in 0..4 {
                            cols[i] = [tint[0] * shade, tint[1] * shade, tint[2] * shade, 1.0];
                        }
                        emit_quad_col(&mut water, corners, uv, cols, dir.map(|v| v as f32));
                    } else {
                        emit_quad(&mut solid, corners, uv, shade, dir.map(|v| v as f32));
                    }
                }
            }
        }
    }
    (
        if solid.is_empty() { None } else { Some(solid) },
        if water.is_empty() { None } else { Some(water) },
    )
}

/// Corners for a face (4 positions). y0/y1 already account for lowbox height.
fn face_corners(x: f32, z: f32, y0: f32, y1: f32, face: usize) -> [(f32, f32, f32); 4] {
    match face {
        0 => [(x + 1.0, y0, z), (x + 1.0, y0, z + 1.0), (x + 1.0, y1, z), (x + 1.0, y1, z + 1.0)],     // +X
        1 => [(x, y0, z + 1.0), (x, y0, z), (x, y1, z + 1.0), (x, y1, z)],                             // -X
        2 => [(x, y1, z), (x + 1.0, y1, z), (x, y1, z + 1.0), (x + 1.0, y1, z + 1.0)],                 // +Y
        3 => [(x, y0, z), (x + 1.0, y0, z), (x, y0, z + 1.0), (x + 1.0, y0, z + 1.0)],                 // -Y
        4 => [(x + 1.0, y0, z + 1.0), (x, y0, z + 1.0), (x + 1.0, y1, z + 1.0), (x, y1, z + 1.0)],     // +Z
        _ => [(x, y0, z), (x + 1.0, y0, z), (x, y1, z), (x + 1.0, y1, z)],                             // -Z
    }
}

/// UVs per face: [u0,v_top, u1,v_bottom]; returns 4 corner uvs aligned with face_corners.
fn face_uvs(face: usize, rect: [f32; 4]) -> [[f32; 2]; 4] {
    let [u0, v0, u1, v1] = rect; // v0 = tile top, v1 = tile bottom
    match face {
        2 => [[u0, v0], [u1, v0], [u0, v1], [u1, v1]],
        3 => [[u0, v0], [u1, v0], [u0, v1], [u1, v1]],
        _ => [[u0, v1], [u1, v1], [u0, v0], [u1, v0]],
    }
}

fn emit_quad(m: &mut VoxelMesh, q: [(f32, f32, f32); 4], uv: [[f32; 2]; 4], bright: f32, n: [f32; 3]) {
    let base = m.positions.len() as u32;
    for i in 0..4 {
        m.positions.push([q[i].0, q[i].1, q[i].2]);
        m.normals.push(n);
        m.colors.push([bright, bright, bright, 1.0]);
        m.uvs.push(uv[i]);
    }
    m.indices.push(base);
    m.indices.push(base + 1);
    m.indices.push(base + 2);
    m.indices.push(base + 2);
    m.indices.push(base + 1);
    m.indices.push(base + 3);
}

fn emit_quad_col(
    m: &mut VoxelMesh,
    q: [(f32, f32, f32); 4],
    uv: [[f32; 2]; 4],
    cols: [[f32; 4]; 4],
    n: [f32; 3],
) {
    let base = m.positions.len() as u32;
    for i in 0..4 {
        m.positions.push([q[i].0, q[i].1, q[i].2]);
        m.normals.push(n);
        m.uvs.push(uv[i]);
        m.colors.push(cols[i]);
    }
    m.indices.push(base);
    m.indices.push(base + 1);
    m.indices.push(base + 2);
    m.indices.push(base + 2);
    m.indices.push(base + 1);
    m.indices.push(base + 3);
}

// ---------- 远景模拟地形（JS ensureFarMesh / tickFar 移植） ----------
// 流式区块视距之外的地表被一张低细节高度场网格覆盖（纯噪声高度 + 地表瓦片平均色），
// 否则高空视角下地形在视距边缘戛然而止，曲率变形与区块流式边缘完全暴露（巨大闪动/残影）。

pub const FAR_N: usize = 129; // 129×129 顶点
pub const FAR_STEP: f32 = 24.0; // 格/单元 → ±1536 格视距
pub const FAR_SNAP: f32 = 64.0; // 中心对齐（格，跨格才重建）
pub const FAR_ROWS_PER_FRAME: usize = 12; // 每帧填充行数（分帧重建避免卡顿）
pub const FAR_SINK: f32 = 2.2; // 下沉偏置：近处由真实区块覆盖

/// 地表瓦片平均色（与 JS `tileAvgColor` 同口径；瓦片缺失时给中性绿）。
fn far_tile_avg(atlas: &crate::textures::Atlas, tile: &str) -> [f32; 3] {
    let Some(&idx) = atlas.index.get(tile) else {
        return [0.5, 0.6, 0.4];
    };
    let t = &atlas.tiles[idx];
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for p in t.iter() {
        r += p[0] as u32;
        g += p[1] as u32;
        b += p[2] as u32;
    }
    let n = t.len() as f32 * 255.0;
    [r as f32 / n, g as f32 / n, b as f32 / n]
}

/// 填充远景地形网格的行 `[from, to)`（原地修改，其余行保留原值）。
/// 高度/地表色与 JS `mapHeightAt` / `mapColorRGB` 同口径（纯噪声高度、海平面抬升、地表瓦片平均色）。
/// 顶点 alpha 留给挖空环（由 far_mesh_system 按玩家位置逐帧更新），这里恒为 1。
pub fn fill_far_rows(
    world: &World,
    atlas: &crate::textures::Atlas,
    cx: f32,
    cz: f32,
    from: usize,
    to: usize,
    mesh: &mut Mesh,
) {
    use bevy::mesh::VertexAttributeValues;
    let n = FAR_N * FAR_N;
    let mut positions: Vec<[f32; 3]> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
        _ => vec![[0.0; 3]; n],
    };
    let mut colors: Vec<[f32; 4]> = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(v)) => v.clone(),
        _ => vec![[1.0; 4]; n],
    };
    let g = &world.g;
    let b = g.biome;
    let seab = g.sea() as f32;
    let wt = b.water_tint;
    let water_rgb = [
        ((wt >> 16) & 0xFF) as f32 / 255.0,
        ((wt >> 8) & 0xFF) as f32 / 255.0,
        (wt & 0xFF) as f32 / 255.0,
    ];
    let no_beach = matches!(
        b.grass,
        "sand" | "basalt" | "ash" | "salt" | "obsidian" | "rust" | "hive" | "amber"
    );
    // 地表瓦片：sub 地面覆盖优先（JS tileFor(BLOCKS[sd.g || biome.grass], 2)），redmoss→redmoss_top
    fn tile_key(k: &'static str) -> &'static str {
        match k {
            "grass" => "grass_top",
            "snow" => "snow_top",
            "alien" => "alien_top",
            "murk" => "murk_top",
            "redmoss" => "redmoss_top",
            other => other,
        }
    }
    let mut tile_cache: std::collections::HashMap<&'static str, [f32; 3]> = std::collections::HashMap::new();
    let sand_avg = far_tile_avg(atlas, "sand");
    let half = (FAR_N as f32 - 1.0) / 2.0 * FAR_STEP;
    let to = to.min(FAR_N);
    for iz in from..to {
        let wz = cz - half + iz as f32 * FAR_STEP;
        for ix in 0..FAR_N {
            let wx = cx - half + ix as f32 * FAR_STEP;
            let mut h = g.height_at(wx.floor(), wz.floor()) as f32;
            // alien 浮岛（JS mapHeightAt：h+6 <= fl.base → fl.base+fl.thick）
            let mut island_h = None;
            if b.key == "alien" {
                if let Some((base, thick)) = g.float_island_at(wx, wz) {
                    if h + 6.0 <= base as f32 {
                        island_h = Some((base, thick));
                        h = base as f32 + thick as f32;
                    }
                }
            }
            let (y, col) = if h < seab && (!b.dry || b.lava) && island_h.is_none() {
                let depth = (seab - h).max(0.0);
                let sh = (0.78 - depth * 0.008).clamp(0.0, 1.0);
                (seab, [water_rgb[0] * sh, water_rgb[1] * sh, water_rgb[2] * sh, 1.0])
            } else {
                let ground_key = g
                    .sub_at(wx.floor(), wz.floor())
                    .map(|s| s.0)
                    .filter(|k| !k.is_empty())
                    .unwrap_or(b.grass);
                let tk = tile_key(ground_key);
                let avg = *tile_cache.entry(tk).or_insert_with(|| far_tile_avg(atlas, tk));
                let avg = if h < seab + 1.0 && !no_beach && island_h.is_none() { sand_avg } else { avg };
                let sh = (0.72 + (h - 14.0) * 0.012).clamp(0.0, 1.35);
                (
                    h,
                    [
                        (avg[0] * sh).min(1.0),
                        (avg[1] * sh).min(1.0),
                        (avg[2] * sh).min(1.0),
                        1.0,
                    ],
                )
            };
            let i = iz * FAR_N + ix;
            positions[i] = [wx, y - FAR_SINK, wz];
            colors[i] = col;
        }
    }
    // 法线：从高度场重算（边界行取自身差分，避免越界）
    let mut normals: Vec<[f32; 3]> = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
        _ => vec![[0.0, 1.0, 0.0]; n],
    };
    let y_at = |ix: usize, iz: usize| positions[iz * FAR_N + ix][1];
    for iz in 0..FAR_N {
        for ix in 0..FAR_N {
            let h_l = if ix > 0 { y_at(ix - 1, iz) } else { y_at(ix, iz) };
            let h_r = if ix + 1 < FAR_N { y_at(ix + 1, iz) } else { y_at(ix, iz) };
            let h_d = if iz > 0 { y_at(ix, iz - 1) } else { y_at(ix, iz) };
            let h_u = if iz + 1 < FAR_N { y_at(ix, iz + 1) } else { y_at(ix, iz) };
            let norm = Vec3::new(h_l - h_r, 2.0 * FAR_STEP, h_d - h_u).normalize();
            normals[iz * FAR_N + ix] = [norm.x, norm.y, norm.z];
        }
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
}

/// 创建远景地形网格（129×129 高度场，±1536 格视距，顶点色为地表色）。
/// 使用默认 RenderAssetUsages（含 MAIN_WORLD），以便渲染后仍可原地更新顶点。
pub fn build_far_mesh(world: &World, atlas: &crate::textures::Atlas, cx: f32, cz: f32) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    let mut indices: Vec<u32> = Vec::with_capacity((FAR_N - 1) * (FAR_N - 1) * 6);
    for iz in 0..(FAR_N as u32 - 1) {
        for ix in 0..(FAR_N as u32 - 1) {
            let a = iz * FAR_N as u32 + ix;
            let b = a + 1;
            let c = a + FAR_N as u32;
            let d = c + 1;
            indices.push(a);
            indices.push(c);
            indices.push(b);
            indices.push(b);
            indices.push(c);
            indices.push(d);
        }
    }
    mesh.insert_indices(Indices::U32(indices));
    fill_far_rows(world, atlas, cx, cz, 0, FAR_N, &mut mesh);
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_roundtrip() {
        let mut data = [0u8; CHUNK_CELLS];
        data[0] = 7;
        data[1] = 7;
        data[2] = 3;
        data[CHUNK_CELLS - 1] = 16;
        let enc = WorldGen::rle_encode(&data);
        let mut dec = [0u8; CHUNK_CELLS];
        assert!(WorldGen::rle_decode(&mut dec, &enc));
        assert_eq!(data, dec);
    }

    #[test]
    fn chunk_gen_deterministic() {
        let biome = data::biome_by_key("lush");
        let g1 = WorldGen::new(12345, biome);
        let g2 = WorldGen::new(12345, biome);
        let a = g1.gen_chunk_data(0, 0);
        let b = g2.gen_chunk_data(0, 0);
        assert_eq!(&*a, &*b);
        // chunk 0,0 of a lush world must have terrain: some stone and grass
        assert!(a.iter().any(|&v| v == ids::STONE));
        assert!(a.iter().any(|&v| v == ids::GRASS || v == ids::SAND));
        // bedrock floor
        assert!(a[lidx(0, 0, 0)] == ids::BARRIER);
    }

    #[test]
    fn heights_in_range() {
        let biome = data::biome_by_key("volcanic");
        let g = WorldGen::new(7777, biome);
        for x in -300..300 {
            for z in -300..300 {
                let h = g.height_at(x as f32 * 1.7, z as f32 * 1.3);
                assert!((3..=88).contains(&h), "height {h} out of range at {x},{z}");
            }
        }
    }

    #[test]
    fn ocean_world_has_water() {
        let biome = data::biome_by_key("ocean");
        let g = WorldGen::new(4242, biome);
        let d = g.gen_chunk_data(0, 0);
        assert!(d.iter().any(|&v| v == ids::WATER));
    }

    #[test]
    fn volcanic_world_has_lava_water() {
        let biome = data::biome_by_key("volcanic");
        let g = WorldGen::new(99, biome);
        let d = g.gen_chunk_data(2, 2);
        // volcanic: dry+lava → water id present (as lava)
        assert!(d.iter().any(|&v| v == ids::WATER));
    }

    #[test]
    fn chunk_mesh_uses_absolute_world_coordinates() {
        // 回归：网格顶点必须是绝对世界坐标 —— spawn_chunk_mesh 以恒等变换渲染，
        // 若这里变成区块局部坐标，每块地形会被双倍偏移、块间出现大空洞。
        let atlas = crate::textures::Atlas::build();
        let mut w = World::new(4242, "lush", 8);
        for cz in 4..=6 {
            for cx in 2..=4 {
                w.ensure_chunk(cx, cz);
            }
        }
        let c = w.get_chunk(3, 5).expect("chunk 3,5 generated");
        let (solid, _water) = build_chunk_meshes(&w, c, &atlas);
        let m = solid.expect("chunk 3,5 must contain terrain");
        assert!(!m.positions.is_empty());
        for p in &m.positions {
            // 块面角点可恰好落在区块边界（lx=15 的 +X 面在 x0+16.0），故上界含等号
            assert!(
                (48.0..=64.0).contains(&p[0]),
                "x {:?} outside chunk 3 world footprint [48,64]",
                p
            );
            assert!(
                (80.0..=96.0).contains(&p[2]),
                "z {:?} outside chunk 5 world footprint [80,96]",
                p
            );
            assert!((0.0..=96.0).contains(&p[1]), "y {:?} outside world height", p);
        }
    }
}
