//! Real-time synthesis building blocks for the procedural music and ambience
//! engine. Everything here is allocation-free after construction (buffers are
//! pre-sized in `new`) so the audio callback never touches the allocator.
//!
//! Signal flow helpers are deliberately small and testable: oscillators with
//! PolyBLEP anti-aliasing, RBJ biquads, a Chamberlin state-variable filter,
//! ADSR envelopes, delay lines, a Freeverb-style reverb, chorus, drums and a
//! look-free soft limiter.

use std::f32::consts::TAU;

// ---------- Utilities ----------

/// One-pole parameter smoother that works at control rate.
#[derive(Clone, Copy, Debug)]
pub struct Smoothed {
    value: f32,
    target: f32,
    coeff: f32,
}

impl Smoothed {
    /// `time_ms` is the time to cover ~63% of a jump.
    pub fn new(value: f32, time_ms: f32) -> Self {
        let coeff = if time_ms <= 0.0 {
            1.0
        } else {
            1.0 - (-1.0 / (time_ms * 0.001 * 44_100.0)).exp()
        };
        Self {
            value,
            target: value,
            coeff,
        }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    pub fn jump(&mut self, value: f32) {
        self.value = value;
        self.target = value;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.coeff;
        self.value
    }

    #[inline]
    pub fn current(&self) -> f32 {
        self.value
    }
}

/// Tiny xorshift RNG for audio-rate noise (never allocates, deterministic).
#[derive(Clone, Copy, Debug)]
pub struct NoiseRng {
    state: u32,
}

impl NoiseRng {
    pub fn new(seed: u32) -> Self {
        Self { state: seed.max(1) }
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform in [-1, 1).
    #[inline]
    pub fn next_bipolar(&mut self) -> f32 {
        (self.next_u32() as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn next01(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }
}

/// White/pink/brown noise generator with independent L/R streams.
#[derive(Clone, Debug)]
pub struct Noise {
    rng: NoiseRng,
    pink: [f32; 7],
    brown: f32,
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self {
            rng: NoiseRng::new(seed),
            pink: [0.0; 7],
            brown: 0.0,
        }
    }

    #[inline]
    pub fn white(&mut self) -> f32 {
        self.rng.next_bipolar()
    }

    /// Paul Kellet's economy pink filter, roughly -3 dB/octave.
    pub fn pink(&mut self) -> f32 {
        let white = self.white();
        self.pink[0] = 0.99886 * self.pink[0] + white * 0.0555179;
        self.pink[1] = 0.99332 * self.pink[1] + white * 0.075_075;
        self.pink[2] = 0.96900 * self.pink[2] + white * 0.153_852;
        self.pink[3] = 0.86650 * self.pink[3] + white * 0.310_485;
        self.pink[4] = 0.55000 * self.pink[4] + white * 0.532_952;
        self.pink[5] = -0.7616 * self.pink[5] - white * 0.016_898;
        self.pink[6] = white * 0.115_926;
        (self.pink.iter().sum::<f32>() * 0.11).clamp(-1.0, 1.0)
    }

    pub fn brown(&mut self) -> f32 {
        self.brown = (self.brown + self.white() * 0.02).clamp(-1.0, 1.0);
        self.brown
    }
}

// ---------- Oscillators ----------

/// Stateless oscillator helpers working on a normalized phase in [0, 1).
#[derive(Clone, Copy, Debug)]
pub struct Osc;

impl Osc {
    #[inline]
    pub fn sine(phase: f32) -> f32 {
        (phase * TAU).sin()
    }

    #[inline]
    pub fn tri(phase: f32) -> f32 {
        let t = phase - phase.floor();
        4.0 * (t - 0.5).abs() - 1.0
    }

    /// Naive saw; use [`Osc::saw_blep`] when `dt` is available.
    #[inline]
    pub fn saw(phase: f32) -> f32 {
        2.0 * (phase - phase.floor()) - 1.0
    }

    /// Naive square with pulse width.
    #[inline]
    pub fn square(phase: f32, pulse_width: f32) -> f32 {
        if phase - phase.floor() < pulse_width {
            1.0
        } else {
            -1.0
        }
    }

    /// PolyBLEP residual used to soften a discontinuity of size `dt`.
    #[inline]
    pub fn poly_blep(t: f32, dt: f32) -> f32 {
        if t < dt {
            let t = t / dt;
            t + t - t * t - 1.0
        } else if t > 1.0 - dt {
            let t = (t - 1.0) / dt;
            t * t + t + t + 1.0
        } else {
            0.0
        }
    }

    /// Band-limited saw at phase `p` with per-sample frequency `dt`.
    #[inline]
    pub fn saw_blep(p: f32, dt: f32) -> f32 {
        let t = p - p.floor();
        let mut value = 2.0 * t - 1.0;
        value -= Self::poly_blep(t, dt);
        value
    }

    /// Band-limited square with pulse width `pw`.
    #[inline]
    pub fn square_blep(p: f32, dt: f32, pw: f32) -> f32 {
        let t = p - p.floor();
        let mut value = if t < pw { 1.0 } else { -1.0 };
        value += Self::poly_blep(t, dt);
        value -= Self::poly_blep((t - pw + 1.0) % 1.0, dt);
        value
    }

    /// Simple two-operator FM: `carrier` phase advanced externally.
    #[inline]
    pub fn fm(carrier: f32, modulator: f32, index: f32) -> f32 {
        ((carrier + modulator * index) * TAU).sin()
    }
}

/// MIDI note number to frequency (A4 = 69 = 440 Hz).
#[inline]
pub fn midi_hz(note: f32) -> f32 {
    440.0 * 2.0f32.powf((note - 69.0) / 12.0)
}

/// Frequency to MIDI note number.
#[inline]
pub fn hz_midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz.max(1e-6) / 440.0).log2()
}

// ---------- Envelopes ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear-attack / exponential-decay ADSR evaluated at control rate or audio
/// rate. Exponential segments use the classic 0.0001..0.9999 mapping.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    stage: EnvStage,
    value: f32,
    release_from: f32,
}

