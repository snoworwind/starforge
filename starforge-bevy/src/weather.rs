//! Planet climate visuals: a native Bevy `FogVolume` volumetric cloud layer,
//! biome weather particles, and procedural cloud shells visible from space.
//!
//! Clouds are rendered by Bevy's built-in volumetric fog (`FogVolume` +
//! [`bevy::light::VolumetricFog`] on the camera + [`bevy::light::VolumetricLight`]
//! on the sun): a large density-textured fog box follows the player, and the
//! 3D density texture (procedural, toroidal Worley noise per biome/seed) is
//! scrolled over time to simulate wind. No custom shaders are involved.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{FogVolume, VolumetricFog};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};

use crate::player::Player;
use crate::save::Settings;
use crate::space::{FlightMode, SpaceScene};
use crate::world::World;

// Keep the layer close to the playable atmosphere instead of putting the
// entire cloud deck high above the camera. A moderately thick layer also makes the
// lower edge read as a soft, nearby underside rather than a distant sheet.
const CLOUD_BOTTOM: f32 = 78.0;
const CLOUD_TOP: f32 = 174.0;
const CLOUD_WIDTH: f32 = 1_100.0;

/// Resolution of the 3D cloud density texture. The texture maps exactly once
/// over the fog box, so each texel covers ~11×3×11 blocks; it must be
/// horizontally toroidal so the box edges and the wind scroll stay seamless.
const DENSITY_W: u32 = 96;
const DENSITY_H: u32 = 32;
const DENSITY_D: u32 = 96;

/// Wind drift in UV units per second. The density texture wraps (Repeat), so
/// a full box-wide drift takes about 4 minutes — slow enough to read as
/// weather rather than a scrolling texture.
const WIND_UV_PER_SEC: f32 = 0.0042;

/// Runtime controls exposed by the in-game cloud tuning panel. The legacy
/// half-resolution render target is gone (native volumetric fog renders at
/// full resolution), but the settings fields are kept for save compatibility.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CloudTuning {
    pub coverage: f32,
    pub density: f32,
    pub raymarch_steps: u32,
    pub render_resolution: UVec2,
}

impl Default for CloudTuning {
    fn default() -> Self {
        Self {
            coverage: 0.61,
            density: 0.09,
            raymarch_steps: 24,
            render_resolution: UVec2::new(1536, 864),
        }
    }
}

impl CloudTuning {
    pub fn from_settings(settings: &Settings) -> Self {
        let mut tuning = Self {
            coverage: settings.cloud_coverage,
            density: settings.cloud_density,
            raymarch_steps: settings.cloud_raymarch_steps,
            render_resolution: UVec2::new(
                settings.cloud_render_width,
                settings.cloud_render_height,
            ),
        };
        tuning.sanitize();
        tuning
    }

    pub fn save_to_settings(self, settings: &mut Settings) {
        settings.cloud_coverage = self.coverage;
        settings.cloud_density = self.density;
        settings.cloud_raymarch_steps = self.raymarch_steps;
        settings.cloud_render_width = self.render_resolution.x;
        settings.cloud_render_height = self.render_resolution.y;
    }

    pub fn sanitize(&mut self) {
        self.coverage = if self.coverage.is_finite() {
            self.coverage.clamp(0.0, 1.0)
        } else {
            0.61
        };
        self.density = if self.density.is_finite() {
            self.density.clamp(0.0, 1.0)
        } else {
            0.09
        };
        self.raymarch_steps = self.raymarch_steps.clamp(4, 64);
    }
}

/// Marker for the single player-following cloud fog volume.
#[derive(Component)]
pub struct CloudVolume;

#[derive(Component)]
pub struct WeatherParticle {
    speed: f32,
    index: u32,
    generation: u32,
}

#[derive(Component)]
pub struct SpaceCloud {
    planet: Entity,
    layer: u8,
    speed: f32,
}

