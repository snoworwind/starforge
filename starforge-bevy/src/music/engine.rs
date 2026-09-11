//! Adaptive arrangement engine.
//!
//! [`MusicEngine`] renders an endless stereo stream: two independent layers
//! (current + previous) are arranged and synthesized; scene changes crossfade
//! between them over several seconds so the soundtrack breathes with the game
//! instead of cutting.
//!
//! All state lives inside the engine; the Bevy side only writes a handful of
//! atomics in [`MusicShared`]. Nothing in the render path allocates, locks or
//! touches the ECS, which keeps the audio callback real-time safe.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::dsp::*;
use super::theory::*;

// ---------- Scenes ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MusicScene {
    Menu,
    Planet,
    Cave,
    Space,
    Station,
    Warp,
}

impl MusicScene {
    pub fn from_u32(value: u32) -> Self {
        match value {
            1 => MusicScene::Planet,
            2 => MusicScene::Cave,
            3 => MusicScene::Space,
            4 => MusicScene::Station,
            5 => MusicScene::Warp,
            _ => MusicScene::Menu,
        }
    }

    pub fn as_u32(self) -> u32 {
        match self {
            MusicScene::Menu => 0,
            MusicScene::Planet => 1,
            MusicScene::Cave => 2,
            MusicScene::Space => 3,
            MusicScene::Station => 4,
            MusicScene::Warp => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MusicScene::Menu => "menu",
            MusicScene::Planet => "planet",
            MusicScene::Cave => "cave",
            MusicScene::Space => "space",
            MusicScene::Station => "station",
            MusicScene::Warp => "warp",
        }
    }
}

/// One-shot musical accent triggered by gameplay events.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stinger {
    None = 0,
    Discovery = 1,
    QuestComplete = 2,
    Alert = 3,
    Scan = 4,
    WarpArrival = 5,
    Milestone = 6,
}

impl Stinger {
    pub fn from_u32(value: u32) -> Self {
        match value {
            1 => Stinger::Discovery,
            2 => Stinger::QuestComplete,
            3 => Stinger::Alert,
            4 => Stinger::Scan,
            5 => Stinger::WarpArrival,
            6 => Stinger::Milestone,
            _ => Stinger::None,
        }
    }
}

// ---------- Shared state ----------

/// Lock-free control surface written by the Bevy systems and read by the audio
/// callback. f32 values are stored as bit patterns.
pub struct MusicShared {
    pub scene: AtomicU32,
    pub biome_index: AtomicU32,
    pub day: AtomicU32,
    pub intensity: AtomicU32,
    pub danger: AtomicU32,
    pub cave: AtomicU32,
    pub warping: AtomicU32,
    pub seed: AtomicU32,
    pub revision: AtomicU32,
    pub volume: AtomicU32,
    pub master: AtomicU32,
    pub paused: AtomicU32,
    pub stinger: AtomicU32,
}