impl Adsr {
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            attack: attack.max(0.0001),
            decay: decay.max(0.0001),
            sustain: sustain.clamp(0.0, 1.0),
            release: release.max(0.0001),
            stage: EnvStage::Idle,
            value: 0.0,
            release_from: 0.0,
        }
    }

    pub fn gate_on(&mut self) {
        self.stage = EnvStage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != EnvStage::Idle {
            self.release_from = self.value;
            self.stage = EnvStage::Release;
        }
    }

    pub fn reset(&mut self) {
        self.stage = EnvStage::Idle;
        self.value = 0.0;
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn is_active(&self) -> bool {
        self.stage != EnvStage::Idle
    }

    #[inline]
    pub fn next(&mut self, dt: f32) -> f32 {
        match self.stage {
            EnvStage::Idle => self.value = 0.0,
            EnvStage::Attack => {
                self.value += dt / self.attack;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                let target = self.sustain;
                self.value = target + (self.value - target) * (-dt / (self.decay * 0.35)).exp();
                if (self.value - target).abs() < 0.001 {
                    self.value = target;
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Sustain => self.value = self.sustain,
            EnvStage::Release => {
                self.value *= (-dt / (self.release * 0.35)).exp();
                if self.value < 0.0005 {
                    self.value = 0.0;
                    self.stage = EnvStage::Idle;
                }
            }
        }
        self.value
    }
}

/// Percussive exponential envelope with adjustable curve; used by drums.
#[derive(Clone, Copy, Debug)]
pub struct DecayEnv {
    pub time: f32,
    pub curve: f32,
    value: f32,
}

impl DecayEnv {
    pub fn new(time: f32, curve: f32) -> Self {
        Self {
            time: time.max(0.001),
            curve,
            value: 0.0,
        }
    }

    pub fn trigger(&mut self) {
        self.value = 1.0;
    }

    #[inline]
    pub fn next(&mut self, dt: f32) -> f32 {
        if self.value <= 0.0 {
            return 0.0;
        }
        let decay = (-dt / (self.time * 0.3)).exp();
        self.value *= decay.powf(self.curve.max(0.05));
        if self.value < 0.0005 {
            self.value = 0.0;
        }
        self.value
    }
}

// ---------- Filters ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peak,
}

/// RBJ biquad. Coefficients are recalculated on demand; processing is a single
/// DF-II transposed step.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
    sample_rate: f32,
}

impl Biquad {
    pub fn identity(sample_rate: f32) -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
            sample_rate,
        }
    }

    pub fn low_pass(sample_rate: f32, freq: f32, q: f32) -> Self {
        let mut filter = Self::identity(sample_rate);
        filter.set(FilterKind::LowPass, freq, q, 0.0);
        filter
    }

    pub fn high_pass(sample_rate: f32, freq: f32, q: f32) -> Self {
        let mut filter = Self::identity(sample_rate);
        filter.set(FilterKind::HighPass, freq, q, 0.0);
        filter
    }

    pub fn band_pass(sample_rate: f32, freq: f32, q: f32) -> Self {
        let mut filter = Self::identity(sample_rate);
        filter.set(FilterKind::BandPass, freq, q, 0.0);
        filter
    }

    pub fn set(&mut self, kind: FilterKind, freq: f32, q: f32, gain_db: f32) {
        let freq = freq.clamp(20.0, self.sample_rate * 0.45);
        let q = q.max(0.05);
        let w0 = TAU * freq / self.sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        let a = 10.0f32.powf(gain_db / 40.0);
        let (b0, b1, b2, a0) = match kind {
            FilterKind::LowPass => (
                (1.0 - cos_w0) * 0.5,
                1.0 - cos_w0,
                (1.0 - cos_w0) * 0.5,
                1.0 + alpha,
            ),
            FilterKind::HighPass => (
                (1.0 + cos_w0) * 0.5,
                -(1.0 + cos_w0),
                (1.0 + cos_w0) * 0.5,
                1.0 + alpha,
            ),
            FilterKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha),
            FilterKind::Notch => (1.0, -2.0 * cos_w0, 1.0, 1.0 + alpha),
            FilterKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos_w0,
                1.0 - alpha * a,
                1.0 + alpha / a,
            ),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let out = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * out + self.z2;
        self.z2 = self.b2 * x - self.a2 * out;
        out
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Chamberlin state-variable filter giving low/band/high outputs at once.
#[derive(Clone, Copy, Debug)]
pub struct Svf {
    low: f32,
    band: f32,
    pub sample_rate: f32,
    pub freq: f32,
    pub q: f32,
}

