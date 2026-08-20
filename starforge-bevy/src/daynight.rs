//! Day/night cycle, sky color, sun light, stars & space preview.

use crate::player::Player;
use crate::space::FlightMode;
use crate::world::World;
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
pub const GROUND_ATMOSPHERE_INNER_RADIUS: f32 = 2_000.0;
pub const GROUND_ATMOSPHERE_OUTER_RADIUS: f32 = 2_320.0;
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

/// Runtime lighting controls exposed by the in-game F3 panel. These are
/// deliberately kept separate from the physical scene setup so artists can
/// tune the presentation without recompiling the renderer.
#[derive(Resource, Clone, Copy, Debug, Serialize, Deserialize)]
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
        self.sunlight_boost = finite_clamp(self.sunlight_boost, 0.0, 150.0, DIRECT_SUNLIGHT_BOOST);
        self.sun_disk_intensity = finite_clamp(self.sun_disk_intensity, 0.0, 200.0, 1.0);
        self.atmosphere_fill = finite_clamp(self.atmosphere_fill, 0.0, 2.0, 1.0);
        self.ambient_multiplier = finite_clamp(self.ambient_multiplier, 0.0, 10.0, 1.0);
        self.space_atmosphere_fill = finite_clamp(self.space_atmosphere_fill, 0.0, 2.0, 1.0);
        self.bloom_intensity = finite_clamp(self.bloom_intensity, 0.0, 8.0, 0.12);
        self.bloom_threshold = finite_clamp(self.bloom_threshold, 0.0, 50.0, 1.5);
        self.bloom_threshold_softness = finite_clamp(self.bloom_threshold_softness, 0.0, 1.0, 0.2);
        self.bloom_low_frequency_boost =
            finite_clamp(self.bloom_low_frequency_boost, 0.0, 5.0, 0.35);
    }
}

fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
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
    mut stars: Query<&mut Visibility, (With<Star>, Without<Sun>)>,
    mut ground_atmosphere: Query<&mut Transform, (With<GroundAtmosphere>, Without<Sun>)>,
    player: Query<&Player>,
    mut clear: ResMut<ClearColor>,
    world: Option<Res<World>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut atmosphere_fill_q: Query<&mut AtmosphereEnvironmentMapLight, With<Camera3d>>,
    mut bloom_q: Query<&mut Bloom, With<Camera3d>>,
    mut space: ResMut<SpaceFactor>,
    mode: Res<FlightMode>,
    tuning: Res<LightingTuning>,
    mut fog_q: Query<&mut DistanceFog, (With<Camera3d>, Without<Player>)>,
) {
    day.0 = (day.0 + time.delta_secs() / 480.0) % 1.0; // JS DAY_LEN=480s 全周期
    let f = day_factor(day.0);

    let Ok(p) = player.single() else { return };
    let sun_direction = sun_travel_direction(day.0, f);

    let sunlight_boost = tuning.sunlight_boost.max(0.0);
    let daylight_boost = 1.0 + f * (sunlight_boost - 1.0);
    for (mut tf, mut light, mut disk, mut visibility, ground_sun) in &mut sun_q {
        // At night the ground atmosphere should not receive a visible solar
        // beam or disk. The orbital sun remains visible because it is outside
        // the day/night horizon model.
        let ground_daylight = ground_sun.is_some();
        disk.intensity = tuning.sun_disk_intensity.max(0.0) * if ground_daylight { f } else { 1.0 };
        if ground_sun.is_none() {
            // The orbital sun is a separate directional-light entity, but it
            // uses the same live direct-light control as the ground sun.
            light.illuminance = lux::RAW_SUNLIGHT * sunlight_boost;
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
        // Keep night illumination low, but scale the daytime direct beam up
        // aggressively. This affects direct sunlight only, not ambient fill.
        // A 2% night floor is still bright enough to turn the raymarched
        // atmosphere blue. Keep only a tiny residual for moonless ambience.
        let sun_illuminance = lux::RAW_SUNLIGHT * (0.0001 + f * 0.9999) * daylight_boost.max(0.0);
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
        *visibility = if mode.space_scene() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }

    // Keep the local atmosphere out of the candidate set while the camera is
    // in the separate space scene. Bevy chooses the nearest Atmosphere entity
    // without consulting Visibility, so moving this proxy far away is the
    // lightweight way to switch between the local and orbital shells.
    let atmosphere_center = if mode.ground_scene() {
        Vec3::new(0.0, -GROUND_ATMOSPHERE_INNER_RADIUS, 0.0)
    } else {
        Vec3::new(0.0, 1.0e9, 0.0)
    };
    for mut transform in &mut ground_atmosphere {
        transform.translation = atmosphere_center;
    }

    // space factor from altitude
    let cam_y = p.eye().y;
    // 太空/曲速/空间站模式强制 1：Bevy 单相机 ClearColor 全屏共享，而太空态玩家坐标
    // 已被镜像到星球球面坐标系（赤道附近 Y≈0），按高度计算会退化成星球大气色
    // （JS 原版太空为独立场景固定底色，不存在此问题）。
    let sf = if mode.space_scene() {
        1.0
    } else {
        ((cam_y - 80.0) / (150.0 - 80.0)).clamp(0.0, 1.0)
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

    // 高度雾（JS planetScene.fog 移植）：远景融入天穹，隐藏流式区块边缘。
    // 原版曲率/淡出着色器已移除：爬升（atmo）时把雾距向相机收拢，让平面体素
    // 地形在出大气前被雾色（≈天空色）吞没——读起来就是大气密度，出大气
    // （EXIT_Y）瞬间由太空场景的球面星球无缝接棒。
    let climb = ((cam_y - 100.0) / (crate::space::EXIT_Y - 100.0)).clamp(0.0, 1.0);
    let fog_color = lerp_color(sky, Color::WHITE, 0.15 * f * (1.0 - sf));
    for mut fog in &mut fog_q {
        if mode.space_scene() {
            fog.falloff = FogFalloff::Linear {
                start: 1e9,
                end: 1e9,
            };
        } else {
            // 地面 90..1050；爬到 EXIT_Y（220）时收拢到 ~58..120，下方地形完全雾化
            let start = 90.0 * (1.0 - climb * 0.35);
            let end = 1050.0 * (1.0 - climb * 0.885);
            fog.falloff = FogFalloff::Linear { start, end };
            fog.color = fog_color;
        }
    }

    // Keep these as scene-wide artist controls. Occlusion must be evaluated by
    // the shadow/occlusion passes per surface; changing them from camera roof
    // detection would incorrectly darken outdoor objects visible through a
    // doorway or window.
    let atmosphere_fill = if mode.space_scene() {
        tuning.space_atmosphere_fill.max(0.0)
    } else {
        tuning.atmosphere_fill.max(0.0)
    };
    for mut fill in &mut atmosphere_fill_q {
        fill.intensity = atmosphere_fill;
    }
    for mut bloom in &mut bloom_q {
        bloom.intensity = tuning.bloom_intensity.max(0.0);
        bloom.low_frequency_boost = tuning.bloom_low_frequency_boost.max(0.0);
        bloom.prefilter.threshold = tuning.bloom_threshold.max(0.0);
        bloom.prefilter.threshold_softness = tuning.bloom_threshold_softness.clamp(0.0, 1.0);
    }

    let day_amb = Color::srgb(0.75, 0.8, 0.9);
    let night_amb = Color::srgb(0.16, 0.17, 0.26);
    ambient.color = lerp_color(day_amb, night_amb, 1.0 - f);
    // JS 原版补光 = AmbientLight 0.35 + HemisphereLight 0.5 ≈ 太阳强度的 85%，
    // 方块地面/树几乎不落黑。Bevy 物理光照下按 Bevy 默认 GlobalAmbientLight
    // （80 cd/m²，正午）取环境光，夜间降到 12 保留暗部层次；F3 面板
    // ambient_multiplier 仍可整体缩放。旧曲线 (3+f*5) 只有 Bevy 默认的 4~10%，
    // 叠加侧/底面顶点色 0.65~0.8/0.5 的烘焙压暗后，背光面近乎全黑。
    ambient.brightness = (12.0 + f * 68.0) * tuning.ambient_multiplier.max(0.0);

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
        Transform::from_xyz(0.0, -GROUND_ATMOSPHERE_INNER_RADIUS, 0.0),
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
        // 原生体积云（FogVolume）需要太阳标记为 volumetric 才能产生
        // 光柱/云影（阴影贴图已开启）。
        VolumetricLight,
        Sun,
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