impl MusicShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            scene: AtomicU32::new(MusicScene::Menu.as_u32()),
            biome_index: AtomicU32::new(u32::MAX),
            day: AtomicU32::new(0.5f32.to_bits()),
            intensity: AtomicU32::new(0.0f32.to_bits()),
            danger: AtomicU32::new(0.0f32.to_bits()),
            cave: AtomicU32::new(0),
            warping: AtomicU32::new(0),
            seed: AtomicU32::new(1),
            revision: AtomicU32::new(0),
            volume: AtomicU32::new(0.7f32.to_bits()),
            master: AtomicU32::new(0.8f32.to_bits()),
            paused: AtomicU32::new(0),
            stinger: AtomicU32::new(0),
        })
    }

    #[inline]
    fn get_f32(value: &AtomicU32) -> f32 {
        f32::from_bits(value.load(Ordering::Relaxed))
    }

    #[inline]
    fn set_f32(value: &AtomicU32, v: f32) {
        value.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn set_scene(&self, scene: MusicScene) {
        self.scene.store(scene.as_u32(), Ordering::Relaxed);
    }

    pub fn scene(&self) -> MusicScene {
        MusicScene::from_u32(self.scene.load(Ordering::Relaxed))
    }

    pub fn set_biome(&self, index: usize) {
        self.biome_index
            .store(index.min(u32::MAX as usize - 1) as u32, Ordering::Relaxed);
    }

    pub fn biome(&self) -> Option<usize> {
        let index = self.biome_index.load(Ordering::Relaxed);
        (index != u32::MAX).then_some(index as usize)
    }

    pub fn set_day(&self, day: f32) {
        Self::set_f32(&self.day, day.clamp(0.0, 1.0));
    }

    pub fn day(&self) -> f32 {
        Self::get_f32(&self.day)
    }

    pub fn set_intensity(&self, intensity: f32) {
        Self::set_f32(&self.intensity, intensity.clamp(0.0, 1.0));
    }

    pub fn intensity(&self) -> f32 {
        Self::get_f32(&self.intensity)
    }

    pub fn set_danger(&self, danger: f32) {
        Self::set_f32(&self.danger, danger.clamp(0.0, 1.0));
    }

    pub fn danger(&self) -> f32 {
        Self::get_f32(&self.danger)
    }

    pub fn set_cave(&self, cave: bool) {
        self.cave.store(cave as u32, Ordering::Relaxed);
    }

    pub fn cave(&self) -> bool {
        self.cave.load(Ordering::Relaxed) != 0
    }

    pub fn set_warping(&self, warping: bool) {
        self.warping.store(warping as u32, Ordering::Relaxed);
    }

    pub fn warping(&self) -> bool {
        self.warping.load(Ordering::Relaxed) != 0
    }

    pub fn set_seed(&self, seed: u32) {
        if self.seed.swap(seed, Ordering::Relaxed) != seed {
            self.revision.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn seed(&self) -> u32 {
        self.seed.load(Ordering::Relaxed)
    }

    pub fn revision(&self) -> u32 {
        self.revision.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: f32) {
        Self::set_f32(&self.volume, volume.clamp(0.0, 1.0));
    }

    pub fn volume(&self) -> f32 {
        Self::get_f32(&self.volume)
    }

    pub fn set_master(&self, master: f32) {
        Self::set_f32(&self.master, master.clamp(0.0, 1.0));
    }

    pub fn master(&self) -> f32 {
        Self::get_f32(&self.master)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused as u32, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed) != 0
    }

    pub fn trigger_stinger(&self, stinger: Stinger) {
        self.stinger.store(stinger as u32, Ordering::Relaxed);
    }

    pub fn take_stinger(&self) -> Stinger {
        Stinger::from_u32(self.stinger.swap(0, Ordering::Relaxed))
    }
}

// ---------- Scene palettes ----------

#[derive(Clone, Copy, Debug)]
struct ScenePalette {
    scene: MusicScene,
    mood: &'static str,
    drums: DrumStyle,
    bpm: f32,
    pad_level: f32,
    bass_level: f32,
    arp_level: f32,
    lead_level: f32,
    drum_level: f32,
    atmos_level: f32,
    brightness: f32,
    reverb_mix: f32,
    delay_mix: f32,
    arp_division: u32,
    lead_chance: f32,
    swing: f32,
}

fn palette_for(scene: MusicScene, _cave: bool, day: f32, context: &str) -> ScenePalette {
    let night = day < 0.3;
    match scene {
        MusicScene::Menu => ScenePalette {
            scene,
            mood: "warm",
            drums: DrumStyle::None,
            bpm: 72.0,
            pad_level: 0.5,
            bass_level: 0.32,
            arp_level: 0.3,
            lead_level: 0.26,
            drum_level: 0.0,
            atmos_level: 0.16,
            brightness: 1.0,
            reverb_mix: 0.34,
            delay_mix: 0.22,
            arp_division: 2,
            lead_chance: 0.35,
            swing: 0.0,
        },
        MusicScene::Planet => {
            let (mood, bpm, drums) = planet_mood(context);
            ScenePalette {
                scene,
                mood,
                drums,
                bpm: if night { bpm * 0.88 } else { bpm },
                pad_level: if night { 0.55 } else { 0.46 },
                bass_level: if night { 0.36 } else { 0.4 },
                arp_level: if night { 0.2 } else { 0.34 },
                lead_level: if night { 0.24 } else { 0.3 },
                drum_level: if night { 0.35 } else { 0.55 },
                atmos_level: 0.14,
                brightness: if night { 0.62 } else { 1.0 },
                reverb_mix: if night { 0.38 } else { 0.26 },
                delay_mix: 0.24,
                arp_division: if night { 4 } else { 2 },
                lead_chance: if night { 0.25 } else { 0.4 },
                swing: 0.06,
            }
        }
        MusicScene::Cave => ScenePalette {
            scene,
            mood: "mysterious",
            drums: DrumStyle::None,
            bpm: 58.0,
            pad_level: 0.6,
            bass_level: 0.42,
            arp_level: 0.16,
            lead_level: 0.22,
            drum_level: 0.0,
            atmos_level: 0.3,
            brightness: 0.4,
            reverb_mix: 0.5,
            delay_mix: 0.35,
            arp_division: 8,
            lead_chance: 0.3,
            swing: 0.0,
        },
        MusicScene::Space => ScenePalette {
            scene,
            mood: "ethereal",
            drums: DrumStyle::Space,
            bpm: 64.0,
            pad_level: 0.6,
            bass_level: 0.34,
            arp_level: 0.3,
            lead_level: 0.3,
            drum_level: 0.24,
            atmos_level: 0.22,
            brightness: 0.85,
            reverb_mix: 0.46,
            delay_mix: 0.34,
            arp_division: 2,
            lead_chance: 0.35,
            swing: 0.0,
        },
        MusicScene::Station => ScenePalette {
            scene,
            mood: "warm",
            drums: DrumStyle::Lounge,
            bpm: 84.0,
            pad_level: 0.38,
            bass_level: 0.38,
            arp_level: 0.34,
            lead_level: 0.26,
            drum_level: 0.5,
            atmos_level: 0.1,
            brightness: 0.95,
            reverb_mix: 0.22,
            delay_mix: 0.2,
            arp_division: 2,
            lead_chance: 0.45,
            swing: 0.12,
        },
        MusicScene::Warp => ScenePalette {
            scene,
            mood: "driving",
            drums: DrumStyle::Warp,
            bpm: 128.0,
            pad_level: 0.3,
            bass_level: 0.5,
            arp_level: 0.5,
            lead_level: 0.4,
            drum_level: 0.7,
            atmos_level: 0.3,
            brightness: 1.15,
            reverb_mix: 0.3,
            delay_mix: 0.4,
            arp_division: 1,
            lead_chance: 0.5,
            swing: 0.0,
        },
    }
}

fn planet_mood(context: &str) -> (&'static str, f32, DrumStyle) {
    match context {
        "lush" => ("hopeful", 92.0, DrumStyle::Soft),
        "desert" => ("mysterious", 80.0, DrumStyle::Tribal),
        "frozen" => ("somber", 72.0, DrumStyle::Space),
        "volcanic" | "obsidian" => ("tense", 96.0, DrumStyle::Tribal),
        "alien" => ("mysterious", 88.0, DrumStyle::Lounge),
        "crystal" => ("ethereal", 76.0, DrumStyle::Soft),
        "fungal" | "murk" => ("calm", 78.0, DrumStyle::Lounge),
        "ocean" => ("calm", 74.0, DrumStyle::Soft),
        "ashen" => ("somber", 70.0, DrumStyle::Space),
        "amber" | "hive" => ("hopeful", 90.0, DrumStyle::Tribal),
        "ferrous" => ("tense", 100.0, DrumStyle::Tribal),
        "salt" => ("calm", 82.0, DrumStyle::Lounge),
        "redmoss" => ("driving", 94.0, DrumStyle::Soft),
        _ => ("warm", 86.0, DrumStyle::Soft),
    }
}

fn context_for(scene: MusicScene, biome_key: &str) -> &'static str {
    match scene {
        MusicScene::Menu => "menu",
        MusicScene::Cave => "cave",
        MusicScene::Space => "space",
        MusicScene::Station => "station",
        MusicScene::Warp => "warp",
        MusicScene::Planet => match biome_key {
            "lush" => "lush",
            "desert" => "desert",
            "frozen" => "frozen",
            "volcanic" => "volcanic",
            "alien" => "alien",
            "crystal" => "crystal",
            "fungal" => "fungal",
            "ocean" => "ocean",
            "ashen" => "ashen",
            "amber" => "amber",
            "ferrous" => "ferrous",
            "salt" => "salt",
            "obsidian" => "obsidian",
            "redmoss" => "redmoss",
            "hive" => "hive",
            "murk" => "murk",
            _ => "warm",
        },
    }
}

