//! Creatures (simple wandering voxel animals) and dropped-item entities.

use crate::data;
use crate::inventory::Slot;
use crate::player::Player;
use crate::rng::Rng;
use crate::ui::IconMaterials;
use crate::world::World;
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy_world_serialization::prelude::WorldAssetRoot;

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

/// Spawn a creature at a ground position（CC0 GLB 模型：crab/blob/strider）。
/// 缩放按模型实测包围盒（strider 135.4 / crab 19.4 / blob 0.03 单位高）换算，
/// 目标尺寸对照原版：strider 高 1.1、crab 高 0.4、blob 高 0.5 格；
/// y 偏移把模型脚底对齐地面（这些模型原点不在脚底）。
pub fn spawn_creature(
    commands: &mut Commands,
    asset_server: &AssetServer,
    world: &World,
    pos: Vec3,
    body: u32,
    legs: u32,
    eye: u32,
    kind: &'static str,
) {
    let _ = (body, legs, eye);
    // (model, scale, y_offset 脚底对齐)
    let (model, scale, y_off) = match kind {
        "crab" => ("models/creatures/crab.glb", 0.4 / 19.42, 8.64 * (0.4 / 19.42)),
        "blob" => ("models/creatures/blob.glb", 0.5 / 0.03, 0.02 * (0.5 / 0.03)),
        _ => ("models/creatures/strider.glb", 1.1 / 135.37, 67.52 * (1.1 / 135.37)),
    };
    let (w, h, d) = match kind {
        "crab" => (0.55, 0.4, 0.7),
        "blob" => (0.7, 0.5, 0.7),
        _ => (0.35, 1.1, 0.35),
    };
    commands.spawn((
        WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(model))),
        Transform::from_translation(pos + Vec3::Y * y_off).with_scale(Vec3::splat(scale)),
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
    ));
}