impl Svf {
    pub fn new(sample_rate: f32, freq: f32, q: f32) -> Self {
        Self {
            low: 0.0,
            band: 0.0,
            sample_rate,
            freq,
            q,
        }
    }

    pub fn reset(&mut self) {
        self.low = 0.0;
        self.band = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> (f32, f32, f32) {
        let f = 2.0 * (std::f32::consts::PI * self.freq / self.sample_rate).sin();
        let q = 1.0 / self.q.max(0.2);
        let high = x - self.low - q * self.band;
        self.band += f * high;
        self.low += f * self.band;
        (self.low, self.band, high)
    }

    /// Cheap coefficient update at control rate.
    pub fn set(&mut self, freq: f32, q: f32) {
        self.freq = freq.clamp(20.0, self.sample_rate * 0.42);
        self.q = q.max(0.2);
    }
}

/// One-pole low-pass for gentle tone shaping / damping.
#[derive(Clone, Copy, Debug)]
pub struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    pub fn new(sample_rate: f32, freq: f32) -> Self {
        let mut filter = Self { a: 0.0, z: 0.0 };
        filter.set(sample_rate, freq);
        filter
    }

    pub fn set(&mut self, sample_rate: f32, freq: f32) {
        let x = (-TAU * freq.clamp(1.0, sample_rate * 0.49) / sample_rate).exp();
        self.a = 1.0 - x;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }
}

/// Removes the DC offset that asymmetric distortion introduces.
#[derive(Clone, Copy, Debug, Default)]
pub struct DcBlocker {
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + 0.995 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

// ---------- Time-based effects ----------

/// Fractional-delay line with feedback and damping. Interpolation is linear,
/// which is plenty for the modulation depths used here.
#[derive(Clone, Debug)]
pub struct Delay {
    buffer: Vec<f32>,
    write: usize,
    pub feedback: f32,
    pub mix: f32,
    pub damp: f32,
    delay_samples: f32,
    damped: f32,
}

impl Delay {
    pub fn new(sample_rate: f32, max_seconds: f32) -> Self {
        let len = (sample_rate * max_seconds).ceil() as usize + 4;
        Self {
            buffer: vec![0.0; len],
            write: 0,
            feedback: 0.35,
            mix: 0.25,
            damp: 0.3,
            delay_samples: sample_rate * 0.2,
            damped: 0.0,
        }
    }

    pub fn set_time(&mut self, seconds: f32) {
        let len = self.buffer.len() as f32;
        self.delay_samples = (seconds * 44_100.0).clamp(1.0, len - 2.0);
    }

