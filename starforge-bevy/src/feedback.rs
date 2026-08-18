//! Shared audiovisual feedback: block-break shards, flashes and lightweight impact cues.
//! Keeping these effects separate from world mutation makes them safe to spawn from
//! mining, combat and creature systems without coupling gameplay state to rendering.

use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct FeedbackAssets {
    pub shard_mesh: Option<Handle<Mesh>>,
    pub flash_mesh: Option<Handle<Mesh>>,
    pub shard_materials: Vec<Option<Handle<StandardMaterial>>>,
    pub flash_materials: Vec<Option<Handle<StandardMaterial>>>,
}

#[derive(Component)]
pub struct BreakShard {
    pub velocity: Vec3,
    pub life: f32,
    pub spin: Vec3,
}

#[derive(Component)]
pub struct BreakFlash {
    pub life: f32,
    pub max_life: f32,
}

/// Spawn a compact burst whose palette follows the broken block family.
pub fn spawn_block_burst(
    commands: &mut Commands,
    cache: &mut FeedbackAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    pos: Vec3,
    block_id: u8,
    seed: u32,
) {
    let key = crate::data::block_by_id(block_id).key;
    let (palette, color) = match key {
        "water" | "ice" | "glass" => (0, Color::srgb(0.28, 0.78, 1.0)),
        "leaves" | "fern" | "sodium_plant" | "oxygen_plant" => (1, Color::srgb(0.35, 0.9, 0.42)),
        "crystal" | "amber" | "glow_shroom" => (2, Color::srgb(0.55, 0.92, 1.0)),
        "metal" | "iron_ore" | "titanium_ore" | "rust" => (3, Color::srgb(0.65, 0.72, 0.78)),
        "sand" | "snow" | "salt" => (4, Color::srgb(0.9, 0.82, 0.58)),
        _ => (5, Color::srgb(0.64, 0.55, 0.43)),
    };
    if cache.shard_mesh.is_none() {
        cache.shard_mesh = Some(meshes.add(Cuboid::new(0.12, 0.12, 0.12)));
    }
    if cache.flash_mesh.is_none() {
        cache.flash_mesh = Some(meshes.add(Cuboid::new(1.02, 1.02, 1.02)));
    }
    if cache.shard_materials.len() < 6 {
        cache.shard_materials.resize_with(6, || None);
    }
    if cache.flash_materials.len() < 6 {
        cache.flash_materials.resize_with(6, || None);
    }
    if cache.shard_materials[palette].is_none() {
        cache.shard_materials[palette] = Some(materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.12,
            ..default()
        }));
    }
    if cache.flash_materials[palette].is_none() {
        let rgba = color.to_srgba();
        cache.flash_materials[palette] = Some(materials.add(StandardMaterial {
            base_color: Color::srgba(rgba.red, rgba.green, rgba.blue, 0.24),
            emissive: color.to_linear() * 0.55,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        }));
    }
    let shard_mesh = cache.shard_mesh.clone().expect("shard mesh initialized");
    let flash_mesh = cache.flash_mesh.clone().expect("flash mesh initialized");
    let shard_material = cache.shard_materials[palette]
        .clone()
        .expect("shard material initialized");
    let flash_material = cache.flash_materials[palette]
        .clone()
        .expect("flash material initialized");
    commands.spawn((
        Mesh3d(flash_mesh),
        MeshMaterial3d(flash_material),
        Transform::from_translation(pos),
        BreakFlash {
            life: 0.18,
            max_life: 0.18,
        },
        crate::InGame,
    ));
    let mut rng = crate::rng::Rng::new(seed ^ (block_id as u32).wrapping_mul(0x9E37_79B9));
    for _ in 0..10 {
        let velocity = Vec3::new(
            (rng.next() - 0.5) * 4.8,
            1.4 + rng.next() * 4.2,
            (rng.next() - 0.5) * 4.8,
        );
        commands.spawn((
            Mesh3d(shard_mesh.clone()),
            MeshMaterial3d(shard_material.clone()),
            Transform::from_translation(pos).with_rotation(Quat::from_euler(
                EulerRot::XYZ,
                rng.next() * std::f32::consts::PI,
                rng.next() * std::f32::consts::PI,
                rng.next() * std::f32::consts::PI,
            )),
            BreakShard {
                velocity,
                life: 0.42 + rng.next() * 0.28,
                spin: Vec3::splat(3.0 + rng.next() * 6.0),
            },
            crate::InGame,
        ));
    }
}

pub fn particle_system(
    time: Res<Time>,
    mut commands: Commands,
    mut shards: Query<(Entity, &mut BreakShard, &mut Transform), Without<BreakFlash>>,
    mut flashes: Query<(Entity, &mut BreakFlash, &mut Transform), Without<BreakShard>>,
) {
    let dt = time.delta_secs();
    for (entity, mut shard, mut transform) in &mut shards {
        shard.life -= dt;
        if shard.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        shard.velocity.y -= 12.0 * dt;
        transform.translation += shard.velocity * dt;
        transform.rotate_x(shard.spin.x * dt);
        transform.rotate_y(shard.spin.y * dt);
        transform.rotate_z(shard.spin.z * dt);
    }
    for (entity, mut flash, mut transform) in &mut flashes {
        flash.life -= dt;
        if flash.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let k = (flash.life / flash.max_life).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(1.0 + (1.0 - k) * 0.18);
    }
}
