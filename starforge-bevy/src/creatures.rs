//! Creatures (simple wandering voxel animals) and dropped-item entities.

use crate::data;
use crate::inventory::Slot;
use crate::player::Player;
use crate::rng::Rng;
use crate::ui::IconMaterials;
use crate::world::World;
use bevy::prelude::*;

// ---------- Creatures ----------

#[derive(Component)]
pub struct Creature {
    pub hp: f32,
    pub radius: f32,
    pub height: f32,
    pub shoot_t: f32,
    pub ai_t: f32,
    pub dir: Vec3,
    pub vel: Vec3,
    pub grounded: bool,
    pub home: Vec3,
    pub jump_t: f32,
    pub kind: &'static str,
}

impl Creature {
    pub fn new(home: Vec3) -> Self {
        Self {
            hp: 3.0,
            radius: 0.5,
            height: 1.0,
            shoot_t: 0.0,
            ai_t: 0.0,
            dir: Vec3::X,
            vel: Vec3::ZERO,
            grounded: false,
            home,
            jump_t: 0.0,
            kind: "strider",
        }
    }
}

#[derive(Resource)]
pub struct CreatureSpawner {
    pub timer: f32,
}

impl Default for CreatureSpawner {
    fn default() -> Self {
        Self { timer: 0.0 }
    }
}

/// Spawn a creature at a ground position.
pub fn spawn_creature(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &World,
    pos: Vec3,
    body: u32,
    legs: u32,
    eye: u32,
    kind: &'static str,
) {
    let body_c = |c: u32| {
        Color::srgb(
            ((c >> 16) & 0xFF) as f32 / 255.0,
            ((c >> 8) & 0xFF) as f32 / 255.0,
            (c & 0xFF) as f32 / 255.0,
        )
    };
    let body_mat = materials.add(StandardMaterial {
        base_color: body_c(body),
        perceptual_roughness: 1.0,
        ..default()
    });
    let legs_mat = materials.add(StandardMaterial {
        base_color: body_c(legs),
        perceptual_roughness: 1.0,
        ..default()
    });
    let eye_mat = materials.add(StandardMaterial {
        base_color: body_c(eye),
        emissive: LinearRgba::new(
            ((eye >> 16) & 0xFF) as f32 / 255.0,
            ((eye >> 8) & 0xFF) as f32 / 255.0,
            (eye & 0xFF) as f32 / 255.0,
            1.0,
        ) * 0.6,
        perceptual_roughness: 1.0,
        ..default()
    });
    let (w, h, d) = match kind {
        "crab" => (0.55, 0.4, 0.7),
        "blob" => (0.7, 0.5, 0.7),
        _ => (0.35, 1.1, 0.35),
    };
    let body_mesh = meshes.add(Cuboid::new(w, h, d));
    let head_mesh = meshes.add(Cuboid::new(w * 0.6, h * 0.28, d * 0.6));
    let eye_mesh = meshes.add(Cuboid::new(w * 0.12, h * 0.08, 0.03));
    let body_e = commands
        .spawn((
            Mesh3d(body_mesh),
            MeshMaterial3d(body_mat.clone()),
            Transform::from_translation(pos),
            Visibility::default(),
            Creature {
                hp: 3.0,
                radius: 0.5,
                height: h + 0.3,
                shoot_t: 0.0,
                ai_t: 0.0,
                dir: Vec3::X,
                vel: Vec3::ZERO,
                grounded: false,
                home: pos,
                jump_t: 0.0,
                kind,
            },
        ))
        .id();
    commands.entity(body_e).with_children(|parent| {
        parent.spawn((
            Mesh3d(head_mesh),
            MeshMaterial3d(body_mat),
            Transform::from_translation(Vec3::Y * (h * 0.5 + h * 0.14)),
        ));
        parent.spawn((
            Mesh3d(eye_mesh.clone()),
            MeshMaterial3d(eye_mat.clone()),
            Transform::from_translation(Vec3::new(w * 0.25, h * 0.5 + h * 0.14, d * 0.31)),
        ));
        parent.spawn((
            Mesh3d(eye_mesh),
            MeshMaterial3d(eye_mat),
            Transform::from_translation(Vec3::new(-w * 0.25, h * 0.5 + h * 0.14, d * 0.31)),
        ));
    });
    let _ = legs_mat;
}

