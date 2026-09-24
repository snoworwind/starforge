//! Day/night cycle, sky color, sun light, stars & space preview.

use crate::player::Player;
use crate::space::FlightMode;
use crate::world::World;
use bevy::camera::Exposure;
use bevy::light::{
    Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, GlobalAmbientLight,
    SunDisk, VolumetricLight, atmosphere::ScatteringMedium, light_consts::lux,
};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The local voxel world is a small section of a much larger planet. These
/// radii keep the atmospheric horizon visually close to flat while still
/// giving Bevy's scattering integrator a real shell to march through.
pub const GROUND_ATMOSPHERE_INNER_RADIUS: f32 =
    crate::planet_scale::PLANET_SCALE.local_planet_radius;
pub const GROUND_ATMOSPHERE_OUTER_RADIUS: f32 =
    GROUND_ATMOSPHERE_INNER_RADIUS + crate::planet_scale::PLANET_SCALE.atmosphere_top;
/// The voxel scene is much smaller than Bevy's physical-scale examples, so
/// the physical RAW_SUNLIGHT value needs a strong direct-light boost.
///
/// NOTE: this is now `1.0` — physical sunlight. The old `15.0` boost was
/// tuned against the removed AutoExposure pass, which permanently clamped the
/// exposure to −3 EV (the `-3..3` histogram saturated at its top bin in
/// daylight), so the boost only compensated for that darkening. With the
/// fixed `Exposure { ev100: 13.0 }` baseline (Bevy's atmosphere example
/// configuration), RAW_SUNLIGHT × 1.0 is the correct physical value.
pub const DIRECT_SUNLIGHT_BOOST: f32 = 1.0;
/// Indirect fill keeps shadowed voxel faces and night terrain readable.
/// The runtime F3 values remain multipliers, so saved settings need no
/// migration.
const GROUND_ATMOSPHERE_FILL_BASE: f32 = 0.90;
const GROUND_AMBIENT_BASE: f32 = 0.90;
const GROUND_DAY_EXPOSURE_EV100: f32 = 13.0;
const GROUND_NIGHT_EXPOSURE_BIAS: f32 = 5.5;
const GROUND_SHELTER_EXPOSURE_BIAS: f32 = 2.0;

fn camera_has_opaque_roof(world: Option<&World>, position: Vec3) -> bool {
    let Some(world) = world else { return false };
    let x = position.x.floor() as i32;
    let z = position.z.floor() as i32;
    let first_y = position.y.floor() as i32 + 1;
    let last_y = (first_y + 12).min(crate::data::WORLD_H - 1);
    for y in first_y..=last_y {
        let block = crate::data::block_by_id(world.get(x, y, z));
        if block.solid && !block.transparent && !block.liquid {
            return true;
        }
    }
    false
}

/// Runtime lighting controls exposed by the in-game F3 panel. These are
/// deliberately kept separate from the physical scene setup so artists can
/// tune the presentation without recompiling the renderer.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightingTuning {
    /// Multiplier for direct sunlight at full day.
    pub sunlight_boost: f32,
    /// Visible Raymarched SunDisk intensity.
    pub sun_disk_intensity: f32,
    /// Outdoor atmosphere-to-PBR environment fill.
    pub atmosphere_fill: f32,
    /// Global indirect ambient-light multiplier.
    pub ambient_multiplier: f32,
    /// Atmosphere environment fill used by the orbital scene.
    pub space_atmosphere_fill: f32,
    /// Bloom spill strength applied to the HDR camera.
    pub bloom_intensity: f32,
    /// Brightness threshold at which Bloom starts contributing.
    pub bloom_threshold: f32,
    /// Soft transition width around the Bloom threshold.
    pub bloom_threshold_softness: f32,
    /// Low-frequency Bloom spill/halo boost.
    pub bloom_low_frequency_boost: f32,
}

impl Default for LightingTuning {
    fn default() -> Self {
        Self {
            sunlight_boost: DIRECT_SUNLIGHT_BOOST,
            sun_disk_intensity: 1.0,
            atmosphere_fill: 1.0,
            ambient_multiplier: 1.0,
            space_atmosphere_fill: 1.0,
            bloom_intensity: 0.12,
            bloom_threshold: 1.5,
            bloom_threshold_softness: 0.2,
            bloom_low_frequency_boost: 0.35,
        }
    }
}

