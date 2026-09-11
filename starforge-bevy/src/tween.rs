//! Shared motion utilities: easings, springs, smoothed values and shake noise.
//!
//! The game deliberately keeps every animated quantity frame-rate independent:
//! springs integrate against delta time, exponential approaches use `exp(-k·dt)`
//! and shake noise is sampled by wall-clock time. Nothing in here touches the
//! ECS so the math can be unit-tested directly.

use bevy::prelude::*;

// ---------- Basic scalar helpers ----------

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
pub fn inv_lerp(a: f32, b: f32, value: f32) -> f32 {
    if (b - a).abs() < f32::EPSILON {
        0.0
    } else {
        (value - a) / (b - a)
    }
}

#[inline]
pub fn remap(value: f32, in0: f32, in1: f32, out0: f32, out1: f32) -> f32 {
    lerp(out0, out1, inv_lerp(in0, in1, value))
}

#[inline]
pub fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

#[inline]
pub fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = clamp01(inv_lerp(edge0, edge1, value));
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn smootherstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = clamp01(inv_lerp(edge0, edge1, value));
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Exponential approach with a configurable rate (larger = snappier).
///
/// Unlike `lerp(current, target, k)` this is stable at any frame rate because
/// the remaining distance shrinks by `exp(-rate·dt)` per step.
#[inline]
pub fn exp_approach(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    if rate <= 0.0 {
        return target;
    }
    let factor = 1.0 - (-rate * dt.max(0.0)).exp();
    current + (target - current) * factor
}

/// A rate expressed as "halving time" — the seconds it takes to close half of
/// the remaining gap. Convenient for artist-facing tuning constants.
#[inline]
pub fn exp_approach_halflife(current: f32, target: f32, halflife: f32, dt: f32) -> f32 {
    if halflife <= 0.0 {
        return target;
    }
    let rate = std::f32::consts::LN_2 / halflife;
    exp_approach(current, target, rate, dt)
}

pub fn exp_approach_vec3(current: Vec3, target: Vec3, rate: f32, dt: f32) -> Vec3 {
    if rate <= 0.0 {
        return target;
    }
    let factor = 1.0 - (-rate * dt.max(0.0)).exp();
    current + (target - current) * factor
}

/// Critically-damped-ish smooth damp for scalars (Game Programming Gems 4).
/// Returns the new value and stores the new velocity in `velocity`.
pub fn smooth_damp(
    current: f32,
    target: f32,
    velocity: &mut f32,
    smooth_time: f32,
    dt: f32,
) -> f32 {
    let smooth_time = smooth_time.max(0.0001);
    let omega = 2.0 / smooth_time;
    let x = omega * dt.max(0.0);
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*velocity + omega * change) * dt.max(0.0);
    *velocity = (*velocity - omega * temp) * exp;
    target + (change + temp) * exp
}

pub fn smooth_damp_vec3(
    current: Vec3,
    target: Vec3,
    velocity: &mut Vec3,
    smooth_time: f32,
    dt: f32,
) -> Vec3 {
    Vec3::new(
        smooth_damp(current.x, target.x, &mut velocity.x, smooth_time, dt),
        smooth_damp(current.y, target.y, &mut velocity.y, smooth_time, dt),
        smooth_damp(current.z, target.z, &mut velocity.z, smooth_time, dt),
    )
}

// ---------- Easings ----------
//
// All easings map 0→0 and 1→1 (except the *back* family which overshoots).
// They intentionally mirror the classic Robert Penner set so tuning notes from
// other tools carry over.

#[inline]
pub fn ease_linear(t: f32) -> f32 {
    t
}

#[inline]
pub fn ease_in_quad(t: f32) -> f32 {
    t * t
}

#[inline]
pub fn ease_out_quad(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

#[inline]
pub fn ease_in_out_quad(t: f32) -> f32 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

#[inline]
pub fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

#[inline]
pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[inline]
pub fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

#[inline]
pub fn ease_in_quart(t: f32) -> f32 {
    t * t * t * t
}

#[inline]
pub fn ease_out_quart(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(4)
}

#[inline]
pub fn ease_in_out_quart(t: f32) -> f32 {
    if t < 0.5 {
        8.0 * t * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(4) / 2.0
    }
}

#[inline]
pub fn ease_out_quint(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(5)
}

#[inline]
pub fn ease_in_sine(t: f32) -> f32 {
    1.0 - (t * std::f32::consts::FRAC_PI_2).cos()
}

#[inline]
pub fn ease_out_sine(t: f32) -> f32 {
    (t * std::f32::consts::FRAC_PI_2).sin()
}

#[inline]
pub fn ease_in_out_sine(t: f32) -> f32 {
    -((std::f32::consts::PI * t).cos() - 1.0) / 2.0
}

#[inline]
pub fn ease_in_expo(t: f32) -> f32 {
    if t <= 0.0 {
        0.0
    } else {
        2.0f32.powf(10.0 * t - 10.0)
    }
}

#[inline]
pub fn ease_out_expo(t: f32) -> f32 {
    if t >= 1.0 {
        1.0
    } else {
        1.0 - 2.0f32.powf(-10.0 * t)
    }
}

#[inline]
pub fn ease_in_circ(t: f32) -> f32 {
    1.0 - (1.0 - t * t).max(0.0).sqrt()
}

#[inline]
pub fn ease_out_circ(t: f32) -> f32 {
    let u = t - 1.0;
    (1.0 - u * u).max(0.0).sqrt()
}

#[inline]
pub fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
}

