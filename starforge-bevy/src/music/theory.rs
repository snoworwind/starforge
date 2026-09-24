//! Music theory helpers for the procedural composer: modes, chord qualities,
//! deterministic progressions, voicing/voice-leading and drum patterns.
//!
//! Everything is deterministic from a seed so a planet always sounds like
//! itself, while `bar` and `intensity` let the arrangement evolve over time.

use crate::rng::Rng;

// ---------- Modes & scales ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Ionian,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Aeolian,
    Locrian,
}

impl Mode {
    pub const ALL: [Mode; 7] = [
        Mode::Ionian,
        Mode::Dorian,
        Mode::Phrygian,
        Mode::Lydian,
        Mode::Mixolydian,
        Mode::Aeolian,
        Mode::Locrian,
    ];

    /// Semitone offsets from the tonic.
    pub fn intervals(self) -> [i32; 7] {
        match self {
            Mode::Ionian => [0, 2, 4, 5, 7, 9, 11],
            Mode::Dorian => [0, 2, 3, 5, 7, 9, 10],
            Mode::Phrygian => [0, 1, 3, 5, 7, 8, 10],
            Mode::Lydian => [0, 2, 4, 6, 7, 9, 11],
            Mode::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
            Mode::Aeolian => [0, 2, 3, 5, 7, 8, 10],
            Mode::Locrian => [0, 1, 3, 5, 6, 8, 10],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Ionian => "ionian",
            Mode::Dorian => "dorian",
            Mode::Phrygian => "phrygian",
            Mode::Lydian => "lydian",
            Mode::Mixolydian => "mixolydian",
            Mode::Aeolian => "aeolian",
            Mode::Locrian => "locrian",
        }
    }

    pub fn from_label(label: &str) -> Mode {
        Mode::ALL
            .into_iter()
            .find(|mode| mode.label() == label)
            .unwrap_or(Mode::Aeolian)
    }
}

/// Root note (MIDI) + mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub root: i32,
    pub mode: Mode,
}

impl Key {
    pub fn new(root: i32, mode: Mode) -> Self {
        Self { root, mode }
    }

    /// Scale degree (0 = tonic) to MIDI note; degrees wrap octaves in both
    /// directions so `-1` is the seventh below the tonic.
    pub fn degree(&self, degree: i32) -> i32 {
        let intervals = self.mode.intervals();
        let octave = degree.div_euclid(7);
        let index = degree.rem_euclid(7) as usize;
        self.root + octave * 12 + intervals[index]
    }

    /// A pitch that leans toward the mode's characteristic color; used for
    /// sparse melodic accents.
    pub fn color_note(&self, degree: i32) -> i32 {
        self.degree(degree)
    }

    /// Transpose the whole key by semitones.
    pub fn transposed(self, semitones: i32) -> Self {
        Self {
            root: self.root + semitones,
            ..self
        }
    }
}

/// Pick a planet/scene flavor deterministically. Roots stay in a comfortable
/// low-mid range so bass and pad sit well under the SFX mix.
pub fn key_for_context(context: &str, seed: u32) -> Key {
    let mut rng = Rng::new(seed ^ 0xA11CE);
    let root = 33 + (rng.next() * 10.0) as i32; // A1..G2-ish
    let mode = match context {
        "lush" => Mode::Lydian,
        "desert" => Mode::Phrygian,
        "frozen" => Mode::Aeolian,
        "volcanic" | "obsidian" => Mode::Phrygian,
        "alien" | "crystal" => Mode::Lydian,
        "fungal" | "murk" => Mode::Dorian,
        "ocean" => Mode::Aeolian,
        "ashen" => Mode::Aeolian,
        "amber" | "hive" => Mode::Mixolydian,
        "ferrous" => Mode::Locrian,
        "salt" => Mode::Dorian,
        "redmoss" => Mode::Phrygian,
        "menu" => Mode::Ionian,
        "station" => Mode::Dorian,
        "space" => Mode::Aeolian,
        "cave" => Mode::Aeolian,
        "combat" => Mode::Phrygian,
        "warp" => Mode::Locrian,
        "night" => Mode::Aeolian,
        _ => Mode::Dorian,
    };
    Key::new(root, mode)
}

