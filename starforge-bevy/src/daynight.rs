//! Day/night cycle, sky color, sun light, stars & space preview.

use crate::player::Player;
use crate::space::FlightMode;
use crate::world::World;
use bevy::light::GlobalAmbientLight;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

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

pub fn daynight_system(
    time: Res<Time>,
    mut day: ResMut<DayTime>,
    mut sun_q: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
    mut disc_q: Query<&mut Transform, (With<crate::SunDisc>, Without<Sun>)>,
    mut stars: Query<&mut Visibility, (With<Star>, Without<Sun>, Without<crate::SunDisc>)>,
    player: Query<&Player>,
    mut clear: ResMut<ClearColor>,
    world: Option<Res<World>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut space: ResMut<SpaceFactor>,
    mode: Res<FlightMode>,
    mut fog_q: Query<&mut DistanceFog, (With<Camera3d>, Without<Player>)>,
) {
    day.0 = (day.0 + time.delta_secs() / 480.0) % 1.0; // JS DAY_LEN=480s 全周期
    let f = day_factor(day.0);

    for (mut tf, mut light) in &mut sun_q {
        let ang = day.0 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2; // elevation
        tf.rotation = Quat::from_rotation_x(-ang);
        tf.translation = Vec3::ZERO;
        let lux = 300.0 + f * 9700.0;
        light.illuminance = lux;
        let warm = Color::srgb(1.0, 0.82, 0.62);
        let cold = Color::srgb(0.75, 0.85, 1.0);
        light.color = if f < 0.5 {
            lerp_color(warm, cold, f * 2.0)
        } else {
            lerp_color(cold, warm, (f - 0.5) * 2.0)
        };
    }

    // space factor from altitude
    let Ok(p) = player.single() else { return };
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

    // sun disc follows the light direction, centered on the player
    let ang = day.0 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
    let sun_dir = Quat::from_rotation_x(-ang) * Vec3::NEG_Z;
    for mut disc in &mut disc_q {
        disc.translation = p.pos + sun_dir * 850.0;
    }
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
    sky = lerp_color(sky, space_black, sf);
    clear.0 = sky;

    // 高度雾（JS planetScene.fog 移植）：远景融入天穹，隐藏流式区块边缘与曲率变形；
    // 太空/空间站模式关闭（太空场景自带黑背景与星光）
    let alt_f = ((cam_y - 80.0) / 170.0).clamp(0.0, 1.0);
    let fog_color = lerp_color(sky, Color::WHITE, 0.15 * f * (1.0 - sf));
    for mut fog in &mut fog_q {
        if mode.space_scene() {
            fog.falloff = FogFalloff::Linear {
                start: 1e9,
                end: 1e9,
            };
        } else {
            fog.falloff = FogFalloff::Linear {
                start: 90.0 + alt_f * 260.0,
                end: 1050.0 + alt_f * 650.0,
            };
            fog.color = fog_color;
        }
    }

    let day_amb = Color::srgb(0.75, 0.8, 0.9);
    let night_amb = Color::srgb(0.16, 0.17, 0.26);
    ambient.color = lerp_color(day_amb, night_amb, 1.0 - f);
    ambient.brightness = 60.0 + f * 60.0;

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
) -> Vec<Entity> {
    // directional sunlight
    commands.spawn((
        DirectionalLight::default(),
        Transform::IDENTITY,
        Sun,
        crate::InGame,
    ));
    // sun disc (emissive sphere, positioned each frame by the daynight system)
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(60.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(2.0, 1.7, 1.2),
            emissive: LinearRgba::new(1.0, 0.85, 0.6, 1.0) * 2.0,
            unlit: true,
            fog_enabled: false,
            ..default()
        })),
        Transform::from_xyz(0.0, 850.0, 0.0),
        crate::SunDisc,
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
