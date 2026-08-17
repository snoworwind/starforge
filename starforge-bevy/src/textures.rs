//! Procedural pixel-art textures — port of js/textures.js (62-tile block atlas + item icons).
//! All painters follow the exact spec in TEXTURES_SPEC.md (registration order = tile index).

use crate::rng::Rng;
use std::collections::HashMap;

pub type Pixel = [u8; 4];

fn hex(h: &str) -> Pixel {
    let h = h.trim_start_matches('#');
    let parse = |s: &str| u8::from_str_radix(s, 16).unwrap_or(0);
    match h.len() {
        6 => {
            let r = parse(&h[0..2]);
            let g = parse(&h[2..4]);
            let b = parse(&h[4..6]);
            [r, g, b, 255]
        }
        8 => {
            let r = parse(&h[0..2]);
            let g = parse(&h[2..4]);
            let b = parse(&h[4..6]);
            let a = parse(&h[6..8]);
            [r, g, b, a]
        }
        _ => [0, 0, 0, 255],
    }
}

fn shade_p(h: &str, f: f32) -> Pixel {
    let p = hex(h);
    let c = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    [c(p[0]), c(p[1]), c(p[2]), p[3]]
}

/// Speckle fill: every pixel uniformly picked from the palette (row-major, seeded stream).
fn speckle(buf: &mut [Pixel; 256], rng: &mut Rng, palette: &[Pixel]) {
    for p in buf.iter_mut() {
        *p = palette[((rng.next() * palette.len() as f32) as usize).min(palette.len() - 1)];
    }
}

fn set(buf: &mut [Pixel; 256], x: i32, y: i32, c: Pixel) {
    if (0..16).contains(&x) && (0..16).contains(&y) {
        buf[(y * 16 + x) as usize] = c;
    }
}

const TS: usize = 16;

/// A painter receives a 16×16 buffer and a deterministic RNG.
type Painter = fn(&mut [Pixel; 256], &mut Rng);

fn ore_inner(buf: &mut [Pixel; 256], rng: &mut Rng, color: Pixel, hi: Pixel, glow: Option<Pixel>, outline: Pixel) {
    speckle(buf, rng, &[hex("#8c8c8c"), hex("#828282"), hex("#969696"), hex("#7a7a7a")]);
    let clusters = 3 + ((rng.next() * 2.0) as usize);
    for _ in 0..clusters {
        let mut x = 2 + ((rng.next() * 9.0) as i32);
        let mut y = 2 + ((rng.next() * 9.0) as i32);
        let steps = 4 + ((rng.next() * 3.0) as usize);
        let mut cells: Vec<(i32, i32)> = Vec::new();
        for _ in 0..steps {
            if !cells.contains(&(x, y)) {
                cells.push((x, y));
            }
            x += (rng.next() * 3.0) as i32 - 1;
            y += (rng.next() * 3.0) as i32 - 1;
            x = x.clamp(1, 14);
            y = y.clamp(1, 14);
        }
        for &(cx, cy) in &cells {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                set(buf, cx + dx, cy + dy, outline);
            }
        }
        for &(cx, cy) in &cells {
            set(buf, cx, cy, color);
        }
        let top = cells.iter().min_by_key(|c| c.1).copied().unwrap_or((0, 0));
        set(buf, top.0, (top.1 - 1).max(0), hi);
        if let Some(g) = glow {
            if rng.next() < 0.7 {
                set(buf, (top.0 + 1).min(15), top.1, g);
            }
        }
    }
}

macro_rules! ore_painter {
    ($name:ident, $color:literal, $hi:literal, $glow:expr, $outline:literal) => {
        fn $name(buf: &mut [Pixel; 256], rng: &mut Rng) {
            let glow: Option<Pixel> = match $glow {
                Some(g) => Some(hex(g)),
                None => None,
            };
            ore_inner(buf, rng, hex($color), hex($hi), glow, hex($outline));
        }
    };
}

ore_painter!(ore_coal, "#2b2b2b", "#5a5a5a", None, "#1a1a1a");
ore_painter!(ore_iron, "#d8af93", "#f0d2b8", None, "#a87a5e");
ore_painter!(ore_copper, "#d17f4a", "#f0a877", None, "#9a5a2e");
ore_painter!(ore_titanium, "#e6eef4", "#ffffff", None, "#7a8a94");
ore_painter!(ore_uranium, "#69d436", "#a2f078", Some("#c6ff9e"), "#3a8a18");
ore_painter!(ore_gold, "#f5cd3a", "#ffe98a", None, "#b8921a");

pub struct Atlas {
    pub index: HashMap<&'static str, usize>,
    pub tiles: Vec<[Pixel; 256]>,
}