// ---------- Voices ----------

struct PadVoice {
    phase_a: f32,
    phase_b: f32,
    detune: f32,
    freq: Smoothed,
    level: f32,
    pan: f32,
    filter: Svf,
}

impl PadVoice {
    fn new(sample_rate: f32, detune: f32, pan: f32) -> Self {
        Self {
            phase_a: 0.0,
            phase_b: 0.37,
            detune,
            freq: Smoothed::new(110.0, 90.0),
            level: 0.16,
            pan,
            filter: Svf::new(sample_rate, 800.0, 0.8),
        }
    }

    fn set_note(&mut self, midi: i32) {
        self.freq.set(midi_hz(midi as f32));
    }

    #[inline]
    fn process(&mut self, dt: f32, brightness: f32, lfo: f32, gain: f32) -> (f32, f32) {
        let freq = self.freq.next().max(20.0);
        let step_a = freq * dt;
        self.phase_a = (self.phase_a + step_a).fract();
        self.phase_b = (self.phase_b + step_a * self.detune).fract();
        let raw = (Osc::saw_blep(self.phase_a, step_a)
            + Osc::saw_blep(self.phase_b, step_a * self.detune))
            * 0.5;
        let cutoff = (freq * brightness * (1.0 + lfo * 0.3)).clamp(60.0, 9_000.0);
        self.filter.set(cutoff, 0.75);
        let (low, _, _) = self.filter.process(raw);
        let value = low * self.level * gain;
        let left = value * (1.0 - self.pan) * 0.5;
        let right = value * (1.0 + self.pan) * 0.5;
        (left, right)
    }
}

struct BassVoice {
    phase: f32,
    sub_phase: f32,
    freq: Smoothed,
    envelope: Adsr,
    level: f32,
    filter: OnePole,
    sample_rate: f32,
}

impl BassVoice {
    fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            sub_phase: 0.25,
            freq: Smoothed::new(55.0, 60.0),
            envelope: Adsr::new(0.012, 0.25, 0.55, 0.18),
            level: 0.42,
            filter: OnePole::new(sample_rate, 420.0),
            sample_rate,
        }
    }

    fn trigger(&mut self, midi: i32, velocity: f32, cutoff: f32) {
        self.freq.set(midi_hz(midi as f32));
        self.level = 0.3 + velocity * 0.3;
        self.filter.set(self.sample_rate, cutoff);
        self.envelope.gate_on();
    }

    fn release(&mut self) {
        self.envelope.gate_off();
    }

    #[inline]
    fn process(&mut self, dt: f32, sidechain: f32) -> f32 {
        let freq = self.freq.next().max(20.0);
        let step = freq * dt;
        self.phase = (self.phase + step).fract();
        self.sub_phase = (self.sub_phase + step * 0.5).fract();
        let raw = Osc::saw_blep(self.phase, step) * 0.6 + Osc::sine(self.sub_phase) * 0.7;
        let tone = self.filter.process(raw);
        let env = self.envelope.next(dt);
        let duck = 1.0 - sidechain * 0.55;
        tone * self.level * env * duck
    }
}

struct LeadVoice {
    carrier: f32,
    modulator: f32,
    ratio: f32,
    index: f32,
    freq: Smoothed,
    envelope: Adsr,
    level: f32,
}

impl LeadVoice {
    fn new() -> Self {
        Self {
            carrier: 0.0,
            modulator: 0.0,
            ratio: 2.0,
            index: 1.4,
            freq: Smoothed::new(440.0, 70.0),
            envelope: Adsr::new(0.008, 0.35, 0.12, 0.5),
            level: 0.2,
        }
    }

    fn trigger(&mut self, midi: i32, velocity: f32, ratio: f32, index: f32) {
        self.freq.set(midi_hz(midi as f32));
        self.ratio = ratio;
        self.index = index;
        self.level = 0.12 + velocity * 0.16;
        self.envelope = Adsr::new(0.006, 0.5, 0.0, 0.55);
        self.envelope.gate_on();
    }

    #[inline]
    fn process(&mut self, dt: f32) -> f32 {
        let freq = self.freq.next().max(30.0);
        let step = freq * dt;
        self.carrier = (self.carrier + step).fract();
        self.modulator = (self.modulator + step * self.ratio).fract();
        let env = self.envelope.next(dt);
        let mod_signal = Osc::sine(self.modulator) * self.index;
        Osc::fm(self.carrier, mod_signal, 1.0) * self.level * env
    }
}

struct Atmosphere {
    noise: Noise,
    filter: Biquad,
    level: f32,
    phase: f32,
}

impl Atmosphere {
    fn new(sample_rate: f32, seed: u32) -> Self {
        Self {
            noise: Noise::new(seed ^ 0xA7A5_5021),
            filter: Biquad::low_pass(sample_rate, 600.0, 0.6),
            level: 0.0,
            phase: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, dt: f32, brightness: f32, level: f32) -> (f32, f32) {
        self.phase = (self.phase + dt * 0.05).fract();
        let sweep = 0.5 + 0.5 * (self.phase * std::f32::consts::TAU).sin();
        let cutoff = (180.0 + brightness * 1_800.0 * (0.4 + sweep * 0.6)).clamp(60.0, 8_000.0);
        self.filter.set(FilterKind::LowPass, cutoff, 0.7, 0.0);
        let sample = self.filter.process(self.noise.pink()) * level * 0.5;
        // Decorrelated stereo for a wide bed.
        (sample, sample * 0.92 + self.noise.white() * level * 0.06)
    }
}

struct StingerVoice {
    carrier: f32,
    envelope: Adsr,
    freq: Smoothed,
    index: f32,
    level: f32,
    active: bool,
}

impl StingerVoice {
    fn new() -> Self {
        Self {
            carrier: 0.0,
            envelope: Adsr::new(0.005, 0.6, 0.0, 0.9),
            freq: Smoothed::new(880.0, 45.0),
            index: 2.0,
            level: 0.0,
            active: false,
        }
    }