/// Maintain the biome's animal population around the player.
#[allow(clippy::too_many_arguments)]
pub fn creature_spawn_system(
    time: Res<Time>,
    mut spawner: ResMut<CreatureSpawner>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    creatures: Query<&Creature>,
    world: Res<World>,
    player: Query<&Player>,
) {
    spawner.timer -= time.delta_secs();
    if spawner.timer > 0.0 {
        return;
    }
    spawner.timer = 1.5;
    let Ok(p) = player.single() else { return };
    let Some(animal) = world.biome().animal else { return };
    let count = creatures.iter().count();
    if count >= animal.4 as usize {
        return;
    }
    // random position within 25 blocks of player
    let mut rng = Rng::new(
        (p.pos.x as u32)
            .wrapping_mul(7919)
            .wrapping_add(time.elapsed_secs() as u32),
    );
    for _ in 0..8 {
        let dx = rng.range_f(-25.0, 25.0);
        let dz = rng.range_f(-25.0, 25.0);
        let x = (p.pos.x + dx).floor() as i32;
        let z = (p.pos.z + dz).floor() as i32;
        if dx * dx + dz * dz < 9.0 {
            continue;
        }
        let top = world.top_at(x, z);
        if top <= data::SEA {
            continue;
        }
        let kind = if count % 3 == 0 {
            "crab"
        } else if count % 3 == 1 {
            "blob"
        } else {
            "strider"
        };
        spawn_creature(&mut commands, &mut meshes, &mut materials, &*world,
            Vec3::new(x as f32 + 0.5, top as f32 + 1.2, z as f32 + 0.5),
            animal.1,
            animal.2,
            animal.3,
            kind,
        );
        break;
    }
}

/// Creature AI: wander around home, hop, stay on terrain.
pub fn creature_system(
    time: Res<Time>,
    mut q: Query<(&mut Creature, &mut Transform)>,
    world: Res<World>,
    player: Query<&Player>,
) {
    let dt = time.delta_secs();
    let Ok(p) = player.single() else { return };
    for (mut c, mut tf) in &mut q {
        if c.hp <= 0.0 {
            continue;
        }
        // death marker handled by despawn system
        c.ai_t -= dt;
        if c.ai_t <= 0.0 {
            c.ai_t = 2.0 + (c.home.x * 0.001).fract() * 3.0;
            let mut rng = Rng::new((tf.translation.x as u32).wrapping_mul(31) + time.elapsed_secs() as u32);
            let a = rng.next() * std::f32::consts::TAU;
            c.dir = Vec3::new(a.cos(), 0.0, a.sin());
            c.vel = c.dir * (if c.kind == "blob" { 0.35 } else if c.kind == "crab" { 0.7 } else { 1.8 });
            if c.kind == "strider" && rng.next() < 0.4 {
                c.vel.y = 5.0;
            }
        }
        // flee if player very close? keep simple: wander
        let mut pos = tf.translation;
        pos += c.vel * dt;
        // home range
        if (pos - c.home).xz().length() > 14.0 {
            c.dir = (c.home - pos).normalize_or_zero();
            c.vel = c.dir * 1.5;
        }
        if !c.grounded {
            c.vel.y -= 22.0 * dt;
        }
        pos += Vec3::Y * c.vel.y * dt;
        let ground = world.top_at(pos.x.floor() as i32, pos.z.floor() as i32);
        let floor_y = ground as f32 + 1.0;
        if pos.y <= floor_y + 0.01 {
            pos.y = floor_y + 0.01;
            c.vel.y = 0.0;
            c.grounded = true;
        } else {
            c.grounded = false;
        }
        // avoid walking into player
        if (pos - p.pos).xz().length() < 1.0 && (pos.y - p.pos.y).abs() < 2.0 {
            pos -= c.dir * dt * 2.0;
        }
        tf.translation = pos;
        // face movement direction + idle bob
        if c.vel.xz().length_squared() > 0.01 {
            let yaw = c.vel.x.atan2(c.vel.z);
            tf.rotation = Quat::from_rotation_y(yaw);
        }
        tf.scale = Vec3::splat(1.0 + (time.elapsed_secs() * 3.0).sin() * 0.03);
        // despawn if dead or too far
        let dist = (pos - p.pos).length();
        if dist > 120.0 {
            // mark for despawn via hp
            c.hp = -1.0;
        }
    }
}