impl Atlas {
    /// Build the full 62-tile atlas in the exact registration order of textures.js.
    pub fn build() -> Self {
        let mut index = HashMap::new();
        let mut tiles: Vec<[Pixel; 256]> = Vec::new();
        // (name, painter) in exact order
        let painters: Vec<(&'static str, Painter)> = vec![
            ("grass_top", |b, r| speckle(b, r, &[hex("#69b23f"), hex("#5da337"), hex("#74bd48"), hex("#619f3b"), hex("#7cc44f")])),
            ("dirt", |b, r| speckle(b, r, &[hex("#8a5f3c"), hex("#7d5535"), hex("#95683f"), hex("#775033"), hex("#8a6039")])),
            ("grass_side", |b, r| {
                speckle(b, r, &[hex("#8a5f3c"), hex("#7d5535"), hex("#95683f"), hex("#775033")]);
                for x in 0..16 {
                    let h = 3 + ((r.next() * 2.4) as i32);
                    for y in 0..h {
                        set(b, x, y, [hex("#69b23f"), hex("#5da337"), hex("#74bd48")][((r.next() * 3.0) as usize).min(2)]);
                    }
                }
            }),
            ("stone", |b, r| {
                speckle(b, r, &[hex("#8c8c8c"), hex("#828282"), hex("#969696"), hex("#7a7a7a")]);
                for _ in 0..5 {
                    let x = (r.next() * 14.0) as i32;
                    let y = (r.next() * 14.0) as i32;
                    set(b, x, y, hex("#a3a3a3"));
                    set(b, x + 1, y, hex("#a3a3a3"));
                }
            }),
            ("sand", |b, r| speckle(b, r, &[hex("#e0d29a"), hex("#d8c98e"), hex("#e8dba6"), hex("#d0c184")])),
            ("gravel", |b, r| speckle(b, r, &[hex("#8f8b87"), hex("#7c7975"), hex("#a09b96"), hex("#6e6a66"), hex("#95908b")])),
            ("log_side", |b, r| {
                let bands = [hex("#6b502f"), hex("#5e4629"), hex("#755834"), hex("#634a2b")];
                for x in 0..16 {
                    let band = bands[(x % 4) as usize];
                    for y in 0..16 {
                        set(b, x, y, if r.next() < 0.85 { band } else { shade_p("#6b502f", 0.8 + r.next() * 0.4) });
                    }
                }
            }),
            ("log_top", |b, r| {
                speckle(b, r, &[hex("#b08d55"), hex("#a5854f")]);
                let mut ring = 7i32;
                while ring >= 1 {
                    for a in 0..64 {
                        let x = 8 + ((a as f32 / 64.0 * std::f32::consts::TAU).cos() * ring as f32 * 0.9).round() as i32;
                        let y = 8 + ((a as f32 / 64.0 * std::f32::consts::TAU).sin() * ring as f32 * 0.9).round() as i32;
                        set(b, x, y, hex("#8a6b3d"));
                    }
                    ring -= 2;
                }
                for i in 0..16 {
                    set(b, i, 0, hex("#6b502f"));
                    set(b, i, 15, hex("#6b502f"));
                    set(b, 0, i, hex("#6b502f"));
                    set(b, 15, i, hex("#6b502f"));
                }
            }),
            ("leaves", |b, r| {
                let pal = [hex("#3f7d2c"), hex("#357024"), hex("#488a33"), hex("#2e6420")];
                for y in 0..16 {
                    for x in 0..16 {
                        if r.next() < 0.24 {
                            set(b, x, y, [0, 0, 0, 0]);
                            continue;
                        }
                        set(b, x, y, pal[((r.next() * 4.0) as usize).min(3)]);
                        if r.next() < 0.06 {
                            set(b, x, y, hex("#5aa93f"));
                        }
                    }
                }
            }),
            ("planks", |b, r| {
                speckle(b, r, &[hex("#a8824f"), hex("#9d7948"), hex("#b28a55")]);
                for y in (3..16).step_by(4) {
                    for x in 0..16 {
                        set(b, x, y, hex("#7a5c35"));
                    }
                }
                set(b, 4, 1, hex("#7a5c35"));
                set(b, 11, 5, hex("#7a5c35"));
                set(b, 2, 9, hex("#7a5c35"));
                set(b, 13, 13, hex("#7a5c35"));
            }),
            ("water", |b, r| speckle(b, r, &[hex("#3e6bd6"), hex("#3862c7"), hex("#4675e0"), hex("#3455b8")])),
            ("ice", |b, r| {
                speckle(b, r, &[hex("#a8d4f0"), hex("#9ccbeb"), hex("#b6ddf5")]);
                for (x, y) in [(3, 4), (4, 5), (10, 9), (11, 10), (12, 3)] {
                    set(b, x, y, hex("#e0f2fc"));
                }
            }),
            ("snow_top", |b, r| speckle(b, r, &[hex("#f2f6fa"), hex("#e8eef5"), hex("#fafcff"), hex("#e0e8f0")])),
            ("snow_side", |b, r| {
                speckle(b, r, &[hex("#8a5f3c"), hex("#7d5535"), hex("#95683f")]);
                for x in 0..16 {
                    for y in 0..4 {
                        set(b, x, y, [hex("#f2f6fa"), hex("#e8eef5")][((r.next() * 2.0) as usize).min(1)]);
                    }
                }
            }),
            ("basalt", |b, r| {
                speckle(b, r, &[hex("#3a3a42"), hex("#33333a"), hex("#42424c"), hex("#2c2c33")]);
                for _ in 0..4 {
                    let x = (r.next() * 15.0) as i32;
                    let y = (r.next() * 15.0) as i32;
                    set(b, x, y, hex("#ff7733"));
                    if r.next() < 0.5 {
                        set(b, x + 1, y, hex("#c94f1e"));
                    }
                }
            }),
            ("alien_top", |b, r| speckle(b, r, &[hex("#9a5fd0"), hex("#8b52c2"), hex("#a86ddb"), hex("#7d47b3"), hex("#b078e0")])),
            ("alien_side", |b, r| {
                speckle(b, r, &[hex("#6e4a8a"), hex("#61407c"), hex("#7b5498")]);
                for x in 0..16 {
                    let h = 3 + ((r.next() * 2.2) as i32);
                    for y in 0..h {
                        set(b, x, y, [hex("#9a5fd0"), hex("#a86ddb")][((r.next() * 2.0) as usize).min(1)]);
                    }
                }
            }),
            ("barrier", |b, r| {
                speckle(b, r, &[hex("#2a2a30"), hex("#222228"), hex("#32323a")]);
                for i in 0..16 {
                    set(b, i, i, hex("#4a4a55"));
                    set(b, 15 - i, i, hex("#4a4a55"));
                }
            }),
            ("crystal", |b, r| {
                speckle(b, r, &[hex("#1a4a50"), hex("#153c42"), hex("#20585e")]);
                for _ in 0..5 {
                    let x = 1 + ((r.next() * 12.0) as i32);
                    let y = 1 + ((r.next() * 12.0) as i32);
                    set(b, x, y, hex("#7fe8e0"));
                    set(b, x + 1, y + 1, hex("#aef7f2"));
                    set(b, x, y + 1, hex("#5ec8c0"));
                    if r.next() < 0.5 {
                        set(b, x + 1, y, hex("#ffffff"));
                    }
                }
            }),
            ("mush_stem", |b, r| {
                let bands = [hex("#e8dcc8"), hex("#dccfb8"), hex("#f0e6d4")];
                for x in 0..16 {
                    let band = bands[(x % 3) as usize];
                    for y in 0..16 {
                        set(b, x, y, if r.next() < 0.9 { band } else { hex("#c4b8a2") });
                    }
                }
                for i in 0..16 {
                    set(b, 0, i, hex("#b8ab94"));
                    set(b, 15, i, hex("#b8ab94"));
                }
            }),
            ("mush_cap", |b, r| {
                speckle(b, r, &[hex("#a04fc8"), hex("#9445ba"), hex("#ad5cd4"), hex("#8a3dad")]);
                for _ in 0..5 {
                    let x = 1 + ((r.next() * 12.0) as i32);
                    let y = 1 + ((r.next() * 12.0) as i32);
                    set(b, x, y, hex("#f0e0f8"));
                    set(b, x + 1, y, hex("#f0e0f8"));
                    set(b, x, y + 1, hex("#f0e0f8"));
                    set(b, x + 1, y + 1, hex("#e0c8ec"));
                }
            }),
            ("ash", |b, r| {
                speckle(b, r, &[hex("#5c5a56"), hex("#524f4c"), hex("#66625e"), hex("#48453f")]);
                for _ in 0..3 {
                    let x = (r.next() * 15.0) as i32;
                    let y = (r.next() * 15.0) as i32;
                    set(b, x, y, if r.next() < 0.5 { hex("#8a4a2a") } else { hex("#3a3a3a") });
                }
            }),
            ("amber", |b, r| {
                speckle(b, r, &[hex("#e0a63a"), hex("#d49830"), hex("#ecb448"), hex("#c88a28")]);
                for _ in 0..4 {
                    let x = 1 + ((r.next() * 13.0) as i32);
                    let y = 1 + ((r.next() * 13.0) as i32);
                    set(b, x, y, hex("#8a5a14"));
                    if r.next() < 0.5 {
                        set(b, x + 1, y, hex("#6e4610"));
                    }
                    set(b, x - 1, y - 1, hex("#f8d878"));
                }
            }),
            ("rust", |b, r| {
                speckle(b, r, &[hex("#9a5a38"), hex("#8a4e30"), hex("#a86a42"), hex("#7c452a")]);
                for _ in 0..5 {
                    let x = (r.next() * 15.0) as i32;
                    let y = (r.next() * 15.0) as i32;
                    set(b, x, y, if r.next() < 0.5 { hex("#c8875a") } else { hex("#5e3520") });
                    if r.next() < 0.3 {
                        set(b, x + 1, y, hex("#d8d8dc"));
                    }
                }
            }),
            ("salt", |b, r| {
                speckle(b, r, &[hex("#f0f2f4"), hex("#e6e9ec"), hex("#f8fafc"), hex("#dde2e6")]);
                for _ in 0..4 {
                    let x = 1 + ((r.next() * 13.0) as i32);
                    let y = 1 + ((r.next() * 13.0) as i32);
                    set(b, x, y, hex("#c2c9ce"));
                    set(b, x + 1, y, hex("#c2c9ce"));
                    set(b, x + 1, y + 1, hex("#c2c9ce"));
                }
            }),
            ("obsidian", |b, r| {
                speckle(b, r, &[hex("#1c1a26"), hex("#16141f"), hex("#24202e"), hex("#120f1a")]);
                for _ in 0..3 {
                    let x = 1 + ((r.next() * 12.0) as i32);
                    let y = 1 + ((r.next() * 12.0) as i32);
                    set(b, x, y, hex("#6a5a9a"));
                    set(b, x + 1, y + 1, hex("#48406e"));
                    if r.next() < 0.4 {
                        set(b, x + 2, y + 2, hex("#8a7ab8"));
                    }
                }
            }),
            ("redmoss_top", |b, r| speckle(b, r, &[hex("#b04a38"), hex("#a04230"), hex("#c05642"), hex("#943a2a"), hex("#c86a50")])),
            ("redmoss_side", |b, r| {
                speckle(b, r, &[hex("#8a5f3c"), hex("#7d5535"), hex("#95683f")]);
                for x in 0..16 {
                    let h = 3 + ((r.next() * 2.2) as i32);
                    for y in 0..h {
                        set(b, x, y, [hex("#b04a38"), hex("#c05642")][((r.next() * 2.0) as usize).min(1)]);
                    }
                }
            }),
            ("hive", |b, r| {
                speckle(b, r, &[hex("#d8862a"), hex("#c87822"), hex("#e69634")]);
                for cy in 0..2i32 {
                    for cx in 0..2i32 {
                        let ox = cx * 8 + (cy % 2) * 4;
                        let oy = cy * 8;
                        for a in 0..12 {
                            let x = (ox + 3 + ((a as f32 / 12.0 * std::f32::consts::TAU).cos() * 2.6).round() as i32) & 15;
                            let y = (oy + 3 + ((a as f32 / 12.0 * std::f32::consts::TAU).sin() * 2.6).round() as i32) & 15;
                            set(b, x, y, hex("#8a5210"));
                        }
                        set(b, (ox + 3) & 15, (oy + 3) & 15, hex("#5e3808"));
                    }
                }
            }),
            ("murk_top", |b, r| {
                speckle(b, r, &[hex("#1e5a4c"), hex("#1a4f42"), hex("#246656"), hex("#16453a")]);
                for _ in 0..4 {
                    set(b, (r.next() * 15.0) as i32, (r.next() * 15.0) as i32, hex("#4ee8b8"));
                }
            }),
            ("murk_side", |b, r| {
                speckle(b, r, &[hex("#4a4238"), hex("#3f382f"), hex("#554c40")]);
                for x in 0..16 {
                    let h = 3 + ((r.next() * 2.0) as i32);
                    for y in 0..h {
                        set(b, x, y, [hex("#1e5a4c"), hex("#246656")][((r.next() * 2.0) as usize).min(1)]);
                    }
                }
            }),
            ("glow_shroom", |b, _r| {
                set(b, 7, 15, hex("#3a5248"));
                set(b, 8, 14, hex("#2e453c"));
                set(b, 7, 13, hex("#3a5248"));
                set(b, 8, 12, hex("#2e453c"));
                let c = hex("#4ee8b8");
                let h = hex("#b8ffe8");
                let d = hex("#2aa882");
                set(b, 6, 9, c);
                set(b, 7, 9, c);
                set(b, 8, 9, c);
                set(b, 9, 9, c);
                set(b, 5, 10, d);
                set(b, 10, 10, d);
                set(b, 6, 8, h);
                set(b, 7, 7, h);
                set(b, 8, 8, c);
                set(b, 9, 8, d);
                set(b, 7, 10, hex("#e8fff6"));
                set(b, 8, 10, hex("#e8fff6"));
            }),
            ("coal_ore", ore_coal),
            ("iron_ore", ore_iron),
            ("copper_ore", ore_copper),
            ("titanium_ore", ore_titanium),
            ("uranium_ore", ore_uranium),
            ("gold_ore", ore_gold),
            ("sodium_plant", |b, _r| {
                for (x, y) in [(7, 15), (8, 15), (7, 14), (8, 13), (7, 12)] {
                    set(b, x, y, if y == 12 { hex("#488a33") } else if (x, y) == (8, 15) { hex("#357024") } else { hex("#3f7d2c") });
                }
                let c = hex("#ffd23e");
                let h = hex("#fff2ae");
                let d = hex("#d9a80f");
                set(b, 7, 8, c);
                set(b, 8, 8, c);
                set(b, 7, 9, c);
                set(b, 8, 9, h);
                set(b, 6, 6, c);
                set(b, 10, 7, d);
                set(b, 7, 5, h);
                set(b, 9, 10, d);
                set(b, 5, 9, c);
                set(b, 9, 5, c);
            }),
            ("oxygen_plant", |b, _r| {
                for (x, y, c) in [(8, 15, "#3f7d2c"), (8, 14, "#357024"), (7, 13, "#3f7d2c"), (8, 12, "#488a33")] {
                    set(b, x, y, hex(c));
                }
                let c = hex("#ff5a4e");
                let h = hex("#ffb0a8");
                let d = hex("#c22e24");
                set(b, 7, 8, c);
                set(b, 8, 8, c);
                set(b, 7, 9, c);
                set(b, 8, 9, h);
                set(b, 6, 7, d);
                set(b, 9, 7, c);
                set(b, 6, 10, c);
                set(b, 9, 10, d);
                set(b, 7, 6, h);
                set(b, 8, 11, c);
            }),
            ("carbon_fern", |b, r| {
                let pal = [hex("#2e6420"), hex("#3f7d2c"), hex("#244f19")];
                for _ in 0..12 {
                    let x = 3 + ((r.next() * 10.0) as i32);
                    let y = 4 + ((r.next() * 11.0) as i32);
                    set(b, x, y, pal[((r.next() * 3.0) as usize).min(2)]);
                }
                set(b, 7, 15, hex("#244f19"));
                set(b, 8, 14, hex("#2e6420"));
                set(b, 7, 13, hex("#244f19"));
                set(b, 8, 12, hex("#2e6420"));
            }),
            ("glass", |b, _r| {
                for i in 0..16 {
                    set(b, i, 0, hex("#cfeef5"));
                    set(b, i, 15, hex("#cfeef5"));
                    set(b, 0, i, hex("#cfeef5"));
                    set(b, 15, i, hex("#cfeef5"));
                }
                set(b, 3, 3, hex("#ffffffcc"));
                set(b, 4, 4, hex("#ffffff99"));
                set(b, 5, 5, hex("#ffffff66"));
            }),
            ("lamp_on", |b, r| {
                speckle(b, r, &[hex("#ffe9a8"), hex("#fff3c8"), hex("#ffdf8e")]);
                for i in 0..16 {
                    set(b, i, 0, hex("#8a6b2d"));
                    set(b, i, 15, hex("#8a6b2d"));
                    set(b, 0, i, hex("#8a6b2d"));
                    set(b, 15, i, hex("#8a6b2d"));
                }
            }),
            ("metal", |b, r| {
                speckle(b, r, &[hex("#9aa7b0"), hex("#909da6"), hex("#a4b1ba"), hex("#8a97a0")]);
                for i in 0..16 {
                    set(b, i, 0, hex("#b8c5ce"));
                    set(b, 0, i, hex("#b8c5ce"));
                    set(b, i, 15, hex("#6a7780"));
                    set(b, 15, i, hex("#6a7780"));
                }
                for (x, y) in [(2, 2), (13, 2), (2, 13), (13, 13)] {
                    set(b, x, y, hex("#5f6b73"));
                }
            }),
            ("metal_dark", |b, r| {
                speckle(b, r, &[hex("#4e5a63"), hex("#46525b"), hex("#57636c")]);
                for i in 0..16 {
                    set(b, i, 0, hex("#68747d"));
                    set(b, 0, i, hex("#68747d"));
                    set(b, i, 15, hex("#333d44"));
                    set(b, 15, i, hex("#333d44"));
                }
            }),
            ("vent", |b, r| {
                speckle(b, r, &[hex("#4e5a63"), hex("#46525b")]);
                for y in (2..14).step_by(3) {
                    for x in 2..14 {
                        set(b, x, y, hex("#222a30"));
                        set(b, x, y + 1, hex("#68747d"));
                    }
                }
            }),
            ("furnace_front", |b, r| {
                speckle(b, r, &[hex("#8c8c8c"), hex("#828282"), hex("#969696")]);
                for y in 8..14 {
                    for x in 4..12 {
                        set(b, x, y, hex("#1d1d1d"));
                    }
                }
                for x in 3..13 {
                    set(b, x, 7, hex("#5a5a5a"));
                    set(b, x, 14, hex("#5a5a5a"));
                }
            }),
            ("furnace_on", |b, r| {
                speckle(b, r, &[hex("#8c8c8c"), hex("#828282"), hex("#969696")]);
                let flame = [hex("#ff8c1a"), hex("#ffb31a"), hex("#ff6600"), hex("#ffd21a")];
                for y in 8..14 {
                    for x in 4..12 {
                        set(b, x, y, flame[((r.next() * 4.0) as usize).min(3)]);
                    }
                }
                for x in 3..13 {
                    set(b, x, 7, hex("#5a5a5a"));
                    set(b, x, 14, hex("#5a5a5a"));
                }
            }),
            ("belt", |b, r| {
                speckle(b, r, &[hex("#3a4148"), hex("#333a40"), hex("#424a52")]);
                for x in 0..16 {
                    set(b, x, 0, hex("#586269"));
                    set(b, x, 15, hex("#586269"));
                }
                for oy in [2i32, 10] {
                    set(b, 3, oy, hex("#ffcf4d"));
                    set(b, 4, oy + 1, hex("#ffcf4d"));
                    set(b, 5, oy + 2, hex("#ffcf4d"));
                    set(b, 4, oy + 3, hex("#ffcf4d"));
                    set(b, 3, oy + 4, hex("#ffcf4d"));
                    set(b, 9, oy, hex("#e6b23a"));
                    set(b, 10, oy + 1, hex("#e6b23a"));
                    set(b, 11, oy + 2, hex("#e6b23a"));
                    set(b, 10, oy + 3, hex("#e6b23a"));
                    set(b, 9, oy + 4, hex("#e6b23a"));
                }
            }),
            ("belt_turn", |b, r| {
                speckle(b, r, &[hex("#3a4148"), hex("#333a40"), hex("#424a52")]);
                for x in 0..16 {
                    set(b, x, 0, hex("#586269"));
                }
                for y in 0..16 {
                    set(b, 0, y, hex("#586269"));
                }
                for a in 0..26 {
                    let t = a as f32 / 25.0 * std::f32::consts::FRAC_PI_2;
                    let x = (15.0 - t.cos() * 12.0).round() as i32;
                    let y = (15.0 - t.sin() * 12.0).round() as i32;
                    set(b, x, y, hex("#ffcf4d"));
                    let x2 = (15.0 - t.cos() * 6.0).round() as i32;
                    let y2 = (15.0 - t.sin() * 6.0).round() as i32;
                    set(b, x2, y2, hex("#e6b23a"));
                }
                set(b, 13, 12, hex("#ffcf4d"));
                set(b, 12, 13, hex("#ffcf4d"));
            }),
            ("wind_pole", |b, r| {
                speckle(b, r, &[hex("#c8d2d8"), hex("#bcc6cc"), hex("#d2dce2")]);
                for i in 0..16 {
                    set(b, 0, i, hex("#98a2a8"));
                    set(b, 15, i, hex("#98a2a8"));
                }
                for (x, y) in [(7, 3), (8, 3), (7, 10), (8, 10)] {
                    set(b, x, y, hex("#8a97a0"));
                }
            }),
            ("miner_top", |b, r| {
                speckle(b, r, &[hex("#9aa7b0"), hex("#909da6"), hex("#a4b1ba")]);
                for y in 4..12 {
                    for x in 4..12 {
                        set(b, x, y, hex("#333d44"));
                    }
                }
                for i in 5..11 {
                    set(b, i, i, hex("#ffcf4d"));
                    set(b, 16 - i, i, hex("#ffcf4d"));
                }
                for i in 0..16 {
                    set(b, i, 0, hex("#b8c5ce"));
                    set(b, 0, i, hex("#b8c5ce"));
                    set(b, i, 15, hex("#6a7780"));
                    set(b, 15, i, hex("#6a7780"));
                }
            }),
            ("assembler_top", |b, r| {
                speckle(b, r, &[hex("#9aa7b0"), hex("#909da6"), hex("#a4b1ba")]);
                for y in 3..13 {
                    for x in 3..13 {
                        set(b, x, y, hex("#1a2a38"));
                    }
                }
                set(b, 7, 7, hex("#35e0e8"));
                set(b, 8, 7, hex("#35e0e8"));
                set(b, 7, 8, hex("#35e0e8"));
                set(b, 8, 8, hex("#7ff5fa"));
                for i in 0..16 {
                    set(b, i, 0, hex("#b8c5ce"));
                    set(b, 0, i, hex("#b8c5ce"));
                    set(b, i, 15, hex("#6a7780"));
                    set(b, 15, i, hex("#6a7780"));
                }
            }),
            ("solar_top", |b, r| {
                let cells = [hex("#16294e"), hex("#1a3160"), hex("#122342")];
                for y in 0..16 {
                    for x in 0..16 {
                        if x % 5 == 0 || y % 8 == 7 {
                            set(b, x, y, hex("#8a97a0"));
                        } else {
                            set(b, x, y, cells[((r.next() * 3.0) as usize).min(2)]);
                        }
                    }
                }
                for (x, y) in [(3, 2), (8, 4), (12, 9)] {
                    set(b, x, y, hex("#4a6dc0"));
                }
            }),
            ("chest_side", |b, r| {
                speckle(b, r, &[hex("#a8824f"), hex("#9d7948"), hex("#b28a55")]);
                for i in 0..16 {
                    set(b, i, 0, hex("#7a5c35"));
                    set(b, i, 15, hex("#7a5c35"));
                    set(b, 0, i, hex("#7a5c35"));
                    set(b, 15, i, hex("#7a5c35"));
                }
                for x in 0..16 {
                    set(b, x, 6, hex("#63482a"));
                }
                set(b, 7, 6, hex("#d8d8d8"));
                set(b, 8, 6, hex("#d8d8d8"));
                set(b, 7, 7, hex("#b8b8b8"));
                set(b, 8, 7, hex("#b8b8b8"));
            }),
            ("refinery_side", |b, r| {
                speckle(b, r, &[hex("#4e5a63"), hex("#46525b"), hex("#57636c")]);
                for y in 3..13 {
                    set(b, 4, y, hex("#ff8c1a"));
                    set(b, 5, y, hex("#c9641a"));
                    set(b, 10, y, hex("#35e0e8"));
                    set(b, 11, y, hex("#1a8a90"));
                }
                for i in 0..16 {
                    set(b, i, 0, hex("#68747d"));
                    set(b, i, 15, hex("#333d44"));
                }
            }),
            ("reactor_side", |b, r| {
                speckle(b, r, &[hex("#4e5a63"), hex("#46525b"), hex("#57636c")]);
                let core = [hex("#69d436"), hex("#a2f078"), hex("#4caf1e")];
                for y in 4..12 {
                    for x in 6..10 {
                        set(b, x, y, core[((r.next() * 3.0) as usize).min(2)]);
                    }
                }
                for i in 0..16 {
                    set(b, i, 0, hex("#68747d"));
                    set(b, 0, i, hex("#68747d"));
                    set(b, i, 15, hex("#333d44"));
                    set(b, 15, i, hex("#333d44"));
                }
            }),
            ("launchpad_top", |b, r| {
                speckle(b, r, &[hex("#4e5a63"), hex("#46525b")]);
                for i in 0..16 {
                    if i % 4 < 2 {
                        set(b, i, 0, hex("#ffcf4d"));
                        set(b, i, 15, hex("#ffcf4d"));
                        set(b, 0, i, hex("#ffcf4d"));
                        set(b, 15, i, hex("#ffcf4d"));
                    }
                }
                for a in 0..40 {
                    let x = 8 + ((a as f32 / 40.0 * std::f32::consts::TAU).cos() * 5.0).round() as i32;
                    let y = 8 + ((a as f32 / 40.0 * std::f32::consts::TAU).sin() * 5.0).round() as i32;
                    set(b, x, y, hex("#ffcf4d"));
                }
                for (x, y) in [(7, 8), (8, 8), (8, 7), (7, 7)] {
                    set(b, x, y, hex("#ffcf4d"));
                }
            }),
            ("storage_top", |b, r| {
                speckle(b, r, &[hex("#a8824f"), hex("#9d7948")]);
                for i in 0..16 {
                    set(b, i, 0, hex("#7a5c35"));
                    set(b, i, 15, hex("#7a5c35"));
                    set(b, 0, i, hex("#7a5c35"));
                    set(b, 15, i, hex("#7a5c35"));
                }
            }),
            ("medbay_top", |b, r| {
                speckle(b, r, &[hex("#4e5a63"), hex("#46525b"), hex("#57636c")]);
                for y in 4..11 {
                    set(b, 7, y, hex("#7dff8a"));
                    set(b, 8, y, hex("#7dff8a"));
                }
                for x in 4..11 {
                    set(b, x, 7, hex("#7dff8a"));
                    set(b, x, 8, hex("#7dff8a"));
                }
                for i in 0..16 {
                    set(b, i, 0, hex("#68747d"));
                    set(b, 0, i, hex("#68747d"));
                    set(b, i, 15, hex("#333d44"));
                    set(b, 15, i, hex("#333d44"));
                }
            }),
            ("slab", |b, r| {
                speckle(b, r, &[hex("#8c8c8c"), hex("#828282"), hex("#969696"), hex("#7a7a7a")]);
                for i in 0..16 {
                    set(b, i, 0, hex("#a8a8a8"));
                    set(b, i, 1, hex("#9c9c9c"));
                    set(b, i, 15, hex("#5a5a5a"));
                }
                for x in (2..14).step_by(4) {
                    set(b, x, 8, hex("#9c9c9c"));
                    set(b, x + 1, 9, hex("#9c9c9c"));
                }
            }),
            ("concrete", |b, r| {
                speckle(b, r, &[hex("#9aa3ab"), hex("#8f989f"), hex("#a5aeb6"), hex("#848d94")]);
                for i in 0..16 {
                    set(b, i, 0, hex("#b8c0c7"));
                    set(b, 0, i, hex("#a8b0b8"));
                }
                for (x, y) in [(3, 4), (4, 5), (11, 9), (12, 10)] {
                    set(b, x, y, hex("#7a828a"));
                }
            }),
        ];
        for (name, painter) in painters {
            let idx = tiles.len();
            let mut buf = [Pixel::default(); 256];
            let seed = (idx as u32).wrapping_mul(7919).wrapping_add(13);
            painter(&mut buf, &mut Rng::new(seed));
            tiles.push(buf);
            index.insert(name, idx);
        }
        Self { index, tiles }
    }

