//! Planet climate visuals: a depth-aware spherical cloud-shell raymarch,
//! biome weather particles, and procedural cloud shells visible from space.

mod diag;

#[allow(unused_imports)]
pub use diag::{
    CLOUD_DIAGNOSTICS_SCHEMA_VERSION, CloudDebug, CloudDebugMode, CloudDiagnostics, DensityStats,
    FACE_CULL_MARGIN, ShellIntervals, ShellRegion, ShellSegment, composite_premultiplied,
    density_remap, density_threshold, ray_sphere, shell_intervals, truncate_to_scene,
};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::system::SystemParam;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;

use crate::save::Settings;
use crate::space::{FlightMode, SpaceScene};
use crate::visual::{CelestialLighting, ResolvedQuality, VisualFrame};
use crate::world::World;

const CLOUD_BOTTOM: f32 = crate::planet_scale::PLANET_SCALE.cloud_bottom;
const CLOUD_TOP: f32 = crate::planet_scale::PLANET_SCALE.cloud_top;
const CLOUD_WIDTH: f32 = 16_384.0;

/// Resolution of the periodic 3D cloud density texture. Horizontal coordinates
/// are mapped from canonical tangent-space distance around the spherical shell;
/// the vertical coordinate spans the shell thickness.
const DENSITY_W: u32 = 192;
const DENSITY_H: u32 = 32;
const DENSITY_D: u32 = 192;

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
        // A05: the runtime panel and the settings file share one registry, so
        // a slider can never accept a value the loader would reject.
        crate::visual::sanitize_cloud_tuning(self, None);
    }
}

#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct CloudShellUniform {
    /// xyz = visual planet center, w = terrain radius.
    pub center_radius: Vec4,
    /// x/y = cloud bottom/top radius, z = texture world period, w = elapsed time.
    pub shell: Vec4,
    /// xyz = direction toward sun, w = direct intensity.
    pub sun: Vec4,
    /// rgb = sun color, w = ambient intensity.
    pub sun_color: Vec4,
    /// rgb = ambient sky color, w = extinction coefficient.
    pub ambient: Vec4,
    /// x = coverage multiplier, y = density multiplier, z = march steps, w = HG g.
    pub quality: Vec4,
    /// xy = periodic wind offset.
    pub wind: Vec4,
    /// x = E01 debug view (0 = shipping path), y = debug scale, zw reserved.
    pub debug: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CloudShellMaterial {
    #[uniform(0)]
    pub params: CloudShellUniform,
    #[texture(1, dimension = "3d")]
    #[sampler(2)]
    pub density: Handle<Image>,
}

impl Material for CloudShellMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/cloud_shell.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/cloud_shell.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
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
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Local spherical cloud field.
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
    fingerprint: Option<(u64, u32, &'static str, bool, bool)>,
    elapsed: f32,
    density: Option<Handle<Image>>,
    material: Option<Handle<CloudShellMaterial>>,
    volume: Option<Entity>,
    density_stats: DensityStats,
}

impl ClimateRuntime {
    pub(crate) fn lifecycle_owned_handles(&self) -> usize {
        usize::from(self.density.is_some()) + usize::from(self.material.is_some())
    }

    fn release_after_world(
        &mut self,
        images: &mut Assets<Image>,
        materials: &mut Assets<CloudShellMaterial>,
    ) {
        if let Some(handle) = self.density.take() {
            images.remove(&handle);
        }
        if let Some(handle) = self.material.take() {
            materials.remove(&handle);
        }
        self.fingerprint = None;
        self.elapsed = 0.0;
        self.volume = None;
    }
}

fn cleanup_climate_runtime(
    mut runtime: ResMut<ClimateRuntime>,
    mut rain: ResMut<RainAudio>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<CloudShellMaterial>>,
) {
    // Flow cleanup despawns every InGame entity first. Drop resource-side
    // handles here so the asset stores can reclaim planet data while in menu.
    runtime.release_after_world(&mut images, &mut materials);
    rain.entity = None;
}

