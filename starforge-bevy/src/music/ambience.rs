//! Environmental ambience: wind, caves, oceans, lava and living forests as an
//! infinite synthesized stereo bed, crossfading whenever the biome or time of
//! day changes. The weather rain loop lives in `weather.rs`; everything else
//! environmental lives here.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::dsp::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AmbienceKind {
    None,
    Wind,
    Cave,
    Ocean,
    Lava,
    Forest,
    Swamp,
    Frozen,
    Desert,
}

impl AmbienceKind {
    pub fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Wind,
            2 => Self::Cave,
            3 => Self::Ocean,
            4 => Self::Lava,
            5 => Self::Forest,
            6 => Self::Swamp,
            7 => Self::Frozen,
            8 => Self::Desert,
            _ => Self::None,
        }
    }

    pub fn as_u32(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Wind => 1,
            Self::Cave => 2,
            Self::Ocean => 3,
            Self::Lava => 4,
            Self::Forest => 5,
            Self::Swamp => 6,
            Self::Frozen => 7,
            Self::Desert => 8,
        }
    }

    /// Pick the bed for a biome key; the caller overrides for caves/weather.
    pub fn for_biome(key: &str) -> Self {
        match key {
            "ocean" | "murk" => Self::Ocean,
            "volcanic" | "obsidian" | "ashen" => Self::Lava,
            "lush" | "fungal" => Self::Forest,
            "redmoss" | "hive" | "amber" => Self::Swamp,
            "frozen" | "crystal" => Self::Frozen,
            "desert" | "salt" => Self::Desert,
            "ferrous" | "alien" => Self::Wind,
            _ => Self::Wind,
        }
    }
}

/// Lock-free control surface for ambience.
pub struct AmbienceShared {
    pub kind: AtomicU32,
    pub intensity: AtomicU32,
    pub volume: AtomicU32,
    pub master: AtomicU32,
    pub night: AtomicU32,
    pub paused: AtomicU32,
}

impl AmbienceShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            kind: AtomicU32::new(AmbienceKind::None.as_u32()),
            intensity: AtomicU32::new(0.5f32.to_bits()),
            volume: AtomicU32::new(0.6f32.to_bits()),
            master: AtomicU32::new(0.8f32.to_bits()),
            night: AtomicU32::new(0.0f32.to_bits()),
            paused: AtomicU32::new(0),
        })
    }

    #[inline]
    fn set_f32(atom: &AtomicU32, value: f32) {
        atom.store(value.to_bits(), Ordering::Relaxed);
    }

    #[inline]
    fn get_f32(atom: &AtomicU32) -> f32 {
        f32::from_bits(atom.load(Ordering::Relaxed))
    }

    pub fn set_kind(&self, kind: AmbienceKind) {
        self.kind.store(kind.as_u32(), Ordering::Relaxed);
    }

    pub fn kind(&self) -> AmbienceKind {
        AmbienceKind::from_u32(self.kind.load(Ordering::Relaxed))
    }

    pub fn set_intensity(&self, value: f32) {
        Self::set_f32(&self.intensity, value.clamp(0.0, 1.0));
    }

    pub fn intensity(&self) -> f32 {
        Self::get_f32(&self.intensity)
    }

    pub fn set_volume(&self, value: f32) {
        Self::set_f32(&self.volume, value.clamp(0.0, 1.0));
    }

    pub fn volume(&self) -> f32 {
        Self::get_f32(&self.volume)
    }

    pub fn set_master(&self, value: f32) {
        Self::set_f32(&self.master, value.clamp(0.0, 1.0));
    }

    pub fn master(&self) -> f32 {
        Self::get_f32(&self.master)
    }

    pub fn set_night(&self, value: f32) {
        Self::set_f32(&self.night, value.clamp(0.0, 1.0));
    }

    pub fn night(&self) -> f32 {
        Self::get_f32(&self.night)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused as u32, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed) != 0
    }
}

/// A short event voice used for drips, chirps and crackles.
#[derive(Clone, Copy, Debug)]
struct EventVoice {
    phase: f32,
    base_freq: f32,
    slide: f32,
    envelope: DecayEnv,
    level: f32,
    active: bool,
    pan: f32,
}

impl EventVoice {
    fn new() -> Self {
        Self {
            phase: 0.0,
            base_freq: 800.0,
            slide: 1.0,
            envelope: DecayEnv::new(0.12, 1.0),
            level: 0.0,
            active: false,
            pan: 0.0,
        }
    }

    fn trigger(&mut self, freq: f32, slide: f32, time: f32, level: f32, pan: f32) {
        self.base_freq = freq;
        self.slide = slide;
        self.envelope = DecayEnv::new(time, 1.0);
        self.envelope.trigger();
        self.level = level;
        self.pan = pan;
        self.active = true;
    }