    pub fn tile_idx(&self, name: &str) -> usize {
        *self.index.get(name).unwrap_or(&0)
    }

    pub fn tile(&self, name: &str) -> &[Pixel; 256] {
        &self.tiles[self.tile_idx(name)]
    }

    /// UV rect for a tile in Bevy convention (v=0 is image top, no flip).
    /// Returns [u0, v0, u1, v1] with v0 = top, v1 = bottom.
    pub fn uv_rect(&self, name: &str) -> [f32; 4] {
        let i = self.tile_idx(name);
        let c = i % 16;
        let r = i / 16;
        [
            c as f32 / 16.0,
            r as f32 / 16.0,
            (c + 1) as f32 / 16.0,
            (r + 1) as f32 / 16.0,
        ]
    }

    /// Flatten the atlas into an RGBA8 image (256×256).
    pub fn to_image(&self) -> Vec<u8> {
        let mut out = vec![0u8; 256 * 256 * 4];
        for (ti, tile) in self.tiles.iter().enumerate() {
            let col = ti % 16;
            let row = ti / 16;
            for y in 0..TS {
                for x in 0..TS {
                    let src = tile[y * TS + x];
                    let ox = col * TS + x;
                    let oy = row * TS + y;
                    let d = (oy * 256 + ox) * 4;
                    out[d..d + 4].copy_from_slice(&src);
                }
            }
        }
        out
    }
}