/// Despawn dead creatures.
pub fn creature_despawn_system(
    creatures: Query<(Entity, &Creature, &Transform)>,
    mut commands: Commands,
) {
    for (e, c, _) in &creatures {
        if c.hp <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

// ---------- Dropped items ----------

#[derive(Component)]
pub struct DropItem {
    pub item: String,
    pub n: i32,
    pub age: f32,
    pub vel: Vec3,
    pub pick_delay: f32,
    pub base_y: f32,
    pub resting: bool,
    pub no_space_t: f32,
}

pub const DROP_CAP: usize = 90;

/// Spawn a dropped item.
pub fn spawn_drop(
    commands: &mut Commands,
    world: &World,
    icon_materials: &IconMaterials,
    pos: Vec3,
    vel: Vec3,
    item: String,
    n: i32,
    pick_delay: f32,
) {
    let mat = icon_materials
        .map
        .get(&item)
        .cloned()
        .unwrap_or_else(|| icon_materials.fallback.clone());
    let ground = world.top_at(pos.x.floor() as i32, pos.z.floor() as i32);
    commands.spawn((
        Mesh3d(icon_materials.quad.clone()),
        MeshMaterial3d(mat),
        Transform::from_translation(pos),
        Visibility::default(),
        DropItem {
            item,
            n,
            age: 0.0,
            vel,
            pick_delay,
            base_y: ground as f32 + 1.0 + 0.3,
            resting: false,
            no_space_t: 0.0,
        },
    ));
}

/// Drop physics: gravity, landing, magnet pickup, despawn.
pub fn drops_system(
    time: Res<Time>,
    mut commands: Commands,
    mut drops: Query<(Entity, &mut DropItem, &mut Transform)>,
    mut player: Query<&mut Player>,
    world: Res<World>,
    sfx: Res<crate::audio::Sfx>,
) {
    let dt = time.delta_secs();
    let Ok(mut p) = player.single_mut() else { return };
    let player_chest = p.pos - Vec3::Y * 1.0;
    let mut pickup_sound = false;
    for (e, mut d, mut tf) in &mut drops {
        d.age += dt;
        if d.age > 240.0 {
            commands.entity(e).despawn();
            continue;
        }
        if d.vel.length_squared() > 0.0001 {
            d.vel.y -= 16.0 * dt;
            let mut np = tf.translation + d.vel * dt;
            // land
            let below = data::block_by_id(world.get(
                np.x.floor() as i32,
                (np.y - 0.28).floor() as i32,
                np.z.floor() as i32,
            ));
            if below.solid {
                let fy = (np.y - 0.28).floor() as f32;
                d.base_y = fy + 1.0 + 0.3;
                np.y = d.base_y;
                d.vel = Vec3::ZERO;
                d.resting = true;
            }
            if np.y < -8.0 {
                let top = world.top_at(np.x.floor() as i32, np.z.floor() as i32);
                np.y = top as f32 + 0.4;
                d.vel = Vec3::ZERO;
                d.resting = true;
                d.base_y = np.y;
            }
            tf.translation = np;
        }
        if d.resting {
            // re-fall if support removed
            let below = world.get(
                tf.translation.x.floor() as i32,
                (d.base_y - 0.4).floor() as i32,
                tf.translation.z.floor() as i32,
            );
            if !data::block_by_id(below).solid {
                d.vel.y = -0.5;
                d.resting = false;
            } else {
                let bob = d.base_y + (d.age * 2.2).sin() * 0.06 + 0.06;
                tf.translation.y = bob;
            }
        }
        // spin + billboard toward player
        tf.rotate_y(dt * 1.6);
        // pickup
        if d.age > d.pick_delay && d.no_space_t <= 0.0 {
            let dist = tf.translation.distance(player_chest);
            if dist < 6.5 {
                let room = p.inv.room_for(&d.item);
                if room <= 0 {
                    d.no_space_t = 1.5;
                } else if dist > 1.05 {
                    let dir = (player_chest - tf.translation).normalize();
                    let spd = (8.0 + (6.5 - dist) * 4.0).min(26.0);
                    tf.translation += dir * spd * dt;
                    d.resting = false;
                } else {
                    let take = d.n.min(room);
                    let added = p.inv.add_item(&d.item, take);
                    d.n -= added;
                    if d.n <= 0 {
                        commands.entity(e).despawn();
                        pickup_sound = true;
                        continue;
                    }
                }
            }
        } else if d.no_space_t > 0.0 {
            d.no_space_t -= dt;
        }
    }
    if pickup_sound {
        crate::audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
    }
    let _ = &mut p;
}

/// Empty helper for callers that need a Slot (keeps API symmetry).
pub fn slot(item: &str, n: i32) -> Slot {
    Slot {
        item: item.to_string(),
        n,
    }
}