#[derive(Resource, Default)]
pub struct ClimateRuntime {
    fingerprint: Option<(u32, &'static str, bool, bool)>,
    elapsed: f32,
    density: Option<Handle<Image>>,
    volume: Option<Entity>,
}

/// Background rain loop owned by the current in-game climate.
#[derive(Resource, Default)]
pub struct RainAudio {
    pub entity: Option<Entity>,
}

/// Play the exterior city-rain bed while weather is visible on the ground.
/// It is deliberately non-spatial so it follows the player as ambience.
pub fn rain_audio_system(
    settings: Res<Settings>,
    mode: Res<FlightMode>,
    world: Option<Res<World>>,
    mut rain: ResMut<RainAudio>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    let raining = world.is_some() && settings.weather && mode.ground_scene();
    if raining && rain.entity.is_none() {
        rain.entity = Some(crate::audio::play_loop(&mut commands, sfx.rain.clone(), 0.18));
    } else if !raining && let Some(entity) = rain.entity.take() {
        commands.entity(entity).despawn();
    }
}

#[derive(Clone, Copy)]
struct WeatherDef {
    color: Color,
    speed: f32,
    count: u32,
    size: Vec3,
}

fn weather_def(key: &str) -> WeatherDef {
    let (rgb, speed, count, size) = match key {
        "ocean" => ((0.56, 0.77, 0.91), 20.0, 300, (0.035, 0.36, 0.035)),
        "murk" => ((0.60, 0.85, 0.69), 14.0, 240, (0.055, 0.16, 0.055)),
        "fungal" => ((0.85, 0.72, 0.94), 12.0, 220, (0.06, 0.14, 0.06)),
        "frozen" => ((0.94, 0.97, 0.98), 5.0, 240, (0.09, 0.09, 0.09)),
        "crystal" => ((0.85, 0.96, 0.98), 5.0, 220, (0.09, 0.09, 0.09)),
        "ashen" => ((0.54, 0.54, 0.54), 7.0, 200, (0.07, 0.08, 0.07)),
        "volcanic" => ((1.0, 0.54, 0.23), 6.0, 160, (0.09, 0.11, 0.09)),
        "desert" => ((0.91, 0.82, 0.63), 9.0, 140, (0.06, 0.08, 0.06)),
        "amber" => ((0.94, 0.78, 0.38), 8.0, 130, (0.06, 0.08, 0.06)),
        "ferrous" => ((0.66, 0.60, 0.88), 16.0, 150, (0.045, 0.28, 0.045)),
        "alien" => ((0.69, 0.44, 0.88), 10.0, 200, (0.07, 0.10, 0.07)),
        "salt" => ((0.96, 0.98, 0.99), 6.0, 160, (0.06, 0.06, 0.06)),
        "obsidian" => ((1.0, 0.42, 0.23), 7.0, 130, (0.08, 0.12, 0.08)),
        "redmoss" => ((0.85, 0.47, 0.41), 10.0, 150, (0.06, 0.08, 0.06)),
        "hive" => ((0.91, 0.72, 0.38), 8.0, 170, (0.06, 0.08, 0.06)),
        _ => ((0.72, 0.86, 0.95), 16.0, 220, (0.035, 0.32, 0.035)),
    };
    WeatherDef {
        color: Color::srgba(rgb.0, rgb.1, rgb.2, 0.62),
        speed,
        count,
        size: Vec3::new(size.0, size.1, size.2),
    }
}

fn particle_position(world: &World, player: Vec3, index: u32, generation: u32) -> Vec3 {
    let mut rng = crate::rng::Rng::new(
        world.seed ^ index.wrapping_mul(0x9E37_79B9) ^ generation.wrapping_mul(0x85EB_CA6B),
    );
    let x = player.x + (rng.next() - 0.5) * 90.0;
    let z = player.z + (rng.next() - 0.5) * 90.0;
    let floor = world.g.height_at(x.floor(), z.floor()) as f32 + 1.0;
    Vec3::new(x, floor.max(player.y - 4.0) + 20.0 + rng.next() * 22.0, z)
}

/// Rebuilds climate entities when the planet or either graphics toggle changes,
/// then keeps the cloud fog volume and weather effects wrapped around the player.
#[allow(clippy::too_many_arguments)]
pub fn climate_system(
    time: Res<Time>,
    settings: Res<Settings>,
    mode: Res<FlightMode>,
    tuning: Res<CloudTuning>,
    world: Res<World>,
    player: Query<&Player>,
    clear: Res<ClearColor>,
    mut runtime: ResMut<ClimateRuntime>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut volume_fog: Query<
        (Entity, &mut FogVolume, &mut Transform, &mut Visibility),
        Without<WeatherParticle>,
    >,
    mut camera_fog: Query<&mut VolumetricFog, With<Camera3d>>,
    mut particles: Query<
        (Entity, &mut WeatherParticle, &mut Transform, &mut Visibility),
        Without<FogVolume>,
    >,
) {
    let Ok(player) = player.single() else { return };
    let fingerprint = (
        world.seed,
        world.biome().key,
        settings.clouds,
        settings.weather,
    );
    if runtime.fingerprint != Some(fingerprint) {
        for (entity, _, _, _) in &particles {
            commands.entity(entity).despawn();
        }
        for (entity, _, _, _) in &volume_fog {
            commands.entity(entity).despawn();
        }
        runtime.fingerprint = Some(fingerprint);
        runtime.elapsed = 0.0;
        runtime.density = None;
        runtime.volume = None;

        if settings.clouds {
            let density = images.add(make_cloud_density_texture(world.seed, world.biome().key));
            let center = cloud_center(player.pos);
            let volume = commands
                .spawn((
                    FogVolume {
                        // White fog lit by the real sun: the cloud layer picks
                        // up the warm daylight color and the biome sky ambient.
                        //
                        // Bevy 的 light_attenuation = exp(-density × bounding_radius
                        // × (absorption+scattering))，bounding_radius 是云盒包围球
                        // 半径（1100 宽云盒 ≈ 779）。吸收/散射必须压到 ~0.02 量级，
                        // 否则 exp(-0.12×779×0.28)=exp(-27)≈0，方向光被 Beer 衰减
                        // 全吃掉，云白天也全黑（官方示例云盒只有 30~55 半径）。
                        fog_color: Color::WHITE,
                        density_factor: 0.10,
                        density_texture: Some(density.clone()),
                        absorption: 0.01,
                        scattering: 0.012,
                        // 补偿压低后的散射系数：云亮度 = light_attenuation ×
                        // scattering × light_intensity，光强因子放大到可见水平。
                        light_intensity: 5.0,
                        scattering_asymmetry: 0.72,
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(
                        center.x,
                        (CLOUD_BOTTOM + CLOUD_TOP) * 0.5,
                        center.z,
                    ))
                    .with_scale(Vec3::new(
                        CLOUD_WIDTH,
                        CLOUD_TOP - CLOUD_BOTTOM,
                        CLOUD_WIDTH,
                    )),
                    Visibility::Visible,
                    CloudVolume,
                    crate::InGame,
                ))
                .id();
            runtime.density = Some(density);
            runtime.volume = Some(volume);
        }

        if settings.weather {
            let def = weather_def(world.biome().key);
            let mesh = meshes.add(Cuboid::new(def.size.x, def.size.y, def.size.z));
            let material = materials.add(StandardMaterial {
                base_color: def.color,
                emissive: def.color.to_linear() * 0.12,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            });
            for index in 0..def.count {
                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(particle_position(&world, player.pos, index, 0)),
                    WeatherParticle {
                        speed: def.speed,
                        index,
                        generation: 0,
                    },
                    crate::InGame,
                ));
            }
        }
        return;
    }

    let dt = time.delta_secs();
    runtime.elapsed += dt;
    let show_clouds = settings.clouds && mode.ground_scene();

    // 体积雾渲染管线忽略 Visibility（隐藏的 FogVolume 仍会被提取渲染），
    // 因此离开地面场景（太空/曲速/空间站）时必须真正销毁体积实体，
    // 回到地面再按需重建（密度纹理保留，重建只需一个实体）。
    if mode.ground_scene() {
        if runtime.volume.is_none() && settings.clouds {
            let density = match runtime.density.clone() {
                Some(d) => d,
                None => {
                    let d = images.add(make_cloud_density_texture(world.seed, world.biome().key));
                    runtime.density = Some(d.clone());
                    d
                }
            };
            let center = cloud_center(player.pos);
            let volume = commands
                .spawn((
                    FogVolume {
                        fog_color: Color::WHITE,
                        density_factor: 0.10,
                        density_texture: Some(density),
                        // 同首建：吸收/散射压低到与 779 包围球半径匹配的量级，
                        // light_intensity 补偿散射亮度（见上）。
                        absorption: 0.01,
                        scattering: 0.012,
                        light_intensity: 5.0,
                        scattering_asymmetry: 0.72,
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(
                        center.x,
                        (CLOUD_BOTTOM + CLOUD_TOP) * 0.5,
                        center.z,
                    ))
                    .with_scale(Vec3::new(
                        CLOUD_WIDTH,
                        CLOUD_TOP - CLOUD_BOTTOM,
                        CLOUD_WIDTH,
                    )),
                    Visibility::Visible,
                    CloudVolume,
                    crate::InGame,
                ))
                .id();
            runtime.volume = Some(volume);
        }
    } else if let Some(entity) = runtime.volume.take() {
        commands.entity(entity).despawn();
    }

    let show_weather = settings.weather && matches!(*mode, FlightMode::Planet | FlightMode::Seated);
    let center = cloud_center(player.pos);

    // Wind: scroll the repeating density texture in UV space. The texture is
    // toroidal, so the drift wraps around seamlessly.
    let wind = Vec3::new(
        runtime.elapsed * WIND_UV_PER_SEC,
        0.0,
        runtime.elapsed * WIND_UV_PER_SEC * 0.15,
    );
    let sky = clear.0.to_linear();
    for (entity, mut fog, mut transform, mut visibility) in &mut volume_fog {
        if runtime.volume != Some(entity) {
            continue;
        }
        transform.translation.x = center.x;
        transform.translation.z = center.z;
        fog.density_texture_offset = wind;
        // Tuning → physical fog coefficients. `coverage` mostly controls how
        // much of the layer is opaque, `density` how strongly it absorbs and
        // scatters light.
        //
        // 系数量级注意：Bevy 的 light_attenuation 用云盒包围球半径
        // （1100 宽 → ≈779）做 Beer 衰减路径，吸收/散射必须 ~0.01 量级，
        // 否则 exp(-density×779×(abs+scat)) ≈ 0，云白天全黑。散射亮度
        // 用 FogVolume::light_intensity 放大补偿。
        fog.density_factor = 0.05 + tuning.coverage.clamp(0.0, 1.0) * 0.12;
        fog.absorption = 0.006 + tuning.density.clamp(0.0, 1.0) * 0.010;
        fog.scattering = 0.008 + tuning.density.clamp(0.0, 1.0) * 0.012;
        fog.light_intensity = 5.0;
        *visibility = if show_clouds {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for mut vf in &mut camera_fog {
        vf.step_count = tuning.raymarch_steps.clamp(4, 64);
        // Shadowed cloud parts fall back to the sky color so they don't read
        // as black holes against the atmosphere. 白天环境光提高：
        // ambient 项也受 exp(-ray_length×(abs+scat)) Beer 衰减，
        // 0.3 太低会让云的背光面接近全黑，2.0 与压低后的系数匹配。
        vf.ambient_color = Color::LinearRgba(sky);
        vf.ambient_intensity = 2.0;
    }

    for (_, mut particle, mut transform, mut visibility) in &mut particles {
        *visibility = if show_weather {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if !show_weather {
            continue;
        }
        transform.translation.y -= particle.speed * dt;
        let floor = world.g.height_at(
            transform.translation.x.floor(),
            transform.translation.z.floor(),
        ) as f32
            + 1.0;
        if transform.translation.y < floor {
            particle.generation = particle.generation.wrapping_add(1);
            transform.translation =
                particle_position(&world, player.pos, particle.index, particle.generation);
        }
    }
}

fn cloud_center(player: Vec3) -> Vec3 {
    // Snapping avoids moving the entire volume every frame while still giving
    // the player a full kilometre of cloud coverage in every direction.
    Vec3::new(
        (player.x / 128.0).floor() * 128.0,
        (CLOUD_BOTTOM + CLOUD_TOP) * 0.5,
        (player.z / 128.0).floor() * 128.0,
    )
}

fn repeat_sampler() -> ImageSampler {
    let mut descriptor = ImageSamplerDescriptor::linear();
    descriptor.address_mode_u = ImageAddressMode::Repeat;
    descriptor.address_mode_v = ImageAddressMode::Repeat;
    descriptor.address_mode_w = ImageAddressMode::Repeat;
    descriptor.mag_filter = ImageFilterMode::Linear;
    ImageSampler::Descriptor(descriptor)
}

/// Builds the 3D cloud density texture for a biome/seed.
///
/// The texture maps exactly once over the fog box (1100×96×1100 blocks), so
/// every octave is *toroidal*: cell indices wrap around the texture and the
/// Worley distance is measured on the circle, which keeps the box edges and
/// the wind scroll free of seams. Coverage comes from 2D Worley FBM in the
/// x/z plane (a few large weather systems), erosion from 3D Worley FBM, and a
/// vertical profile makes the deck soft at the bottom and top.
fn make_cloud_density_texture(seed: u32, biome: &str) -> Image {
    let biome_seed = seed
        ^ biome.bytes().fold(0u32, |value, byte| {
            value.wrapping_mul(33).wrapping_add(byte as u32)
        });
    let threshold = cloud_threshold(biome);
    let mut bytes = vec![0u8; (DENSITY_W * DENSITY_H * DENSITY_D) as usize];
    for z in 0..DENSITY_D {
        let pz = z as f32 / DENSITY_D as f32;
        for y in 0..DENSITY_H {
            let py = y as f32 / (DENSITY_H - 1) as f32;
            // vertical profile: soft bottom/top, densest mid band
            let profile = smoothstep(0.0, 0.28, py) * (1.0 - smoothstep(0.58, 0.92, py));
            for x in 0..DENSITY_W {
                let px = x as f32 / DENSITY_W as f32;
                // large-scale coverage in the x/z plane (4 base cells per box)
                let cover = worley_fbm2_periodic(
                    Vec2::new(px, pz) * 4.0,
                    4.0,
                    biome_seed ^ 0x51A7,
                );
                let cover = smoothstep(threshold, (threshold + 0.22).min(0.95), cover);
                // 3D detail erosion
                let detail = worley_fbm3_periodic(
                    Vec3::new(px, py, pz) * 6.0,
                    6.0,
                    biome_seed ^ 0xC011,
                );
                let detail = smoothstep(0.42, 0.78, detail);
                let density = (cover * (0.30 + 0.70 * detail) * profile).clamp(0.0, 1.0);
                let at = ((z * DENSITY_H + y) * DENSITY_W + x) as usize;
                bytes[at] = (density * 255.0) as u8;
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: DENSITY_W,
            height: DENSITY_H,
            depth_or_array_layers: DENSITY_D,
        },
        TextureDimension::D3,
        bytes,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = repeat_sampler();
    image
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn hash01(x: i32, y: i32, z: i32, seed: u32, channel: u32) -> f32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x9E37_79B9)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ (z as u32).wrapping_mul(0xC2B2_AE35)
        ^ channel.wrapping_mul(0x27D4_EB2D);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32
}

/// Circular distance on a line of length `period` (toroidal wrap).
#[inline]
fn circular_dist(a: f32, b: f32, period: f32) -> f32 {
    let d = (a - b).abs();
    d.min(period - d)
}

fn worley2_periodic(p: Vec2, period: f32, seed: u32) -> f32 {
    let period_i = period as i32;
    let cell = p.floor();
    let mut distance = f32::MAX;
    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            let cx = (cell.x as i32 + dx).rem_euclid(period_i);
            let cy = (cell.y as i32 + dy).rem_euclid(period_i);
            let fx = cx as f32 + hash01(cx, cy, 0, seed, 0);
            let fy = cy as f32 + hash01(cx, cy, 0, seed, 1);
            let ddx = circular_dist(fx, p.x, period);
            let ddy = circular_dist(fy, p.y, period);
            distance = distance.min((ddx * ddx + ddy * ddy).sqrt());
        }
    }
    1.0 - (distance / std::f32::consts::SQRT_2).clamp(0.0, 1.0)
}

fn worley3_periodic(p: Vec3, period: f32, seed: u32) -> f32 {
    let period_i = period as i32;
    let cell = p.floor();
    let mut distance = f32::MAX;
    for dz in -1..=1i32 {
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                let cx = (cell.x as i32 + dx).rem_euclid(period_i);
                let cy = (cell.y as i32 + dy).rem_euclid(period_i);
                let cz = (cell.z as i32 + dz).rem_euclid(period_i);
                let fx = cx as f32 + hash01(cx, cy, cz, seed, 0);
                let fy = cy as f32 + hash01(cx, cy, cz, seed, 1);
                let fz = cz as f32 + hash01(cx, cy, cz, seed, 2);
                let ddx = circular_dist(fx, p.x, period);
                let ddy = circular_dist(fy, p.y, period);
                let ddz = circular_dist(fz, p.z, period);
                distance = distance.min((ddx * ddx + ddy * ddy + ddz * ddz).sqrt());
            }
        }
    }
    1.0 - (distance / 1.732_050_8).clamp(0.0, 1.0)
}