// ================= Item icons (32×32) =================

pub type IconBuf = [[Pixel; 32]; 32];

fn icon_set(buf: &mut IconBuf, x: i32, y: i32, w: i32, h: i32, c: Pixel) {
    for yy in y..y + h {
        for xx in x..x + w {
            if (0..32).contains(&xx) && (0..32).contains(&yy) {
                buf[yy as usize][xx as usize] = c;
            }
        }
    }
}

/// scale/multiply an RGB pixel by f (source-atop darken).
fn mul_p(c: Pixel, f: f32) -> Pixel {
    let m = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    [m(c[0]), m(c[1]), m(c[2]), c[3]]
}

/// drawImage with a 2×2 affine matrix, nearest-neighbor, from a 16×16 tile scaled to (sx, sy).
fn affine_blit(
    dst: &mut IconBuf,
    src: &[Pixel; 256],
    m: [[f32; 2]; 2],
    tx: f32,
    ty: f32,
    sx: f32,
    sy: f32,
    darken: f32,
) {
    for sy_ in 0..TS {
        for sx_ in 0..TS {
            let u = sx_ as f32 / TS as f32 * sx;
            let v = sy_ as f32 / TS as f32 * sy;
            let dx = (m[0][0] * u + m[0][1] * v + tx).round() as i32;
            let dy = (m[1][0] * u + m[1][1] * v + ty).round() as i32;
            if (0..32).contains(&dx) && (0..32).contains(&dy) {
                let p = src[sy_ * TS + sx_];
                dst[dy as usize][dx as usize] = if darken < 1.0 { mul_p(p, darken) } else { p };
            }
        }
    }
}