#[inline]
pub fn ease_in_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    C3 * t * t * t - C1 * t * t
}

#[inline]
pub fn ease_out_elastic(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    const C4: f32 = std::f32::consts::TAU / 3.0;
    2.0f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * C4).sin() + 1.0
}

#[inline]
pub fn ease_out_bounce(t: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;
    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / D1;
        N1 * t * t + 0.984375
    }
}

/// Ease an angle toward a target the short way around the circle.
pub fn smooth_damp_angle(
    current: f32,
    target: f32,
    velocity: &mut f32,
    smooth_time: f32,
    dt: f32,
) -> f32 {
    let mut delta = (target - current).rem_euclid(std::f32::consts::TAU);
    if delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    }
    let stepped = smooth_damp(delta, 0.0, velocity, smooth_time, dt);
    current + (delta - stepped)
}

/// Wrap an angle into [-π, π].
pub fn wrap_angle(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut wrapped = angle % tau;
    if wrapped > std::f32::consts::PI {
        wrapped -= tau;
    }
    if wrapped < -std::f32::consts::PI {
        wrapped += tau;
    }
    wrapped
}

// ---------- Springs ----------

/// Semi-implicit damped spring for a scalar. Stable for large dt values and
/// never overshoots past the requested stiffness when critically damped.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    /// ω (rad/s); higher = faster.
    pub stiffness: f32,
    /// Damping ratio: 1.0 = critical, <1 bouncy, >1 sluggish.
    pub damping: f32,
}

impl Spring {
    pub fn new(value: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            stiffness,
            damping,
        }
    }

    /// Critically damped spring tuned by "time to settle" in seconds.
    pub fn critically_damped(value: f32, settle_time: f32) -> Self {
        let stiffness = 4.0 * std::f32::consts::PI.powi(2) / settle_time.max(0.01).powi(2);
        Self::new(value, stiffness, 1.0)
    }

    pub fn update(&mut self, target: f32, dt: f32) -> f32 {
        let dt = dt.clamp(0.0, 0.05);
        // Sub-step so very stiff springs stay stable at low frame rates.
        let steps = ((self.stiffness.sqrt() * dt * 4.0).ceil() as usize).clamp(1, 16);
        let h = dt / steps as f32;
        for _ in 0..steps {
            let accel = (target - self.value) * self.stiffness
                - self.velocity * 2.0 * self.damping * self.stiffness.sqrt();
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }
        self.value
    }

    pub fn impulse(&mut self, amount: f32) {
        self.velocity += amount;
    }
}

/// Vector counterpart of [`Spring`], applying the same coefficients per axis.
#[derive(Clone, Copy, Debug)]
pub struct SpringVec3 {
    pub value: Vec3,
    pub velocity: Vec3,
    pub stiffness: f32,
    pub damping: f32,
}

impl SpringVec3 {
    pub fn new(value: Vec3, stiffness: f32, damping: f32) -> Self {
        Self {
            value,
            velocity: Vec3::ZERO,
            stiffness,
            damping,
        }
    }

    pub fn update(&mut self, target: Vec3, dt: f32) -> Vec3 {
        let dt = dt.clamp(0.0, 0.05);
        let steps = ((self.stiffness.sqrt() * dt * 4.0).ceil() as usize).clamp(1, 16);
        let h = dt / steps as f32;
        for _ in 0..steps {
            let accel = (target - self.value) * self.stiffness
                - self.velocity * 2.0 * self.damping * self.stiffness.sqrt();
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }
        self.value
    }

    pub fn impulse(&mut self, amount: Vec3) {
        self.velocity += amount;
    }

    pub fn jump(&mut self, value: Vec3) {
        self.value = value;
        self.velocity = Vec3::ZERO;
    }
}

// ---------- Timed value tween ----------

