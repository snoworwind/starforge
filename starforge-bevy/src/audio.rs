//! Procedurally generated sound effects (16-bit PCM mono WAV), no external assets.
//! Sounds are synthesized at startup into AudioSource assets.

use bevy::audio::{AudioSource, PlaybackMode, PlaybackSettings};
use bevy::prelude::*;
use std::sync::Arc;

fn write_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    let put16 = |out: &mut Vec<u8>, v: u16| {
        out.push((v & 0xFF) as u8);
        out.push((v >> 8) as u8);
    };
    let put32 = |out: &mut Vec<u8>, v: u32| {
        put16(out, (v & 0xFFFF) as u16);
        put16(out, (v >> 16) as u16);
    };
    out.extend_from_slice(b"RIFF");
    put32(&mut out, 36 + data_len);
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    put32(&mut out, 16);
    put16(&mut out, 1); // PCM
    put16(&mut out, 1); // mono
    put32(&mut out, sample_rate);
    put32(&mut out, sample_rate * 2);
    put16(&mut out, 2);
    put16(&mut out, 16);
    out.extend_from_slice(b"data");
    put32(&mut out, data_len);
    for &s in samples {
        put16(&mut out, s as u16);
    }
    out
}

fn synth(f: impl Fn(f32) -> f32, dur: f32, sample_rate: u32, vol: f32) -> Vec<u8> {
    let n = (dur * sample_rate as f32) as usize;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let env = (1.0 - t / dur).powf(1.6).clamp(0.0, 1.0);
        let v = (f(t) * env * vol * 32000.0).clamp(-32000.0, 32000.0) as i16;
        samples.push(v);
    }
    write_wav(&samples, sample_rate)
}

pub const SR: u32 = 22050;

#[derive(Resource)]
pub struct Sfx {
    pub dig: Handle<AudioSource>,
    pub place: Handle<AudioSource>,
    pub break_block: Handle<AudioSource>,
    pub jump: Handle<AudioSource>,
    pub hurt: Handle<AudioSource>,
    pub pickup: Handle<AudioSource>,
    pub click: Handle<AudioSource>,
    pub craft: Handle<AudioSource>,
    pub jet: Handle<AudioSource>,
    pub laser_hit: Handle<AudioSource>,
    pub error: Handle<AudioSource>,
    pub alarm: Handle<AudioSource>,
}

impl Sfx {
    pub fn build(assets: &mut Assets<AudioSource>) -> Self {
        let mut add = |bytes: Vec<u8>| -> Handle<AudioSource> {
            assets.add(AudioSource {
                bytes: Arc::from(bytes.into_boxed_slice()),
            })
        };
        // dig: low saw tick
        let dig = add(synth(
            |t| {
                let f = 90.0 - t * 300.0;
                let ph = f * t;
                (ph - ph.floor()) * 2.0 - 1.0
            },
            0.09,
            SR,
            0.5,
        ));
        // place: soft thud
        let place = add(synth(
            |t| (t * 2.0 * std::f32::consts::PI * 70.0).sin() * (1.0 - t * 3.0).max(0.0),
            0.12,
            SR,
            0.7,
        ));
        // break: crunchier burst
        let break_block = add(synth(
            |t| {
                let f = 160.0 - t * 500.0;
                let ph = f * t;
                (ph - ph.floor()) * 2.0 - 1.0
            },
            0.16,
            SR,
            0.6,
        ));
        // jump: rising chirp
        let jump = add(synth(
            |t| (t * 2.0 * std::f32::consts::PI * (240.0 + t * 900.0)).sin(),
            0.18,
            SR,
            0.5,
        ));
        // hurt: descending saw
        let hurt = add(synth(
            |t| {
                let f = 320.0 - t * 600.0;
                let ph = f * t;
                (ph - ph.floor()) * 2.0 - 1.0
            },
            0.25,
            SR,
            0.6,
        ));
        // pickup: high blip
        let pickup = add(synth(
            |t| (t * 2.0 * std::f32::consts::PI * (660.0 + t * 700.0)).sin(),
            0.09,
            SR,
            0.45,
        ));
        // click: tiny tick
        let click = add(synth(
            |t| (t * 2.0 * std::f32::consts::PI * 1400.0).sin(),
            0.03,
            SR,
            0.4,
        ));
        // craft: two-tone
        let craft = add(synth(
            |t| {
                let f = if t < 0.06 { 440.0 } else { 660.0 };
                (t * 2.0 * std::f32::consts::PI * f).sin()
            },
            0.14,
            SR,
            0.4,
        ));
        // jet: loopable rumble (long, seamless-ish)
        let mut jet_samples = Vec::new();
        let n = (SR as f32 * 0.5) as usize;
        let mut phase = 0.0f32;
        let mut rng = crate::rng::Rng::new(4242);
        for _ in 0..n {
            phase += (70.0 + rng.next() * 30.0) / SR as f32;
            jet_samples.push((phase.sin() * 0.6 * 32000.0) as i16);
        }
        let jet = add(write_wav(&jet_samples, SR));
        // laser_hit: zap
        let laser_hit = add(synth(
            |t| {
                let f = 900.0 - t * 2500.0;
                let ph = f * t;
                (ph - ph.floor()) * 2.0 - 1.0
            },
            0.1,
            SR,
            0.4,
        ));
        // error: low buzz
        let error = add(synth(
            |t| (t * 2.0 * std::f32::consts::PI * 140.0).sin(),
            0.15,
            SR,
            0.5,
        ));
        // alarm: siren-ish
        let alarm = add(synth(
            |t| {
                let f = 400.0 + (t * 3.0).sin() * 180.0;
                let ph = f * t;
                (ph - ph.floor()) * 2.0 - 1.0
            },
            0.9,
            SR,
            0.5,
        ));
        Self {
            dig,
            place,
            break_block,
            jump,
            hurt,
            pickup,
            click,
            craft,
            jet,
            laser_hit,
            error,
            alarm,
        }
    }
}

/// Play a one-shot sound.
pub fn play(
    commands: &mut Commands,
    handle: Handle<AudioSource>,
    volume: f32,
    pitch: Option<f32>,
) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: bevy::audio::Volume::Linear(volume),
            speed: pitch.unwrap_or(1.0),
            ..default()
        },
    ));
}

/// Entity component for the looping jetpack sound (stop by despawning).
#[derive(Component)]
pub struct JetSound;