/// Isometric block icon (blockIcon).
fn block_icon(atlas: &Atlas, top: &str, side: &str, side2: &str) -> IconBuf {
    let mut buf = IconBuf::default();
    // top (unshaded)
    affine_blit(&mut buf, atlas.tile(top), [[1.0, -1.0], [0.5, 0.5]], 16.0, 1.0, 15.0, 15.0, 1.0);
    // left (25% black)
    affine_blit(&mut buf, atlas.tile(side), [[1.0, 0.0], [0.5, 1.0]], 1.0, 8.5, 15.0, 15.5, 0.75);
    // right (45% black)
    affine_blit(&mut buf, atlas.tile(side2), [[1.0, 0.0], [-0.5, 1.0]], 16.0, 16.0, 15.0, 15.5, 0.55);
    buf
}

/// Flat 2× nearest upscale (flatIcon).
fn flat_icon(atlas: &Atlas, tile: &str) -> IconBuf {
    let mut buf = IconBuf::default();
    let src = atlas.tile(tile);
    for y in 0..TS {
        for x in 0..TS {
            let p = src[y * TS + x];
            for yy in 0..2 {
                for xx in 0..2 {
                    buf[y * 2 + yy][x * 2 + xx] = p;
                }
            }
        }
    }
    buf
}

