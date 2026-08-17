//! Custom terrain material: StandardMaterial + vertex displacement extension
//! (planet curvature, water waves) + glow/scan-pulse fragment pass.

use bevy::image::{ImageFilterMode, ImageSampler};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, StandardMaterial};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// Uniform block for the terrain extension (binding slot 100).
#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct CurveUniform {
    pub center: Vec2,   // curvature center x/z (player)
    pub amt: f32,       // 0..1 curvature amount
    pub grow: f32,      // vertical squash/stretch about SEA_Y
    pub wave_time: f32, // water wave time
    pub wave_on: f32,   // 1.0 enables water waves
    pub fade: f32,      // global alpha fade
    pub edge_r: f32,    // radial edge fade radius (blocks)
    pub pad: f32,
    pub scan_r: f32,  // scan pulse radius
    pub scan_cx: f32,  // scan center x
    pub scan_cz: f32,  // scan center z
    pub scan_a: f32,   // scan alpha
    // 远景挖空环（far_hole_on=1 时在片元着色器里按到 far_hole_cx/cz 的距离淡出，
    // 替代旧实现每帧在 CPU 上改写 129×129 顶点 alpha——JS farMesh 用 shader uniform 同口径）
    pub far_hole_on: f32,
    pub far_hole_r0: f32,
    pub far_hole_r1: f32,
    pub far_hole_cx: f32,
    pub far_hole_cz: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct TerrainExtension {
    #[uniform(100)]
    pub curve: CurveUniform,
}

impl MaterialExtension for TerrainExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/terrain_vertex.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain_fragment.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/terrain_prepass_vertex.wgsl".into()
    }
}

pub type TerrainMat = ExtendedMaterial<StandardMaterial, TerrainExtension>;

/// Shared material handles (one instance per world so the uniform is a single upload).
#[derive(Resource, Clone)]
pub struct TerrainMaterials {
    pub solid: Handle<TerrainMat>,
    pub water: Handle<TerrainMat>,
    /// 远景模拟地形（顶点色直接作为地表色，无图集纹理——JS farMesh 同口径）
    pub far: Handle<TerrainMat>,
    pub atlas_image: Handle<Image>,
}

impl TerrainMaterials {
    pub fn build(
        materials: &mut Assets<TerrainMat>,
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

        let solid = materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color_texture: Some(atlas_image.clone()),
                double_sided: true,
                cull_mode: None,
                alpha_mode: AlphaMode::Mask(0.4),
                ..default()
            },
            extension: TerrainExtension {
                curve: CurveUniform {
                    amt: 0.0,
                    grow: 1.0,
                    fade: 1.0,
                    edge_r: 9999.0,
                    wave_on: 0.0,
                    ..default()
                },
            },
        });
        let (tr, tg, tb) = (
            ((water_tint >> 16) & 0xFF) as f32 / 255.0,
            ((water_tint >> 8) & 0xFF) as f32 / 255.0,
            (water_tint & 0xFF) as f32 / 255.0,
        );
        let water = materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color: Color::srgba(tr, tg, tb, 0.72),
                base_color_texture: Some(atlas_image.clone()),
                double_sided: true,
                cull_mode: None,
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.4,
                ..default()
            },
            extension: TerrainExtension {
                curve: CurveUniform {
                    amt: 0.0,
                    grow: 1.0,
                    fade: 1.0,
                    edge_r: 9999.0,
                    wave_on: 1.0,
                    ..default()
                },
            },
        });
        let far = materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                double_sided: true,
                cull_mode: None,
                // 半透明：顶点 alpha 恒为 1，挖空环由 far_hole_* uniform 在片元着色器计算
                alpha_mode: AlphaMode::Blend,
                ..default()
            },
            extension: TerrainExtension {
                curve: CurveUniform {
                    amt: 0.0,
                    grow: 1.0,
                    fade: 1.0,
                    edge_r: 9999.0,
                    wave_on: 0.0,
                    far_hole_on: 1.0,
                    ..default()
                },
            },
        });
        Self {
            solid,
            water,
            far,
            atlas_image,
        }
    }
}

/// Update the shared curvature uniform from the player position/altitude.
pub fn curve_system(
    time: Res<Time>,
    player: Query<&crate::player::Player>,
    ship: Res<crate::space::ShipState>,
    mode: Res<crate::space::FlightMode>,
    world: Option<Res<crate::world::World>>,
    mut materials: ResMut<Assets<TerrainMat>>,
    mats: Res<TerrainMaterials>,
    mut wave_t: Local<f32>,
) {
    let Ok(p) = player.single() else { return };
    *wave_t += time.delta_secs();
    let cam_y = p.eye().y;
    let amt = ((cam_y - 62.0) / (150.0 - 62.0)).clamp(0.0, 1.0);
    // 出大气过渡：最后 60 格地形/远景淡出（球面 LOD 无缝接棒，避免飞出大气瞬间贴图突变）
    let fade = if *mode == crate::space::FlightMode::Atmo
        || *mode == crate::space::FlightMode::AtmoLand
    {
        ((crate::space::EXIT_Y - ship.pos.y) / 60.0).clamp(0.0, 1.0)
    } else {
        1.0
    };
    // 远景挖空环半径随区块视距联动（JS farHoleU 同口径），片元着色器按玩家距离计算
    let (r0, r1) = crate::far_hole_radii(
        world.map(|w| w.view_dist).unwrap_or(10),
    );
    for handle in [&mats.solid, &mats.water, &mats.far] {
        if let Some(mut m) = materials.get_mut(handle) {
            let c = &mut m.extension.curve;
            c.center = Vec2::new(p.pos.x, p.pos.z);
            c.amt = amt;
            c.grow = 1.0;
            c.fade = fade;
            c.edge_r = 9999.0;
            c.wave_time = *wave_t;
            // 远景挖空环（片元着色器计算，替代每帧 CPU 改写 129×129 顶点 alpha）
            c.far_hole_on = if handle == &mats.far { 1.0 } else { 0.0 };
            c.far_hole_r0 = r0;
            c.far_hole_r1 = r1;
            c.far_hole_cx = p.pos.x;
            c.far_hole_cz = p.pos.z;
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
        let Ok((mut l, mut tf)) = lights.get_mut(*e) else { continue };
        if let Some((_, cell, id)) = found.get(i) {
            // JS: l.position.set(x+0.5, y+0.9, z+0.5) —— 光必须跟随灯块
            tf.translation = Vec3::new(cell[0] as f32 + 0.5, cell[1] as f32 + 0.9, cell[2] as f32 + 0.5);
            let (r, g, b) = glow_color(crate::data::block_by_id(*id).key);
            l.color = Color::srgb(r / 255.0, g / 255.0, b / 255.0);
            l.intensity = 220.0;
            l.range = 11.0;
        } else {
            l.intensity = 0.0;
        }
    }
}
