//! Wildlife behaviour pass layered on top of the base creature system:
//! herd cohesion, night resting, startle reactions, distance-based
//! vocalizations and footstep dust. The base system owns locomotion; this one
//! only nudges velocity and presentation so both stay deterministic.

use bevy::prelude::*;

use crate::creatures::Creature;
use crate::daynight::{DayTime, day_factor};
use crate::particles::{EmitOptions, ParticleStyle, ParticleSystem};
use crate::player::Player;
use crate::schedule::GameState;
use crate::tween::exp_approach;

pub const HERD_RADIUS: f32 = 14.0;
pub const STARTLE_RADIUS: f32 = 5.5;
pub const WAKE_DISTANCE: f32 = 8.0;

/// One creature's steering contribution from its nearest same-species
/// neighbour. Separated for testability.
pub fn cohesion_steer(self_pos: Vec3, neighbour: Vec3, strength: f32) -> Vec3 {
    let offset = neighbour - self_pos;
    let distance = offset.length();
    if !(0.001..=HERD_RADIUS).contains(&distance) {
        return Vec3::ZERO;
    }
    offset.normalize_or_zero() * strength
}

/// Should a passive creature be resting at this time of day?
pub fn rests_at(night: f32, already_resting: bool) -> bool {
    night > 0.7 || already_resting
}

/// Vocal pitch per species: gives each animal a recognizable voice.
pub fn species_voice(kind: &str) -> (f32, f32) {
    match kind {
        "strider" => (0.72, 0.9),
        "hopper" => (1.25, 1.5),
        "crab" => (0.55, 0.7),
        "beetle" => (0.45, 0.6),
        "manta" => (1.5, 1.9),
        "blob" => (0.35, 0.5),
        "sentinel" => (0.28, 0.4),
        _ => (0.8, 1.1),
    }
}