fn invalidate_climate_after_epoch_change(
    epoch: Res<crate::visual::WorldEpoch>,
    mut runtime: ResMut<ClimateRuntime>,
    mut rain: ResMut<RainAudio>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<CloudShellMaterial>>,
    stale_entities: Query<Entity, Or<(With<CloudVolume>, With<WeatherParticle>)>>,
) {
    let stale = runtime
        .fingerprint
        .is_some_and(|fingerprint| fingerprint.0 != epoch.0);
    if !stale {
        return;
    }
    // Planet switching happens after the regular climate system. Remove the
    // old shell/precipitation in that same frame instead of showing one frame
    // from the previous planet and waiting for next frame's fingerprint pass.
    for entity in &stale_entities {
        commands.entity(entity).despawn();
    }
    if let Some(entity) = rain.entity.take() {
        commands.entity(entity).despawn();
    }
    runtime.release_after_world(&mut images, &mut materials);
}

/// Background rain loop owned by the current in-game climate.
#[derive(Resource, Default)]
pub struct RainAudio {
    pub entity: Option<Entity>,
}

/// Play the exterior city-rain bed while weather is visible on the ground.
/// It is deliberately non-spatial so it follows the player as ambience.
pub fn rain_audio_system(
    quality: Res<ResolvedQuality>,
    mode: Res<FlightMode>,
    world: Option<Res<World>>,
    storm: Res<crate::storms::StormState>,
    mut rain: ResMut<RainAudio>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    let raining = world.as_deref().is_some_and(|world| {
        quality.weather.effective
            && matches!(*mode, FlightMode::Planet | FlightMode::Seated)
            && precipitation_active(world.biome().key, storm.kind)
            && matches!(
                world.biome().key,
                "lush" | "ocean" | "murk" | "fungal" | "amber"
            )
    });
    if raining && rain.entity.is_none() {
        rain.entity = Some(crate::audio::play_loop(
            &mut commands,
            sfx.rain.clone(),
            0.18,
        ));
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

/// Climate particles follow a visible weather event. The setting only
/// controls whether the effect may render; it does not itself mean rain.
fn precipitation_active(biome: &str, storm: crate::storms::StormKind) -> bool {
    use crate::storms::StormKind;
    match biome {
        "desert" | "salt" => storm == StormKind::Sandstorm,
        "frozen" | "crystal" => storm == StormKind::Aurora,
        "alien" | "redmoss" | "hive" => storm == StormKind::MeteorShower,
        _ => storm == StormKind::Lightning,
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

#[derive(SystemParam)]
pub struct ClimateAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    standard_materials: ResMut<'w, Assets<StandardMaterial>>,
    cloud_materials: ResMut<'w, Assets<CloudShellMaterial>>,
}

/// Cloud shader inputs from the shared celestial snapshot: direction toward
/// the sun, normalized energy and linear color.
fn cloud_sun(light: &CelestialLighting) -> (Vec3, f32, Vec3) {
    (
        light.to_sun_direction,
        light.cloud_energy(),
        Vec3::new(
            light.sun_color.red,
            light.sun_color.green,
            light.sun_color.blue,
        ),
    )
}

/// A02 consumer: the cloud shell reads the shared camera/planet snapshot and
/// the celestial-light snapshot instead of querying the sun and player again.
/// This removes the old `direction_to_sun` recomputation that could disagree
/// with the actual directional light (R043).
#[allow(clippy::too_many_arguments)]
pub fn climate_system(
    time: Res<Time>,
    quality: Res<ResolvedQuality>,
    mode: Res<FlightMode>,
    tuning: Res<CloudTuning>,
    debug: Res<CloudDebug>,
    world: Res<World>,
    storm: Res<crate::storms::StormState>,
    epoch: Res<crate::visual::WorldEpoch>,
    frame: Res<VisualFrame>,
    light: Res<CelestialLighting>,
    clear: Res<ClearColor>,
    mut runtime: ResMut<ClimateRuntime>,
    mut commands: Commands,
    mut assets: ClimateAssets,
    mut volume_clouds: Query<
        (Entity, &mut Transform, &mut Visibility),
        (With<CloudVolume>, Without<WeatherParticle>),
    >,
    mut particles: Query<
        (
            Entity,
            &mut WeatherParticle,
            &mut Transform,
            &mut Visibility,
        ),
        Without<CloudVolume>,
    >,
) {
    let show_weather = quality.weather.effective
        && matches!(*mode, FlightMode::Planet | FlightMode::Seated)
        && precipitation_active(world.biome().key, storm.kind);
    // Fingerprint the *effective* flags so a capability downgrade despawns the
    // previous climate assets instead of leaving a half-updated scene.
    let fingerprint = (
        epoch.0,
        world.seed,
        world.biome().key,
        quality.clouds.effective,
        quality.weather.effective,
    );
    if runtime.fingerprint != Some(fingerprint) {
        for (entity, _, _, _) in &particles {
            commands.entity(entity).despawn();
        }
        for (entity, _, _) in &volume_clouds {
            commands.entity(entity).despawn();
        }
        runtime.fingerprint = Some(fingerprint);
        runtime.elapsed = 0.0;
        runtime.density = None;
        runtime.material = None;
        runtime.volume = None;
        runtime.density_stats = DensityStats::default();

        if quality.clouds.effective {
            let (density_image, stats) = make_cloud_density_texture(world.seed, world.biome().key);
            let density = assets.images.add(density_image);
            runtime.density_stats = stats;
            let (sun_direction, sun_energy, sun_color) = cloud_sun(&light);
            let sky = clear.0.to_linear();
            let material = assets.cloud_materials.add(CloudShellMaterial {
                params: CloudShellUniform {
                    center_radius: frame.planet.center.extend(frame.planet.radius),
                    shell: Vec4::new(
                        frame.planet.radius + CLOUD_BOTTOM,
                        frame.planet.radius + CLOUD_TOP,
                        CLOUD_WIDTH,
                        0.0,
                    ),
                    sun: sun_direction.extend(sun_energy),
                    sun_color: sun_color.extend(0.55),
                    ambient: Vec4::new(sky.red, sky.green, sky.blue, 0.018),
                    quality: Vec4::new(
                        tuning.coverage,
                        tuning.density,
                        tuning.raymarch_steps as f32,
                        0.72,
                    ),
                    wind: Vec4::ZERO,
                    debug: Vec4::new(debug.mode.shader_mode() as f32, 1.0, 0.0, 0.0),
                },
                density: density.clone(),
            });
            let volume = spawn_cloud_volume(&mut commands, &mut assets.meshes, &material, &frame);
            runtime.density = Some(density);
            runtime.material = Some(material);
            runtime.volume = Some(volume);
        }

        if quality.weather.effective {
            let def = weather_def(world.biome().key);
            let mesh = assets
                .meshes
                .add(Cuboid::new(def.size.x, def.size.y, def.size.z));
            let material = assets.standard_materials.add(StandardMaterial {
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
                    Transform::from_translation(particle_position(&world, frame.eye, index, 0)),
                    WeatherParticle {
                        speed: def.speed,
                        index,
                        generation: 0,
                    },
                    if show_weather {
                        Visibility::Visible
                    } else {
                        Visibility::Hidden
                    },
                    crate::InGame,
                ));
            }
        }
        return;
    }

    let dt = time.delta_secs();
    runtime.elapsed += dt;
    let show_clouds = quality.clouds.effective && mode.ground_scene();

    if mode.ground_scene() && runtime.volume.is_none() && quality.clouds.effective {
        let density = match runtime.density.clone() {
            Some(d) => d,
            None => {
                let (image, stats) = make_cloud_density_texture(world.seed, world.biome().key);
                runtime.density_stats = stats;
                let d = assets.images.add(image);
                runtime.density = Some(d.clone());
                d
            }
        };
        let (sun_direction, sun_energy, sun_color) = cloud_sun(&light);
        let sky = clear.0.to_linear();
        let material = assets.cloud_materials.add(CloudShellMaterial {
            params: CloudShellUniform {
                center_radius: frame.planet.center.extend(frame.planet.radius),
                shell: Vec4::new(
                    frame.planet.radius + CLOUD_BOTTOM,
                    frame.planet.radius + CLOUD_TOP,
                    CLOUD_WIDTH,
                    runtime.elapsed,
                ),
                sun: sun_direction.extend(sun_energy),
                sun_color: sun_color.extend(0.55),
                ambient: Vec4::new(sky.red, sky.green, sky.blue, 0.018),
                quality: Vec4::new(
                    tuning.coverage,
                    tuning.density,
                    tuning.raymarch_steps as f32,
                    0.72,
                ),
                wind: Vec4::ZERO,
                debug: Vec4::new(debug.mode.shader_mode() as f32, 1.0, 0.0, 0.0),
            },
            density,
        });
        runtime.volume = Some(spawn_cloud_volume(
            &mut commands,
            &mut assets.meshes,
            &material,
            &frame,
        ));
        runtime.material = Some(material);
    }

    // Wind: scroll the repeating density texture in UV space. The texture is
    // toroidal, so the drift wraps around seamlessly.
    let wind = Vec3::new(
        runtime.elapsed * WIND_UV_PER_SEC,
        0.0,
        runtime.elapsed * WIND_UV_PER_SEC * 0.15,
    );
    let sky = clear.0.to_linear();
    let (sun_direction, sun_energy, sun_color) = cloud_sun(&light);
    if let Some(mut material) = runtime
        .material
        .as_ref()
        .and_then(|handle| assets.cloud_materials.get_mut(handle))
    {
        let high_altitude_fade = 1.0
            - crate::planet_scale::smoothstep(
                crate::planet_scale::PLANET_SCALE.sky_space_fade_start,
                crate::planet_scale::PLANET_SCALE.sky_space_fade_end,
                frame.eye.y,
            );
        material.params.center_radius = frame.planet.center.extend(frame.planet.radius);
        material.params.shell = Vec4::new(
            frame.planet.radius + CLOUD_BOTTOM,
            frame.planet.radius + CLOUD_TOP,
            CLOUD_WIDTH,
            runtime.elapsed,
        );
        material.params.sun = sun_direction.extend(sun_energy);
        material.params.sun_color = sun_color.extend(0.55);
        material.params.ambient = Vec4::new(sky.red, sky.green, sky.blue, 0.018);
        material.params.quality = Vec4::new(
            tuning.coverage.clamp(0.0, 1.0),
            tuning.density.clamp(0.0, 1.0) * high_altitude_fade,
            tuning.raymarch_steps.clamp(4, 64) as f32,
            0.72,
        );
        material.params.wind = wind.extend(0.0);
        // E01: the debug view is read from the uniform every frame so a switch
        // does not rebuild the material or the density asset.
        material.params.debug = Vec4::new(debug.mode.shader_mode() as f32, 1.0, 0.0, 0.0);
    }
    for (entity, mut transform, mut visibility) in &mut volume_clouds {
        if runtime.volume != Some(entity) {
            continue;
        }
        transform.translation = frame.planet.center;
        *visibility = if show_clouds {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
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
                particle_position(&world, frame.eye, particle.index, particle.generation);
        }
    }
}

/// E01: publish the settings truth check and the center-ray interval model
/// every frame. Values are read back from the live material uniform, so the
/// report cannot claim a request that the shader is not using (R061/R117).
pub fn collect_cloud_diagnostics(
    frame: Res<VisualFrame>,
    runtime: Res<ClimateRuntime>,
    tuning: Res<CloudTuning>,
    debug: Res<CloudDebug>,
    world: Option<Res<World>>,
    materials: Res<Assets<CloudShellMaterial>>,
    mut diagnostics: ResMut<CloudDiagnostics>,
) {
    let shell_inner = crate::planet_scale::PLANET_SCALE.cloud_bottom;
    let shell_outer = crate::planet_scale::PLANET_SCALE.cloud_top;
    let inner_radius = frame.planet.radius + shell_inner;
    let outer_radius = frame.planet.radius + shell_outer;
    let center_ray = shell_intervals(
        frame.eye,
        frame.forward,
        frame.planet.center,
        inner_radius,
        outer_radius,
    );
    let effective = runtime
        .material
        .as_ref()
        .and_then(|handle| materials.get(handle))
        .map(|material| {
            (
                material.params.quality.x,
                material.params.quality.y,
                material.params.quality.z as u32,
            )
        });
    let (coverage_effective, density_effective, steps_effective) = effective.unwrap_or_default();

    diagnostics.cloud_active = effective.is_some();
    diagnostics.biome = world
        .as_deref()
        .map(|world| world.biome().key.to_string())
        .unwrap_or_default();
    diagnostics.world_seed = world.as_deref().map(|world| world.seed).unwrap_or(0);
    diagnostics.coverage_requested = tuning.coverage;
    diagnostics.density_requested = tuning.density;
    diagnostics.steps_requested = tuning.raymarch_steps;
    diagnostics.coverage_effective = coverage_effective;
    diagnostics.density_effective = density_effective;
    diagnostics.steps_effective = steps_effective;
    diagnostics.legacy_render_resolution = [tuning.render_resolution.x, tuning.render_resolution.y];
    diagnostics.effective_render_resolution = [frame.viewport.x, frame.viewport.y];
    diagnostics.render_resolution_inert = true;
    diagnostics.shell_inner_altitude = shell_inner;
    diagnostics.shell_outer_altitude = shell_outer;
    diagnostics.center_ray = center_ray;
    diagnostics.density_stats = runtime.density_stats;
    diagnostics.debug_mode = debug.mode.key().to_string();
    diagnostics.notes = vec![
        "density stats are computed once, when the 3D texture is built".to_string(),
        "the single-interval shader drops `far` when the ray crosses the cloud bottom; E04 decides whether to march it"
            .to_string(),
        "cloud_render_width/height are legacy save fields and do not resize any render target (R061)"
            .to_string(),
    ];
}

fn spawn_cloud_volume(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<CloudShellMaterial>,
    frame: &VisualFrame,
) -> Entity {
    commands
        .spawn((
            Mesh3d(
                meshes.add(
                    Sphere::new(frame.planet.radius + CLOUD_TOP + 16.0)
                        .mesh()
                        .ico(5)
                        .expect("cloud shell sphere"),
                ),
            ),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(frame.planet.center),
            Visibility::Visible,
            NoFrustumCulling,
            CloudVolume,
            crate::InGame,
        ))
        .id()
}

fn sync_cloud_visual_frame(
    frame: Res<crate::planet_scale::PlanetVisualFrame>,
    runtime: Res<ClimateRuntime>,
    mut materials: ResMut<Assets<CloudShellMaterial>>,
    mut clouds: Query<(Entity, &mut Transform), With<CloudVolume>>,
) {
    if let Some(mut material) = runtime
        .material
        .as_ref()
        .and_then(|handle| materials.get_mut(handle))
    {
        material.params.center_radius = frame.center.extend(frame.radius);
        material.params.shell.x = frame.radius + CLOUD_BOTTOM;
        material.params.shell.y = frame.radius + CLOUD_TOP;
    }
    for (entity, mut transform) in &mut clouds {
        if runtime.volume == Some(entity) {
            transform.translation = frame.center;
        }
    }
}

fn repeat_sampler() -> ImageSampler {
    let mut descriptor = ImageSamplerDescriptor::linear();
    descriptor.address_mode_u = ImageAddressMode::Repeat;
    descriptor.address_mode_v = ImageAddressMode::ClampToEdge;
    descriptor.address_mode_w = ImageAddressMode::Repeat;
    descriptor.mag_filter = ImageFilterMode::Linear;
    ImageSampler::Descriptor(descriptor)
}

/// Builds the 3D cloud density texture for a biome/seed.
///
/// The texture maps exactly once over the fog box, so every octave is
/// *toroidal*: cell indices wrap around the texture and the
/// Worley distance is measured on the circle, which keeps the box edges and
/// the wind scroll free of seams. Coverage comes from 2D Worley FBM in the
/// x/z plane (a few large weather systems), erosion from 3D Worley FBM, and a
/// vertical profile makes the deck soft at the bottom and top.
///
/// Returns the texture plus its statistics (E01): scanning the bytes here is
/// free compared to rescanning 1.2M voxels every frame for diagnostics.
fn make_cloud_density_texture(seed: u32, biome: &str) -> (Image, DensityStats) {
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
                // Preserve roughly the old cloud-system world size after the
                // field grew from 1.1 km to 16 km.
                let cover =
                    worley_fbm2_periodic(Vec2::new(px, pz) * 60.0, 60.0, biome_seed ^ 0x51A7);
                let cover = smoothstep(threshold, (threshold + 0.22).min(0.95), cover);
                // 3D detail erosion
                let detail =
                    worley_fbm3_periodic(Vec3::new(px, py, pz) * 90.0, 90.0, biome_seed ^ 0xC011);
                let detail = smoothstep(0.42, 0.78, detail);
                let density = (cover * (0.30 + 0.70 * detail) * profile).clamp(0.0, 1.0);
                let at = ((z * DENSITY_H + y) * DENSITY_W + x) as usize;
                bytes[at] = (density * 255.0) as u8;
            }
        }
    }
    let stats = DensityStats::from_bytes(&bytes);
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
    (image, stats)
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
    quality: Res<ResolvedQuality>,
    scene: Option<Res<SpaceScene>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(Entity, &SpaceCloud)>,
    mut clouds: Query<(&SpaceCloud, &mut Transform)>,
) {
    if !quality.clouds.effective {
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

/// Weather/clouds plugin. Tuning is captured from settings in `main` and
/// passed in (no resource lookup during plugin build).
pub struct WeatherPlugin {
    pub cloud: CloudTuning,
}

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<CloudShellMaterial>::default())
            .insert_resource(self.cloud)
            .init_resource::<ClimateRuntime>()
            .init_resource::<RainAudio>()
            .init_resource::<CloudDebug>()
            .init_resource::<CloudDiagnostics>()
            .add_systems(
                Update,
                (climate_system, rain_audio_system, collect_cloud_diagnostics)
                    .chain()
                    .after(crate::storms::storm_director_system)
                    .in_set(crate::schedule::GameSet::CommonWeather)
                    .run_if(in_state(crate::schedule::GameState::Playing)),
            )
            .add_systems(
                Update,
                // 与 JS 原版一致：云壳独立于地面天气链，逐一注册
                space_cloud_system.run_if(in_state(crate::schedule::GameState::Playing)),
            )
            .add_systems(
                Update,
                invalidate_climate_after_epoch_change
                    .after(crate::schedule::GameSet::LateSwitchFlow)
                    .run_if(in_state(crate::schedule::GameState::Playing)),
            )
            .add_systems(
                PostUpdate,
                sync_cloud_visual_frame
                    .after(crate::planet_scale::update_visual_frame)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                OnExit(crate::schedule::GameState::Playing),
                cleanup_climate_runtime,
            );
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
            assert!(
                (a - b).abs() < 1e-4,
                "2D seam at period {period}: {a} vs {b}"
            );
        }
        let a = worley_fbm3_periodic(Vec3::new(0.0, 0.5, 0.3) * 6.0, 6.0, seed);
        let b = worley_fbm3_periodic(Vec3::new(1.0, 0.5, 0.3) * 6.0, 6.0, seed);
        assert!((a - b).abs() < 1e-4, "3D seam: {a} vs {b}");
    }
}