// ---------- Chords ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChordQuality {
    Major,
    Minor,
    Major7,
    Minor7,
    Dominant7,
    Minor9,
    Major9,
    Sus2,
    Sus4,
    Add9,
    Minor6,
    Major6,
    Diminished,
    Augmented,
    Power,
    Minor11,
    Major7Sharp11,
}

impl ChordQuality {
    /// Semitone intervals from the chord root.
    pub fn intervals(self) -> &'static [i32] {
        match self {
            ChordQuality::Major => &[0, 4, 7],
            ChordQuality::Minor => &[0, 3, 7],
            ChordQuality::Major7 => &[0, 4, 7, 11],
            ChordQuality::Minor7 => &[0, 3, 7, 10],
            ChordQuality::Dominant7 => &[0, 4, 7, 10],
            ChordQuality::Minor9 => &[0, 3, 7, 10, 14],
            ChordQuality::Major9 => &[0, 4, 7, 11, 14],
            ChordQuality::Sus2 => &[0, 2, 7],
            ChordQuality::Sus4 => &[0, 5, 7],
            ChordQuality::Add9 => &[0, 4, 7, 14],
            ChordQuality::Minor6 => &[0, 3, 7, 9],
            ChordQuality::Major6 => &[0, 4, 7, 9],
            ChordQuality::Diminished => &[0, 3, 6],
            ChordQuality::Augmented => &[0, 4, 8],
            ChordQuality::Power => &[0, 7],
            ChordQuality::Minor11 => &[0, 3, 7, 10, 14, 17],
            ChordQuality::Major7Sharp11 => &[0, 4, 7, 11, 18],
        }
    }

    pub fn is_minor(self) -> bool {
        self.intervals().contains(&3)
    }
}

/// A chord expressed as a scale degree plus quality; this makes progressions
/// mode-aware without hard-coding accidentals.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub degree: i32,
    pub quality: ChordQuality,
}

impl Chord {
    pub const fn new(degree: i32, quality: ChordQuality) -> Self {
        Self { degree, quality }
    }

    pub fn root_midi(&self, key: &Key) -> i32 {
        key.degree(self.degree)
    }

    pub fn notes(&self, key: &Key) -> Vec<i32> {
        let root = self.root_midi(key);
        self.quality
            .intervals()
            .iter()
            .map(|interval| root + interval)
            .collect()
    }
}

const I: i32 = 0;
const II: i32 = 1;
const III: i32 = 2;
const IV: i32 = 3;
const V: i32 = 4;
const VI: i32 = 5;
const VII: i32 = 6;

use ChordQuality::*;