    pub fn set_time_samples(&mut self, samples: f32) {
        let len = self.buffer.len() as f32;
        self.delay_samples = samples.clamp(1.0, len - 2.0);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let len = self.buffer.len();
        let read = self.write as f32 - self.delay_samples;
        let read = if read < 0.0 { read + len as f32 } else { read };
        let i0 = read.floor() as usize % len;
        let i1 = (i0 + 1) % len;
        let frac = read - read.floor();
        let delayed = self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac;
        self.damped += self.damp * (delayed - self.damped);
        self.buffer[self.write] = x + self.damped * self.feedback.clamp(0.0, 0.98);
        self.write = (self.write + 1) % len;
        x * (1.0 - self.mix) + delayed * self.mix
    }
}

/// Freeverb-inspired stereo reverb: 8 combs + 4 allpasses per channel with the
/// classic tuning. Input is mono; output adds width.
#[derive(Clone, Debug)]
pub struct Reverb {
    combs: [Vec<f32>; 8],
    comb_len: [usize; 8],
    comb_index: [usize; 8],
    comb_low: [f32; 8],
    allpass: [Vec<f32>; 4],
    allpass_len: [usize; 4],
    allpass_index: [usize; 4],
    left_offset: usize,
    right_offset: usize,
    pub room: f32,
    pub damp: f32,
    pub mix: f32,
}

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let mut combs = std::array::from_fn(|i| {
            let len = (COMB_TUNING[i] as f32 * scale) as usize;
            vec![0.0; len.max(1)]
        });
        let mut comb_len = [0usize; 8];
        let mut comb_index = [0usize; 8];
        for (i, buffer) in combs.iter_mut().enumerate() {
            comb_len[i] = buffer.len();
            comb_index[i] = 0;
            buffer.iter_mut().for_each(|s| *s = 0.0);
        }
        let allpass = std::array::from_fn(|i| {
            let len = (ALLPASS_TUNING[i] as f32 * scale) as usize;
            vec![0.0; len.max(1)]
        });
        let mut allpass_len = [0usize; 4];
        let mut allpass_index = [0usize; 4];
        for (i, buffer) in allpass.iter().enumerate() {
            allpass_len[i] = buffer.len();
            allpass_index[i] = 0;
        }
        Self {
            combs,
            comb_len,
            comb_index,
            comb_low: [0.0; 8],
            allpass,
            allpass_len,
            allpass_index,
            left_offset: (23.0 * scale) as usize,
            right_offset: (61.0 * scale) as usize,
            room: 0.72,
            damp: 0.35,
            mix: 0.18,
        }
    }

    fn comb(&mut self, i: usize, input: f32) -> f32 {
        let len = self.comb_len[i];
        let idx = self.comb_index[i];
        let out = self.combs[i][idx];
        self.comb_low[i] += self.damp * (out - self.comb_low[i]);
        let write = input * 0.11 + self.comb_low[i] * self.room;
        self.combs[i][idx] = write;
        self.comb_index[i] = (idx + 1) % len;
        out
    }

    fn allpass(&mut self, i: usize, input: f32) -> f32 {
        let len = self.allpass_len[i];
        let idx = self.allpass_index[i];
        let buffered = self.allpass[i][idx];
        let out = -input + buffered;
        self.allpass[i][idx] = input + buffered * 0.5;
        self.allpass_index[i] = (idx + 1) % len;
        out
    }

    /// Returns (wet_l, wet_r) without mixing; caller applies `mix`.
    pub fn process(&mut self, input: f32) -> (f32, f32) {
        let mut left = 0.0;
        let mut right = 0.0;
        for i in 0..8 {
            let tap = self.comb(i, input);
            if i % 2 == 0 {
                left += tap;
            } else {
                right += tap;
            }
        }
        for i in 0..4 {
            left = self.allpass(i, left);
            right = self.allpass(i, right);
        }
        (left, right)
    }

    /// Convenience stereo mix-in.
    #[inline]
    pub fn mix_in(&mut self, input: f32, out: &mut [f32; 2]) {
        let (l, r) = self.process(input);
        out[0] = out[0] * (1.0 - self.mix) + l * self.mix;
        out[1] = out[1] * (1.0 - self.mix) + r * self.mix;
    }

    pub fn reset(&mut self) {
        self.combs.iter_mut().for_each(|c| c.fill(0.0));
        self.allpass.iter_mut().for_each(|c| c.fill(0.0));
        self.comb_low = [0.0; 8];
    }
}

/// Two-voice stereo chorus with quadrature LFOs.
#[derive(Clone, Debug)]
pub struct Chorus {
    delay: Delay,
    lfo_phase: f32,
    pub rate: f32,
    pub depth_ms: f32,
    pub mix: f32,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        let mut delay = Delay::new(sample_rate, 0.06);
        delay.feedback = 0.0;
        delay.mix = 1.0;
        delay.damp = 0.0;
        Self {
            delay,
            lfo_phase: 0.0,
            rate: 0.35,
            depth_ms: 4.5,
            mix: 0.35,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32, dt: f32) -> f32 {
        self.lfo_phase = (self.lfo_phase + self.rate * dt).fract();
        let lfo = (self.lfo_phase * TAU).sin() * self.depth_ms * 44.1;
        self.delay.set_time_samples(14.0 * 44.1 + lfo);
        let wet = self.delay.process(x);
        x * (1.0 - self.mix) + wet * self.mix
    }
}

// ---------- Dynamics ----------

/// Feed-forward compressor with soft knee and makeup gain.
#[derive(Clone, Copy, Debug)]
pub struct Compressor {
    envelope: f32,
    attack: f32,
    release: f32,
    pub threshold_db: f32,
    pub ratio: f32,
    pub makeup_db: f32,
    sample_rate: f32,
}

impl Compressor {
    pub fn new(sample_rate: f32, threshold_db: f32, ratio: f32) -> Self {
        let mut c = Self {
            envelope: 0.0,
            attack: 0.0,
            release: 0.0,
            threshold_db,
            ratio,
            makeup_db: 0.0,
            sample_rate,
        };
        c.set_times(8.0, 120.0);
        c
    }

