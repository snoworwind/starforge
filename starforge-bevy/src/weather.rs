//! Planet climate visuals: a procedural volumetric cloud layer, biome weather
//! particles, and procedural cloud shells visible from space.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy_volumetric_clouds::{CloudsConfig, SkyboxPlane};

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
const COVERAGE_WIDTH: u32 = 256;
const COVERAGE_HEIGHT: u32 = 128;
const DETAIL_SIZE: u32 = 48;
const USE_UPSTREAM_CLOUDS: bool = true;

/// Runtime controls exposed by the in-game cloud tuning panel.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CloudTuning {
    pub coverage: f32,
    pub density: f32,
    pub raymarch_steps: u32,
    pub render_resolution: UVec2,
}

pub const CLOUD_RESOLUTION_PRESETS: &[(u32, u32)] =
    &[(1280, 720), (1536, 864), (1920, 1080), (2560, 1600)];

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
        if !CLOUD_RESOLUTION_PRESETS
            .iter()
            .any(|&(width, height)| self.render_resolution == UVec2::new(width, height))
        {
            self.render_resolution = UVec2::new(1536, 864);
        }
    }
}

/// GPU parameters for the volume shader. Keeping the fields in vec4s makes
/// the WGSL layout explicit and avoids backend-specific uniform padding.
#[derive(ShaderType, Clone, Copy, Debug)]
pub struct CloudUniform {
    pub bounds_min: Vec4,
    pub bounds_max: Vec4,
    /// xy = wind in world units/sec, z = animation time.
    pub wind_time: Vec4,
    /// x/y = cloud layer bounds, z = coverage UV scale, w = detail UV scale.
    pub shape: Vec4,
    /// x = extinction density, y = forward phase g, z = multi-scatter energy,
    /// w = detail erosion strength.
    pub scattering: Vec4,
    /// xyz = direction toward the sun, w = normalized sun energy.
    pub sun: Vec4,
    pub sun_color: Vec4,
    /// xyz = sky/ambient color, w = ambient energy.
    pub ambient: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CloudMaterial {
    #[uniform(0)]
    pub params: CloudUniform,
    #[texture(1, dimension = "2d")]
    #[sampler(2)]
    pub coverage: Handle<Image>,
    #[texture(3, dimension = "3d")]
    #[sampler(4)]
    pub detail: Handle<Image>,
}

impl Material for CloudMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/volumetric_cloud.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/volumetric_cloud.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // A ray-marched volume must remain drawable after the camera enters
        // the AABB. The default back-face culling would remove every face in
        // that situation and make the clouds pop out until the camera exits.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Component)]
pub struct VolumeCloud;

type CloudSunFilter = (
    With<crate::daynight::Sun>,
    Without<VolumeCloud>,
    Without<WeatherParticle>,
);

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
    material: Option<Handle<CloudMaterial>>,
    volume: Option<Entity>,
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
/// then keeps the volume and weather effects wrapped around the player.
#[allow(clippy::too_many_arguments)]
pub fn climate_system(
    time: Res<Time>,
    settings: Res<Settings>,
    mode: Res<FlightMode>,
    world: Res<World>,
    player: Query<&Player>,
    mut runtime: ResMut<ClimateRuntime>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cloud_materials: ResMut<Assets<CloudMaterial>>,
    clear: Res<ClearColor>,
    sun: Query<(&Transform, &DirectionalLight), CloudSunFilter>,
    climate_entities: Query<Entity, With<WeatherParticle>>,
    volume_entities: Query<
        (Entity, &mut Transform),
        (
            With<VolumeCloud>,
            Without<crate::daynight::Sun>,
            Without<WeatherParticle>,
        ),
    >,
    mut particles: Query<
        (&mut WeatherParticle, &mut Transform, &mut Visibility),
        (Without<VolumeCloud>, Without<crate::daynight::Sun>),
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
        for entity in &climate_entities {
            commands.entity(entity).despawn();
        }
        for (entity, _) in &volume_entities {
            commands.entity(entity).despawn();
        }
        runtime.fingerprint = Some(fingerprint);
        runtime.elapsed = 0.0;
        runtime.material = None;
        runtime.volume = None;

        if settings.clouds && !USE_UPSTREAM_CLOUDS {
            let coverage = images.add(make_coverage_image(world.seed, world.biome().key));
            let detail = images.add(make_detail_image(world.seed ^ 0xD37A_11, world.biome().key));
            let center = cloud_center(player.pos);
            let params = cloud_uniform(center, runtime.elapsed, &sun, clear.0);
            let material = cloud_materials.add(CloudMaterial {
                params,
                coverage,
                detail,
            });
            let mesh = meshes.add(Cuboid::new(
                CLOUD_WIDTH,
                CLOUD_TOP - CLOUD_BOTTOM,
                CLOUD_WIDTH,
            ));
            let volume = commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(Vec3::new(
                        center.x,
                        (CLOUD_BOTTOM + CLOUD_TOP) * 0.5,
                        center.z,
                    )),
                    VolumeCloud,
                    crate::InGame,
                ))
                .id();
            runtime.material = Some(material);
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
    let show_weather = settings.weather && matches!(*mode, FlightMode::Planet | FlightMode::Seated);
    let center = cloud_center(player.pos);
    if let Some(mut material) = runtime
        .material
        .as_ref()
        .and_then(|handle| cloud_materials.get_mut(handle))
    {
        material.params.bounds_min = Vec4::new(
            center.x - CLOUD_WIDTH * 0.5,
            CLOUD_BOTTOM,
            center.z - CLOUD_WIDTH * 0.5,
            0.0,
        );
        material.params.bounds_max = Vec4::new(
            center.x + CLOUD_WIDTH * 0.5,
            CLOUD_TOP,
            center.z + CLOUD_WIDTH * 0.5,
            0.0,
        );
        material.params.wind_time.z = runtime.elapsed;
        let (sun_dir, sun_energy, sun_color) = sun_parameters(&sun);
        material.params.sun = Vec4::new(sun_dir.x, sun_dir.y, sun_dir.z, sun_energy);
        material.params.sun_color = Vec4::new(sun_color.x, sun_color.y, sun_color.z, 1.0);
        let sky = clear.0.to_linear();
        material.params.ambient = Vec4::new(sky.red, sky.green, sky.blue, 0.30);
    }
    for (entity, mut transform) in volume_entities {
        if runtime.volume != Some(entity) {
            continue;
        }
        transform.translation.x = center.x;
        transform.translation.z = center.z;
        commands.entity(entity).insert(if show_clouds {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
    }
    for (mut particle, mut transform, mut visibility) in &mut particles {
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

/// Drives the vendored bevy-volumetric-clouds renderer from STARFORGE's
/// atmosphere state. The upstream Horizon/Frostbite pass is kept intact, but
/// its skybox is only visible in ground/atmosphere modes; the runtime tuning
/// panel can switch the cloud target between several resolutions.
pub fn upstream_cloud_config_system(
    settings: Res<Settings>,
    mode: Res<FlightMode>,
    clear: Res<ClearColor>,
    sun: Query<(&Transform, &DirectionalLight), CloudSunFilter>,
    tuning: Res<CloudTuning>,
    mut config: ResMut<CloudsConfig>,
    mut skybox: Query<&mut Visibility, With<SkyboxPlane>>,
) {
    let visible = settings.clouds && mode.ground_scene();
    let (sun_dir, sun_energy, sun_color) = sun_parameters(&sun);
    let sky = clear.0.to_linear();

    config.clouds_raymarch_steps_count = if visible {
        tuning.raymarch_steps.clamp(4, 64)
    } else {
        1
    };
    config.clouds_shadow_raymarch_steps_count = if visible { 4 } else { 1 };
    config.planet_radius = 150.0;
    config.clouds_bottom_height = CLOUD_BOTTOM;
    config.clouds_top_height = CLOUD_TOP;
    config.clouds_coverage = if visible {
        tuning.coverage.clamp(0.0, 1.0)
    } else {
        0.0
    };
    config.clouds_detail_strength = 0.30;
    config.clouds_base_edge_softness = 0.14;
    config.clouds_bottom_softness = 0.18;
    config.clouds_density = if visible {
        tuning.density.clamp(0.0, 1.0)
    } else {
        0.0
    };
    config.clouds_shadow_raymarch_step_size = 22.0;
    config.clouds_shadow_raymarch_step_multiply = 1.25;
    config.forward_scattering_g = 0.78;
    config.backward_scattering_g = -0.18;
    config.scattering_lerp = 0.58;
    config.clouds_ambient_color_top = Vec4::new(sky.red, sky.green, sky.blue, 0.0) * 0.34;
    config.clouds_ambient_color_bottom =
        Vec4::new(sky.red * 0.42, sky.green * 0.44, sky.blue * 0.48, 0.0);
    config.clouds_min_transmittance = 0.06;
    config.clouds_base_scale = 1.25;
    config.clouds_detail_scale = 48.0;
    config.sun_dir = Vec4::new(sun_dir.x, sun_dir.y, sun_dir.z, 0.0);
    config.sun_color = Vec4::new(
        sun_color.x * sun_energy,
        sun_color.y * sun_energy,
        sun_color.z * sun_energy,
        1.0,
    );
    config.reprojection_strength = if visible { 0.78 } else { 0.0 };
    config.render_resolution = Vec2::new(
        tuning.render_resolution.x as f32,
        tuning.render_resolution.y as f32,
    );
    config.wind_velocity = Vec3::new(0.48, 0.0, 0.07);

    for mut visibility in &mut skybox {
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
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

fn sun_parameters(
    sun: &Query<(&Transform, &DirectionalLight), CloudSunFilter>,
) -> (Vec3, f32, Vec3) {
    let Ok((transform, light)) = sun.single() else {
        return (Vec3::new(0.25, 0.85, 0.35).normalize(), 1.0, Vec3::ONE);
    };
    let direction = (transform.rotation * Vec3::NEG_Z).normalize();
    let color = light.color.to_linear();
    (
        direction,
        (light.illuminance / 5_000.0).clamp(0.35, 3.0),
        Vec3::new(color.red, color.green, color.blue),
    )
}

fn cloud_uniform(
    center: Vec3,
    elapsed: f32,
    sun: &Query<(&Transform, &DirectionalLight), CloudSunFilter>,
    sky: Color,
) -> CloudUniform {
    let (sun_dir, sun_energy, sun_color) = sun_parameters(sun);
    let sky = sky.to_linear();
    CloudUniform {
        bounds_min: Vec4::new(
            center.x - CLOUD_WIDTH * 0.5,
            CLOUD_BOTTOM,
            center.z - CLOUD_WIDTH * 0.5,
            0.0,
        ),
        bounds_max: Vec4::new(
            center.x + CLOUD_WIDTH * 0.5,
            CLOUD_TOP,
            center.z + CLOUD_WIDTH * 0.5,
            0.0,
        ),
        // Deliberately slow drift: the cloud layer should read as weather,
        // not as a texture scrolling over the camera.
        wind_time: Vec4::new(0.48, 0.07, elapsed, 0.0),
        // Lower detail frequency keeps the cloud lobes broad at horizon scale.
        shape: Vec4::new(CLOUD_BOTTOM, CLOUD_TOP, 1.0 / CLOUD_WIDTH, 1.0 / 235.0),
        // Lower extinction prevents a dense column from turning the whole sky
        // into an opaque white sheet when viewed through a long grazing ray.
        scattering: Vec4::new(0.032, 0.62, 0.32, 0.62),
        sun: Vec4::new(sun_dir.x, sun_dir.y, sun_dir.z, sun_energy),
        sun_color: Vec4::new(sun_color.x, sun_color.y, sun_color.z, 1.0),
        ambient: Vec4::new(sky.red, sky.green, sky.blue, 0.30),
    }
}

fn repeat_sampler() -> ImageSampler {
    let mut descriptor = ImageSamplerDescriptor::linear();
    descriptor.address_mode_u = ImageAddressMode::Repeat;
    descriptor.address_mode_v = ImageAddressMode::Repeat;
    descriptor.address_mode_w = ImageAddressMode::Repeat;
    descriptor.mag_filter = ImageFilterMode::Linear;
    ImageSampler::Descriptor(descriptor)
}

fn make_coverage_image(seed: u32, biome: &str) -> Image {
    let biome_seed = seed
        ^ biome.bytes().fold(0u32, |value, byte| {
            value.wrapping_mul(33).wrapping_add(byte as u32)
        });
    let mut bytes = vec![0u8; (COVERAGE_WIDTH * COVERAGE_HEIGHT * 4) as usize];
    for y in 0..COVERAGE_HEIGHT {
        for x in 0..COVERAGE_WIDTH {
            let p = Vec2::new(
                x as f32 / COVERAGE_WIDTH as f32 * 3.6,
                y as f32 / COVERAGE_HEIGHT as f32 * 2.2,
            );
            // Three bands deliberately produce different cloud scales: a few
            // large weather systems, medium cumulus groups, and small islands.
            let large = worley_fbm2(p * 0.48 + Vec2::new(7.3, -2.4), biome_seed ^ 0x51A7);
            let medium = worley_fbm2(p * 1.0 + Vec2::new(13.7, -4.2), biome_seed ^ 0xA31F);
            let small = worley_fbm2(p * 2.1 + Vec2::new(-3.8, 11.6), biome_seed ^ 0xD00D);
            let value = large * 0.58 + medium * 0.32 + small * 0.10;
            // Keep separated cloud islands and let the shader soften their
            // edges. The other channels retain scale masks for the shader.
            let coverage = smoothstep(0.70, 0.86, value);
            let large_mask = smoothstep(0.66, 0.84, large);
            let medium_mask = smoothstep(0.62, 0.84, medium);
            let at = ((y * COVERAGE_WIDTH + x) * 4) as usize;
            bytes[at] = (coverage * 255.0) as u8;
            bytes[at + 1] = (large_mask * 255.0) as u8;
            bytes[at + 2] = (medium_mask * 255.0) as u8;
            bytes[at + 3] = 255;
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: COVERAGE_WIDTH,
            height: COVERAGE_HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = repeat_sampler();
    image
}

fn make_detail_image(seed: u32, biome: &str) -> Image {
    let biome_seed = seed
        ^ biome
            .bytes()
            .fold(0u32, |value, byte| value.rotate_left(5) ^ byte as u32);
    let mut bytes = vec![0u8; (DETAIL_SIZE * DETAIL_SIZE * DETAIL_SIZE * 4) as usize];
    for z in 0..DETAIL_SIZE {
        for y in 0..DETAIL_SIZE {
            for x in 0..DETAIL_SIZE {
                let p = Vec3::new(
                    x as f32 / DETAIL_SIZE as f32 * 5.5,
                    y as f32 / DETAIL_SIZE as f32 * 5.5,
                    z as f32 / DETAIL_SIZE as f32 * 5.5,
                );
                let curl = curl_noise(p * 1.7, biome_seed);
                let warped = p + curl * 0.23;
                let worley = worley_fbm3(warped * 1.35, biome_seed ^ 0xC011);
                let high = value_fbm3(warped * 3.8, biome_seed ^ 0xF00D);
                let value = (worley * 0.72 + high * 0.28).clamp(0.0, 1.0);
                let at = (((z * DETAIL_SIZE + y) * DETAIL_SIZE + x) * 4) as usize;
                let v = (value * 255.0) as u8;
                bytes[at..at + 3].fill(v);
                bytes[at + 3] = 255;
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: DETAIL_SIZE,
            height: DETAIL_SIZE,
            depth_or_array_layers: DETAIL_SIZE,
        },
        TextureDimension::D3,
        bytes,
        TextureFormat::Rgba8Unorm,
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

fn worley2(p: Vec2, seed: u32) -> f32 {
    let cell = p.floor().as_ivec2();
    let fract = p - cell.as_vec2();
    let mut distance = f32::MAX;
    for y in -1..=1 {
        for x in -1..=1 {
            let feature = Vec2::new(
                x as f32 + hash01(cell.x + x, cell.y + y, 0, seed, 0),
                y as f32 + hash01(cell.x + x, cell.y + y, 0, seed, 1),
            );
            distance = distance.min((feature - fract).length());
        }
    }
    1.0 - (distance / std::f32::consts::SQRT_2).clamp(0.0, 1.0)
}

fn worley3(p: Vec3, seed: u32) -> f32 {
    let cell = p.floor().as_ivec3();
    let fract = p - cell.as_vec3();
    let mut distance = f32::MAX;
    for z in -1..=1 {
        for y in -1..=1 {
            for x in -1..=1 {
                let feature = Vec3::new(
                    x as f32 + hash01(cell.x + x, cell.y + y, cell.z + z, seed, 0),
                    y as f32 + hash01(cell.x + x, cell.y + y, cell.z + z, seed, 1),
                    z as f32 + hash01(cell.x + x, cell.y + y, cell.z + z, seed, 2),
                );
                distance = distance.min((feature - fract).length());
            }
        }
    }
    1.0 - (distance / 1.732_050_8).clamp(0.0, 1.0)
}

fn worley_fbm2(p: Vec2, seed: u32) -> f32 {
    let mut frequency = 1.0;
    let mut amplitude = 0.58;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for octave in 0..4 {
        sum += worley2(
            p * frequency + Vec2::new(octave as f32 * 5.1, octave as f32 * -3.7),
            seed ^ octave * 977,
        ) * amplitude;
        norm += amplitude;
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    sum / norm
}

fn worley_fbm3(p: Vec3, seed: u32) -> f32 {
    let mut frequency = 1.0;
    let mut amplitude = 0.58;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for octave in 0..3 {
        sum += worley3(
            p * frequency + Vec3::splat(octave as f32 * 4.13),
            seed ^ octave * 1_301,
        ) * amplitude;
        norm += amplitude;
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    sum / norm
}

fn value_fbm3(p: Vec3, seed: u32) -> f32 {
    let mut frequency = 1.0;
    let mut amplitude = 0.58;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for octave in 0..4 {
        sum += crate::rng::vnoise3(
            p.x * frequency,
            p.y * frequency,
            p.z * frequency,
            seed ^ octave * 2_003,
            seed,
        ) * amplitude;
        norm += amplitude;
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    sum / norm
}

fn vector_noise(p: Vec3, seed: u32) -> Vec3 {
    Vec3::new(
        crate::rng::vnoise3(p.x, p.y, p.z, seed ^ 0x11, seed),
        crate::rng::vnoise3(p.x, p.y, p.z, seed ^ 0x23, seed),
        crate::rng::vnoise3(p.x, p.y, p.z, seed ^ 0x37, seed),
    )
}

fn curl_noise(p: Vec3, seed: u32) -> Vec3 {
    let epsilon = 0.08;
    let dx = Vec3::X * epsilon;
    let dy = Vec3::Y * epsilon;
    let dz = Vec3::Z * epsilon;
    let x = (vector_noise(p + dx, seed) - vector_noise(p - dx, seed)) / (2.0 * epsilon);
    let y = (vector_noise(p + dy, seed) - vector_noise(p - dy, seed)) / (2.0 * epsilon);
    let z = (vector_noise(p + dz, seed) - vector_noise(p - dz, seed)) / (2.0 * epsilon);
    Vec3::new(z.y - y.z, x.z - z.x, y.x - x.y).clamp_length_max(1.0)
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
}