fn worley_fbm2_periodic(p: Vec2, base_period: f32, seed: u32) -> f32 {
    let mut frequency = 1.0;
    let mut amplitude = 0.58;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for octave in 0..3 {
        sum += worley2_periodic(
            p * frequency,
            base_period * frequency,
            seed ^ (octave * 977),
        ) * amplitude;
        norm += amplitude;
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    sum / norm
}

fn worley_fbm3_periodic(p: Vec3, base_period: f32, seed: u32) -> f32 {
    let mut frequency = 1.0;
    let mut amplitude = 0.58;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for octave in 0..2 {
        sum += worley3_periodic(
            p * frequency,
            base_period * frequency,
            seed ^ (octave * 1_301),
        ) * amplitude;
        norm += amplitude;
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    sum / norm
}

fn cloud_threshold(key: &str) -> f32 {
    match key {
        "lush" => 0.55,
        "ocean" => 0.52,
        "fungal" => 0.50,
        "frozen" => 0.44,
        "murk" => 0.42,
        "alien" | "crystal" => 0.40,
        "hive" => 0.38,
        "redmoss" => 0.36,
        "ferrous" => 0.34,
        "salt" => 0.30,
        "obsidian" => 0.28,
        "amber" => 0.24,
        "desert" => 0.20,
        "volcanic" => 0.16,
        "ashen" => 0.12,
        _ => 0.40,
    }
}

fn cloud_texture(images: &mut Assets<Image>, key: &str, seed: u32) -> Handle<Image> {
    let (width, height) = (256usize, 128usize);
    let noise = crate::rng::Noise2::new(seed);
    let threshold = cloud_threshold(key);
    let stormy = matches!(key, "ferrous" | "murk");
    let mut bytes = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let seam = x as f32 / width as f32;
            let sample = |x: f32, y: f32| {
                let warp_x = noise.fbm2(x * 0.018 + 19.0, y * 0.024 - 7.0, 3, 2.0, 0.5) * 8.0;
                let warp_y = noise.fbm2(x * 0.021 - 11.0, y * 0.017 + 5.0, 3, 2.0, 0.5) * 6.0;
                noise.fbm2((x + warp_x) * 0.042, (y + warp_y) * 0.068, 5, 2.0, 0.5) * 0.5 + 0.5
            };
            let a = sample(x as f32, y as f32);
            let b = sample(x as f32 - width as f32, y as f32);
            let mut value = a * (1.0 - seam) + b * seam;
            let v = y as f32 / height as f32;
            value *= 0.88 + (1.0 - (v * 2.0 - 1.0).abs()) * 0.12;
            if stormy {
                value += (seam * std::f32::consts::TAU * 3.0).sin() * 0.16;
            }
            let opacity = ((value - threshold) / 0.30).clamp(0.0, 1.0);
            let opacity = opacity * opacity * (3.0 - 2.0 * opacity);
            let offset = (y * width + x) * 4;
            bytes[offset..offset + 3].fill(255);
            bytes[offset + 3] = (opacity * 220.0) as u8;
        }
    }
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        bytes,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = bevy::image::ImageSampler::linear();
    images.add(image)
}