/// Emotional/procedural palettes. Each entry is a full progression the
/// composer can pick from; keeping several per mood avoids monotony without
/// ever producing something atonal.
const CALM: &[&[Chord]] = &[
    &[
        Chord::new(I, Minor9),
        Chord::new(VI, Major7),
        Chord::new(III, Major7),
        Chord::new(VII, Dominant7),
    ],
    &[
        Chord::new(I, Minor7),
        Chord::new(IV, Minor7),
        Chord::new(VI, Major9),
        Chord::new(V, Minor7),
    ],
    &[
        Chord::new(I, Minor7),
        Chord::new(III, Major7),
        Chord::new(IV, Minor7),
        Chord::new(VI, Major7),
    ],
    &[
        Chord::new(I, Minor9),
        Chord::new(V, Minor7),
        Chord::new(VI, Major7),
        Chord::new(IV, Minor7),
    ],
];
const HOPEFUL: &[&[Chord]] = &[
    &[
        Chord::new(I, Major7),
        Chord::new(V, Add9),
        Chord::new(VI, Minor7),
        Chord::new(IV, Major9),
    ],
    &[
        Chord::new(I, Major9),
        Chord::new(IV, Major7),
        Chord::new(V, Sus4),
        Chord::new(V, Major),
    ],
    &[
        Chord::new(I, Major7),
        Chord::new(III, Minor7),
        Chord::new(IV, Major9),
        Chord::new(V, Add9),
    ],
    &[
        Chord::new(I, Add9),
        Chord::new(V, Major),
        Chord::new(VI, Minor9),
        Chord::new(IV, Major7Sharp11),
    ],
];
const SOMBER: &[&[Chord]] = &[
    &[
        Chord::new(I, Minor7),
        Chord::new(VII, Major),
        Chord::new(VI, Major7),
        Chord::new(VII, Sus4),
    ],
    &[
        Chord::new(I, Minor9),
        Chord::new(IV, Minor7),
        Chord::new(VII, Major7),
        Chord::new(III, Major7),
    ],
    &[
        Chord::new(I, Minor),
        Chord::new(VI, Minor7),
        Chord::new(VII, Major7),
        Chord::new(V, Minor7),
    ],
    &[
        Chord::new(I, Minor7),
        Chord::new(V, Minor7),
        Chord::new(VI, Major7),
        Chord::new(VII, Dominant7),
    ],
];
const TENSE: &[&[Chord]] = &[
    &[
        Chord::new(I, Minor),
        Chord::new(II, Diminished),
        Chord::new(VII, Major),
        Chord::new(VI, Major7),
    ],
    &[
        Chord::new(I, Minor7),
        Chord::new(VI, Minor7),
        Chord::new(II, Diminished),
        Chord::new(V, Major),
    ],
    &[
        Chord::new(I, Minor),
        Chord::new(V, Power),
        Chord::new(VI, Major),
        Chord::new(II, Diminished),
    ],
    &[
        Chord::new(I, Minor9),
        Chord::new(VII, Major7),
        Chord::new(VI, Minor6),
        Chord::new(V, Dominant7),
    ],
];
const MYSTERIOUS: &[&[Chord]] = &[
    &[
        Chord::new(I, Minor9),
        Chord::new(II, Sus2),
        Chord::new(VI, Major7Sharp11),
        Chord::new(VII, Sus4),
    ],
    &[
        Chord::new(I, Minor7),
        Chord::new(IV, Minor6),
        Chord::new(VII, Major9),
        Chord::new(III, Major7),
    ],
    &[
        Chord::new(I, Minor11),
        Chord::new(VI, Major9),
        Chord::new(II, Sus2),
        Chord::new(V, Minor7),
    ],
];
const DRIVING: &[&[Chord]] = &[
    &[
        Chord::new(I, Minor7),
        Chord::new(VI, Major),
        Chord::new(III, Major),
        Chord::new(VII, Major),
    ],
    &[
        Chord::new(I, Minor),
        Chord::new(VII, Major),
        Chord::new(VI, Major),
        Chord::new(VII, Dominant7),
    ],
    &[
        Chord::new(I, Minor7),
        Chord::new(IV, Minor7),
        Chord::new(VII, Major),
        Chord::new(III, Major9),
    ],
];
const ETHEREAL: &[&[Chord]] = &[
    &[
        Chord::new(I, Major9),
        Chord::new(II, Sus2),
        Chord::new(IV, Major7Sharp11),
        Chord::new(VI, Minor9),
    ],
    &[
        Chord::new(I, Major7),
        Chord::new(V, Sus2),
        Chord::new(VI, Minor9),
        Chord::new(IV, Major9),
    ],
    &[
        Chord::new(I, Minor9),
        Chord::new(III, Major9),
        Chord::new(VII, Sus4),
        Chord::new(VI, Major7Sharp11),
    ],
];
const WARM: &[&[Chord]] = &[
    &[
        Chord::new(I, Minor7),
        Chord::new(IV, Minor7),
        Chord::new(VI, Major7),
        Chord::new(V, Minor7),
    ],
    &[
        Chord::new(I, Add9),
        Chord::new(IV, Major7),
        Chord::new(II, Minor7),
        Chord::new(V, Dominant7),
    ],
    &[
        Chord::new(I, Major7),
        Chord::new(VI, Minor7),
        Chord::new(IV, Major7),
        Chord::new(V, Sus4),
    ],
];
const DEFAULT_PALETTE: &[&[Chord]] = &[
    &[
        Chord::new(I, Minor7),
        Chord::new(VI, Major7),
        Chord::new(IV, Minor7),
        Chord::new(V, Minor7),
    ],
    &[
        Chord::new(I, Major7),
        Chord::new(IV, Major7),
        Chord::new(VI, Minor7),
        Chord::new(V, Dominant7),
    ],
];