    #[inline]
    fn process(&mut self, sample_rate: f32) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }
        let dt = 1.0 / sample_rate;
        self.base_freq *= self.slide;
        self.phase = (self.phase + self.base_freq / sample_rate).fract();
        let env = self.envelope.next(dt);
        if env <= 0.0 {
            self.active = false;
        }
        let value = Osc::sine(self.phase) * env * self.level;
        (
            value * (1.0 - self.pan) * 0.5,
            value * (1.0 + self.pan) * 0.5,
        )
    }
}

/// One ambience bed with its own noise and filters.
struct Bed {
    kind: AmbienceKind,
    noise: Noise,
    band: Biquad,
    low: Biquad,
    noise_r: Noise,
    band_r: Biquad,
    low_r: Biquad,
    lfo: f32,
    lfo_rate: f32,
    swell: f32,
    events: [EventVoice; 3],
    event_timer: f32,
    rng: u32,
}

impl Bed {
    fn new(sample_rate: f32, seed: u32) -> Self {
        Self {
            kind: AmbienceKind::None,
            noise: Noise::new(seed ^ 0xA1B2),
            band: Biquad::band_pass(sample_rate, 500.0, 0.7),
            low: Biquad::low_pass(sample_rate, 900.0, 0.7),
            noise_r: Noise::new(seed ^ 0xC3D4),
            band_r: Biquad::band_pass(sample_rate, 520.0, 0.7),
            low_r: Biquad::low_pass(sample_rate, 940.0, 0.7),
            lfo: 0.0,
            lfo_rate: 0.07,
            swell: 0.0,
            events: [EventVoice::new(); 3],
            event_timer: 2.0,
            rng: seed.max(1),
        }
    }

    fn configure(&mut self, kind: AmbienceKind, sample_rate: f32) {
        self.kind = kind;
        match kind {
            AmbienceKind::None => {}
            AmbienceKind::Wind | AmbienceKind::Desert => {
                self.band.set(FilterKind::BandPass, 420.0, 0.6, 0.0);
                self.band_r.set(FilterKind::BandPass, 460.0, 0.6, 0.0);
                self.lfo_rate = 0.09;
            }
            AmbienceKind::Cave => {
                self.low.set(FilterKind::LowPass, 260.0, 0.6, 0.0);
                self.low_r.set(FilterKind::LowPass, 250.0, 0.6, 0.0);
                self.lfo_rate = 0.03;
            }
            AmbienceKind::Ocean => {
                self.band.set(FilterKind::BandPass, 700.0, 0.45, 0.0);
                self.band_r.set(FilterKind::BandPass, 760.0, 0.45, 0.0);
                self.lfo_rate = 0.06;
            }
            AmbienceKind::Lava => {
                self.low.set(FilterKind::LowPass, 180.0, 0.9, 0.0);
                self.low_r.set(FilterKind::LowPass, 170.0, 0.9, 0.0);
                self.lfo_rate = 0.04;
            }
            AmbienceKind::Forest | AmbienceKind::Swamp => {
                self.band.set(FilterKind::BandPass, 900.0, 0.4, 0.0);
                self.band_r.set(FilterKind::BandPass, 950.0, 0.4, 0.0);
                self.lfo_rate = 0.05;
            }
            AmbienceKind::Frozen => {
                self.band.set(FilterKind::BandPass, 1_800.0, 1.4, 0.0);
                self.band_r.set(FilterKind::BandPass, 2_100.0, 1.4, 0.0);
                self.lfo_rate = 0.11;
            }
        }
        self.lfo = 0.0;
        self.events = [EventVoice::new(); 3];
        let _ = sample_rate;
    }

