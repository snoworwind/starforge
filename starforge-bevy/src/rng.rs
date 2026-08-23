//! Deterministic RNG & noise — exact ports of the original JS implementations
//! (`mulberry32` from textures.js, Perlin value noise + 3D lattice noise from world.js).

/// 32-bit mulberry32 PRNG. Produces floats in [0,1).
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    /// Next float in [0,1). Exact port of the JS mulberry32.
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4294967296.0
    }

    /// Integer in [0, n).
    #[inline]
    pub fn range(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        ((self.next() * n as f32) as usize).min(n - 1)
    }

    /// Float in [a, b).
    #[inline]
    pub fn range_f(&mut self, a: f32, b: f32) -> f32 {
        a + self.next() * (b - a)
    }
}

/// 2D Perlin (gradient) value noise — exact port of `makeNoise` in world.js.
#[derive(Clone)]
pub struct Noise2 {
    perm: [u8; 512],
}

impl Noise2 {
    pub fn new(seed: u32) -> Self {
        let mut rng = Rng::new(seed);
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        // Fisher-Yates shuffle
        for i in (1..256).rev() {
            let j = (rng.next() * (i + 1) as f32) as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        for (i, v) in perm.iter_mut().enumerate() {
            *v = p[i & 255];
        }
        Self { perm }
    }

    #[inline]
    fn fade(t: f32) -> f32 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    #[inline]
    fn grad2(h: usize, x: f32, y: f32) -> f32 {
        match h & 3 {
            0 => x + y,
            1 => -x + y,
            2 => x - y,
            _ => -x - y,
        }
    }

    /// Single-octave gradient noise, roughly [-1, 1].
    pub fn n2(&self, x: f32, y: f32) -> f32 {
        let xi = x.floor();
        let yi = y.floor();
        let x = x - xi;
        let y = y - yi;
        let ix = xi as i32 & 255;
        let iy = yi as i32 & 255;
        let u = Self::fade(x);
        let v = Self::fade(y);
        let a = self.perm[ix as usize] as usize + iy as usize;
        let b = self.perm[ix as usize + 1] as usize + iy as usize;
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        lerp(
            lerp(
                Self::grad2(self.perm[a] as usize, x, y),
                Self::grad2(self.perm[b] as usize, x - 1.0, y),
                u,
            ),
            lerp(
                Self::grad2(self.perm[a + 1] as usize, x, y - 1.0),
                Self::grad2(self.perm[b + 1] as usize, x - 1.0, y - 1.0),
                u,
            ),
            v,
        )
    }

    /// Fractal Brownian motion, normalized to roughly [-1, 1].
    pub fn fbm2(&self, x: f32, y: f32, octaves: usize, lacunarity: f32, gain: f32) -> f32 {
        let mut amp = 1.0f32;
        let mut f = 1.0f32;
        let mut sum = 0.0f32;
        let mut norm = 0.0f32;
        for _ in 0..octaves {
            sum += self.n2(x * f, y * f) * amp;
            norm += amp;
            amp *= gain;
            f *= lacunarity;
        }
        sum / norm
    }

    /// Default octave settings (4, 2, 0.5).
    #[inline]
    pub fn fbm(&self, x: f32, y: f32) -> f32 {
        self.fbm2(x, y, 4, 2.0, 0.5)
    }
}

/// 32-bit multiply helper (JS `Math.imul`).
#[inline]
fn imul(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

/// Lattice hash for 3D value noise — exact port of `lattice3` in world.js.
/// All args are integers (i32, JS coerces via ToInt32).
/// NOTE: JS `>>>` is a LOGICAL shift; Rust `>>` on i32 is arithmetic, so we
/// cast to u32 for the shift (this matters for negative coordinates).
#[inline]
pub fn lattice3(x: i32, y: i32, z: i32, salt: u32, seed: u32) -> f32 {
    let mut h: i32 = (seed ^ salt) as i32;
    h = imul(h ^ x, 374_761_393);
    h = imul(h ^ y, 217_645_177);
    h = imul(h ^ z, 668_265_263);
    h = imul(h ^ ((h as u32 >> 15) as i32), 2_246_822_519u32 as i32);
    (((h ^ ((h as u32 >> 13) as i32)) as u32) as f32) / 4294967296.0
}

/// 3D value noise with smoothstep trilinear interpolation — port of `vnoise3`.
/// x/y/z may be fractional; `floor` is applied (JS semantics: floor of f64).
pub fn vnoise3(x: f32, y: f32, z: f32, salt: u32, seed: u32) -> f32 {
    let ix = x.floor();
    let iy = y.floor();
    let iz = z.floor();
    let mut fx = x - ix;
    let mut fy = y - iy;
    let mut fz = z - iz;
    fx = fx * fx * (3.0 - 2.0 * fx);
    fy = fy * fy * (3.0 - 2.0 * fy);
    fz = fz * fz * (3.0 - 2.0 * fz);
    let ix = ix as i32;
    let iy = iy as i32;
    let iz = iz as i32;
    let c000 = lattice3(ix, iy, iz, salt, seed);
    let c100 = lattice3(ix + 1, iy, iz, salt, seed);
    let c010 = lattice3(ix, iy + 1, iz, salt, seed);
    let c110 = lattice3(ix + 1, iy + 1, iz, salt, seed);
    let c001 = lattice3(ix, iy, iz + 1, salt, seed);
    let c101 = lattice3(ix + 1, iy, iz + 1, salt, seed);
    let c011 = lattice3(ix, iy + 1, iz + 1, salt, seed);
    let c111 = lattice3(ix + 1, iy + 1, iz + 1, salt, seed);
    let x00 = c000 + (c100 - c000) * fx;
    let x10 = c010 + (c110 - c010) * fx;
    let x01 = c001 + (c101 - c001) * fx;
    let x11 = c011 + (c111 - c011) * fx;
    let y0 = x00 + (x10 - x00) * fy;
    let y1 = x01 + (x11 - x01) * fy;
    y0 + (y1 - y0) * fz
}

/// Deterministic per-column / per-chunk RNG — port of `hash2`.
pub fn hash2(x: i32, z: i32, salt: u32, seed: u32) -> Rng {
    let mut h: i32 = (seed ^ salt) as i32;
    h = imul(h ^ x, 374_761_393);
    h = imul(h ^ z, 668_265_263);
    Rng::new((h ^ ((h as u32 >> 13) as i32)) as u32)
}

/// JS creatures.js `batchSeedOf`：24m 网格生物批次种子（世界生成式掷骰，跨客户端确定性一致）。
pub fn batch_seed(seed: u32, x: i32, z: i32) -> u32 {
    let mut h: i32 = (seed ^ 0xC7EA5) as i32;
    h = imul(h ^ x, 374_761_393);
    h = imul(h ^ z, 668_265_263);
    (h ^ ((h as u32 >> 13) as i32)) as u32
}

/// Leaf plugin: RNG & noise are pure functions today; kept as a plugin so any
/// future RNG-driven system has a home.
pub struct RngPlugin;

impl bevy::prelude::Plugin for RngPlugin {
    fn build(&self, _app: &mut bevy::prelude::App) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden values generated from the original JavaScript (world.js/textures.js).
    #[test]
    fn mulberry32_matches_js() {
        let mut r = Rng::new(12345);
        // Golden values captured from the archived JS generator
        // (`../legacy-web/tools/golden.mjs`).
        let expected = [
            0.979_728_3,
            0.30675226,
            0.48420542,
            0.817_934_4,
            0.509_428_4,
            0.34747186,
        ];
        for e in expected {
            let v = r.next();
            assert!((v - e).abs() < 1e-6, "got {v}, expected {e}");
        }
    }

    #[test]
    fn empty_range_is_safe() {
        assert_eq!(Rng::new(1).range(0), 0);
    }

    #[test]
    fn noise2_matches_js() {
        // Golden values extracted from the original JS makeNoise(7777)
        let n = Noise2::new(7777);
        let v = [
            n.n2(1.5, 2.5),
            n.n2(100.25, -40.75),
            n.fbm2(12.5, 7.5, 4, 2.0, 0.5),
            n.fbm2(-3.1, 9.9, 5, 2.0, 0.5),
        ];
        let expected = [-0.5, -0.03032684, -0.13333333, 0.05411678];
        for (got, exp) in v.iter().zip(expected.iter()) {
            assert!(
                (got - exp).abs() < 1e-5,
                "noise mismatch: got {got}, expected {exp}"
            );
        }
    }

    #[test]
    fn lattice3_matches_js() {
        let v = [
            lattice3(3, 4, 5, 0xCAFE01, 7777),
            lattice3(-10, 2, 33, 0xCAFE01, 7777),
        ];
        let expected = [0.390_475_9, 0.04641483];
        for (got, exp) in v.iter().zip(expected.iter()) {
            assert!(
                (got - exp).abs() < 1e-6,
                "lattice3 mismatch: got {got}, expected {exp}"
            );
        }
    }
}