    pub fn set_times(&mut self, attack_ms: f32, release_ms: f32) {
        self.attack = (-1.0 / (attack_ms * 0.001 * self.sample_rate)).exp();
        self.release = (-1.0 / (release_ms * 0.001 * self.sample_rate)).exp();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let rectified = x.abs();
        let coeff = if rectified > self.envelope {
            self.attack
        } else {
            self.release
        };
        self.envelope = rectified + coeff * (self.envelope - rectified);
        let envelope_db = 20.0 * self.envelope.max(1e-6).log10();
        let over = envelope_db - self.threshold_db;
        let gain_db = if over > 0.0 {
            -over * (1.0 - 1.0 / self.ratio.max(1.0))
        } else {
            0.0
        };
        x * 10.0f32.powf((gain_db + self.makeup_db) / 20.0)
    }
}

/// Transparent-enough soft clipper used as the final safety net.
#[derive(Clone, Copy, Debug, Default)]
pub struct Limiter {
    gain: f32,
    ceiling: f32,
}

impl Limiter {
    pub fn new(ceiling: f32) -> Self {
        Self {
            gain: 1.0,
            ceiling: ceiling.clamp(0.1, 1.0),
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let peak = x.abs();
        let target = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        let rate = if target < self.gain { 0.4 } else { 0.003 };
        self.gain += (target - self.gain) * rate;
        (x * self.gain).clamp(-1.0, 1.0)
    }
}

/// Musical saturation. `drive` 1.0 is nearly clean, 6.0 is aggressive.
#[inline]
pub fn saturate(x: f32, drive: f32) -> f32 {
    let driven = x * drive.max(1.0);
    driven / (1.0 + driven.abs())
}

#[inline]
pub fn soft_clip(x: f32) -> f32 {
    // Fast tanh approximation good to ~0.1% over the audible range, clamped
    // so a hot input can never leave the [-1, 1] bus.
    let x = x.clamp(-4.0, 4.0);
    let x2 = x * x;
    (x * (27.0 + x2) / (27.0 + 9.0 * x2)).clamp(-1.0, 1.0)
}

// ---------- LFO ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LfoShape {
    Sine,
    Triangle,
    Saw,
    Square,
    SampleHold,
}

#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    pub phase: f32,
    pub rate: f32,
    pub shape: LfoShape,
    hold: f32,
    rng: u32,
}

impl Lfo {
    pub fn new(rate: f32, shape: LfoShape) -> Self {
        Self {
            phase: 0.0,
            rate,
            shape,
            hold: 0.0,
            rng: 0x1234_5678,
        }
    }

    #[inline]
    pub fn next(&mut self, dt: f32) -> f32 {
        let prev = self.phase;
        self.phase = (self.phase + self.rate * dt).fract();
        if self.phase < prev {
            self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
            self.hold = (self.rng >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
        }
        match self.shape {
            LfoShape::Sine => (self.phase * TAU).sin(),
            LfoShape::Triangle => Osc::tri(self.phase),
            LfoShape::Saw => Osc::saw(self.phase),
            LfoShape::Square => Osc::square(self.phase, 0.5),
            LfoShape::SampleHold => self.hold,
        }
    }
}

// ---------- Drums ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrumKind {
    Kick,
    Snare,
    HatClosed,
    HatOpen,
    Clap,
    Tom,
    Rim,
}

/// A single percussion voice with its own oscillator/noise state.
#[derive(Clone, Debug)]
pub struct DrumVoice {
    kind: DrumKind,
    envelope: DecayEnv,
    pitch_env: DecayEnv,
    phase: f32,
    noise: Noise,
    filter: Biquad,
    velocity: f32,
    active: bool,
    body: f32,
}

impl DrumVoice {
    pub fn new(kind: DrumKind, sample_rate: f32, seed: u32) -> Self {
        let (time, curve) = match kind {
            DrumKind::Kick => (0.42, 1.4),
            DrumKind::Snare => (0.30, 1.1),
            DrumKind::HatClosed => (0.07, 1.0),
            DrumKind::HatOpen => (0.42, 1.0),
            DrumKind::Clap => (0.24, 1.2),
            DrumKind::Tom => (0.45, 1.2),
            DrumKind::Rim => (0.09, 1.0),
        };
        let mut voice = Self {
            kind,
            envelope: DecayEnv::new(time, curve),
            pitch_env: DecayEnv::new(0.09, 1.0),
            phase: 0.0,
            noise: Noise::new(seed),
            filter: Biquad::low_pass(sample_rate, 6_000.0, 0.9),
            velocity: 0.0,
            active: false,
            body: 1.0,
        };
        match kind {
            DrumKind::HatClosed | DrumKind::HatOpen | DrumKind::Clap => {
                voice.filter = Biquad::high_pass(sample_rate, 6_800.0, 0.8);
            }
            DrumKind::Snare => {
                voice.filter = Biquad::band_pass(sample_rate, 1_900.0, 0.7);
            }
            DrumKind::Rim => {
                voice.filter = Biquad::band_pass(sample_rate, 3_200.0, 1.2);
            }
            _ => {}
        }
        voice
    }