/// Maintain the biome's animal population around the player.
#[allow(clippy::too_many_arguments)]
pub fn creature_spawn_system(
    time: Res<Time>,
    mut spawner: ResMut<CreatureSpawner>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
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
        // 生态动物类型（JS BIOMES[].animal.type），不再按 count%3 轮换
        let kind = data::biome_animal_kind(world.biome().key);
        spawn_creature(
            &mut commands,
            &asset_server,
            &*world,
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
        // 守卫由 sentinel_system 单独驱动
        if c.kind == "sentinel" {
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

/// Despawn dead creatures (+ 掉落：默认碳 1-2；守卫掉电路板+装甲板)。
pub fn creature_despawn_system(
    creatures: Query<(Entity, &Creature, &Transform)>,
    mut commands: Commands,
    world: Res<World>,
    icons: Res<crate::ui::IconMaterials>,
    sfx: Res<crate::audio::Sfx>,
) {
    for (e, c, tf) in &creatures {
        if c.hp <= 0.0 {
            let mut rng = crate::rng::Rng::new(
                (tf.translation.x as u32).wrapping_mul(31)
                    ^ (tf.translation.z as u32).wrapping_mul(57),
            );
            if c.kind == "sentinel" {
                // 遗迹守卫（JS）：电路板×1 + 装甲板×1(50%)
                spawn_drop(
                    &mut commands,
                    &world,
                    &icons,
                    tf.translation + Vec3::Y * 0.4,
                    Vec3::new(0.0, 2.2, 0.0),
                    "circuit".into(),
                    1,
                    0.4,
                );
                if rng.next() < 0.5 {
                    spawn_drop(
                        &mut commands,
                        &world,
                        &icons,
                        tf.translation + Vec3::Y * 0.8,
                        Vec3::new(0.0, 2.2, 0.0),
                        "plate".into(),
                        1,
                        0.4,
                    );
                }
                crate::audio::play(&mut commands, sfx.break_block.clone(), 0.7, None);
            } else {
                let n = 1 + (rng.next() * 2.0) as i32;
                spawn_drop(
                    &mut commands,
                    &world,
                    &icons,
                    tf.translation + Vec3::Y * 0.4,
                    Vec3::new(0.0, 2.2, 0.0),
                    "carbon".into(),
                    n,
                    0.4,
                );
                crate::audio::play(&mut commands, sfx.break_block.clone(), 0.5, None);
            }
            commands.entity(e).despawn();
        }
    }
}

/// 遗迹守卫生成计时。
#[derive(Resource, Default)]
pub struct SentinelSpawner {
    pub timer: f32,
}

/// 遗迹守卫（JS sentinel）：遗迹附近生成；16 格内追击玩家、接触伤害 2（1.15s CD）；
/// 远离 40 格后消失。生成在 world.g.structures 的 Ruin 处。
#[allow(clippy::too_many_arguments)]
pub fn sentinel_system(
    time: Res<Time>,
    mut spawner: ResMut<SentinelSpawner>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    world: Res<World>,
    mut player: Query<&mut Player>,
    mut creatures: Query<(Entity, &mut Creature, &mut Transform)>,
    mut dmg_cd: Local<f32>,
) {
    let dt = time.delta_secs();
    let ppos = player.single().map(|p| p.pos).unwrap_or(Vec3::ZERO);
    // 生成
    spawner.timer -= dt;
    if spawner.timer <= 0.0 {
        spawner.timer = 2.0;
        let mut nearest: Option<([i32; 3], f32)> = None;
        for s in &world.g.structures {
            if let crate::world::Structure::Ruin { x, z, .. } = s {
                let dx = ppos.x - *x as f32;
                let dz = ppos.z - *z as f32;
                let d = (dx * dx + dz * dz).sqrt();
                if d < 40.0 && nearest.map(|(_, bd)| d < bd).unwrap_or(true) {
                    nearest = Some(([*x, 0, *z], d));
                }
            }
        }
        if let Some((cell, _)) = nearest {
            let has = creatures.iter().any(|(_, c, tf)| {
                c.kind == "sentinel"
                    && tf
                        .translation
                        .distance(Vec3::new(cell[0] as f32, 0.0, cell[2] as f32))
                        < 30.0
            });
            if !has {
                let x = cell[0];
                let z = cell[2];
                let top = world.top_at(x, z);
                commands.spawn((
                    WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(
                        "models/creatures/sentinel.glb",
                    ))),
                    // 骷髅实测高 2.17，目标 1.9 格（脚底即模型原点，无需偏移）
                    Transform::from_translation(Vec3::new(
                        x as f32 + 0.5,
                        top as f32 + 1.0,
                        z as f32 + 0.5,
                    ))
                    .with_scale(Vec3::splat(1.9 / 2.17)),
                    Creature {
                        hp: 10.0,
                        radius: 0.6,
                        height: 1.8,
                        shoot_t: 0.0,
                        ai_t: 0.0,
                        dir: Vec3::X,
                        vel: Vec3::ZERO,
                        grounded: true,
                        home: Vec3::new(x as f32 + 0.5, top as f32 + 1.0, z as f32 + 0.5),
                        jump_t: 0.0,
                        kind: "sentinel",
                    },
                    crate::InGame,
                ));
            }
        }
    }
    // 守卫 AI：追击 + 接触伤害 + 远离消失
    for (_e, mut c, mut tf) in &mut creatures {
        if c.kind != "sentinel" {
            continue;
        }
        let dist = tf.translation.distance(ppos);
        if dist < 16.0 {
            let dir = (ppos - tf.translation).normalize_or_zero();
            tf.translation += dir * 4.7 * dt; // speed 1.8 × 2.6 追击
            let yaw = dir.x.atan2(dir.z);
            tf.rotation = Quat::from_rotation_y(yaw);
            if dist < 1.9 {
                *dmg_cd -= dt;
                if *dmg_cd <= 0.0 {
                    *dmg_cd = 1.15;
                    if let Ok(mut pp) = player.single_mut() {
                        pp.damage(2.0);
                    }
                }
            }        } else if dist > 40.0 {
            c.hp = -1.0; // 标记消失
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

/// Drop physics: gravity, landing, magnet pickup, despawn, merge & cap.
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
    let all: Vec<Entity> = drops.iter().map(|(e, _, _)| e).collect();
    let mut snap: Vec<(Entity, DropItem, Vec3)> = Vec::new();
    for e in &all {
        if let Ok((_, d, tf)) = drops.get(*e) {
            snap.push((
                *e,
                DropItem {
                    item: d.item.clone(),
                    n: d.n,
                    age: d.age,
                    vel: d.vel,
                    pick_delay: d.pick_delay,
                    base_y: d.base_y,
                    resting: d.resting,
                    no_space_t: d.no_space_t,
                },
                tf.translation,
            ));
        }
    }
    // 同类合并（JS: dist²<1.2 合并，n 相加、age 重置）
    let mut merged: Vec<usize> = Vec::new();
    for i in 0..snap.len() {
        if merged.contains(&i) {
            continue;
        }
        for j in (i + 1)..snap.len() {
            if merged.contains(&j) {
                continue;
            }
            let (_, di, pi) = &snap[i];
            let (_, dj, pj) = &snap[j];
            if di.item == dj.item && di.pick_delay <= 0.0 && dj.pick_delay <= 0.0 {
                let d2 = (pi.x - pj.x).powi(2) + (pi.y - pj.y).powi(2) + (pi.z - pj.z).powi(2);
                if d2 < 1.44 {
                    snap[i].1.n += dj.n;
                    snap[i].1.age = 0.0;
                    commands.entity(snap[j].0).despawn();
                    merged.push(j);
                }
            }
        }
    }
    // 掉落上限（JS DROP_CAP 90：超限最旧入包）
    if snap.len() > DROP_CAP {
        let mut order: Vec<usize> = (0..snap.len()).collect();
        order.sort_by(|a, b| {
            snap[*b]
                .1
                .age
                .partial_cmp(&snap[*a].1.age)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &idx in order.iter().take(snap.len() - DROP_CAP) {
            let (e, d, _) = &snap[idx];
            let added = p.inv.add_item(&d.item, d.n);
            if added >= d.n {
                commands.entity(*e).despawn();
            } else if added > 0 {
                if let Ok((_, mut dd, _)) = drops.get_mut(*e) {
                    dd.n -= added;
                }
            }
        }
    }
    // 合并结果写回实体（幸存者数量/年龄）
    for (e, d, _) in &snap {
        if let Ok((_, mut dd, _)) = drops.get_mut(*e) {
            dd.n = d.n;
            dd.age = d.age;
        }
    }
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