fn ingot_icon(c1: &str, c2: &str) -> IconBuf {
    let mut buf = IconBuf::default();
    let dark = shade_p(c1, 0.6);
    let hi = hex(c2);
    icon_set(&mut buf, 6, 16, 20, 8, dark);
    icon_set(&mut buf, 4, 14, 20, 8, hex(c1));
    icon_set(&mut buf, 4, 12, 20, 3, hi);
    icon_set(&mut buf, 6, 24, 20, 1, shade_p(c1, 0.45));
    icon_set(&mut buf, 5, 13, 8, 1, hex("#ffffff88"));
    buf
}

fn crystal_icon(c1: &str, c2: &str) -> IconBuf {
    let mut buf = IconBuf::default();
    let d = shade_p(c1, 0.55);
    icon_set(&mut buf, 14, 4, 4, 4, hex(c2));
    icon_set(&mut buf, 12, 8, 8, 10, hex(c1));
    icon_set(&mut buf, 10, 12, 4, 8, d);
    icon_set(&mut buf, 18, 10, 6, 12, hex(c1));
    icon_set(&mut buf, 8, 18, 6, 8, hex(c1));
    icon_set(&mut buf, 20, 6, 2, 4, hex(c2));
    icon_set(&mut buf, 15, 9, 2, 5, hex("#ffffffaa"));
    icon_set(&mut buf, 6, 26, 20, 2, d);
    buf
}