    fn trigger(&mut self, midi: i32, velocity: f32, index: f32) {
        self.freq.set(midi_hz(midi as f32));
        self.envelope = Adsr::new(0.004, 1.2, 0.0, 1.1);
        self.envelope.gate_on();
        self.index = index;
        self.level = velocity * 0.22;
        self.active = true;
    }

    #[inline]
    fn process(&mut self, dt: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let freq = self.freq.next();
        let step = freq * dt;
        self.carrier = (self.carrier + step).fract();
        let env = self.envelope.next(dt);
        if !self.envelope.is_active() {
            self.active = false;
        }
        Osc::sine(self.carrier + Osc::sine(self.carrier * 2.0) * self.index * 0.1)
            * self.level
            * env
    }
}

// ---------- Arrangement layer ----------

const MAX_CHORD_NOTES: usize = 8;

struct NoteBuf {
    notes: [i32; MAX_CHORD_NOTES],
    len: usize,
}

impl NoteBuf {
    fn new() -> Self {
        Self {
            notes: [0; MAX_CHORD_NOTES],
            len: 0,
        }
    }

    fn as_slice(&self) -> &[i32] {
        &self.notes[..self.len]
    }

    fn push(&mut self, note: i32) {
        if self.len < MAX_CHORD_NOTES {
            self.notes[self.len] = note;
            self.len += 1;
        }
    }
}

struct Layer {
    palette: ScenePalette,
    key: Key,
    progression: [Chord; 8],
    progression_len: usize,
    chord_index: usize,
    pad: [PadVoice; 4],
    bass: BassVoice,
    pluck: [Pluck; 2],
    lead: LeadVoice,
    stinger: StingerVoice,
    stinger_queue: [Option<(i32, f32, f32)>; 4],
    drums: DrumKit,
    atmosphere: Atmosphere,
    arp_shape: ArpShape,
    arp_pool: NoteBuf,
    melody: [(usize, i32); 12],
    melody_len: usize,
    melody_index: usize,
    voicing: NoteBuf,
    last_voicing: NoteBuf,
    step: u32,
    bar: u32,
    sidechain: f32,
    gain: f32,
    active: bool,
    seed: u32,
    rng: u32,
    sample_rate: f32,
    drum_pattern: DrumPattern,
    lead_gate: bool,
}

impl Layer {
    fn new(sample_rate: f32, seed: u32) -> Self {
        Self {
            palette: palette_for(MusicScene::Menu, false, 0.5, "menu"),
            key: Key::new(45, Mode::Aeolian),
            progression: [Chord::new(0, ChordQuality::Minor7); 8],
            progression_len: 0,
            chord_index: 0,
            pad: [
                PadVoice::new(sample_rate, 1.004, -0.5),
                PadVoice::new(sample_rate, 0.996, 0.5),
                PadVoice::new(sample_rate, 1.008, -0.2),
                PadVoice::new(sample_rate, 0.992, 0.25),
            ],
            bass: BassVoice::new(sample_rate),
            pluck: [Pluck::new(sample_rate, 55.0), Pluck::new(sample_rate, 55.0)],
            lead: LeadVoice::new(),
            stinger: StingerVoice::new(),
            stinger_queue: [None; 4],
            drums: DrumKit::new(sample_rate, seed),
            atmosphere: Atmosphere::new(sample_rate, seed),
            arp_shape: ArpShape::Up,
            arp_pool: NoteBuf::new(),
            melody: [(0, 0); 12],
            melody_len: 0,
            melody_index: 0,
            voicing: NoteBuf::new(),
            last_voicing: NoteBuf::new(),
            step: 0,
            bar: 0,
            sidechain: 0.0,
            gain: 0.0,
            active: false,
            seed,
            rng: seed.max(1),
            sample_rate,
            drum_pattern: DrumPattern::empty(),
            lead_gate: false,
        }
    }