pub fn progression_palette(mood: &str) -> &'static [&'static [Chord]] {
    match mood {
        "calm" => CALM,
        "hopeful" => HOPEFUL,
        "somber" => SOMBER,
        "tense" => TENSE,
        "mysterious" => MYSTERIOUS,
        "driving" => DRIVING,
        "ethereal" => ETHEREAL,
        "warm" => WARM,
        _ => DEFAULT_PALETTE,
    }
}

/// Choose a progression for a context/mood and seed.
pub fn choose_progression(mood: &str, seed: u32) -> Vec<Chord> {
    let palette = progression_palette(mood);
    let mut rng = Rng::new(seed ^ 0xBEEF_1234);
    let index = rng.range(palette.len());
    palette[index].to_vec()
}

/// Extend a 4-chord progression to 8 bars by varying the second half; this
/// keeps long sections interesting without changing the tonal center.
pub fn extend_progression(chords: &[Chord], seed: u32) -> Vec<Chord> {
    let mut rng = Rng::new(seed ^ 0x8BAD_F00D);
    let mut out = chords.to_vec();
    for _ in 0..4 {
        let pick = chords[rng.range(chords.len())];
        // Occasionally substitute a sus/9 color for the repeat.
        let varied = match (rng.next(), pick.quality) {
            (v, ChordQuality::Major7) if v < 0.4 => Chord {
                quality: ChordQuality::Major9,
                ..pick
            },
            (v, ChordQuality::Minor7) if v < 0.4 => Chord {
                quality: ChordQuality::Minor9,
                ..pick
            },
            (v, ChordQuality::Minor) if v < 0.35 => Chord {
                quality: ChordQuality::Sus2,
                ..pick
            },
            _ => pick,
        };
        out.push(varied);
    }
    out
}

/// Move `notes` into a register close to `previous` (voice leading) and make
/// sure they stay inside `[low, high]`.
pub fn voice_lead(previous: &[i32], notes: &[i32], low: i32, high: i32) -> Vec<i32> {
    if previous.is_empty() {
        return notes
            .iter()
            .map(|note| {
                let mut note = *note;
                while note < low {
                    note += 12;
                }
                while note > high {
                    note -= 12;
                }
                note
            })
            .collect();
    }
    let center = previous.iter().sum::<i32>() / previous.len().max(1) as i32;
    notes
        .iter()
        .map(|note| {
            let mut note = *note;
            while note - center > 6 {
                note -= 12;
            }
            while center - note > 6 {
                note += 12;
            }
            while note < low {
                note += 12;
            }
            while note > high {
                note -= 12;
            }
            note
        })
        .collect()
}

/// Spread a chord upward for pads: root doubled an octave down, upper
/// extensions kept high.
pub fn pad_voicing(chord: &Chord, key: &Key, base: i32) -> Vec<i32> {
    let root = chord.root_midi(key);
    let mut notes = vec![root + 12]; // inside the pad register
    for interval in chord.quality.intervals().iter().skip(1) {
        let mut note = root + interval;
        while note < base {
            note += 12;
        }
        if note < base + 30 {
            notes.push(note);
        }
    }
    notes.sort_unstable();
    notes.dedup();
    notes
}

// ---------- Drum patterns ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrumStyle {
    None,
    Soft,
    Lounge,
    Electronica,
    Space,
    Combat,
    Warp,
    Tribal,
}