#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub duration: f32,
    pub elapsed: f32,
}

impl Tween {
    pub fn new(from: f32, to: f32, duration: f32) -> Self {
        Self {
            from,
            to,
            duration: duration.max(0.0001),
            elapsed: 0.0,
        }
    }

    pub fn advance(&mut self, dt: f32) -> bool {
        self.elapsed = (self.elapsed + dt).min(self.duration);
        self.finished()
    }

    pub fn finished(&self) -> bool {
        self.elapsed >= self.duration
    }

    pub fn t(&self) -> f32 {
        clamp01(self.elapsed / self.duration)
    }

    pub fn value(&self, easing: impl Fn(f32) -> f32) -> f32 {
        lerp(self.from, self.to, easing(self.t()))
    }
}

/// Small helper for one-shot decaying envelopes (flash timers, hit pulses).
#[derive(Clone, Copy, Debug, Default)]
pub struct Decay {
    pub value: f32,
    pub decay_rate: f32,
}

impl Decay {
    pub fn trigger(&mut self, amount: f32) {
        self.value = self.value.max(amount);
    }

    pub fn update(&mut self, dt: f32) -> f32 {
        self.value = (self.value - self.decay_rate * dt).max(0.0);
        self.value
    }
}

// ---------- Value noise (shake / wind / drift) ----------

/// Smooth deterministic 1D value noise. Cheap enough to call per frame per
/// active shake and good enough for camera trauma that humans read as random.
#[derive(Clone, Copy, Debug)]
pub struct ValueNoise1 {
    seed: u32,
}

impl ValueNoise1 {
    pub fn new(seed: u32) -> Self {
        Self { seed }
    }

    #[inline]
    fn hash(&self, cell: i32, channel: u32) -> f32 {
        let mut h =
            self.seed ^ (cell as u32).wrapping_mul(0x9E37_79B9) ^ channel.wrapping_mul(0x85EB_CA6B);
        h ^= h >> 16;
        h = h.wrapping_mul(0x7FEB_352D);
        h ^= h >> 15;
        h = h.wrapping_mul(0x846C_A68B);
        h ^= h >> 16;
        h as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// Sample in [-1, 1], smoothly interpolated, `channel` selects an axis.
    pub fn sample(&self, time: f32, channel: u32) -> f32 {
        let x = time;
        let i = x.floor() as i32;
        let f = x - i as f32;
        let t = f * f * (3.0 - 2.0 * f);
        let a = self.hash(i, channel);
        let b = self.hash(i + 1, channel);
        lerp(a, b, t)
    }

    /// Two octaves for a slightly grittier shake.
    pub fn sample_fbm(&self, time: f32, channel: u32) -> f32 {
        self.sample(time, channel) * 0.68 + self.sample(time * 2.31 + 7.7, channel + 17) * 0.32
    }
}

/// Three-axis trauma shake built on [`ValueNoise1`].
#[derive(Clone, Copy, Debug)]
pub struct ShakeNoise {
    noise: ValueNoise1,
}

impl ShakeNoise {
    pub fn new(seed: u32) -> Self {
        Self {
            noise: ValueNoise1::new(seed),
        }
    }