    #[inline]
    fn chance(&mut self, probability: f32) -> bool {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.rng >> 8) as f32 / (1 << 24) as f32) < probability
    }

    /// Re-seed the layer for a new palette & musical context.
    fn configure(&mut self, palette: ScenePalette, context: &str, seed: u32, day: f32) {
        self.palette = palette;
        self.seed = seed;
        let key_seed = seed
            ^ (context
                .bytes()
                .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32)));
        self.key = key_for_context(context, key_seed);
        let chords = choose_progression(palette.mood, key_seed);
        self.progression_len = chords.len().min(8);
        for (slot, chord) in self.progression.iter_mut().zip(chords.iter()) {
            *slot = *chord;
        }
        // Longer progressions for the calmer scenes.
        if palette.scene == MusicScene::Menu || palette.scene == MusicScene::Space {
            let extended = extend_progression(&chords, key_seed);
            self.progression_len = extended.len().min(8);
            for (slot, chord) in self.progression.iter_mut().zip(extended.iter()) {
                *slot = *chord;
            }
        }
        self.chord_index = 0;
        self.arp_shape = ArpShape::from_seed(key_seed ^ 0x9911);
        self.step = 0;
        self.bar = 0;
        self.melody_len = 0;
        self.melody_index = 0;
        self.sidechain = 0.0;
        self.rng = key_seed.max(1);
        self.stinger_queue = [None; 4];
        self.drums = DrumKit::new(self.sample_rate, key_seed ^ 0xD00D);
        self.drum_pattern =
            DrumPattern::generate(palette.drums, 0, 0.4, key_seed ^ (day * 1000.0) as u32);
        // Start the harmony immediately so the fade-in has material.
        self.update_voicing(0.0);
        self.rebuild_arp_pool();
        self.rebuild_melody(key_seed);
        self.lead_gate = true;
        self.active = true;
    }

    fn current_chord(&self) -> Chord {
        if self.progression_len == 0 {
            return Chord::new(0, ChordQuality::Minor7);
        }
        self.progression[self.chord_index % self.progression_len]
    }

    /// Rebuild pad/bass voicings for the current chord with smooth voice
    /// leading into `last_voicing`.
    fn update_voicing(&mut self, intensity: f32) {
        let chord = self.current_chord();
        let root = chord.root_midi(&self.key);
        let low = 62 + (self.key.root % 5);
        let mut target = NoteBuf::new();
        // Root doubled low, then upper chord tones spread upward.
        target.push(root + 12);
        let intervals = chord.quality.intervals();
        for interval in intervals.iter().skip(1) {
            let mut note = root + interval;
            while note < low {
                note += 12;
            }
            if note < low + 30 {
                target.push(note);
            }
        }
        if intensity > 0.55 {
            // Combat tension: a cluster a semitone above the ninth.
            target.push(root + 25);
        }
        // Voice leading against the previous voicing.
        if self.last_voicing.len > 0 {
            let center =
                self.last_voicing.as_slice().iter().sum::<i32>() / self.last_voicing.len as i32;
            for i in 0..target.len {
                let mut note = target.notes[i];
                while note - center > 7 {
                    note -= 12;
                }
                while center - note > 7 {
                    note += 12;
                }
                target.notes[i] = note.clamp(45, 96);
            }
        }
        target.notes[..target.len].sort_unstable();
        self.voicing = target;
        // Assign the four pad voices cyclically (upper voices doubled by
        // detuned pairs already inside each voice).
        for (index, voice) in self.pad.iter_mut().enumerate() {
            let note = if self.voicing.len == 0 {
                60
            } else {
                self.voicing.notes[index % self.voicing.len]
            };
            voice.set_note(note);
        }
        // Bass follows the chord root, dropping an octave when intense.
        let bass_note = if intensity > 0.6 { root } else { root + 12 };
        self.bass
            .trigger(bass_note, 0.6 + intensity * 0.3, 300.0 + intensity * 900.0);
        self.last_voicing = NoteBuf {
            notes: self.voicing.notes,
            len: self.voicing.len,
        };
    }

    fn rebuild_arp_pool(&mut self) {
        self.arp_pool = NoteBuf::new();
        let chord = self.current_chord();
        for note in self.voicing.as_slice() {
            self.arp_pool.push(note + 12);
        }
        if self.arp_pool.len == 0 {
            for interval in chord.quality.intervals() {
                self.arp_pool
                    .push(chord.root_midi(&self.key) + 24 + interval);
            }
        }
    }

    fn rebuild_melody(&mut self, seed: u32) {
        let chord = self.current_chord();
        let phrase = melody_phrase(
            &self.key,
            &chord,
            1,
            seed ^ self.bar.wrapping_mul(2654435761),
        );
        self.melody_len = phrase.len().min(self.melody.len());
        for (slot, (step, note)) in self.melody.iter_mut().zip(phrase.iter()) {
            *slot = (*step % 16, *note);
        }
        self.melody_index = 0;
    }

    fn advance_bar(&mut self, intensity: f32, seed: u32) {
        self.bar = self.bar.wrapping_add(1);
        self.drum_pattern = DrumPattern::generate(
            self.palette.drums,
            self.bar as usize,
            intensity,
            seed ^ self.bar.wrapping_mul(0x9E37_79B9),
        );
    }

    fn advance_chord(&mut self, intensity: f32, seed: u32) {
        self.chord_index = (self.chord_index + 1) % self.progression_len.max(1);
        self.update_voicing(intensity);
        self.rebuild_arp_pool();
        self.rebuild_melody(seed);
    }

    /// Process one 16th-note step worth of triggers.
    fn step_trigger(&mut self, step: u32, intensity: f32, danger: f32, danger_pulse: bool) {
        let in_bar = step % 16;
        // Bass rhythm: root on the beat, extra eighth notes when driving.
        if in_bar.is_multiple_of(4) {
            let chord = self.current_chord();
            let root = chord.root_midi(&self.key) + if intensity > 0.6 { 0 } else { 12 };
            self.bass
                .trigger(root, 0.5 + intensity * 0.4, 260.0 + intensity * 1000.0);
        } else if intensity > 0.5 && in_bar.is_multiple_of(2) {
            let chord = self.current_chord();
            let root = chord.root_midi(&self.key) + 12;
            self.bass
                .trigger(root + 7, 0.35 + intensity * 0.2, 400.0 + intensity * 800.0);
        }

        // Arpeggio: two alternating string voices for a natural plucked feel.
        let division = self.palette.arp_division.max(1);
        if step.is_multiple_of(division) {
            let octave_span = if intensity > 0.6 { 2 } else { 1 };
            if let Some(note) = arp_note(
                self.arp_shape,
                self.arp_pool.as_slice(),
                step as usize,
                octave_span,
                self.seed.wrapping_add(step.wrapping_mul(0x1234_5677)),
            ) {
                let velocity = 0.5 + intensity * 0.4;
                let slot = (step as usize / division as usize) % 2;
                self.pluck[slot].pluck(self.sample_rate, midi_hz(note as f32), 0.55, velocity);
            }
        }

        // Lead melody (sparse, gated per bar for breathing room).
        while self.melody_index < self.melody_len
            && self.melody[self.melody_index].0 <= in_bar as usize
        {
            let (step_pos, note) = self.melody[self.melody_index];
            let phrase_top = self.melody_index.is_multiple_of(4);
            if step_pos == in_bar as usize
                && self.lead_gate
                && (phrase_top || self.chance(self.palette.lead_chance))
            {
                self.lead.trigger(note, 0.6, 2.0, 1.2 + intensity);
            }
            self.melody_index += 1;
        }

        // Drums.
        let drum_step = self.drum_pattern.steps[(in_bar % 16) as usize];
        let drum_gain = self.palette.drum_level * (0.55 + intensity * 0.75);
        if drum_step.kick > 0.05 {
            self.drums
                .trigger(DrumKind::Kick, drum_step.kick * drum_gain);
            self.sidechain = 1.0;
        }
        if drum_step.snare > 0.05 {
            self.drums
                .trigger(DrumKind::Snare, drum_step.snare * drum_gain);
        }
        if drum_step.hat > 0.05 {
            self.drums
                .trigger(DrumKind::HatClosed, drum_step.hat * drum_gain);
        }
        if drum_step.open_hat > 0.05 {
            self.drums
                .trigger(DrumKind::HatOpen, drum_step.open_hat * drum_gain);
        }
        if drum_step.clap > 0.05 {
            self.drums
                .trigger(DrumKind::Clap, drum_step.clap * drum_gain);
        }
        if drum_step.perc > 0.05 {
            self.drums
                .trigger(DrumKind::Rim, drum_step.perc * drum_gain);
        }
        if drum_step.tom > 0.05 {
            self.drums.trigger(DrumKind::Tom, drum_step.tom * drum_gain);
        }
        // Danger heartbeat on the offbeats.
        if danger_pulse && danger > 0.25 {
            self.drums.trigger(DrumKind::Kick, 0.5 * danger);
        }
    }

    #[inline]
    fn process(&mut self, dt: f32, intensity: f32) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }
        let brightness = self.palette.brightness * (0.7 + intensity * 0.6);
        let lfo = (self.step as f32 * 0.03).sin();
        let mut left = 0.0;
        let mut right = 0.0;
        let pad_gain = self.palette.pad_level * (1.0 - self.sidechain * 0.25);
        for voice in &mut self.pad {
            let (l, r) = voice.process(dt, brightness, lfo, pad_gain);
            left += l;
            right += r;
        }
        // Bass is a centered voice: render it once and feed both channels.
        let bass = self.bass.process(dt, self.sidechain) * self.palette.bass_level * 2.0;
        left += bass;
        right += bass;

        let pluck_l = self.pluck[0].next(self.sample_rate);
        let pluck_r = self.pluck[1].next(self.sample_rate);
        left += pluck_l * self.palette.arp_level * 0.9;
        right += pluck_r * self.palette.arp_level * 0.9;

        let lead = self.lead.process(dt);
        left += lead * self.palette.lead_level;
        right += lead * self.palette.lead_level;

        // Stinger notes are scheduled a few hundred ms apart.
        for slot in &mut self.stinger_queue {
            if let Some((note, velocity, delay)) = slot.as_mut() {
                *delay -= dt;
                if *delay <= 0.0 {
                    let (note, velocity) = (*note, *velocity);
                    *slot = None;
                    self.stinger.trigger(note, velocity, 1.4);
                    break;
                }
            }
        }
        let stinger = self.stinger.process(dt);
        left += stinger;
        right += stinger;

        let (body, top) = self.drums.next(self.sample_rate);
        left += body * self.palette.drum_level * 0.55;
        right += body * self.palette.drum_level * 0.55;
        left += top * self.palette.drum_level * 0.42;
        right += top * self.palette.drum_level * 0.36;

        let (al, ar) = self
            .atmosphere
            .process(dt, brightness, self.palette.atmos_level);
        left += al;
        right += ar;

        self.sidechain = (self.sidechain - dt / 0.16).max(0.0);
        (left * self.gain, right * self.gain)
    }

    fn trigger_stinger(&mut self, stinger: Stinger) {
        let base = self.key.degree(7);
        let tonic = self.key.degree(0);
        let mut notes = [(0i32, 0.0f32); 4];
        let count = match stinger {
            Stinger::None => 0,
            Stinger::Discovery => {
                notes[0] = (base + 4, 0.8);
                notes[1] = (base + 7, 0.6);
                2
            }
            Stinger::QuestComplete => {
                notes[0] = (base, 0.9);
                notes[1] = (base + 7, 0.7);
                notes[2] = (base + 12, 0.5);
                3
            }
            Stinger::Alert => {
                notes[0] = (tonic + 1, 0.9);
                1
            }
            Stinger::Scan => {
                notes[0] = (base + 12, 0.5);
                notes[1] = (base + 19, 0.4);
                2
            }
            Stinger::WarpArrival => {
                notes[0] = (base, 0.9);
                notes[1] = (base + 5, 0.6);
                2
            }
            Stinger::Milestone => {
                notes[0] = (base + 4, 0.8);
                notes[1] = (base + 9, 0.6);
                notes[2] = (base + 12, 0.4);
                3
            }
        };
        for (index, (note, velocity)) in notes.iter().take(count).enumerate() {
            self.stinger_queue[index] = Some((*note, *velocity, index as f32 * 0.14));
        }
    }
}