impl DrumStyle {
    pub fn label(self) -> &'static str {
        match self {
            DrumStyle::None => "none",
            DrumStyle::Soft => "soft",
            DrumStyle::Lounge => "lounge",
            DrumStyle::Electronica => "electronica",
            DrumStyle::Space => "space",
            DrumStyle::Combat => "combat",
            DrumStyle::Warp => "warp",
            DrumStyle::Tribal => "tribal",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrumStep {
    pub kick: f32,
    pub snare: f32,
    pub hat: f32,
    pub open_hat: f32,
    pub perc: f32,
    pub tom: f32,
    pub clap: f32,
}

/// 16-step pattern for one bar.
#[derive(Clone, Copy, Debug)]
pub struct DrumPattern {
    pub steps: [DrumStep; 16],
}

impl DrumPattern {
    pub fn empty() -> Self {
        Self {
            steps: [DrumStep::default(); 16],
        }
    }

    /// Generate one bar. `bar` matters because fills appear at phrase ends and
    /// intensity controls density, so the groove breathes over time.
    pub fn generate(style: DrumStyle, bar: usize, intensity: f32, seed: u32) -> Self {
        let mut rng = Rng::new(seed ^ (bar as u32).wrapping_mul(0x9E37_79B9));
        let intensity = intensity.clamp(0.0, 1.0);
        let mut pattern = Self::empty();
        let fill = bar % 8 == 7;
        match style {
            DrumStyle::None => {}
            DrumStyle::Soft => {
                pattern.set_kick(0, 0.9);
                pattern.set_kick(8, 0.7);
                pattern.set_snare(4, 0.55);
                pattern.set_snare(12, 0.6);
                for step in (0..16).step_by(2) {
                    pattern.set_hat(step, 0.3 + 0.12 * (step % 4 == 0) as i32 as f32);
                }
                if intensity > 0.5 {
                    pattern.set_perc(14, 0.3);
                }
                if fill {
                    pattern.set_tom(14, 0.5);
                    pattern.set_tom(15, 0.6);
                }
            }
            DrumStyle::Lounge => {
                pattern.set_kick(0, 0.85);
                pattern.set_kick(10, 0.6);
                pattern.set_snare(4, 0.5);
                pattern.set_snare(11, 0.35);
                pattern.set_rim(4, 0.4);
                pattern.set_rim(12, 0.4);
                for step in (2..16).step_by(4) {
                    pattern.set_hat(step, 0.28);
                }
                pattern.set_open_hat(14, 0.3);
                if fill {
                    pattern.set_snare(15, 0.7);
                }
            }
            DrumStyle::Electronica => {
                for step in (0..16).step_by(4) {
                    pattern.set_kick(step, 0.95);
                }
                pattern.set_clap(4, 0.7);
                pattern.set_clap(12, 0.75);
                for step in (2..16).step_by(4) {
                    pattern.set_open_hat(step, 0.45);
                }
                for step in (0..16).step_by(2) {
                    pattern.set_hat(step, 0.22);
                }
                if intensity > 0.4 {
                    for step in (3..16).step_by(4) {
                        pattern.set_perc(step, 0.3);
                    }
                }
                if fill {
                    for step in 12..16 {
                        pattern.set_tom(step, 0.4 + (step - 12) as f32 * 0.1);
                    }
                }
            }
            DrumStyle::Space => {
                pattern.set_kick(0, 0.7);
                if intensity > 0.35 {
                    pattern.set_kick(8, 0.45);
                }
                for step in (6..16).step_by(5) {
                    pattern.set_hat(step, 0.18);
                }
                if intensity > 0.6 {
                    pattern.set_snare(12, 0.3);
                }
                if fill {
                    pattern.set_perc(15, 0.35);
                }
            }
            DrumStyle::Combat => {
                for step in (0..16).step_by(4) {
                    pattern.set_kick(step, 1.0);
                }
                pattern.set_snare(4, 0.9);
                pattern.set_snare(12, 0.95);
                for step in 0..16 {
                    pattern.set_hat(step, if step % 2 == 0 { 0.32 } else { 0.2 });
                }
                pattern.set_clap(12, 0.55);
                pattern.set_perc(7, 0.4);
                pattern.set_perc(15, 0.45);
                if fill {
                    pattern.set_tom(13, 0.7);
                    pattern.set_tom(14, 0.8);
                    pattern.set_tom(15, 0.9);
                }
            }
            DrumStyle::Warp => {
                let ramp = 0.5 + (bar % 4) as f32 * 0.12;
                for step in (0..16).step_by(4) {
                    pattern.set_kick(step, ramp.min(1.0));
                }
                for step in 0..16 {
                    pattern.set_hat(step, 0.2 + intensity * 0.25);
                }
                pattern.set_snare(12, 0.5 * ramp);
                if fill {
                    pattern.set_tom(15, 0.9);
                }
            }
            DrumStyle::Tribal => {
                pattern.set_tom(0, 0.9);
                pattern.set_tom(3, 0.6);
                pattern.set_tom(6, 0.75);
                pattern.set_tom(10, 0.7);
                pattern.set_kick(8, 0.8);
                pattern.set_snare(12, 0.5);
                for step in (0..16).step_by(2) {
                    pattern.set_perc(step, 0.22 + rng.next() * 0.15);
                }
                if fill {
                    pattern.set_tom(14, 0.7);
                    pattern.set_tom(15, 0.85);
                }
            }
        }
        // Humanize velocities a touch so the loop never sounds like a machine.
        for step in &mut pattern.steps {
            let jitter = 0.94 + rng.next() * 0.1;
            step.kick *= jitter;
            step.snare *= jitter;
            step.hat *= jitter;
        }
        pattern
    }
}

impl DrumPattern {
    fn set_kick(&mut self, step: usize, velocity: f32) {
        self.steps[step].kick = velocity.clamp(0.0, 1.5);
    }

    fn set_snare(&mut self, step: usize, velocity: f32) {
        self.steps[step].snare = velocity.clamp(0.0, 1.5);
    }

    fn set_hat(&mut self, step: usize, velocity: f32) {
        self.steps[step].hat = velocity.clamp(0.0, 1.5);
    }

    fn set_open_hat(&mut self, step: usize, velocity: f32) {
        self.steps[step].open_hat = velocity.clamp(0.0, 1.5);
    }

    fn set_perc(&mut self, step: usize, velocity: f32) {
        self.steps[step].perc = velocity.clamp(0.0, 1.5);
    }

    fn set_tom(&mut self, step: usize, velocity: f32) {
        self.steps[step].tom = velocity.clamp(0.0, 1.5);
    }

    fn set_rim(&mut self, step: usize, velocity: f32) {
        self.steps[step].perc = velocity.clamp(0.0, 1.5);
    }

    fn set_clap(&mut self, step: usize, velocity: f32) {
        self.steps[step].clap = velocity.clamp(0.0, 1.5);
    }
}

// ---------- Arpeggios & melodies ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArpShape {
    Up,
    Down,
    UpDown,
    Random,
    Pedal,
    Skip,
}

impl ArpShape {
    pub fn from_seed(seed: u32) -> Self {
        match seed % 6 {
            0 => ArpShape::Up,
            1 => ArpShape::Down,
            2 => ArpShape::UpDown,
            3 => ArpShape::Random,
            4 => ArpShape::Pedal,
            _ => ArpShape::Skip,
        }
    }
}

/// Stateless arpeggio note for one step (allocation-free, used by the audio
/// engine). Returns `None` for a rest.
pub fn arp_note(
    shape: ArpShape,
    pool: &[i32],
    step: usize,
    octave_span: i32,
    seed: u32,
) -> Option<i32> {
    if pool.is_empty() {
        return None;
    }
    let mut rng = Rng::new(seed ^ (step as u32).wrapping_mul(0x9E37_79B9));
    let rest = rng.next() < if step % 8 == 7 { 0.35 } else { 0.08 };
    if rest {
        return None;
    }
    let len = pool.len() as i32;
    let index = match shape {
        ArpShape::Up => step as i32 % len,
        ArpShape::Down => (len - 1 - step as i32 % len).rem_euclid(len),
        ArpShape::UpDown => {
            let period = (len * 2 - 2).max(1);
            let phase = step as i32 % period;
            if phase < len { phase } else { period - phase }
        }
        ArpShape::Random => rng.range(pool.len()) as i32,
        ArpShape::Pedal => if step.is_multiple_of(2) {
            0
        } else {
            1 + (step as i32 / 2) % len.max(1)
        }
        .rem_euclid(len),
        ArpShape::Skip => ((step as i32 * 2) % len).rem_euclid(len),
    };
    let mut note = pool[index.rem_euclid(len) as usize];
    let octave = if octave_span > 0 {
        (rng.next() * (octave_span as f32 + 1.0)) as i32
    } else {
        0
    };
    note += octave * 12;
    Some(note)
}

/// Build one bar (16 steps) of arpeggio note indices into `pool`.
pub fn arp_sequence(
    shape: ArpShape,
    pool: &[i32],
    steps: usize,
    octave_span: i32,
    seed: u32,
) -> Vec<Option<i32>> {
    (0..steps)
        .map(|step| arp_note(shape, pool, step, octave_span, seed))
        .collect()
}

/// A sparse melodic phrase over `bars` bars, returning (step_index_in_bar, note).
pub fn melody_phrase(key: &Key, chord: &Chord, bars: usize, seed: u32) -> Vec<(usize, i32)> {
    let mut rng = Rng::new(seed ^ 0x7A11_0FEE);
    let mut out = Vec::new();
    let chord_notes = chord.notes(key);
    let high = key.degree(14);
    for bar in 0..bars {
        let density = 2 + rng.range(3);
        for _ in 0..density {
            let step = rng.range(16);
            let use_chord_tone = rng.next() < 0.62;
            let note = if use_chord_tone {
                chord_notes[rng.range(chord_notes.len())] + 12
            } else {
                let degree = 7 + rng.range(7) as i32;
                key.degree(degree)
            };
            let note = note.min(high).max(key.degree(7));
            out.push((bar * 16 + step, note));
        }
    }
    out.sort_by_key(|(step, _)| *step);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_have_seven_degrees_and_tonic_zero() {
        for mode in Mode::ALL {
            let intervals = mode.intervals();
            assert_eq!(intervals[0], 0, "{mode:?}");
            assert_eq!(intervals.len(), 7);
            for pair in intervals.windows(2) {
                assert!(pair[1] > pair[0], "{mode:?} is not ascending");
            }
            assert!(*intervals.iter().max().unwrap() < 12, "{mode:?}");
        }
    }

    #[test]
    fn key_degree_wraps_both_directions() {
        let key = Key::new(60, Mode::Aeolian);
        assert_eq!(key.degree(0), 60);
        assert_eq!(key.degree(7), 72);
        assert_eq!(key.degree(-1), 60 - 2); // seventh below tonic
        assert_eq!(key.degree(8), 74);
        assert_eq!(key.degree(-7), 48);
    }

    #[test]
    fn progressions_are_deterministic_and_tonal() {
        for seed in 0..50 {
            let a = choose_progression("calm", seed);
            let b = choose_progression("calm", seed);
            assert_eq!(a, b);
            assert!(!a.is_empty());
            for chord in &a {
                let key = Key::new(40, Mode::Aeolian);
                let notes = chord.notes(&key);
                assert!(!notes.is_empty());
                for note in notes {
                    assert!((20..110).contains(&note), "note {note} out of range");
                }
            }
        }
    }

    #[test]
    fn all_moods_have_palettes_with_valid_chords() {
        for mood in [
            "calm",
            "hopeful",
            "somber",
            "tense",
            "mysterious",
            "driving",
            "ethereal",
            "warm",
            "",
        ] {
            let palette = progression_palette(mood);
            assert!(!palette.is_empty(), "mood {mood}");
            for progression in palette {
                assert!(progression.len() >= 4, "mood {mood}");
            }
        }
    }

    #[test]
    fn voice_leading_keeps_notes_near_previous() {
        let previous = vec![60, 63, 67, 70];
        let target = vec![41, 45, 48, 52];
        let led = voice_lead(&previous, &target, 48, 84);
        for note in &led {
            assert!((48..=84).contains(note), "note {note} out of range");
            let closest = previous.iter().map(|p| (p - note).abs()).min().unwrap_or(0);
            assert!(closest <= 12, "note {note} leapt too far from {previous:?}");
        }
    }

    #[test]
    fn pad_voicing_is_ascending_and_in_range() {
        let key = Key::new(45, Mode::Dorian);
        let chord = Chord::new(0, ChordQuality::Minor9);
        let notes = pad_voicing(&chord, &key, 52);
        assert!(notes.len() >= 3);
        for pair in notes.windows(2) {
            assert!(pair[0] <= pair[1]);
        }
        assert!(notes.iter().all(|n| (40..90).contains(n)));
    }

    #[test]
    fn drum_patterns_are_deterministic_and_bounded() {
        let styles = [
            DrumStyle::None,
            DrumStyle::Soft,
            DrumStyle::Lounge,
            DrumStyle::Electronica,
            DrumStyle::Space,
            DrumStyle::Combat,
            DrumStyle::Warp,
            DrumStyle::Tribal,
        ];
        for style in styles {
            let a = DrumPattern::generate(style, 3, 0.7, 1234);
            let b = DrumPattern::generate(style, 3, 0.7, 1234);
            assert_eq!(a.steps, b.steps, "{style:?}");
            for step in &a.steps {
                for value in [
                    step.kick,
                    step.snare,
                    step.hat,
                    step.open_hat,
                    step.perc,
                    step.tom,
                    step.clap,
                ] {
                    assert!((0.0..=1.5).contains(&value), "{style:?}");
                }
            }
            if style != DrumStyle::None {
                let energy: f32 = a
                    .steps
                    .iter()
                    .map(|s| s.kick + s.snare + s.hat + s.perc + s.tom + s.clap + s.open_hat)
                    .sum();
                assert!(energy > 0.5, "{style:?} produced an empty groove");
            }
        }
    }

    #[test]
    fn fills_appear_at_phrase_ends() {
        let without = DrumPattern::generate(DrumStyle::Soft, 0, 0.5, 7);
        let with = DrumPattern::generate(DrumStyle::Soft, 7, 0.5, 7);
        let tom_without: f32 = without.steps.iter().map(|s| s.tom).sum();
        let tom_with: f32 = with.steps.iter().map(|s| s.tom).sum();
        assert!(tom_with > tom_without);
    }

    #[test]
    fn arp_sequence_respects_rests_and_pool() {
        let pool = [60, 63, 67, 70];
        for shape in [
            ArpShape::Up,
            ArpShape::Down,
            ArpShape::UpDown,
            ArpShape::Random,
            ArpShape::Pedal,
            ArpShape::Skip,
        ] {
            let seq = arp_sequence(shape, &pool, 16, 1, 42);
            assert_eq!(seq.len(), 16);
            let played: Vec<i32> = seq.iter().flatten().copied().collect();
            assert!(!played.is_empty(), "{shape:?}");
            for note in played {
                assert!((48..=82).contains(&note), "{shape:?} note {note}");
            }
        }
    }

    #[test]
    fn melody_phrase_is_sorted_and_in_key() {
        let key = Key::new(45, Mode::Aeolian);
        let chord = Chord::new(0, ChordQuality::Minor7);
        let phrase = melody_phrase(&key, &chord, 2, 99);
        assert!(!phrase.is_empty());
        for pair in phrase.windows(2) {
            assert!(pair[0].0 <= pair[1].0);
        }
        let max_step = phrase.iter().map(|(step, _)| *step).max().unwrap();
        assert!(max_step < 32);
        for (_, note) in phrase {
            assert!((key.degree(7)..=key.degree(14)).contains(&note));
        }
    }

    #[test]
    fn extend_progression_keeps_palette_material() {
        let chords = choose_progression("hopeful", 5);
        let extended = extend_progression(&chords, 5);
        assert_eq!(extended.len(), chords.len() + 4);
        for chord in &extended {
            let valid = chords.iter().any(|source| source.degree == chord.degree);
            assert!(valid);
        }
    }
}