impl LightingTuning {
    /// Load the runtime lighting controls from the user settings file. Older
    /// settings files deserialize to `Default` because the resource carries
    /// serde defaults for every field.
    pub fn from_settings(settings: &crate::save::Settings) -> Self {
        let mut tuning = settings.lighting;
        tuning.sanitize();
        tuning
    }

    pub fn save_to_settings(self, settings: &mut crate::save::Settings) {
        let mut tuning = self;
        tuning.sanitize();
        settings.lighting = tuning;
    }

    pub fn sanitize(&mut self) {
        // A05: ranges/defaults come from the shared settings registry instead
        // of a second copy in this file.
        crate::visual::sanitize_lighting_tuning(self, None);
    }
}

#[derive(Resource, Clone)]
pub struct AtmosphereAssets {
    pub earth: Handle<ScatteringMedium>,
}

#[derive(Component)]
pub struct GroundAtmosphere;

/// Day time in [0,1): 0.25 = noon, 0.75 = midnight.
#[derive(Resource)]
pub struct DayTime(pub f32);

/// Space factor 0 (ground) .. 1 (space) — drives star visibility & black sky.
#[derive(Resource, Default)]
pub struct SpaceFactor(pub f32);

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub struct GroundMoon;

#[derive(Component)]
pub struct Star;

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let a = a.to_linear();
    let b = b.to_linear();
    Color::LinearRgba(LinearRgba {
        red: a.red + (b.red - a.red) * t,
        green: a.green + (b.green - a.green) * t,
        blue: a.blue + (b.blue - a.blue) * t,
        alpha: a.alpha + (b.alpha - a.alpha) * t,
    })
}