// ---------- Engine ----------

pub struct MusicEngine {
    shared: Arc<MusicShared>,
    sample_rate: f32,
    sample_counter: u32,
    // Two arrangement layers: `layers[0]` is the target, `layers[1]` fades out.
    layers: [Layer; 2],
    fade: f32,
    fade_time: f32,
    last_revision: u32,
    last_scene: MusicScene,
    last_biome: Option<usize>,
    // Engine-wide processing.
    delay: [Delay; 2],
    delay_feedback: f32,
    last_delay: [f32; 2],
    reverb: Reverb,
    chorus: Chorus,
    compressor: Compressor,
    limiter: Limiter,
    dc_l: DcBlocker,
    dc_r: DcBlocker,
    volume: Smoothed,
    intensity: Smoothed,
    danger: Smoothed,
    danger_beat: f32,
    warp_amount: f32,
    noise: Noise,
    frame: [f32; 2],
    channel: usize,
    control_accum: u32,
}

const CONTROL_INTERVAL: u32 = 32;

impl MusicEngine {
    pub fn new(shared: Arc<MusicShared>, sample_rate: f32) -> Self {
        let seed = shared.seed();
        let mut engine = Self {
            shared,
            sample_rate,
            sample_counter: 0,
            layers: [
                Layer::new(sample_rate, seed),
                Layer::new(sample_rate, seed ^ 0xDEAD),
            ],
            fade: 1.0,
            fade_time: 6.0,
            last_revision: u32::MAX,
            last_scene: MusicScene::Menu,
            last_biome: None,
            delay: [Delay::new(sample_rate, 2.0), Delay::new(sample_rate, 2.0)],
            delay_feedback: 0.32,
            last_delay: [0.0; 2],
            reverb: Reverb::new(sample_rate),
            chorus: Chorus::new(sample_rate),
            compressor: Compressor::new(sample_rate, -14.0, 3.0),
            limiter: Limiter::new(0.92),
            dc_l: DcBlocker::default(),
            dc_r: DcBlocker::default(),
            volume: Smoothed::new(0.0, 400.0),
            intensity: Smoothed::new(0.0, 900.0),
            danger: Smoothed::new(0.0, 700.0),
            danger_beat: 0.0,
            warp_amount: 0.0,
            noise: Noise::new(seed ^ 0x5EED_1234),
            frame: [0.0; 2],
            channel: 0,
            control_accum: 0,
        };
        engine.layers[0].configure(
            palette_for(MusicScene::Menu, false, 0.5, "menu"),
            "menu",
            seed,
            0.5,
        );
        engine.layers[0].gain = 1.0;
        engine.layers[1].active = false;
        engine.last_scene = MusicScene::Menu;
        engine.last_biome = engine.shared.biome();
        engine.last_revision = engine.shared.revision();
        engine
    }

