//! Terrain materials — native `StandardMaterial` instances (no custom WGSL).
//!
//! The old port used an `ExtendedMaterial` with hand-written WGSL to apply
//! planet curvature, water waves, scan pulses and edge fades on the GPU. This
//! native version uses only Bevy's built-in PBR pipeline: face shading and
//! glow-block brightness are carried per-vertex through the mesh `COLOR`
//! attribute (which Bevy's `StandardMaterial` multiplies into the base color
//! automatically when the mesh layout contains it), and the remaining effects
//! (far-mesh fade ring, altitude haze) are handled by CPU vertex alpha and
//! Bevy's native `DistanceFog` respectively.

use bevy::image::{ImageFilterMode, ImageSampler};
use bevy::pbr::StandardMaterial;
use bevy::prelude::*;

/// Shared terrain material handles (one instance per world so the atlas
/// upload is shared by every chunk mesh).
#[derive(Resource, Clone)]
pub struct TerrainMaterials {
    pub solid: Handle<StandardMaterial>,
    pub water: Handle<StandardMaterial>,
    /// 远景模拟地形（顶点色直接作为地表色，无图集纹理——JS farMesh 同口径）。
    /// Vertex alpha is rewritten on the CPU every frame for the far-hole ring.
    pub far: Handle<StandardMaterial>,
    /// Opaque, fully rough material for hierarchical LOD sections. Keeping it
    /// separate from `far` avoids transparent-section sorting seams.
    pub lod: Handle<StandardMaterial>,
    pub atlas_image: Handle<Image>,
}

impl TerrainMaterials {
    pub fn build(
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        atlas_bytes: Vec<u8>,
        water_tint: u32,
    ) -> Self {
        let mut image = Image::new(
            bevy::render::render_resource::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            atlas_bytes,
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        let mut sampler = ImageSampler::default();
        let d = sampler.get_or_init_descriptor();
        d.mag_filter = ImageFilterMode::Nearest;
        d.min_filter = ImageFilterMode::Nearest;
        d.mipmap_filter = ImageFilterMode::Nearest;
        image.sampler = sampler;
        let atlas_image = images.add(image);

        let solid = materials.add(StandardMaterial {
            base_color_texture: Some(atlas_image.clone()),
            double_sided: true,
            cull_mode: None,
            // Leaves and cross-shaped plants share this mesh/material and
            // contain transparent atlas texels. Keep alpha cutout so the
            // visible silhouette and its shadow use the same coverage.
            alpha_mode: AlphaMode::Mask(0.4),
            ..default()
        });
        let (tr, tg, tb) = (
            ((water_tint >> 16) & 0xFF) as f32 / 255.0,
            ((water_tint >> 8) & 0xFF) as f32 / 255.0,
            (water_tint & 0xFF) as f32 / 255.0,
        );
        let water = materials.add(StandardMaterial {
            // The tint is applied per-vertex in the water mesh (COLOR attribute,
            // alpha 0.72); the material color is just a fallback.
            base_color: Color::srgba(tr, tg, tb, 0.72),
            base_color_texture: Some(atlas_image.clone()),
            double_sided: true,
            cull_mode: None,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.18,
            metallic: 0.12,
            ..default()
        });
        let far = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            double_sided: true,
            cull_mode: None,
            // 半透明：RGB 来自顶点色（地表色），alpha 由 CPU 每帧写入挖空环
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let lod = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            double_sided: true,
            cull_mode: None,
            perceptual_roughness: 1.0,
            reflectance: 0.1,
            ..default()
        });
        Self {
            solid,
            water,
            far,
            lod,
            atlas_image,
        }
    }
}

/// Lamp pool: 6 point lights that follow the nearest glow blocks (lamp/crystal/glow_shroom).
#[derive(Resource)]
pub struct LampPool {
    pub entities: Vec<Entity>,
    pub timer: f32,
}

pub fn lamp_pool_system(
    time: Res<Time>,
    mut pool: ResMut<LampPool>,
    mut lights: Query<(&mut PointLight, &mut Transform)>,
    world: Res<crate::world::World>,
    player: Query<&crate::player::Player>,
) {
    pool.timer -= time.delta_secs();
    if pool.timer > 0.0 {
        return;
    }
    pool.timer = 0.5;
    let Ok(p) = player.single() else { return };
    let mut found: Vec<(f32, [i32; 3], u8)> = Vec::new();
    for chunk in world.chunks.values() {
        let bx = chunk.cx * crate::data::CHUNK;
        let bz = chunk.cz * crate::data::CHUNK;
        // quick reject
        let dx = (bx as f32 - p.pos.x).abs();
        let dz = (bz as f32 - p.pos.z).abs();
        if dx > 80.0 || dz > 80.0 {
            continue;
        }
        for y in 0..crate::data::WORLD_H {
            for lz in 0..crate::data::CHUNK {
                for lx in 0..crate::data::CHUNK {
                    let id = chunk.data[crate::world::lidx(lx, y, lz)];
                    let key = crate::data::block_by_id(id).key;
                    if matches!(key, "lamp" | "crystal" | "glow_shroom") {
                        let x = bx + lx;
                        let z = bz + lz;
                        let d2 = (x as f32 - p.pos.x).powi(2)
                            + (y as f32 - p.pos.y).powi(2)
                            + (z as f32 - p.pos.z).powi(2);
                        if d2 < 3600.0 {
                            found.push((d2, [x, y, z], id));
                        }
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let glow_color = |key: &str| match key {
        "crystal" => (0x7f as f32, 0xe8 as f32, 0xe0 as f32),
        "glow_shroom" => (0x4e as f32, 0xe8 as f32, 0xb8 as f32),
        _ => (0xff as f32, 0xd9 as f32, 0xa0 as f32),
    };
    for (i, e) in pool.entities.iter().enumerate() {
        let Ok((mut l, mut tf)) = lights.get_mut(*e) else {
            continue;
        };
        if let Some((_, cell, id)) = found.get(i) {
            // JS: l.position.set(x+0.5, y+0.9, z+0.5) —— 光必须跟随灯块
            tf.translation = Vec3::new(
                cell[0] as f32 + 0.5,
                cell[1] as f32 + 0.9,
                cell[2] as f32 + 0.5,
            );
            let (r, g, b) = glow_color(crate::data::block_by_id(*id).key);
            l.color = Color::srgb(r / 255.0, g / 255.0, b / 255.0);
            l.intensity = 220.0;
            l.range = 11.0;
        } else {
            l.intensity = 0.0;
        }
    }
}

/// Terrain materials & lamp pool plugin: runtime light-follow system only;
/// `TerrainMaterials` is rebuilt per world (biome water tint) by the game flow.
pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            lamp_pool_system
                .in_set(crate::schedule::GameSet::CommonLamp)
                .run_if(in_state(crate::schedule::GameState::Playing)),
        );
    }
}