/// Day factor: 1.0 at noon, 0.0 at midnight (smooth).
pub fn day_factor(t: f32) -> f32 {
    ((t * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2).sin() * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Direction that sunlight travels from the sun toward the world. Deriving
/// elevation from the same day factor used for illuminance keeps a bright sun
/// high in the sky, which gives the cascades a useful, visible shadow length
/// instead of leaving the default starting frame almost horizontal.
fn sun_travel_direction(day_time: f32, daylight: f32) -> Vec3 {
    // Keep the noon sun high enough for a natural sky, but not vertical: a
    // small amount of horizontal travel makes voxel-caster silhouettes
    // readable on the ground instead of hiding them directly underneath.
    let elevation = (daylight.mul_add(2.0, -1.0)).clamp(-1.0, 1.0).asin() * 0.68;
    let horizontal = elevation.cos();
    let azimuth = day_time * std::f32::consts::TAU;
    Vec3::new(
        azimuth.cos() * horizontal,
        -elevation.sin(),
        azimuth.sin() * horizontal,
    )
}

/// D01 QA helper: per-channel scales for sun / ambient / environment fill.
/// `Full` is the shipping look; the other two modes let the visual QA matrix
/// store paired "sun only" and "ambient only" frames for exposure review.
fn probe_scales(mode: crate::rendering::LightingProbeMode) -> (f32, f32, f32) {
    use crate::rendering::LightingProbeMode as Mode;
    match mode {
        Mode::Full => (1.0, 1.0, 1.0),
        Mode::SunOnly => (1.0, 0.0, 0.0),
        Mode::AmbientOnly => (0.0, 1.0, 1.0),
        // B04 overcast: dim hard sun, lift the sky/environment fill so
        // roughness is judged by broad reflection instead of a key light.
        Mode::Overcast => (0.35, 1.15, 1.0),
    }
}

pub fn daynight_system(
    time: Res<Time>,
    mut day: ResMut<DayTime>,
    mut sun_q: Query<
        (
            &mut Transform,
            &mut DirectionalLight,
            &mut SunDisk,
            &mut Visibility,
            Option<&Sun>,
        ),
        (With<SunDisk>, Without<Star>, Without<GroundAtmosphere>),
    >,
    mut moon_q: Query<
        (&mut Transform, &mut DirectionalLight, &mut Visibility),
        (
            With<GroundMoon>,
            Without<SunDisk>,
            Without<Star>,
            Without<GroundAtmosphere>,
        ),
    >,
    mut stars: Query<&mut Visibility, (With<Star>, Without<Sun>)>,
    mut ground_atmosphere: Query<&mut Transform, (With<GroundAtmosphere>, Without<Sun>)>,
    frame: Res<crate::visual::VisualFrame>,
    mut celestial: ResMut<crate::visual::CelestialLighting>,
    mut clear: ResMut<ClearColor>,
    world: Option<Res<World>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    // One bundled query keeps the system within Bevy's 16-parameter limit while
    // still owning every per-camera presentation component.
    mut cameras: Query<
        (
            &mut AtmosphereEnvironmentMapLight,
            &mut Bloom,
            &mut DistanceFog,
            Option<&mut Exposure>,
            &GlobalTransform,
        ),
        (With<Camera3d>, Without<Player>),
    >,
    mut space: ResMut<SpaceFactor>,
    mode: Res<FlightMode>,
    tuning: Res<LightingTuning>,
    probe: Option<Res<crate::rendering::LightingProbe>>,
) {
    day.0 = (day.0 + time.delta_secs() / 480.0) % 1.0; // JS DAY_LEN=480s 全周期
    let f = day_factor(day.0);

    let sun_direction = sun_travel_direction(day.0, f);
    // D01 QA matrix: one-effect-at-a-time scales. `Full` is the shipping look;
    // the probe never touches `Settings` or the save file.
    let (sun_scale, ambient_scale, environment_scale) =
        probe_scales(probe.as_deref().map(|probe| probe.mode).unwrap_or_default());

    let sunlight_boost = tuning.sunlight_boost.max(0.0);
    let daylight_boost = 1.0 + f * (sunlight_boost - 1.0);
    for (mut tf, mut light, mut disk, mut visibility, ground_sun) in &mut sun_q {
        // At night the ground atmosphere should not receive a visible solar
        // beam or disk. The orbital sun remains visible because it is outside
        // the day/night horizon model.
        let ground_daylight = ground_sun.is_some();
        let probe_ambient_only = sun_scale <= 0.0;
        disk.intensity = if probe_ambient_only {
            0.0
        } else {
            tuning.sun_disk_intensity.max(0.0) * if ground_daylight { f } else { 1.0 }
        };
        if ground_sun.is_none() {
            // The orbital sun is a separate directional-light entity, but it
            // uses the same live direct-light control as the ground sun.
            light.illuminance = lux::RAW_SUNLIGHT * sunlight_boost * sun_scale;
            // There must be exactly one direct sun in the ground scene. The
            // orbital light otherwise fills every cast shadow from a second
            // direction and makes the ground look shadowless.
            *visibility = if mode.space_scene() {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            continue;
        }
        // `DirectionalLight` shines along local -Z. `sun_direction` is the
        // direction the sunlight travels from the sun toward the ground, so
        // align -Z with it (rather than +Z, which would point the light up
        // into the sky and leave the terrain unlit).
        tf.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, sun_direction);
        tf.translation = Vec3::ZERO;
        // Keep a subtle moonlit direct beam at night so PBR surfaces remain
        // legible; the atmosphere still carries the cool night palette.
        // Daylight retains the full physical sunlight range.
        let sun_illuminance =
            lux::RAW_SUNLIGHT * (0.003 + f * 0.997) * daylight_boost.max(0.0) * sun_scale;
        light.illuminance = sun_illuminance;
        // 日出/日落（f≈0.5，太阳贴近地平线）阳光是暖橙红色，正午（f=1）
        // 是暖白，夜间（f<0.5）是冷蓝。旧曲线方向反了：f=0.5 处给冷色、
        // 正午给暖黄，导致日出日落没有彩霞。方向光颜色同时驱动太阳盘、
        // 大气散射和体积云散射，改这里三处一起变暖。
        let sunrise = Color::srgb(1.0, 0.55, 0.28);
        let noon = Color::srgb(1.0, 0.97, 0.9);
        let night = Color::srgb(0.6, 0.72, 1.0);
        light.color = if f >= 0.5 {
            // 日出 → 正午 / 正午 → 日落：低角度暖橙，高角度暖白
            lerp_color(sunrise, noon, (f - 0.5) * 2.0)
        } else {
            // 午夜 → 日出：冷蓝 → 暖橙（夜间光照本就接近零，颜色影响很小）
            lerp_color(night, sunrise, f * 2.0)
        };
        // A02: publish the same numbers other presentation systems consume.
        // Cloud shading previously re-queried this light and recomputed the
        // energy; keeping one source avoids divergent units (R043).
        celestial.daylight = f;
        celestial.travel_direction = sun_direction;
        celestial.to_sun_direction = -sun_direction;
        celestial.sun_color = light.color.to_linear();
        celestial.sun_illuminance_lux = sun_illuminance;
        celestial.sun_disk_intensity = disk.intensity;
        celestial.ground_sun_visible = true;
        *visibility = if mode.space_scene() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }

    // Moonlight comes from above the horizon, independently of the sun's
    // below-horizon direction. A low blue key light keeps terrain and PBR
    // surfaces readable without lifting the whole night sky.
    let moon_direction = Vec3::new(0.35, -0.82, 0.45).normalize();
    for (mut transform, mut light, mut visibility) in &mut moon_q {
        transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, moon_direction);
        transform.translation = Vec3::ZERO;
        light.illuminance = lux::RAW_SUNLIGHT * 0.002 * (1.0 - f) * sun_scale;
        light.color = Color::srgb(0.58, 0.70, 1.0);
        *visibility = if mode.ground_scene() && f < 0.98 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    // Keep the local atmosphere out of the candidate set while the camera is
    // in the separate space scene. Bevy chooses the nearest Atmosphere entity
    // without consulting Visibility, so moving this proxy far away is the
    // lightweight way to switch between the local and orbital shells.
    let atmosphere_center = if mode.ground_scene() {
        frame.planet.center
    } else {
        Vec3::new(0.0, 1.0e9, 0.0)
    };
    for mut transform in &mut ground_atmosphere {
        transform.translation = atmosphere_center;
    }

    // space factor from altitude (finalized camera snapshot)
    let cam_y = frame.eye.y;
    // 太空/曲速/空间站模式强制 1：Bevy 单相机 ClearColor 全屏共享，而太空态玩家坐标
    // 已被镜像到星球球面坐标系（赤道附近 Y≈0），按高度计算会退化成星球大气色
    // （JS 原版太空为独立场景固定底色，不存在此问题）。
    let sf = if mode.space_scene() {
        1.0
    } else {
        crate::planet_scale::space_fade(cam_y)
    };
    space.0 = sf;

    let space_black = Color::srgb(0.005, 0.008, 0.02);
    let day_sky = match &world {
        Some(w) => {
            let b = w.biome();
            Color::srgb(b.sky.0, b.sky.1, b.sky.2)
        }
        None => Color::srgb(0.48, 0.72, 0.95),
    };
    let night_sky = Color::srgb(0.012, 0.016, 0.05);
    let mut sky = lerp_color(day_sky, night_sky, 1.0 - f);
    // 日出/日落彩霞：太阳贴近地平线（f≈0.5）时天空混入暖橙辉光，
    // 远离地平线（f→0 或 f→1）时辉光消失。ClearColor 同时驱动雾色和
    // 云的 ambient_color，所以彩霞会自然蔓延到远景和云层。
    let horizon_glow = (1.0 - (f - 0.5).abs() * 4.0).clamp(0.0, 1.0);
    let glow_color = Color::srgb(1.0, 0.55, 0.3);
    sky = lerp_color(sky, glow_color, horizon_glow * 0.45);
    sky = lerp_color(sky, space_black, sf);
    clear.0 = sky;

    // Distance fog now models haze instead of erasing the flat terrain before
    // the orbital hand-off.  The old climb curve crushed the far distance to
    // roughly 120 units at exit and produced a visibly raised fog horizon.
    let atmosphere_thin = crate::planet_scale::smoothstep(600.0, 2_400.0, cam_y);
    let fog_color = lerp_color(sky, Color::WHITE, 0.15 * f * (1.0 - sf));

    // Keep these as scene-wide artist controls. Occlusion must be evaluated by
    // the shadow/occlusion passes per surface; changing them from camera roof
    // detection would incorrectly darken outdoor objects visible through a
    // doorway or window.
    let atmosphere_fill = if mode.space_scene() {
        tuning.space_atmosphere_fill.max(0.0)
    } else {
        GROUND_ATMOSPHERE_FILL_BASE * tuning.atmosphere_fill.max(0.0)
    } * environment_scale;
    let mut exposure_ev100 = GROUND_DAY_EXPOSURE_EV100;
    let exposure_blend = (1.0 - (-time.delta_secs().clamp(0.0, 0.1) * 12.0).exp()).clamp(0.0, 1.0);
    for (mut fill, mut bloom, mut fog, mut exposure, camera_transform) in &mut cameras {
        if mode.space_scene() {
            fog.falloff = FogFalloff::Linear {
                start: 1e9,
                end: 1e9,
            };
        } else {
            let start = 120.0 + atmosphere_thin * 1_080.0;
            let end = 2_400.0 + atmosphere_thin * 5_600.0;
            fog.falloff = FogFalloff::Linear { start, end };
            fog.color = fog_color;
        }
        fill.intensity = atmosphere_fill;
        bloom.intensity = tuning.bloom_intensity.max(0.0);
        bloom.low_frequency_boost = tuning.bloom_low_frequency_boost.max(0.0);
        bloom.prefilter.threshold = tuning.bloom_threshold.max(0.0);
        bloom.prefilter.threshold_softness = tuning.bloom_threshold_softness.clamp(0.0, 1.0);
        let shelter_bias =
            if camera_has_opaque_roof(world.as_deref(), camera_transform.translation()) {
                GROUND_SHELTER_EXPOSURE_BIAS
            } else {
                0.0
            };
        let target_exposure =
            (GROUND_DAY_EXPOSURE_EV100 - (1.0 - f) * GROUND_NIGHT_EXPOSURE_BIAS - shelter_bias)
                .max(7.5);
        if let Some(exposure) = exposure.as_deref_mut() {
            exposure.ev100 += (target_exposure - exposure.ev100) * exposure_blend;
            exposure_ev100 = exposure.ev100;
        }
    }

    let day_amb = Color::srgb(0.75, 0.8, 0.9);
    let night_amb = Color::srgb(0.16, 0.17, 0.26);
    ambient.color = lerp_color(day_amb, night_amb, 1.0 - f);
    // 夜间保留暗部层次，并为阴影中的地形留出可辨识的亮度。
    // F3 ambient_multiplier 仍可整体缩放。
    ambient.brightness = (30.0 + f * 50.0)
        * GROUND_AMBIENT_BASE
        * tuning.ambient_multiplier.max(0.0)
        * ambient_scale;

    celestial.ambient_color = ambient.color.to_linear();
    celestial.ambient_brightness = ambient.brightness;
    celestial.environment_fill = atmosphere_fill;
    celestial.exposure_ev100 = exposure_ev100;

    for mut vis in &mut stars {
        *vis = if sf > 0.6 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// Spawn the sun, stars and lamp pool lights.
pub fn spawn_sky(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    earth_medium: Handle<ScatteringMedium>,
) -> Vec<Entity> {
    // Bevy's atmosphere pass renders both the Raymarched sky and the visible
    // SunDisk. The old emissive sun mesh is intentionally gone so there is a
    // single physically-sized sun in ground and atmospheric flight.
    commands.spawn((
        Atmosphere {
            inner_radius: GROUND_ATMOSPHERE_INNER_RADIUS,
            outer_radius: GROUND_ATMOSPHERE_OUTER_RADIUS,
            ground_albedo: Vec3::splat(0.3),
            medium: earth_medium,
        },
        Transform::from_xyz(
            0.0,
            crate::data::SEA_Y - GROUND_ATMOSPHERE_INNER_RADIUS,
            0.0,
        ),
        GroundAtmosphere,
        crate::InGame,
    ));

    // directional sunlight
    commands.spawn((
        DirectionalLight {
            illuminance: lux::RAW_SUNLIGHT * DIRECT_SUNLIGHT_BOOST,
            shadow_maps_enabled: true,
            contact_shadows_enabled: true,
            // The voxel terrain is small relative to the 4096px cascade.
            // Keep the bias tight so nearby casters do not detach from the
            // ground and make their shadows appear to be missing.
            shadow_depth_bias: 0.005,
            shadow_normal_bias: 0.25,
            ..default()
        },
        // Slightly overexpose the physical disk so Bloom has a stable bright
        // source at the low angular size of an Earth-like sun.
        SunDisk {
            intensity: 18.0,
            ..SunDisk::EARTH
        },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            first_cascade_far_bound: 40.0,
            maximum_distance: 1200.0,
            ..default()
        }
        .build(),
        Transform::IDENTITY,
        // Keep the volumetric marker for other fog/light-shaft effects; the
        // spherical cloud shader reads the same live sun transform directly.
        VolumetricLight,
        Sun,
        crate::InGame,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 0.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::IDENTITY,
        GroundMoon,
        crate::InGame,
    ));
    // stars: small emissive quads on a dome
    let mut rng = crate::rng::Rng::new(0x57A1);
    let star_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::WHITE * 1.5,
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let quad = meshes.add(Plane3d::default().mesh().size(3.0, 3.0));
    for _ in 0..400 {
        let az = rng.next() * std::f32::consts::TAU;
        let el = (rng.next() * 2.0 - 1.0) * 0.95;
        let r = 950.0;
        let y = el * r;
        let rr = (r * r - y * y).max(0.0).sqrt();
        let pos = Vec3::new(az.cos() * rr, y, az.sin() * rr);
        commands.spawn((
            Mesh3d(quad.clone()),
            MeshMaterial3d(star_mat.clone()),
            Transform::from_translation(pos).looking_at(Vec3::ZERO, Vec3::Y),
            Visibility::Hidden,
            Star,
            crate::InGame,
        ));
    }
    // lamp pool (6 point lights)
    let mut pool = Vec::new();
    for _ in 0..6 {
        let e = commands
            .spawn((
                PointLight {
                    color: Color::srgb(1.0, 0.85, 0.62),
                    intensity: 0.0,
                    range: 11.0,
                    ..default()
                },
                crate::InGame,
            ))
            .id();
        pool.push(e);
    }
    pool
}

/// Day/night, sky & atmosphere plugin. Tuning is captured from settings in
/// `main` and passed in (no resource lookup during plugin build).
pub struct DayNightPlugin {
    pub lighting: LightingTuning,
}

fn sync_ground_atmosphere(
    mode: Res<FlightMode>,
    visual_frame: Res<crate::planet_scale::PlanetVisualFrame>,
    mut atmosphere: Query<&mut Transform, With<GroundAtmosphere>>,
) {
    let center = if mode.ground_scene() {
        visual_frame.center
    } else {
        Vec3::new(0.0, 1.0e9, 0.0)
    };
    for mut transform in &mut atmosphere {
        transform.translation = center;
    }
}

impl Plugin for DayNightPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.lighting)
            .add_systems(
                Update,
                daynight_system
                    .in_set(crate::schedule::GameSet::CommonDaynight)
                    .run_if(in_state(crate::schedule::GameState::Playing)),
            )
            .add_systems(
                Update,
                space_sky_sync_system
                    .in_set(crate::schedule::GameSet::LateSwitchSky)
                    .run_if(in_state(crate::schedule::GameState::Playing)),
            )
            .add_systems(
                PostUpdate,
                sync_ground_atmosphere
                    .after(crate::planet_scale::update_visual_frame)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}
fn space_sky_sync_system(
    mode: Res<crate::space::FlightMode>,
    mut stars: Query<&mut Visibility, With<Star>>,
) {
    let show = mode.ground_scene();
    for mut vis in &mut stars {
        *vis = if show && stars_ok() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn stars_ok() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::LightingProbeMode;

    #[test]
    fn lighting_probe_scales_isolate_one_source_at_a_time() {
        assert_eq!(probe_scales(LightingProbeMode::Full), (1.0, 1.0, 1.0));
        assert_eq!(probe_scales(LightingProbeMode::SunOnly), (1.0, 0.0, 0.0));
        assert_eq!(
            probe_scales(LightingProbeMode::AmbientOnly),
            (0.0, 1.0, 1.0)
        );
        let (sun, ambient, environment) = probe_scales(LightingProbeMode::Overcast);
        assert!(sun < 0.5 && ambient > 1.0 && environment == 1.0);
    }
}