    fn control_tick(&mut self) {
        let scene = self.shared.scene();
        let biome = self.shared.biome();
        let revision = self.shared.revision();
        let day = self.shared.day();
        // Only start a new crossfade once the previous one has landed, which
        // prevents rapid planet switching from stacking hard stops.
        if (scene != self.last_scene || biome != self.last_biome || revision != self.last_revision)
            && self.fade >= 0.999
        {
            self.last_scene = scene;
            self.last_biome = biome;
            self.last_revision = revision;
            let biome_key = biome
                .and_then(|index| crate::data::BIOMES.get(index))
                .map(|biome| biome.key)
                .unwrap_or("lush");
            let context = context_for(scene, biome_key);
            let cave = self.shared.cave();
            let seed = self.shared.seed();
            let palette = palette_for(scene, cave, day, context);
            // Swap: current layer becomes the fading previous one.
            self.layers.swap(0, 1);
            self.layers[0].configure(palette, context, seed, day);
            self.layers[0].gain = 0.0;
            self.fade = 0.0;
            self.fade_time = match scene {
                MusicScene::Warp => 1.2,
                MusicScene::Menu => 3.0,
                _ => 6.5,
            };
            // Delay time follows the new tempo (dotted eighth).
            let beat = 60.0 / palette.bpm;
            self.delay[0].set_time(beat * 0.75);
            self.delay[1].set_time(beat * 0.5);
            self.last_delay = [0.0; 2];
        }
        let stinger = self.shared.take_stinger();
        if stinger != Stinger::None {
            self.layers[0].trigger_stinger(stinger);
        }
        self.intensity.set(self.shared.intensity());
        self.danger.set(self.shared.danger());
        if self.shared.paused() {
            // Pausing must be instant (menus are silent immediately).
            self.volume.jump(0.0);
        } else {
            self.volume.set(self.shared.volume() * self.shared.master());
        }
    }

    #[inline]
    fn render_frame(&mut self) -> [f32; 2] {
        let dt = 1.0 / self.sample_rate;
        if self.control_accum == 0 {
            self.control_tick();
        }
        self.control_accum = (self.control_accum + 1) % CONTROL_INTERVAL;
        let intensity = self.intensity.next().clamp(0.0, 1.0);
        let danger = self.danger.next().clamp(0.0, 1.0);
        let warping = self.shared.warping();
        self.warp_amount =
            (self.warp_amount + if warping { dt * 0.8 } else { -dt * 0.5 }).clamp(0.0, 1.0);

        // Advance arrangement steps on a swung 16th-note grid.
        let palette = self.layers[0].palette;
        let samples_per_step = self.sample_rate * (60.0 / palette.bpm) / 4.0;
        let phase = (self.sample_counter as f32 % samples_per_step) / samples_per_step;
        let odd_step = self.layers[0].step % 2 == 1;
        let trigger_at = if odd_step { palette.swing } else { 0.0 };
        let delta = 1.0 / samples_per_step;
        let step_now = self.layers[0].active && phase >= trigger_at && phase < trigger_at + delta;
        let bar_boundary = step_now && self.layers[0].step.is_multiple_of(16);
        let chord_boundary = bar_boundary && self.layers[0].step.is_multiple_of(16 * 4);

        // Danger heartbeat pulse at ~72 bpm, only when danger is meaningful.
        let beat_period = 60.0 / 72.0;
        self.danger_beat = (self.danger_beat + dt) % beat_period;
        let danger_pulse = danger > 0.25 && self.danger_beat < dt;

        if step_now {
            let layer = &mut self.layers[0];
            let step = layer.step;
            let seed = layer.seed;
            if bar_boundary {
                let bar = layer.bar;
                layer.advance_bar(intensity, seed ^ bar.wrapping_mul(0x85EB_CA6B));
            }
            if chord_boundary && step > 0 {
                layer.advance_chord(intensity, seed);
            }
            layer.step_trigger(step, intensity, danger, danger_pulse);
            layer.step = layer.step.wrapping_add(1);
        }

        // Crossfade equal-power between layers.
        self.fade = (self.fade + dt / self.fade_time).min(1.0);
        let angle = self.fade * std::f32::consts::FRAC_PI_2;
        self.layers[0].gain = angle.sin();
        self.layers[1].gain = angle.cos();
        if self.fade >= 1.0 {
            self.layers[1].active = false;
        }

        let mut left = 0.0;
        let mut right = 0.0;
        for index in 0..2 {
            if !self.layers[index].active {
                continue;
            }
            let (l, r) = self.layers[index].process(dt, intensity);
            left += l;
            right += r;
        }

        // Warp riser: swept noise + rising sine layered on top.
        if self.warp_amount > 0.001 {
            let sweep = self.warp_amount;
            let phase =
                (self.sample_counter as f32 * (40.0 + sweep * 800.0) / self.sample_rate).fract();
            let riser = Osc::sine(phase) * 0.12 * sweep;
            let noise = self.noise.white() * 0.05 * sweep;
            left += riser + noise;
            right += riser + noise * 0.9;
        }

        // Ping-pong delay.
        let send = palette.delay_mix * (0.6 + intensity * 0.4);
        let delayed_l =
            self.delay[0].process(left * send + self.last_delay[1] * self.delay_feedback);
        let delayed_r =
            self.delay[1].process(right * send + self.last_delay[0] * self.delay_feedback);
        self.last_delay = [delayed_l, delayed_r];
        left += delayed_l * 0.8;
        right += delayed_r * 0.8;

        // Chorus widens pads; disabled in warp/combat for tightness.
        let chorus_mix = if warping { 0.0 } else { 0.22 };
        self.chorus.mix = chorus_mix;
        left = self.chorus.process(left, dt);
        right = self.chorus.process(right, dt);

        // Reverb send.
        self.reverb.mix = palette.reverb_mix;
        let mono = (left + right) * 0.5;
        self.reverb.mix_in(mono, &mut [left, right]);

        // Dynamics.
        left = self.compressor.process(left);
        right = self.compressor.process(right);
        left = self.dc_l.process(left);
        right = self.dc_r.process(right);
        let gain = self.volume.next();
        left = self.limiter.process(left * gain);
        right = self.limiter.process(right * gain);

        self.sample_counter = self.sample_counter.wrapping_add(1);
        [left, right]
    }