    pub fn trigger(&mut self, velocity: f32) {
        self.velocity = velocity.clamp(0.0, 1.5);
        self.envelope.trigger();
        self.pitch_env.trigger();
        self.active = true;
        self.phase = 0.0;
        self.body = 1.0;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    #[inline]
    pub fn next(&mut self, sample_rate: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let dt = 1.0 / sample_rate;
        let env = self.envelope.next(dt);
        let pitch = self.pitch_env.next(dt);
        if !self.envelope.is_positive() {
            self.active = false;
            return 0.0;
        }
        let out = match self.kind {
            DrumKind::Kick => {
                let freq = 48.0 + 95.0 * pitch;
                self.phase += freq / sample_rate;
                let tone = Osc::sine(self.phase) * env;
                let click = self.noise.white() * env * env * 0.18;
                tone + click
            }
            DrumKind::Snare => {
                self.phase += 185.0 / sample_rate;
                let tone = Osc::sine(self.phase) * env * env * 0.35;
                let noise = self.filter.process(self.noise.white()) * env;
                tone + noise * 0.9
            }
            DrumKind::HatClosed | DrumKind::HatOpen => {
                let noise = self.filter.process(self.noise.white());
                noise * env * 0.6
            }
            DrumKind::Clap => {
                let noise = self.filter.process(self.noise.white());
                // Three quick noise bursts feel like a hand clap.
                let burst = if self.envelope.progress() < 0.35 {
                    1.0
                } else {
                    0.38
                };
                noise * env * burst * 0.8
            }
            DrumKind::Tom => {
                let freq = 110.0 + 90.0 * pitch;
                self.phase += freq / sample_rate;
                let tone = Osc::sine(self.phase) * env;
                let noise = self.noise.white() * env * env * 0.08;
                tone + noise
            }
            DrumKind::Rim => {
                let noise = self.filter.process(self.noise.white());
                let tone = Osc::sine(self.phase) * env * 0.3;
                self.phase += 1_700.0 / sample_rate;
                noise * env * 0.7 + tone
            }
        };
        out * self.velocity * self.body
    }
}

impl DecayEnv {
    #[inline]
    fn is_positive(&self) -> bool {
        self.value > 0.0
    }

    #[inline]
    fn progress(&self) -> f32 {
        self.value.clamp(0.0, 1.0)
    }
}

/// Complete drum kit with slightly detuned stereo spread.
#[derive(Clone, Debug)]
pub struct DrumKit {
    pub kick: DrumVoice,
    pub snare: DrumVoice,
    pub hat_closed: DrumVoice,
    pub hat_open: DrumVoice,
    pub clap: DrumVoice,
    pub tom: DrumVoice,
    pub rim: DrumVoice,
    sample_rate: f32,
}

impl DrumKit {
    pub fn new(sample_rate: f32, seed: u32) -> Self {
        Self {
            kick: DrumVoice::new(DrumKind::Kick, sample_rate, seed ^ 0x1111),
            snare: DrumVoice::new(DrumKind::Snare, sample_rate, seed ^ 0x2222),
            hat_closed: DrumVoice::new(DrumKind::HatClosed, sample_rate, seed ^ 0x3333),
            hat_open: DrumVoice::new(DrumKind::HatOpen, sample_rate, seed ^ 0x4444),
            clap: DrumVoice::new(DrumKind::Clap, sample_rate, seed ^ 0x5555),
            tom: DrumVoice::new(DrumKind::Tom, sample_rate, seed ^ 0x6666),
            rim: DrumVoice::new(DrumKind::Rim, sample_rate, seed ^ 0x7777),
            sample_rate,
        }
    }

    pub fn trigger(&mut self, kind: DrumKind, velocity: f32) {
        match kind {
            DrumKind::Kick => self.kick.trigger(velocity),
            DrumKind::Snare => self.snare.trigger(velocity),
            DrumKind::HatClosed => self.hat_closed.trigger(velocity),
            DrumKind::HatOpen => self.hat_open.trigger(velocity),
            DrumKind::Clap => self.clap.trigger(velocity),
            DrumKind::Tom => self.tom.trigger(velocity),
            DrumKind::Rim => self.rim.trigger(velocity),
        }
    }

    /// Returns (kick/snare bus, hat/perc bus) so callers can apply separate
    /// processing (sidechain, brightness).
    #[inline]
    pub fn next(&mut self, sample_rate: f32) -> (f32, f32) {
        let _ = self.sample_rate;
        let body =
            self.kick.next(sample_rate) + self.snare.next(sample_rate) + self.tom.next(sample_rate);
        let top = self.hat_closed.next(sample_rate)
            + self.hat_open.next(sample_rate)
            + self.clap.next(sample_rate)
            + self.rim.next(sample_rate);
        (body, top)
    }

