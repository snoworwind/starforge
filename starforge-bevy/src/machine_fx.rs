//! Factory life: smoke stacks, glowing reactors, mining drill beams, solar
//! glints, healing mist and item cubes riding conveyor belts.
//!
//! The module only *reads* machine state, so it cannot desynchronise the
//! simulation. A small pool of point lights tracks the nearest active
//! machines, and belt cargo is mirrored with pooled billboard entities keyed
//! by machine entity.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::factory::{Machine, MachineKind, MachineState};
use crate::particles::{EmitOptions, ParticleStyle, ParticleSystem};
use crate::player::Player;

const FX_RANGE: f32 = 56.0;
const MACHINE_LIGHT_POOL: usize = 4;

#[derive(Resource, Default)]
pub struct MachineFx {
    /// Belt cargo visuals: belt entity -> pooled item entities.
    belt_visuals: HashMap<Entity, Vec<Entity>>,
    /// Pool of point lights assigned to the nearest glowing machines.
    light_pool: Vec<Entity>,
}

#[derive(Component)]
pub struct MachineLight {
    pub slot: usize,
}

/// Billboarded item cube riding a conveyor belt.
#[derive(Component)]
pub struct BeltItemVisual {
    pub belt: Entity,
    pub slot: usize,
}

pub fn setup_machine_fx(mut commands: Commands, mut fx: ResMut<MachineFx>) {
    for slot in 0..MACHINE_LIGHT_POOL {
        let entity = commands
            .spawn((
                PointLight {
                    color: Color::srgb(1.0, 0.8, 0.55),
                    intensity: 0.0,
                    range: 14.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::default(),
                MachineLight { slot },
                crate::InGame,
            ))
            .id();
        fx.light_pool.push(entity);
    }
}

/// Light colour for an active machine, or `None` when it should stay dark.
fn machine_glow(kind: MachineKind, state: &MachineState) -> Option<(Color, f32)> {
    match state {
        MachineState::Reactor(reactor) if reactor.fuel > 0.0 => {
            Some((Color::srgb(0.45, 1.0, 0.6), 1.0))
        }
        MachineState::Furnace(furnace) if furnace.on || furnace.prog > 0.0 => {
            Some((Color::srgb(1.0, 0.55, 0.25), 0.8))
        }
        MachineState::Burner(burner) if burner.burn > 0.0 => {
            Some((Color::srgb(1.0, 0.65, 0.3), 0.7))
        }
        MachineState::Colony(colony) if colony.prog > 0.0 || colony.residents > 0 => {
            Some((Color::srgb(0.5, 0.85, 1.0), 0.9))
        }
        MachineState::Battery(battery) if battery.charge > 1.0 => {
            let level = (battery.charge / crate::factory::BATTERY_CAPACITY).clamp(0.0, 1.0);
            Some((Color::srgb(0.4, 0.9, 0.5), 0.3 + level * 0.7))
        }
        MachineState::Medbay(_) => Some((Color::srgb(0.5, 1.0, 0.7), 0.5)),
        _ => {
            // Solar panels glint in daylight; detected by the caller.
            if kind == MachineKind::Solar {
                Some((Color::srgb(0.75, 0.85, 1.0), 0.4))
            } else {
                None
            }
        }
    }
}

/// Machines emit particles with a deterministic per-entity phase so smoke
/// puffs never synchronise across a factory row.
#[allow(clippy::too_many_arguments)]
pub fn machine_particle_system(
    time: Res<Time>,
    day: Option<Res<crate::daynight::DayTime>>,
    machines: Query<(Entity, &Machine, &MachineState, &Transform)>,
    player: Query<&Player>,
    mut particles: ParticleSystem,
) {
    let Ok(player) = player.single() else { return };
    let elapsed = time.elapsed_secs();
    let daylight = day
        .as_deref()
        .map(|day| crate::daynight::day_factor(day.0))
        .unwrap_or(1.0);
    for (entity, machine, state, transform) in &machines {
        let distance = transform.translation.distance(player.pos);
        if distance > FX_RANGE {
            continue;
        }
        // Distance falloff: machines beyond 28 m emit at a reduced rate.
        let rate_scale = if distance > 28.0 { 0.35 } else { 1.0 };
        // Deterministic per-machine phase so they do not all puff together.
        let phase = (entity.index().index() % 97) as f32 * 0.137;
        let tick = |period: f32| -> bool {
            let t = (elapsed + phase) / period;
            t.fract() < 0.05 * rate_scale + 0.001
        };
        let base = transform.translation;
        match (machine.kind, state) {
            (MachineKind::Furnace, MachineState::Furnace(furnace))
                if furnace.on || furnace.prog > 0.0 =>
            {
                if tick(0.5) {
                    particles.machine_smoke(base + Vec3::Y * 1.1, 0.6);
                }
                if tick(0.28) {
                    particles.emit(
                        ParticleStyle::Ember,
                        base + Vec3::Y * 0.95,
                        EmitOptions::default()
                            .count(1)
                            .speed(0.4, 1.2)
                            .spread(1.0)
                            .size_scale(0.7),
                    );
                }
            }
            (MachineKind::Burner, MachineState::Burner(burner)) if burner.burn > 0.0 => {
                if tick(0.4) {
                    particles.machine_smoke(base + Vec3::Y * 1.0, 0.7);
                }
                if tick(0.2) {
                    particles.emit(
                        ParticleStyle::Flame,
                        base + Vec3::Y * 0.8,
                        EmitOptions::default()
                            .count(1)
                            .speed(0.4, 1.4)
                            .spread(0.9)
                            .size_scale(0.7),
                    );
                }
            }
            (MachineKind::Reactor, MachineState::Reactor(reactor)) if reactor.fuel > 0.0 => {
                if tick(0.9) {
                    particles.steam(base + Vec3::Y * 1.15, 0.7);
                }
            }
            (MachineKind::Miner, MachineState::Miner(miner))
                if machine.active && (miner.prog > 0.0 || miner.output.is_some()) =>
            {
                // Drill beam down to the deposit plus contact sparks.
                if tick(0.16) {
                    particles.emit(
                        ParticleStyle::Spark,
                        base - Vec3::Y * 0.45,
                        EmitOptions::default()
                            .count(1)
                            .dir(Vec3::Y)
                            .speed(1.0, 3.0)
                            .spread(1.2)
                            .size_scale(0.8),
                    );
                }
            }
            (MachineKind::Assembler | MachineKind::Refinery, MachineState::Crafter(crafter))
                if crafter.prog > 0.0 =>
            {
                if tick(0.55) {
                    particles.machine_smoke(base + Vec3::Y * 1.05, 0.4);
                }
                if tick(0.7) {
                    particles.emit(
                        ParticleStyle::Spark,
                        base + Vec3::Y * 0.8,
                        EmitOptions::default()
                            .count(1)
                            .speed(0.8, 2.0)
                            .spread(1.6)
                            .size_scale(0.55),
                    );
                }
            }
            (MachineKind::Solar, _) if daylight > 0.4 && tick(2.4) => {
                particles.emit(
                    ParticleStyle::Glow,
                    base + Vec3::Y * 0.75,
                    EmitOptions::default()
                        .count(1)
                        .speed(0.2, 0.6)
                        .spread(2.0)
                        .size_scale(0.8),
                );
            }
            (MachineKind::Medbay, MachineState::Medbay(medbay)) if medbay.heal_acc > 0.0 => {
                if tick(0.5) {
                    particles.emit(
                        ParticleStyle::Heal,
                        base + Vec3::Y * 0.9,
                        EmitOptions::default()
                            .count(1)
                            .speed(0.4, 1.2)
                            .spread(1.6)
                            .size_scale(0.7),
                    );
                }
            }
            (MachineKind::Geothermal, _) if tick(1.2) => {
                particles.steam(base + Vec3::Y * 0.8, 0.8);
            }
            (MachineKind::Pump, MachineState::Belt(belt)) if !belt.items.is_empty() => {
                if tick(0.6) {
                    particles.emit(
                        ParticleStyle::Bubble,
                        base + Vec3::Y * 0.5,
                        EmitOptions::default()
                            .count(1)
                            .speed(0.3, 0.9)
                            .spread(1.4)
                            .size_scale(0.6),
                    );
                }
            }
            (MachineKind::ColonyCore, MachineState::Colony(colony))
                if (colony.prog > 0.0 || colony.residents > 0) && tick(1.6) =>
            {
                particles.emit(
                    ParticleStyle::Shield,
                    base + Vec3::Y * 1.2,
                    EmitOptions::default()
                        .count(1)
                        .speed(0.4, 1.2)
                        .spread(2.0)
                        .size_scale(0.8),
                );
            }
            _ => {}
        }
    }
}

/// Assigns the shared point-light pool to the nearest active machines.
pub fn machine_light_system(
    day: Option<Res<crate::daynight::DayTime>>,
    player: Query<&Player>,
    machines: Query<
        (Entity, &Machine, &MachineState, &Transform),
        (With<Machine>, Without<MachineLight>),
    >,
    mut lights: Query<(&MachineLight, &mut Transform, &mut PointLight)>,
) {
    let Ok(player) = player.single() else { return };
    let daylight = day
        .as_deref()
        .map(|day| crate::daynight::day_factor(day.0))
        .unwrap_or(1.0);
    let mut candidates: Vec<(f32, Entity, Vec3, Color, f32)> = Vec::new();
    for (entity, machine, state, transform) in &machines {
        let distance = transform.translation.distance(player.pos);
        if distance > FX_RANGE {
            continue;
        }
        if let Some((color, strength)) = machine_glow(machine.kind, state) {
            // Solar glints only during the day and are dimmer than active
            // producers, so they never dominate the light pool at night.
            let adjusted = if machine.kind == MachineKind::Solar {
                strength * daylight
            } else {
                strength
            };
            if adjusted > 0.05 {
                candidates.push((distance, entity, transform.translation, color, adjusted));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut assigned: Vec<(Vec3, Color, f32)> = candidates
        .into_iter()
        .take(MACHINE_LIGHT_POOL)
        .map(|(_, _, position, color, strength)| (position, color, strength))
        .collect();
    for (_light, mut transform, mut point) in &mut lights {
        if let Some((position, color, strength)) = assigned.first().copied() {
            transform.translation = position + Vec3::Y * 0.95;
            point.color = color;
            point.intensity = 2_600.0 * strength * (1.0 - daylight * 0.55).max(0.25);
            assigned.remove(0);
        } else {
            point.intensity = 0.0;
        }
    }
}

/// Mirrors belt cargo with small textured quads that ride the belt.
#[allow(clippy::too_many_arguments)]
pub fn belt_visual_system(
    mut commands: Commands,
    player: Query<&Player>,
    icons: Res<crate::ui::IconMaterials>,
    machines: Query<
        (Entity, &Machine, &MachineState, &Transform),
        (With<Machine>, Without<BeltItemVisual>),
    >,
    mut fx: ResMut<MachineFx>,
    mut visuals: Query<(&BeltItemVisual, &mut Transform, &mut Visibility)>,
) {
    let Ok(player) = player.single() else { return };
    let mut live_belts: Vec<Entity> = Vec::new();
    for (entity, machine, state, transform) in &machines {
        let items: &[crate::factory::BeltItem] = match state {
            MachineState::Belt(belt) => &belt.items,
            _ => continue,
        };
        if machine.kind != MachineKind::Belt && machine.kind != MachineKind::Pump {
            continue;
        }
        let distance = transform.translation.distance(player.pos);
        if distance > FX_RANGE {
            continue;
        }
        live_belts.push(entity);
        // Direction of travel comes from the machine orientation.
        let direction = crate::factory::DIRS[machine.dir as usize % 4];
        let forward = Vec3::new(direction.0 as f32, 0.0, direction.1 as f32);
        let pool = fx.belt_visuals.entry(entity).or_default();
        // Grow the pool.
        while pool.len() < items.len().min(4) {
            let icon = icons
                .map
                .get(&items[pool.len().min(items.len().saturating_sub(1))].item)
                .cloned()
                .unwrap_or_else(|| icons.fallback.clone());
            let child = commands
                .spawn((
                    Mesh3d(icons.quad.clone()),
                    MeshMaterial3d(icon),
                    Transform::from_translation(transform.translation + Vec3::Y * 0.85)
                        .with_scale(Vec3::splat(0.5)),
                    BeltItemVisual {
                        belt: entity,
                        slot: pool.len(),
                    },
                    crate::InGame,
                ))
                .id();
            pool.push(child);
        }
        // Position existing slots and hide extras.
        for (slot, visual_entity) in pool.iter().enumerate() {
            let Ok((visual, mut visual_transform, mut visibility)) =
                visuals.get_mut(*visual_entity)
            else {
                continue;
            };
            let _ = visual;
            if let Some(item) = items.get(slot) {
                let t = item.t.clamp(0.0, 1.0);
                visual_transform.translation =
                    transform.translation + forward * (t - 0.5) * 0.92 + Vec3::Y * 0.82;
                *visibility = Visibility::Visible;
            } else {
                *visibility = Visibility::Hidden;
            }
        }
    }
    // Recycle pools for belts that went away or are out of range.
    let stale: Vec<Entity> = fx
        .belt_visuals
        .keys()
        .copied()
        .filter(|entity| !live_belts.contains(entity))
        .collect();
    for entity in stale {
        if let Some(pool) = fx.belt_visuals.remove(&entity) {
            for child in pool {
                commands.entity(child).despawn();
            }
        }
    }
}

pub struct MachineFxPlugin;

impl Plugin for MachineFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MachineFx>()
            .add_systems(Startup, setup_machine_fx)
            .add_systems(
                Update,
                (
                    machine_particle_system,
                    machine_light_system,
                    belt_visual_system,
                )
                    .chain()
                    .in_set(crate::schedule::GameSet::LateFactory)
                    .run_if(in_state(crate::schedule::GameState::Playing))
                    .run_if(crate::schedule::ground_mode),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glow_tracks_activity() {
        let idle = MachineState::for_kind(MachineKind::Furnace);
        assert!(machine_glow(MachineKind::Furnace, &idle).is_none());
        let mut furnace = match MachineState::for_kind(MachineKind::Furnace) {
            MachineState::Furnace(state) => state,
            _ => unreachable!(),
        };
        furnace.on = true;
        assert!(machine_glow(MachineKind::Furnace, &MachineState::Furnace(furnace)).is_some());
    }

    #[test]
    fn solar_glints_and_reactor_glows() {
        assert!(machine_glow(MachineKind::Solar, &MachineState::Plain).is_some());
        let mut reactor = match MachineState::for_kind(MachineKind::Reactor) {
            MachineState::Reactor(state) => state,
            _ => unreachable!(),
        };
        reactor.fuel = 30.0;
        assert!(machine_glow(MachineKind::Reactor, &MachineState::Reactor(reactor)).is_some());
    }

    #[test]
    fn battery_brightness_scales_with_charge() {
        let mut battery = match MachineState::for_kind(MachineKind::Battery) {
            MachineState::Battery(state) => state,
            _ => unreachable!(),
        };
        battery.charge = 10.0;
        let low = machine_glow(MachineKind::Battery, &MachineState::Battery(battery))
            .unwrap()
            .1;
        let mut battery = match MachineState::for_kind(MachineKind::Battery) {
            MachineState::Battery(state) => state,
            _ => unreachable!(),
        };
        battery.charge = crate::factory::BATTERY_CAPACITY;
        let high = machine_glow(MachineKind::Battery, &MachineState::Battery(battery))
            .unwrap()
            .1;
        assert!(high > low);
    }

    #[test]
    fn belt_state_is_readable() {
        let entries = match MachineState::for_kind(MachineKind::Belt) {
            MachineState::Belt(belt) => belt,
            _ => unreachable!(),
        };
        assert!(entries.items.is_empty());
    }
}