fn chunk_icon(c1: &str) -> IconBuf {
    let mut buf = IconBuf::default();
    let d = shade_p(c1, 0.6);
    let h = shade_p(c1, 1.35);
    icon_set(&mut buf, 8, 10, 10, 9, hex(c1));
    icon_set(&mut buf, 16, 14, 8, 8, d);
    icon_set(&mut buf, 10, 18, 8, 6, d);
    icon_set(&mut buf, 12, 8, 4, 3, h);
    icon_set(&mut buf, 20, 12, 3, 2, h);
    icon_set(&mut buf, 7, 14, 3, 5, d);
    buf
}

fn fill_circle(buf: &mut IconBuf, cx: i32, cy: i32, r: f32, c: Pixel) {
    let r2 = r * r;
    for y in (cy as f32 - r).floor() as i32..=(cy as f32 + r).ceil() as i32 {
        for x in (cx as f32 - r).floor() as i32..=(cx as f32 + r).ceil() as i32 {
            let dx = x as f32 + 0.5 - cx as f32;
            let dy = y as f32 + 0.5 - cy as f32;
            if dx * dx + dy * dy <= r2 && (0..32).contains(&x) && (0..32).contains(&y) {
                buf[y as usize][x as usize] = c;
            }
        }
    }
}

fn gear_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    let g = hex("#aab6bf");
    let d = hex("#77848d");
    let h = hex("#d5dde2");
    for a in 0..8 {
        let x = 16 + ((a as f32 / 8.0 * std::f32::consts::TAU).cos() * 11.0).round() as i32 - 2;
        let y = 16 + ((a as f32 / 8.0 * std::f32::consts::TAU).sin() * 11.0).round() as i32 - 2;
        icon_set(&mut buf, x, y, 5, 5, d);
    }
    fill_circle(&mut buf, 16, 16, 9.0, g);
    fill_circle(&mut buf, 14, 14, 4.0, h);
    fill_circle(&mut buf, 16, 16, 4.0, hex("#2c353b"));
    buf
}

fn circuit_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    icon_set(&mut buf, 5, 7, 22, 18, hex("#1d7a3c"));
    icon_set(&mut buf, 5, 7, 22, 3, hex("#25914a"));
    icon_set(&mut buf, 9, 12, 5, 5, hex("#ffd24d"));
    icon_set(&mut buf, 19, 16, 6, 4, hex("#2c353b"));
    icon_set(&mut buf, 7, 20, 16, 1, hex("#d17f4a"));
    icon_set(&mut buf, 7, 10, 1, 11, hex("#d17f4a"));
    icon_set(&mut buf, 14, 14, 8, 1, hex("#d17f4a"));
    icon_set(&mut buf, 24, 9, 1, 8, hex("#d17f4a"));
    icon_set(&mut buf, 11, 22, 2, 3, hex("#c0c0c0"));
    icon_set(&mut buf, 17, 22, 2, 3, hex("#c0c0c0"));
    buf
}

fn data_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    icon_set(&mut buf, 6, 6, 20, 20, hex("#122c48"));
    icon_set(&mut buf, 6, 6, 20, 4, hex("#1a3d63"));
    icon_set(&mut buf, 10, 13, 12, 2, hex("#35e0e8"));
    icon_set(&mut buf, 10, 17, 8, 2, hex("#35e0e8"));
    icon_set(&mut buf, 10, 21, 10, 1, hex("#2596a0"));
    icon_set(&mut buf, 24, 12, 2, 2, hex("#7dff8a"));
    for i in 0..4 {
        icon_set(&mut buf, 8 + i * 5, 3, 2, 3, hex("#8a97a0"));
        icon_set(&mut buf, 8 + i * 5, 26, 2, 3, hex("#8a97a0"));
    }
    buf
}