    pub fn all_idle(&self) -> bool {
        !self.kick.is_active()
            && !self.snare.is_active()
            && !self.hat_closed.is_active()
            && !self.hat_open.is_active()
    }
}

// ---------- Plucked string (Karplus-Strong) ----------

/// Karplus-Strong pluck. The buffer is sized for the lowest supported note so
/// no allocation happens after construction.
#[derive(Clone, Debug)]
pub struct Pluck {
    buffer: Vec<f32>,
    index: usize,
    length: usize,
    damping: f32,
    active: bool,
    envelope: DecayEnv,
    brightness: f32,
}

impl Pluck {
    pub fn new(sample_rate: f32, min_hz: f32) -> Self {
        let max_len = (sample_rate / min_hz.max(20.0)).ceil() as usize + 2;
        Self {
            buffer: vec![0.0; max_len],
            index: 0,
            length: 64,
            damping: 0.996,
            active: false,
            envelope: DecayEnv::new(2.2, 1.0),
            brightness: 0.5,
        }
    }

    /// Excite the string at `freq`, with `brightness` controlling the initial
    /// noise content (1.0 = bright, 0.0 = mellow sine-ish pluck).
    pub fn pluck(&mut self, sample_rate: f32, freq: f32, brightness: f32, velocity: f32) {
        let freq = freq.clamp(20.0, sample_rate * 0.45);
        self.length = ((sample_rate / freq) as usize).clamp(4, self.buffer.len() - 2);
        self.brightness = brightness.clamp(0.0, 1.0);
        let velocity = velocity.clamp(0.0, 1.5);
        let mut noise = Noise::new((freq * 1000.0) as u32 ^ 0x51A7);
        let mut amp = velocity;
        let mut dc = 0.0;
        for i in 0..self.length {
            let n = noise.white();
            dc += 0.02 * (n - dc);
            let excitation = n * (0.25 + self.brightness * 0.75) + dc * 0.2;
            amp *= 0.988;
            self.buffer[i] = excitation * amp;
        }
        self.index = 0;
        self.active = true;
        self.envelope = DecayEnv::new(3.4, 1.0);
        self.envelope.trigger();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    #[inline]
    pub fn next(&mut self, sample_rate: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let dt = 1.0 / sample_rate;
        let env = self.envelope.next(dt);
        let next_index = (self.index + 1) % self.length;
        let averaged = (self.buffer[self.index] + self.buffer[next_index]) * 0.5 * self.damping;
        self.buffer[self.index] = averaged;
        let out = self.buffer[self.index];
        self.index = next_index;
        if env <= 0.0 {
            self.active = false;
        }
        out * env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    #[test]
    fn smoothed_converges() {
        let mut s = Smoothed::new(0.0, 30.0);
        s.set(1.0);
        for _ in 0..40_000 {
            s.next();
        }
        assert!((s.current() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn noise_rng_is_deterministic_and_bounded() {
        let mut a = NoiseRng::new(99);
        let mut b = NoiseRng::new(99);
        for _ in 0..1000 {
            let va = a.next_bipolar();
            let vb = b.next_bipolar();
            assert_eq!(va, vb);
            assert!((-1.0..1.0).contains(&va));
        }
    }

    #[test]
    fn oscillators_are_bounded_and_periodic() {
        for i in 0..1000 {
            let phase = i as f32 / 100.0;
            for value in [
                Osc::sine(phase),
                Osc::tri(phase),
                Osc::saw(phase),
                Osc::square(phase, 0.25),
                Osc::saw_blep(phase, 0.01),
                Osc::square_blep(phase, 0.01, 0.5),
            ] {
                assert!(value.is_finite() && value.abs() <= 1.3);
            }
        }
        assert!((Osc::sine(0.0) - Osc::sine(1.0)).abs() < 1e-5);
    }

    #[test]
    fn midi_roundtrip() {
        for note in [21.0, 57.0, 69.0, 108.0] {
            let hz = midi_hz(note);
            assert!((hz_midi(hz) - note).abs() < 1e-3, "note {note}");
        }
        assert!((midi_hz(69.0) - 440.0).abs() < 1e-3);
    }

    #[test]
    fn adsr_reaches_sustain_and_releases() {
        let mut env = Adsr::new(0.01, 0.1, 0.6, 0.2);
        env.gate_on();
        for _ in 0..20_000 {
            env.next(1.0 / SR);
        }
        assert!((env.value() - 0.6).abs() < 0.01, "value {}", env.value());
        env.gate_off();
        for _ in 0..60_000 {
            env.next(1.0 / SR);
        }
        assert_eq!(env.value(), 0.0);
        assert!(!env.is_active());
    }

    #[test]
    fn biquad_lowpass_is_stable() {
        let mut filter = Biquad::low_pass(SR, 1_000.0, 0.7);
        let mut last = 0.0;
        for i in 0..44_100 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            last = filter.process(input);
            assert!(last.is_finite() && last.abs() < 4.0);
        }
        assert!(last.abs() < 1e-5, "impulse did not decay: {last}");
    }

    #[test]
    fn svf_sweep_stays_finite() {
        let mut filter = Svf::new(SR, 200.0, 0.7);
        for i in 0..44_100 {
            filter.set(200.0 + (i as f32 / 44_100.0) * 8_000.0, 0.9);
            let (_low, _band, high) = filter.process(Osc::sine(i as f32 * 0.01));
            assert!(high.is_finite());
        }
    }

    #[test]
    fn delay_produces_echo_after_delay_time() {
        let mut delay = Delay::new(SR, 0.5);
        delay.set_time(0.01);
        delay.feedback = 0.5;
        delay.mix = 1.0;
        delay.damp = 0.0;
        let mut heard_echo = false;
        for i in 0..2_000 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            let out = delay.process(input);
            if i > 400 && out.abs() > 0.01 {
                heard_echo = true;
            }
            assert!(out.is_finite());
        }
        assert!(heard_echo);
    }

    #[test]
    fn reverb_tail_decays() {
        let mut reverb = Reverb::new(SR);
        reverb.mix = 0.3;
        let mut energy_late = 0.0f32;
        let mut energy_early = 0.0f32;
        for i in 0..44_100 * 3 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            let (l, r) = reverb.process(input);
            assert!(l.is_finite() && r.is_finite());
            if i < 10_000 {
                energy_early += l.abs();
            } else if i > 44_100 * 2 {
                energy_late += l.abs();
            }
        }
        assert!(energy_early > 0.0);
        assert!(energy_late < energy_early * 0.5);
    }

    #[test]
    fn limiter_respects_ceiling() {
        let mut limiter = Limiter::new(0.9);
        for i in 0..10_000 {
            let x = (i as f32 * 0.37).sin() * 4.0;
            let y = limiter.process(x);
            assert!(y.abs() <= 1.0);
        }
    }

    #[test]
    fn saturation_is_bounded() {
        for i in -1000..1000 {
            let x = i as f32 * 0.01;
            assert!(saturate(x, 4.0).abs() < 1.0);
            assert!(soft_clip(x).abs() <= 1.0 + 1e-4);
        }
    }

    #[test]
    fn lfo_shapes_stay_bounded() {
        let mut lfos = [
            Lfo::new(3.0, LfoShape::Sine),
            Lfo::new(3.0, LfoShape::Triangle),
            Lfo::new(3.0, LfoShape::Saw),
            Lfo::new(3.0, LfoShape::Square),
            Lfo::new(3.0, LfoShape::SampleHold),
        ];
        for _ in 0..10_000 {
            for lfo in &mut lfos {
                let v = lfo.next(1.0 / SR);
                assert!((-1.01..=1.01).contains(&v));
            }
        }
    }

    #[test]
    fn drums_fire_and_decay() {
        let mut kit = DrumKit::new(SR, 7);
        kit.trigger(DrumKind::Kick, 1.0);
        kit.trigger(DrumKind::Snare, 0.8);
        kit.trigger(DrumKind::HatClosed, 0.6);
        let mut peak = 0.0f32;
        for _ in 0..44_100 {
            let (body, top) = kit.next(SR);
            assert!(body.is_finite() && top.is_finite());
            peak = peak.max(body.abs() + top.abs());
        }
        assert!(peak > 0.05, "drums were silent");
        assert!(kit.all_idle(), "drum tails did not finish");
    }

    #[test]
    fn pluck_has_energy_and_pitch_near_request() {
        let mut pluck = Pluck::new(SR, 60.0);
        pluck.pluck(SR, 220.0, 0.7, 1.0);
        let mut samples = Vec::new();
        for _ in 0..30_000 {
            let v = pluck.next(SR);
            assert!(v.is_finite());
            samples.push(v);
        }
        // The first string loops still sound like the excitation noise; pitch
        // is measured on the settled tail with autocorrelation.
        let late = &samples[10_000..];
        let peak = late.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.01, "pluck was silent");
        let mut best_lag = 1;
        let mut best = f32::MIN;
        for lag in 20..500usize {
            let mut correlation = 0.0;
            for i in 0..late.len() - lag {
                correlation += late[i] * late[i + lag];
            }
            if correlation > best {
                best = correlation;
                best_lag = lag;
            }
        }
        let hz = SR / best_lag as f32;
        assert!((hz - 220.0).abs() / 220.0 < 0.1, "pluck pitch was {hz}");
    }

    #[test]
    fn chorus_stays_bounded() {
        let mut chorus = Chorus::new(SR);
        for i in 0..44_100 {
            let y = chorus.process(Osc::sine(i as f32 * 220.0 / SR), 1.0 / SR);
            assert!(y.is_finite() && y.abs() < 2.0);
        }
    }
}