/// Adds two translucent cloud shells to every space-scene planet and rotates
/// them at slightly different speeds so the globe reads as layered, living
/// clouds instead of a flat sticker. Low opacity lets the surface show through.
#[allow(clippy::too_many_arguments)]
pub fn space_cloud_system(
    time: Res<Time>,
    settings: Res<Settings>,
    scene: Option<Res<SpaceScene>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(Entity, &SpaceCloud)>,
    mut clouds: Query<(&SpaceCloud, &mut Transform)>,
) {
    if !settings.clouds {
        for (entity, _) in &existing {
            commands.entity(entity).despawn();
        }
        return;
    }
    let Some(scene) = scene else { return };
    for planet in &scene.planets {
        // 起源星（家园星系 id 0）的地球模型自带云层壳，不再叠加程序化云壳
        if scene.galaxy_seed == crate::data::HOME_GALAXY_SEED && planet.def.id == 0 {
            continue;
        }
        let have: Vec<u8> = existing
            .iter()
            .filter(|(_, cloud)| cloud.planet == planet.entity)
            .map(|(_, cloud)| cloud.layer)
            .collect();
        for layer in 0..2u8 {
            if have.contains(&layer) {
                continue;
            }
            // 双层云壳：内层较密、外层稀薄，不同转速形成视差
            let (radius_k, alpha, seed, speed) = if layer == 0 {
                (
                    1.045f32,
                    0.42f32,
                    90_210 + planet.def.id as u32 * 777,
                    0.010,
                )
            } else {
                (1.09, 0.30, 371_015 + planet.def.id as u32 * 913, -0.007)
            };
            let texture = cloud_texture(
                &mut images,
                planet.def.biome,
                seed + planet.def.biome.len() as u32 * 131,
            );
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(texture),
                base_color: Color::srgba(1.0, 1.0, 1.0, alpha),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 1.0,
                cull_mode: None,
                ..default()
            });
            let entity = commands
                .spawn((
                    Mesh3d(meshes.add(Sphere::new(planet.def.radius * radius_k))),
                    MeshMaterial3d(material),
                    Transform::from_rotation(Quat::from_rotation_y(
                        planet.def.id as f32 * 1.7 + layer as f32 * 2.4,
                    )),
                    SpaceCloud {
                        planet: planet.entity,
                        layer,
                        speed,
                    },
                    crate::InGame,
                ))
                .id();
            commands.entity(planet.entity).add_child(entity);
        }
    }
    for (cloud, mut transform) in &mut clouds {
        transform.rotate_y(cloud.speed * time.delta_secs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_biome_has_weather_and_cloud_density() {
        for biome in crate::data::BIOMES {
            let weather = weather_def(biome.key);
            assert!(weather.count >= 100);
            assert!(weather.speed > 0.0);
            assert!((0.0..=1.0).contains(&cloud_threshold(biome.key)));
        }
    }

    #[test]
    fn density_texture_is_toroidal() {
        // The field must be periodic at the wrap point: sampling exactly at
        // u=0 and u=1 (×period, the seam) must give identical neighborhoods
        // and therefore identical values.
        let seed = 0x5EED;
        for period in [4.0f32, 8.0, 16.0] {
            let a = worley_fbm2_periodic(Vec2::new(0.0, 0.5) * period, period, seed);
            let b = worley_fbm2_periodic(Vec2::new(1.0, 0.5) * period, period, seed);
            assert!((a - b).abs() < 1e-4, "2D seam at period {period}: {a} vs {b}");
        }
        let a = worley_fbm3_periodic(Vec3::new(0.0, 0.5, 0.3) * 6.0, 6.0, seed);
        let b = worley_fbm3_periodic(Vec3::new(1.0, 0.5, 0.3) * 6.0, 6.0, seed);
        assert!((a - b).abs() < 1e-4, "3D seam: {a} vs {b}");
    }
}