fn fuel_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    icon_set(&mut buf, 10, 6, 12, 4, hex("#8a97a0"));
    icon_set(&mut buf, 8, 10, 16, 16, hex("#c0392b"));
    icon_set(&mut buf, 8, 10, 16, 5, hex("#e74c3c"));
    icon_set(&mut buf, 12, 15, 8, 7, hex("#f8d347"));
    icon_set(&mut buf, 14, 17, 4, 3, hex("#c0392b"));
    icon_set(&mut buf, 8, 26, 16, 2, hex("#7f2418"));
    icon_set(&mut buf, 13, 3, 6, 3, hex("#5f6b73"));
    buf
}

fn oxygen_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    fill_circle(&mut buf, 13, 14, 8.0, hex("#c2392b"));
    fill_circle(&mut buf, 20, 20, 6.0, hex("#e74c3c"));
    fill_circle(&mut buf, 10, 11, 3.0, hex("#ffb3ab"));
    fill_circle(&mut buf, 19, 18, 2.0, hex("#ff8a80"));
    buf
}

fn wire_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    // approximate arcs with circles/rings
    fill_circle(&mut buf, 16, 16, 10.0, hex("#d17f4a"));
    fill_circle(&mut buf, 16, 16, 6.5, [0, 0, 0, 0]);
    fill_circle(&mut buf, 16, 16, 9.0, hex("#f0a877"));
    fill_circle(&mut buf, 16, 16, 7.5, [0, 0, 0, 0]);
    buf
}

fn plate_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    icon_set(&mut buf, 6, 8, 20, 16, hex("#8a97a0"));
    icon_set(&mut buf, 6, 8, 20, 4, hex("#aab6bf"));
    icon_set(&mut buf, 6, 22, 20, 2, hex("#5f6b73"));
    for (x, y) in [(9, 11), (21, 11), (9, 19), (21, 19)] {
        icon_set(&mut buf, x, y, 2, 2, hex("#4a545b"));
    }
    buf
}

fn warp_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    // radial gradient approximation: nested filled circles
    let stops = [hex("#3a1d66"), hex("#6a3aa0"), hex("#8a55c8"), hex("#a872e0"), hex("#c090f0"), hex("#d8b8f8"), hex("#e0d0ff")];
    for (i, s) in stops.iter().enumerate() {
        let r = 12.0 * (1.0 - i as f32 / stops.len() as f32);
        fill_circle(&mut buf, 16, 16, r, *s);
    }
    fill_circle(&mut buf, 16, 16, 3.0, hex("#e0d0ff"));
    buf
}

fn antimatter_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    let stops = [hex("#401030"), hex("#2a0a20"), hex("#1a0a2e"), hex("#120612"), hex("#000000")];
    for (i, s) in stops.iter().enumerate() {
        let r = 12.0 * (1.0 - i as f32 / stops.len() as f32);
        fill_circle(&mut buf, 16, 16, r, *s);
    }
    fill_circle(&mut buf, 16, 16, 5.0, hex("#e838a8"));
    fill_circle(&mut buf, 16, 16, 2.0, hex("#000000"));
    icon_set(&mut buf, 15, 15, 2, 2, hex("#ffffff"));
    buf
}

/// Mining laser icon (from ui.js laserIcon).
pub fn laser_icon() -> IconBuf {
    let mut buf = IconBuf::default();
    icon_set(&mut buf, 6, 14, 16, 6, hex("#4e5a63"));
    icon_set(&mut buf, 8, 12, 12, 2, hex("#68747d"));
    icon_set(&mut buf, 20, 15, 8, 4, hex("#333d44"));
    icon_set(&mut buf, 27, 14, 2, 6, hex("#c9641a"));
    icon_set(&mut buf, 9, 20, 3, 6, hex("#333d44"));
    icon_set(&mut buf, 10, 15, 5, 3, hex("#35e0e8"));
    icon_set(&mut buf, 5, 15, 2, 4, hex("#c9641a"));
    buf
}

/// Build the 32×32 icon for an item id. `laser` special-cases the mining laser.
pub fn item_icon(atlas: &Atlas, item_key: &str) -> IconBuf {
    if item_key == "laser" {
        return laser_icon();
    }
    let Some(def) = crate::data::item_by_key(item_key) else {
        return IconBuf::default();
    };
    if let Some(bkey) = def.icon_block {
        let b = crate::data::block_by_key(bkey);
        if b.cross {
            let t = b.tiles.side.or(b.tiles.all).unwrap_or("grass_top");
            return flat_icon(atlas, t);
        }
        let top = b.tiles.top.or(b.tiles.all).unwrap_or("grass_top");
        let side = b.tiles.side.or(b.tiles.all).unwrap_or("grass_top");
        let front = b.tiles.front.or(b.tiles.side).or(b.tiles.all).unwrap_or(side);
        return block_icon(atlas, top, side, front);
    }
    match def.icon_fn {
        Some("carbon") => crystal_icon("#3a3a3a", "#6e6e6e"),
        Some("iron_ore") => chunk_icon("#d8af93"),
        Some("copper_ore") => chunk_icon("#d17f4a"),
        Some("titanium_ore") => chunk_icon("#cdd6dd"),
        Some("gold_ore") => chunk_icon("#f5cd3a"),
        Some("iron") => ingot_icon("#b8c4cc", "#e2eaef"),
        Some("copper") => ingot_icon("#d17f4a", "#f0a877"),
        Some("titanium") => ingot_icon("#dfe8ee", "#ffffff"),
        Some("gold") => ingot_icon("#f5cd3a", "#ffe98a"),
        Some("gear") => gear_icon(),
        Some("wire") => wire_icon(),
        Some("circuit") => circuit_icon(),
        Some("plate") => plate_icon(),
        Some("data") => data_icon(),
        Some("fuel") => fuel_icon(),
        Some("warp") => warp_icon(),
        Some("antimatter") => antimatter_icon(),
        Some("oxygen") => oxygen_icon(),
        Some("coal") => chunk_icon("#2f2f2f"),
        Some("sodium") => crystal_icon("#ffd23e", "#fff2ae"),
        Some("uranium") => crystal_icon("#69d436", "#c6ff9e"),
        Some("tritium") => crystal_icon("#4da6ff", "#b3dbff"),
        _ => crystal_icon("#888888", "#cccccc"),
    }
}