    fn next_sample(&mut self) -> f32 {
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

/// Iterator/Source adapter handed to rodio through the `Decodable` trait.
pub struct MusicDecoder {
    engine: MusicEngine,
}

impl MusicDecoder {
    pub fn new(shared: Arc<MusicShared>, sample_rate: f32) -> Self {
        Self {
            engine: MusicEngine::new(shared, sample_rate),
        }
    }
}

impl Iterator for MusicDecoder {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        Some(self.engine.next_sample())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, None)
    }
}

impl rodio::Source for MusicDecoder {
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

    fn render(shared: &Arc<MusicShared>, samples: usize) -> Vec<f32> {
        let mut decoder = MusicDecoder::new(shared.clone(), 44_100.0);
        (0..samples).map(|_| decoder.next().unwrap()).collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |a, b| a.max(b.abs()))
    }

    #[test]
    fn engine_renders_finite_audio_for_every_scene() {
        let scenes = [
            MusicScene::Menu,
            MusicScene::Planet,
            MusicScene::Cave,
            MusicScene::Space,
            MusicScene::Station,
            MusicScene::Warp,
        ];
        for scene in scenes {
            let shared = MusicShared::new();
            shared.set_seed(42);
            shared.set_scene(scene);
            shared.set_volume(0.7);
            shared.set_master(0.8);
            let samples = render(&shared, 44_100);
            assert_eq!(samples.len(), 44_100);
            assert!(samples.iter().all(|s| s.is_finite()), "{scene:?}");
            assert!(peak(&samples) > 0.0005, "{scene:?} was silent");
            assert!(
                peak(&samples) <= 1.0,
                "{scene:?} clipped: {}",
                peak(&samples)
            );
        }
    }

    #[test]
    fn engine_is_deterministic_for_a_fixed_seed() {
        let build = || {
            let shared = MusicShared::new();
            shared.set_seed(7);
            shared.set_scene(MusicScene::Planet);
            shared.set_biome(0);
            render(&shared, 20_000)
        };
        let a = build();
        let b = build();
        assert_eq!(a, b);
    }

    #[test]
    fn scene_change_crossfades_without_clicks() {
        let shared = MusicShared::new();
        shared.set_seed(3);
        shared.set_scene(MusicScene::Planet);
        shared.set_biome(0);
        let mut decoder = MusicDecoder::new(shared.clone(), 44_100.0);
        let mut previous = 0.0f32;
        let mut max_slope = 0.0f32;
        for i in 0..44_100 * 4 {
            if i == 44_100 {
                shared.set_scene(MusicScene::Space);
            }
            let sample = decoder.next().unwrap();
            assert!(sample.is_finite());
            max_slope = max_slope.max((sample - previous).abs());
            previous = sample;
        }
        // No sample-to-sample jump larger than a full-scale step, which would
        // indicate a hard cut or an unstable filter.
        assert!(max_slope < 0.9, "discontinuity of {max_slope}");
    }

    #[test]
    fn stingers_and_intensity_produce_audio() {
        let shared = MusicShared::new();
        shared.set_seed(11);
        shared.set_scene(MusicScene::Planet);
        shared.set_intensity(0.9);
        shared.set_danger(0.8);
        let mut decoder = MusicDecoder::new(shared.clone(), 44_100.0);
        for _ in 0..10_000 {
            let _ = decoder.next().unwrap();
        }
        shared.trigger_stinger(Stinger::QuestComplete);
        let samples: Vec<f32> = (0..44_100).map(|_| decoder.next().unwrap()).collect();
        assert!(samples.iter().all(|s| s.is_finite()));
        assert!(peak(&samples) > 0.001);
    }

    #[test]
    fn paused_engine_fades_to_silence() {
        let shared = MusicShared::new();
        shared.set_seed(5);
        shared.set_scene(MusicScene::Menu);
        let mut decoder = MusicDecoder::new(shared.clone(), 44_100.0);
        for _ in 0..44_100 {
            let _ = decoder.next();
        }
        shared.set_paused(true);
        for _ in 0..44_100 * 2 {
            let _ = decoder.next();
        }
        let tail: Vec<f32> = (0..8_000).map(|_| decoder.next().unwrap()).collect();
        assert!(peak(&tail) < 0.001, "paused peak was {}", peak(&tail));
    }
}