    fn next_random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.rng >> 8) as f32 / (1 << 24) as f32
    }

    #[inline]
    fn process(&mut self, sample_rate: f32, night: f32) -> (f32, f32) {
        let dt = 1.0 / sample_rate;
        self.lfo = (self.lfo + self.lfo_rate * dt).fract();
        let lfo = (self.lfo * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        self.swell += (lfo - self.swell) * dt * 0.7;
        let mut left;
        let mut right;
        match self.kind {
            AmbienceKind::None => {
                left = 0.0;
                right = 0.0;
            }
            AmbienceKind::Wind | AmbienceKind::Desert | AmbienceKind::Frozen => {
                let wind = 0.35 + self.swell * 0.65;
                let l = self.band.process(self.noise.pink()) * wind;
                let r = self.band_r.process(self.noise_r.pink()) * wind;
                left = l * 0.5;
                right = r * 0.5;
                // Occasional gust.
                self.event_timer -= dt;
                if self.event_timer <= 0.0 {
                    self.event_timer = 6.0 + self.next_random() * 10.0;
                    let pan = self.next_random() * 2.0 - 1.0;
                    let freq = 90.0 + self.next_random() * 60.0;
                    self.events[0].trigger(freq, 1.0002, 1.6, 0.25, pan);
                }
            }
            AmbienceKind::Cave => {
                let l = self.low.process(self.noise.brown()) * 0.55;
                let r = self.low_r.process(self.noise_r.brown()) * 0.55;
                left = l;
                right = r;
                self.event_timer -= dt;
                if self.event_timer <= 0.0 {
                    self.event_timer = 1.6 + self.next_random() * 4.0;
                    let pan = self.next_random() * 2.0 - 1.0;
                    let slot = (self.next_random() * 3.0) as usize % 3;
                    let freq = 900.0 + self.next_random() * 900.0;
                    self.events[slot].trigger(freq, 0.996, 0.18, 0.12, pan);
                }
            }
            AmbienceKind::Ocean => {
                let wave = 0.25 + self.swell * 0.9;
                let l = self.band.process(self.noise.pink()) * wave;
                let r = self.band_r.process(self.noise_r.pink()) * wave;
                left = l * 0.6;
                right = r * 0.6;
                // Foam hiss on the crest.
                if self.swell > 0.8 {
                    left += self.noise.white() * 0.03 * (self.swell - 0.8) * 5.0;
                    right += self.noise_r.white() * 0.03 * (self.swell - 0.8) * 5.0;
                }
            }
            AmbienceKind::Lava => {
                let rumble = 0.5 + self.swell * 0.5;
                let l = self.low.process(self.noise.brown()) * rumble;
                let r = self.low_r.process(self.noise_r.brown()) * rumble;
                left = l * 0.8;
                right = r * 0.8;
                self.event_timer -= dt;
                if self.event_timer <= 0.0 {
                    self.event_timer = 0.7 + self.next_random() * 2.4;
                    let pan = self.next_random() * 2.0 - 1.0;
                    let slot = (self.next_random() * 3.0) as usize % 3;
                    let freq = 220.0 + self.next_random() * 500.0;
                    self.events[slot].trigger(freq, 0.99, 0.12, 0.1, pan);
                }
            }
            AmbienceKind::Forest | AmbienceKind::Swamp => {
                let breeze = 0.18 + self.swell * 0.3;
                let l = self.band.process(self.noise.pink()) * breeze;
                let r = self.band_r.process(self.noise_r.pink()) * breeze;
                left = l * 0.4;
                right = r * 0.4;
                // Birds by day, insects by night.
                self.event_timer -= dt;
                if self.event_timer <= 0.0 {
                    let pan = self.next_random() * 2.0 - 1.0;
                    let slot = (self.next_random() * 3.0) as usize % 3;
                    if night > 0.5 {
                        // Crickets: fast repeated high pulses.
                        self.event_timer = 0.35 + self.next_random() * 0.9;
                        let freq = 3_600.0 + self.next_random() * 900.0;
                        self.events[slot].trigger(freq, 1.0, 0.05, 0.05, pan);
                    } else {
                        self.event_timer = 2.2 + self.next_random() * 5.0;
                        let freq = 1_800.0 + self.next_random() * 1_600.0;
                        self.events[slot].trigger(freq, 0.985, 0.16, 0.09, pan);
                    }
                }
            }
        }
        for voice in &mut self.events {
            let (l, r) = voice.process(sample_rate);
            left += l;
            right += r;
        }
        (left, right)
    }
}

/// Infinite ambience decoder.
pub struct AmbienceDecoder {
    shared: Arc<AmbienceShared>,
    beds: [Bed; 2],
    active: usize,
    fade: f32,
    target: AmbienceKind,
    sample_rate: f32,
    frame: [f32; 2],
    channel: usize,
    gain: Smoothed,
    intensity: Smoothed,
    control: u32,
    sample_counter: u64,
}

impl AmbienceDecoder {
    pub fn new(shared: Arc<AmbienceShared>, sample_rate: f32) -> Self {
        let seed = 0x51A7_0001;
        Self {
            shared,
            beds: [
                Bed::new(sample_rate, seed),
                Bed::new(sample_rate, seed ^ 0xBEEF),
            ],
            active: 0,
            fade: 1.0,
            target: AmbienceKind::None,
            sample_rate,
            frame: [0.0; 2],
            channel: 0,
            gain: Smoothed::new(0.0, 600.0),
            intensity: Smoothed::new(0.5, 500.0),
            control: 0,
            sample_counter: 0,
        }
    }