/// Gameplay-facing state shared with other modules (used by the frontier
/// ecology surveys to know which species live nearby).
#[derive(Resource, Default)]
pub struct WildlifeState {
    pub near_species: Vec<&'static str>,
    pub scan_timer: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn wildlife_system(
    time: Res<Time>,
    day: Option<Res<DayTime>>,
    mut state: ResMut<WildlifeState>,
    player: Query<&Player>,
    mut creatures: Query<(Entity, &mut Creature, &mut Transform)>,
    mut fx: ParticleSystem,
) {
    let Ok(player) = player.single() else { return };
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let night = day
        .as_deref()
        .map(|day| 1.0 - day_factor(day.0))
        .unwrap_or(0.0);

    // Snapshot positions so steering can look at neighbours without aliasing.
    let positions: Vec<(Entity, Vec3, &'static str, f32)> = creatures
        .iter()
        .filter(|(_, c, _)| c.hp > 0.0 && !c.fading)
        .map(|(e, c, t)| (e, t.translation, c.kind, c.hp))
        .collect();

    state.scan_timer -= dt;
    if state.scan_timer <= 0.0 {
        state.scan_timer = 1.5;
        state.near_species.clear();
        for (_, position, kind, _) in &positions {
            if position.distance(player.pos) < 40.0 && !state.near_species.contains(kind) {
                state.near_species.push(*kind);
            }
        }
    }

    for (entity, mut creature, mut transform) in &mut creatures {
        if creature.hp <= 0.0 || creature.fading {
            continue;
        }
        let distance = transform.translation.distance(player.pos);
        // Only simulate behaviours near the player; distant herds keep their
        // base wander without per-frame cost.
        if distance > 56.0 {
            continue;
        }
        let is_sentinel = creature.kind == "sentinel";
        let aggressive_species = matches!(creature.kind, "crab" | "beetle" | "hopper");
        let threatened = creature.aggro_t > 0.0 && creature.hp < 3.0;

        // ---------- Startle ----------
        if !is_sentinel && distance < STARTLE_RADIUS && player.vel.length() > 5.5 {
            creature.walking = true;
            let away = transform.translation - player.pos;
            if let Some(dir) = away.try_normalize() {
                creature.vel.x = creature.vel.x * 0.5 + dir.x * creature.speed * 1.6;
                creature.vel.z = creature.vel.z * 0.5 + dir.z * creature.speed * 1.6;
            }
            creature.jump_t = creature.jump_t.min(0.1);
        }

        // ---------- Night resting (passive species only) ----------
        if !aggressive_species && !is_sentinel && creature.aggro_t <= 0.0 {
            if night > 0.7 && distance > WAKE_DISTANCE {
                creature.walking = false;
                creature.vel.x = exp_approach(creature.vel.x, 0.0, 2.5, dt);
                creature.vel.z = exp_approach(creature.vel.z, 0.0, 2.5, dt);
            } else if rests_at(night, creature.walking) && night > 0.7 {
                // Woken by the player: resume wandering briskly.
                creature.walking = true;
            }
        }

        // ---------- Herd cohesion ----------
        if creature.walking && !threatened && !is_sentinel {
            let mut neighbours = 0.0f32;
            let mut pull = Vec3::ZERO;
            for (other, position, kind, _) in &positions {
                if *other == entity || *kind != creature.kind {
                    continue;
                }
                let steer = cohesion_steer(transform.translation, *position, 0.25);
                if steer != Vec3::ZERO {
                    pull += steer;
                    neighbours += 1.0;
                }
            }
            if neighbours > 0.0 {
                pull /= neighbours;
                creature.vel.x += pull.x * dt;
                creature.vel.z += pull.z * dt;
            }
            // Soft speed cap so cohesion never outruns the base speed.
            let horizontal = Vec2::new(creature.vel.x, creature.vel.z);
            let cap = creature.speed * 2.2;
            if horizontal.length() > cap {
                let scale = cap / horizontal.length();
                creature.vel.x *= scale;
                creature.vel.z *= scale;
            }
        }

        // ---------- Footstep dust ----------
        let horizontal_speed = Vec2::new(creature.vel.x, creature.vel.z).length();
        if creature.grounded && creature.walking && horizontal_speed > 0.25 {
            creature.anim_t += dt * horizontal_speed;
            let stride = (horizontal_speed * 0.55).max(0.18);
            if creature.anim_t % stride < dt {
                let dust = match creature.kind {
                    "crab" | "beetle" => ParticleStyle::Sand,
                    "manta" | "blob" => ParticleStyle::Frost,
                    _ => ParticleStyle::Dust,
                };
                fx.emit(
                    dust,
                    transform.translation + Vec3::Y * 0.06,
                    EmitOptions::default()
                        .count(1)
                        .speed(0.3, 1.0)
                        .spread(1.6)
                        .size_scale(0.6),
                );
            }
        }

        // Keep transforms finite even if steering pushed something odd.
        if !transform.translation.is_finite() {
            transform.translation = player.pos;
        }
    }
}

/// Species calls: quiet, spatial and rare. Aggressive creatures growl when
/// they acquire a target.
pub fn wildlife_audio_system(
    time: Res<Time>,
    sfx: Res<crate::audio::Sfx>,
    mut commands: Commands,
    mut timer: Local<f32>,
    player: Query<&Player>,
    creatures: Query<(&Creature, &Transform)>,
) {
    let Ok(player) = player.single() else { return };
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    let mut rng = crate::rng::Rng::new((time.elapsed_secs() * 60.0) as u32 ^ 0x51A7);
    *timer = 3.5 + rng.next() * 6.0;
    // Anger first: an aggroed creature is the most important cue.
    let mut chosen: Option<(&'static str, Vec3, bool)> = None;
    for (creature, transform) in &creatures {
        if creature.hp <= 0.0 || creature.fading {
            continue;
        }
        let distance = transform.translation.distance(player.pos);
        if distance > 42.0 {
            continue;
        }
        let angry = creature.aggro_t > 0.0;
        if angry {
            chosen = Some((creature.kind, transform.translation, true));
            break;
        } else if chosen.is_none() && distance < 26.0 {
            chosen = Some((creature.kind, transform.translation, false));
        }
    }
    let Some((kind, position, angry)) = chosen else {
        return;
    };
    let (low, high) = species_voice(kind);
    let pitch = if angry {
        low * 0.85
    } else {
        low + rng.next() * (high - low)
    };
    crate::audio::play_spatial(
        &mut commands,
        if angry {
            sfx.creature_hit.clone()
        } else {
            sfx.hover.clone()
        },
        position + Vec3::Y * 0.8,
        if angry { 0.22 } else { 0.12 },
        Some(pitch),
    );
}

pub struct WildlifePlugin;

impl Plugin for WildlifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WildlifeState>().add_systems(
            Update,
            (wildlife_system, wildlife_audio_system)
                .chain()
                .in_set(crate::schedule::GameSet::LateCreatures)
                .before(crate::creatures::creature_despawn_system)
                .run_if(in_state(GameState::Playing))
                .run_if(crate::schedule::creature_mode),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cohesion_pulls_toward_nearby_neighbour() {
        let steer = cohesion_steer(Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0), 0.25);
        assert!(steer.x > 0.0);
        assert!(cohesion_steer(Vec3::ZERO, Vec3::new(40.0, 0.0, 0.0), 0.25) == Vec3::ZERO);
        assert_eq!(cohesion_steer(Vec3::ZERO, Vec3::ZERO, 0.25), Vec3::ZERO);
    }

    #[test]
    fn resting_follows_night() {
        assert!(!rests_at(0.1, false));
        assert!(rests_at(0.9, false));
        assert!(rests_at(0.1, true));
    }

    #[test]
    fn voices_are_ordered_by_size() {
        let (beetle_low, beetle_high) = species_voice("beetle");
        let (manta_low, _manta_high) = species_voice("manta");
        assert!(beetle_high < manta_low);
        assert!(beetle_low > 0.0);
        assert!(species_voice("unknown").0 > 0.0);
    }
}