    /// Returns a shake offset in [-1, 1] per axis.
    pub fn sample(&self, time: f32) -> Vec3 {
        Vec3::new(
            self.noise.sample_fbm(time * 18.0, 0),
            self.noise.sample_fbm(time * 17.0 + 3.1, 1),
            self.noise.sample_fbm(time * 15.0 + 9.4, 2),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn easings_hit_their_endpoints() {
        let easings: [(&str, fn(f32) -> f32); 20] = [
            ("linear", ease_linear),
            ("in_quad", ease_in_quad),
            ("out_quad", ease_out_quad),
            ("in_out_quad", ease_in_out_quad),
            ("in_cubic", ease_in_cubic),
            ("out_cubic", ease_out_cubic),
            ("in_out_cubic", ease_in_out_cubic),
            ("in_quart", ease_in_quart),
            ("out_quart", ease_out_quart),
            ("in_out_quart", ease_in_out_quart),
            ("out_quint", ease_out_quint),
            ("in_sine", ease_in_sine),
            ("out_sine", ease_out_sine),
            ("in_out_sine", ease_in_out_sine),
            ("in_expo", ease_in_expo),
            ("out_expo", ease_out_expo),
            ("in_circ", ease_in_circ),
            ("out_circ", ease_out_circ),
            ("out_back", ease_out_back),
            ("in_back", ease_in_back),
        ];
        for (name, f) in easings {
            assert!(approx(f(0.0), 0.0, 1e-4), "{name} f(0)");
            assert!(approx(f(1.0), 1.0, 1e-4), "{name} f(1)");
            for i in 0..=32 {
                let v = f(i as f32 / 32.0);
                assert!(v.is_finite(), "{name} produced non-finite at {i}");
            }
        }
        assert!(approx(ease_out_elastic(0.0), 0.0, 1e-4));
        assert!(approx(ease_out_elastic(1.0), 1.0, 1e-4));
        assert!(approx(ease_out_bounce(0.0), 0.0, 1e-4));
        assert!(approx(ease_out_bounce(1.0), 1.0, 1e-4));
    }

    #[test]
    fn exp_approach_is_frame_rate_independent() {
        let rate = 8.0;
        let mut slow = 0.0f32;
        for _ in 0..60 {
            slow = exp_approach(slow, 1.0, rate, 1.0 / 60.0);
        }
        let mut fast = 0.0f32;
        for _ in 0..120 {
            fast = exp_approach(fast, 1.0, rate, 1.0 / 120.0);
        }
        assert!(
            (slow - fast).abs() < 1e-3,
            "approaches diverged: {slow} vs {fast}"
        );
    }

    #[test]
    fn approach_halflife_closes_half_the_gap() {
        let v = exp_approach_halflife(0.0, 1.0, 0.5, 0.5);
        assert!(approx(v, 0.5, 1e-3));
    }

    #[test]
    fn smooth_damp_converges_without_step_size_sensitivity() {
        let mut vel_a = 0.0;
        let mut a = 0.0;
        for _ in 0..240 {
            a = smooth_damp(a, 10.0, &mut vel_a, 0.4, 1.0 / 60.0);
        }
        let mut vel_b = 0.0;
        let mut b = 0.0;
        for _ in 0..120 {
            b = smooth_damp(b, 10.0, &mut vel_b, 0.4, 1.0 / 30.0);
        }
        assert!((a - b).abs() < 0.05, "smooth damp diverged: {a} vs {b}");
        assert!((a - 10.0).abs() < 0.2, "did not converge: {a}");
    }

    #[test]
    fn spring_settles_and_is_stable_at_large_dt() {
        let mut spring = Spring::new(0.0, 60.0, 1.0);
        for _ in 0..300 {
            spring.update(1.0, 1.0 / 60.0);
        }
        assert!((spring.value - 1.0).abs() < 1e-3);
        let mut violent = Spring::new(0.0, 120.0, 0.8);
        for _ in 0..40 {
            violent.update(1.0, 0.05);
            assert!(violent.value.is_finite() && violent.value.abs() < 10.0);
        }
    }

    #[test]
    fn spring_vec3_converges_per_axis() {
        let mut spring = SpringVec3::new(Vec3::ZERO, 40.0, 1.0);
        let target = Vec3::new(1.0, -2.0, 0.5);
        for _ in 0..240 {
            spring.update(target, 1.0 / 60.0);
        }
        assert!(spring.value.distance(target) < 1e-3);
    }

    #[test]
    fn tween_interpolates_and_finishes() {
        let mut tween = Tween::new(2.0, 6.0, 1.0);
        assert!(approx(tween.value(ease_linear), 2.0, 1e-4));
        tween.advance(0.5);
        assert!(approx(tween.value(ease_linear), 4.0, 1e-4));
        assert!(tween.advance(1.0));
        assert!(approx(tween.value(ease_linear), 6.0, 1e-4));
    }

    #[test]
    fn wrap_angle_normalizes() {
        use std::f32::consts::PI;
        assert!(approx(wrap_angle(PI * 3.0), PI, 1e-3));
        assert!(approx(wrap_angle(-PI * 3.0).abs(), PI, 1e-3));
        assert!(approx(wrap_angle(0.0), 0.0, 1e-6));
        assert!(approx(wrap_angle(PI + 0.001), -PI + 0.001, 1e-4));
    }

    #[test]
    fn shake_noise_is_bounded_and_deterministic() {
        let noise = ShakeNoise::new(1234);
        let a = noise.sample(3.5);
        let b = noise.sample(3.5);
        assert_eq!(a, b);
        for i in 0..200 {
            let v = noise.sample(i as f32 * 0.037);
            assert!(v.x.abs() <= 1.0 && v.y.abs() <= 1.0 && v.z.abs() <= 1.0);
        }
    }

    #[test]
    fn value_noise_continuity() {
        let noise = ValueNoise1::new(99);
        let mut prev = noise.sample(0.0, 0);
        for i in 1..=1000 {
            let v = noise.sample(i as f32 * 0.01, 0);
            assert!((v - prev).abs() < 0.2, "noise jumped at {i}");
            prev = v;
        }
    }
}