    fn control_tick(&mut self) {
        let kind = self.shared.kind();
        if kind != self.target {
            self.target = kind;
            if self.fade >= 0.999 {
                let next = 1 - self.active;
                self.beds[next].configure(kind, self.sample_rate);
                self.active = next;
                self.fade = 0.0;
            }
        }
        self.intensity.set(self.shared.intensity());
        let volume = self.shared.volume() * self.shared.master();
        if self.shared.paused() {
            self.gain.jump(0.0);
        } else {
            self.gain.set(volume);
        }
    }

    #[inline]
    fn render_frame(&mut self) -> [f32; 2] {
        if self.control == 0 {
            self.control_tick();
        }
        self.control = (self.control + 1) % 32;
        let dt = 1.0 / self.sample_rate;
        let night = self.shared.night();
        self.fade = (self.fade + dt / 2.5).min(1.0);
        let angle = self.fade * std::f32::consts::FRAC_PI_2;
        let current = angle.sin();
        let previous = angle.cos();
        let intensity = self.intensity.next();
        let scale = 0.5 + intensity * 0.7;
        let mut left = 0.0;
        let mut right = 0.0;
        if self.fade < 1.0 {
            let (l, r) = self.beds[1 - self.active].process(self.sample_rate, night);
            left += l * previous;
            right += r * previous;
        }
        let (l, r) = self.beds[self.active].process(self.sample_rate, night);
        left += l * current;
        right += r * current;
        let gain = self.gain.next();
        let mono = (left + right) * 0.5;
        let _ = mono;
        self.sample_counter = self.sample_counter.wrapping_add(1);
        [
            (left * scale * gain).clamp(-1.0, 1.0),
            (right * scale * gain).clamp(-1.0, 1.0),
        ]
    }

    pub fn next_sample(&mut self) -> f32 {
        if self.channel == 0 {
            self.frame = self.render_frame();
            self.channel = 1;
            self.frame[0]
        } else {
            self.channel = 0;
            self.frame[1]
        }
    }
}

impl Iterator for AmbienceDecoder {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        Some(self.next_sample())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, None)
    }
}

impl rodio::Source for AmbienceDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).expect("stereo is non-zero")
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(44_100).expect("sample rate is non-zero")
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(shared: &Arc<AmbienceShared>, samples: usize) -> Vec<f32> {
        let mut decoder = AmbienceDecoder::new(shared.clone(), 44_100.0);
        (0..samples).map(|_| decoder.next().unwrap()).collect()
    }

    #[test]
    fn every_kind_renders_finite_audio() {
        let kinds = [
            AmbienceKind::None,
            AmbienceKind::Wind,
            AmbienceKind::Cave,
            AmbienceKind::Ocean,
            AmbienceKind::Lava,
            AmbienceKind::Forest,
            AmbienceKind::Swamp,
            AmbienceKind::Frozen,
            AmbienceKind::Desert,
        ];
        for kind in kinds {
            let shared = AmbienceShared::new();
            shared.set_kind(kind);
            shared.set_volume(0.6);
            shared.set_master(0.8);
            let samples = render(&shared, 44_100 * 2);
            assert!(samples.iter().all(|s| s.is_finite()), "{kind:?}");
            if kind != AmbienceKind::None {
                let peak = samples.iter().fold(0.0f32, |a, b| a.max(b.abs()));
                assert!(peak > 0.0005, "{kind:?} was silent");
            }
        }
    }

    #[test]
    fn kinds_follow_biomes() {
        assert_eq!(AmbienceKind::for_biome("ocean"), AmbienceKind::Ocean);
        assert_eq!(AmbienceKind::for_biome("volcanic"), AmbienceKind::Lava);
        assert_eq!(AmbienceKind::for_biome("lush"), AmbienceKind::Forest);
        assert_eq!(AmbienceKind::for_biome("frozen"), AmbienceKind::Frozen);
        assert_eq!(AmbienceKind::for_biome("desert"), AmbienceKind::Desert);
    }

    #[test]
    fn pausing_fades_to_silence() {
        let shared = AmbienceShared::new();
        shared.set_kind(AmbienceKind::Wind);
        let mut decoder = AmbienceDecoder::new(shared.clone(), 44_100.0);
        for _ in 0..44_100 {
            let _ = decoder.next();
        }
        shared.set_paused(true);
        for _ in 0..44_100 * 2 {
            let _ = decoder.next();
        }
        let tail: Vec<f32> = (0..4_000).map(|_| decoder.next().unwrap()).collect();
        let peak = tail.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak < 0.002, "paused ambience peak {peak}");
    }
}
