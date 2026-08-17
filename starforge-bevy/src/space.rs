//! 太空 · 大气层飞行 · 曲速跃迁 — port of js/space.js + js/main.js flight code.
//! 空间站停靠/站内行走在 station.rs。

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use std::collections::HashMap;

use crate::data::{self, Galaxy, PlanetDef, ShipClass};
use crate::factory::MachineSave;
use crate::inventory::Slot;
use crate::player::Player;
use crate::quests::{BigMessageEvent, FlagEvent};
use crate::save;
use crate::ui::{Panel, UiState};
use crate::world::World as VoxelWorld;

// ---------- 常量（js/main.js + js/space.js） ----------

pub const MAX_SPEED: f32 = 46.0;
pub const BOOST_SPEED: f32 = 110.0;
pub const PULSE_SPEED: f32 = 900.0;
pub const WARP_ENGAGE_SPEED: f32 = 700.0;
pub const SUN_POS: Vec3 = Vec3::new(6000.0, 2400.0, 1800.0);
pub const SUN_R: f32 = 450.0;
pub const EXIT_Y: f32 = 220.0;
pub const HANDOFF_Y: f32 = 150.0;
pub const WRAP_X: f32 = std::f32::consts::PI * 2.0 / 0.004; // ≈1570.8
pub const WRAP_Z: f32 = 2.3 / 0.004; // =575
pub const SHIP_BOX: [f32; 3] = [1.6, 1.1, 1.9];
pub const SHIP_R: f32 = 3.0;

/// 体素→太空缩放
pub fn voxel_scale(planet: &PlanetDef) -> f32 {
    planet.radius * 0.004
}
/// 太空→大气 握手高度（球面距离）
pub fn handoff_dist(planet: &PlanetDef) -> f32 {
    (HANDOFF_Y - data::SEA_Y) * voxel_scale(planet)
}

// ---------- 飞行模式 ----------

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FlightMode {
    /// 地面步行
    #[default]
    Planet,
    /// 坐在降落舱内
    Seated,
    /// 大气层飞行
    Atmo,
    /// 降落动画
    AtmoLand,
    /// 太空
    Space,
    /// 曲速跃迁
    Warping,
    /// 空间站
    Station,
}

impl FlightMode {
    /// 飞船/飞行相机驱动的模式
    pub fn ship_cam(&self) -> bool {
        matches!(self, Self::Atmo | Self::AtmoLand | Self::Space | Self::Warping | Self::Station)
    }
    /// 需要飞船飞行输入的模式
    pub fn flight_input(&self) -> bool {
        matches!(self, Self::Atmo | Self::Space)
    }
    /// 太空场景需要存在的模式
    pub fn space_scene(&self) -> bool {
        matches!(self, Self::Space | Self::Warping | Self::Station)
    }
    /// 地面体素场景可见的模式
    pub fn ground_scene(&self) -> bool {
        matches!(self, Self::Planet | Self::Seated | Self::Atmo | Self::AtmoLand)
    }
}

// ---------- 输入 ----------

#[derive(Resource, Default)]
pub struct SpaceInput {
    pub thrust: bool,
    pub brake: bool,
    pub boost: bool,
    pub roll_left: bool,
    pub roll_right: bool,
    pub pulse: bool,
    pub mouse_dx: f32,
    pub mouse_dy: f32,
}

// ---------- 飞船状态 ----------

#[derive(Resource, Clone, Debug)]
pub struct ShipState {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub cam_roll: f32,
    pub speed: f32,
    pub pulse_charge: f32,
    pub pulsing: bool,
    pub tritium_drain: f32,
    pub board_yaw: f32,
    pub presaved: bool,
    pub warmed: bool,
    pub pitch_lim: f32,
    pub pitch_floor: Option<f32>,
    pub sun_heat_t: f32,
    pub reentry_t: f32,
}

impl Default for ShipState {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            cam_roll: 0.0,
            speed: 0.0,
            pulse_charge: 0.0,
            pulsing: false,
            tritium_drain: 0.0,
            board_yaw: 0.0,
            presaved: false,
            warmed: false,
            pitch_lim: 1.2,
            pitch_floor: None,
            sun_heat_t: 0.0,
            reentry_t: 0.0,
        }
    }
}

pub type ShipData = save::ShipSave;

#[derive(Resource)]
pub struct ShipAsset {
    pub entity: Option<Entity>,
    pub flames: Vec<Entity>,
    pub data: ShipData,
}

// ---------- 飞行相机 ----------

#[derive(Resource, Default)]
pub struct FlightCamera {
    pub pos: Vec3,
    pub rot: Quat,
    pub fov: f32,
}

impl FlightCamera {
    pub fn set(pos: Vec3, rot: Quat, fov: f32) -> Self {
        Self { pos, rot, fov }
    }
}

// ---------- 星球档案 / 星系 ----------

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PlanetArchive {
    pub seed: u32,
    pub biome: String,
    pub ship_pos: [f32; 3],
    pub machines: Vec<MachineSave>,
    /// 已修改区块 RLE（星球离开时归档，返回时恢复）
    #[serde(default)]
    pub mods: std::collections::HashMap<String, Vec<u16>>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Mark {
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub label: String,
    pub gal: bool,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct GalaxyArchive {
    pub planets: HashMap<usize, PlanetArchive>,
    pub marks: HashMap<usize, Vec<Mark>>,
}

#[derive(Clone, Debug)]
pub struct WarpLock {
    pub seed: u32,
    pub name: String,
}

/// 太空游戏全局状态。
#[derive(Resource)]
pub struct SpaceGame {
    pub galaxy: Galaxy,
    pub current_planet: usize,
    pub galaxy_count: u32,
    pub visited: HashMap<usize, PlanetArchive>,
    pub archives: HashMap<u32, GalaxyArchive>,
    pub warp_lock: Option<WarpLock>,
    pub fuel_loaded: i32,
    pub ship_pos: Vec3,
    pub landed_planet: i32,
    pub play_time: f32,
    /// 飞船舱货物（当前座驾）
    pub ship_inv: Vec<Option<Slot>>,
    /// 机库飞船
    pub garage: Vec<save::ShipSave>,
}

impl SpaceGame {
    pub fn new(galaxy: Galaxy) -> Self {
        Self {
            galaxy,
            current_planet: 0,
            galaxy_count: 1,
            visited: HashMap::new(),
            archives: HashMap::new(),
            warp_lock: None,
            fuel_loaded: 0,
            ship_pos: Vec3::ZERO,
            landed_planet: -1,
            play_time: 0.0,
            ship_inv: Vec::new(),
            garage: Vec::new(),
        }
    }

    pub fn planet(&self) -> &PlanetDef {
        let idx = self.current_planet.min(self.galaxy.planets.len() - 1);
        &self.galaxy.planets[idx]
    }

    pub fn market(&self) -> &HashMap<String, f32> {
        &self.galaxy.market
    }
}

// ---------- 太空场景 ----------

pub struct PlanetVis {
    pub def: PlanetDef,
    pub entity: Entity,
    pub atmo: Entity,
}

#[derive(Resource)]
pub struct SpaceScene {
    pub planets: Vec<PlanetVis>,
    pub station_pos: Vec3,
    pub station: Entity,
    pub sun: Entity,
    pub sun_glow: Entity,
    pub asteroids: Vec<Entity>,
    pub dir_light: Entity,
}

#[derive(Component)]
pub struct Asteroid {
    pub spin: Vec3,
}

#[derive(Component)]
pub struct LaserBolt {
    pub dir: Vec3,
    pub life: f32,
    pub origin: Vec3,
    pub speed: f32,
}

// ---------- 曲速 ----------

#[derive(Resource, Default)]
pub struct WarpAnim {
    pub active: bool,
    pub t: f32,
    pub seed: u32,
    pub yaw: f32,
    pub pitch: f32,
    pub v0: f32,
}

pub const WARP_LAUNCH: f32 = 6.7;
pub const WARP_RIDE: f32 = 9.0;

/// 曲速抵达事件（重建太空场景 + 切换星系在 warp_system 内完成后由本事件触发场景重建）。
#[derive(Message)]
pub struct WarpArriveEvent;

/// 需要切换星球世界（无缝再入着陆时触发）。
#[derive(Message)]
pub struct LandPlanetEvent {
    pub pid: usize,
}

// ---------- 降落动画 ----------

#[derive(Resource, Default)]
pub struct AtmoLand {
    pub t: f32,
    pub from: Vec3,
    pub to: Vec3,
}

// ---------- 邻域星系 ----------

pub fn neighbor_seeds(cur: u32) -> Vec<u32> {
    let mut rnd = crate::rng::Rng::new(cur ^ 0x9E37_79B9);
    let mut arr: Vec<u32> = Vec::new();
    for _ in 0..55 {
        arr.push((rnd.next() * 1e9) as u32);
    }
    if cur != data::HOME_GALAXY_SEED {
        arr.push(data::HOME_GALAXY_SEED);
    }
    arr
}

/// 星系精灵方向（种子确定性）。
pub fn galaxy_dir(seed: u32) -> Vec3 {
    let mut rnd = crate::rng::Rng::new(seed);
    let theta = rnd.next() * std::f32::consts::PI * 2.0;
    let elev = (rnd.next() - 0.5) * 1.1;
    Vec3::new(
        elev.cos() * theta.cos(),
        elev.sin(),
        elev.cos() * theta.sin(),
    )
}

// ---------- 颜色/材质辅助 ----------

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

pub fn parse_hex(hex: &str) -> Option<Color> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color::srgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn metal_mat(mats: &mut Assets<StandardMaterial>, color: Color) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.75,
        metallic: 0.55,
        ..default()
    })
}

fn emissive_mat(mats: &mut Assets<StandardMaterial>, color: Color, mult: f32) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: color,
        emissive: color.to_linear() * mult,
        unlit: true,
        ..default()
    })
}

// ---------- 场景构建 ----------

/// 程序化飞船（JS buildShip 方块兜底模型），返回 (根实体, 尾焰实体)。
pub fn spawn_ship(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    pos: Vec3,
    yaw: f32,
    cls: &ShipClass,
) -> (Entity, Vec<Entity>) {
    let hull = metal_mat(mats, Color::srgb(0.62, 0.68, 0.74));
    let dark = metal_mat(mats, Color::srgb(0.30, 0.34, 0.38));
    let glass = mats.add(StandardMaterial {
        base_color: Color::srgba(0.4, 0.86, 0.93, 0.7),
        emissive: LinearRgba::new(0.07, 0.2, 0.27, 1.0),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let accent_color = parse_hex(cls.color).unwrap_or(Color::srgb(0.79, 0.39, 0.1));
    let accent = metal_mat(mats, accent_color);
    let engine_glow = emissive_mat(mats, Color::srgb(0.21, 0.69, 1.0), 2.0);

    let root = commands
        .spawn((
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            crate::InGame,
        ))
        .id();
    let b = |commands: &mut Commands,
             meshes: &mut Assets<Mesh>,
             root: Entity,
             w: f32,
             h: f32,
             d: f32,
             m: &Handle<StandardMaterial>,
             x: f32,
             y: f32,
             z: f32| {
        let e = commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(w, h, d))),
                MeshMaterial3d(m.clone()),
                Transform::from_xyz(x, y, z),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(e);
    };
    // 机身（-z 朝前）
    b(commands, meshes, root, 1.4, 0.9, 3.6, &hull, 0.0, 0.0, 0.0);
    b(commands, meshes, root, 1.0, 0.5, 1.4, &dark, 0.0, 0.6, 0.4);
    b(commands, meshes, root, 0.9, 0.62, 1.2, &glass, 0.0, 0.55, -0.9);
    b(commands, meshes, root, 1.2, 0.3, 1.1, &dark, 0.0, -0.35, -1.9);
    b(commands, meshes, root, 0.8, 0.42, 0.9, &hull, 0.0, -0.1, -2.4);
    // 机翼
    b(commands, meshes, root, 2.6, 0.16, 1.4, &hull, -1.9, -0.1, 0.7);
    b(commands, meshes, root, 2.6, 0.16, 1.4, &hull, 1.9, -0.1, 0.7);
    b(commands, meshes, root, 0.5, 0.5, 1.0, &accent, -3.0, 0.05, 0.8);
    b(commands, meshes, root, 0.5, 0.5, 1.0, &accent, 3.0, 0.05, 0.8);
    // 引擎
    b(commands, meshes, root, 0.55, 0.55, 0.9, &dark, -0.55, -0.05, 1.9);
    b(commands, meshes, root, 0.55, 0.55, 0.9, &dark, 0.55, -0.05, 1.9);
    // 起落架
    b(commands, meshes, root, 0.14, 0.5, 0.14, &dark, -0.5, -0.7, -0.8);
    b(commands, meshes, root, 0.14, 0.5, 0.14, &dark, 0.5, -0.7, -0.8);
    b(commands, meshes, root, 0.14, 0.5, 0.14, &dark, 0.0, -0.7, 1.2);
    // 引擎光斑
    b(commands, meshes, root, 0.4, 0.4, 0.12, &engine_glow, -0.55, -0.05, 2.4);
    b(commands, meshes, root, 0.4, 0.4, 0.12, &engine_glow, 0.55, -0.05, 2.4);
    // 尾焰
    let flame_mat = mats.add(StandardMaterial {
        base_color: Color::srgba(0.4, 0.8, 1.0, 0.7),
        emissive: LinearRgba::new(0.3, 0.6, 1.0, 1.0) * 2.0,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let mut flames = Vec::new();
    for x in [-0.55f32, 0.55] {
        let e = commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(0.3, 0.3, 1.6))),
                MeshMaterial3d(flame_mat.clone()),
                Transform::from_xyz(x, -0.05, 3.3),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(e);
        flames.push(e);
    }
    (root, flames)
}

/// 程序化星球贴图（128×256，噪声着色 + 极冠）。
pub fn planet_texture(images: &mut Assets<Image>, biome_key: &str, seed: u32) -> Handle<Image> {
    let b = data::biome_by_key(biome_key);
    let noise = crate::rng::Noise2::new(seed);
    let w = 128usize;
    let h = 256usize;
    let mut buf = vec![0u8; w * h * 4];
    for y in 0..h {
        let lat = (y as f32 / h as f32 - 0.5) * std::f32::consts::PI;
        for x in 0..w {
            let lon = x as f32 / w as f32 * std::f32::consts::TAU;
            let n = noise.fbm2(lon.cos() * 3.0, lat.sin() * 3.0, 3, 2.0, 0.5);
            let n2 = noise.fbm2(lon.sin() * 5.0 + 17.0, lat.cos() * 5.0 + 3.0, 3, 2.0, 0.5);
            let mut r = ((b.tint >> 16) & 0xFF) as u8;
            let mut g = ((b.tint >> 8) & 0xFF) as u8;
            let mut bl = (b.tint & 0xFF) as u8;
            let ice = ((lat.abs() - 1.1).max(0.0) * 4.0).min(1.0);
            r = lerp_u8(r, 0xf2, ice);
            g = lerp_u8(g, 0xf6, ice);
            bl = lerp_u8(bl, 0xfa, ice);
            let shade = (0.82 + n * 0.22 + n2 * 0.10).clamp(0.5, 1.2);
            r = (r as f32 * shade).min(255.0) as u8;
            g = (g as f32 * shade).min(255.0) as u8;
            bl = (bl as f32 * shade).min(255.0) as u8;
            let i = (y * w + x) * 4;
            buf[i] = r;
            buf[i + 1] = g;
            buf[i + 2] = bl;
            buf[i + 3] = 255;
        }
    }
    let mut img = Image::new(
        bevy::render::render_resource::Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        buf,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::nearest();
    images.add(img)
}

/// 构建太空场景（恒星/星球/星空/小行星/空间站）。
#[allow(clippy::too_many_arguments)]
pub fn spawn_space_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    mats: &mut Assets<StandardMaterial>,
    galaxy: &Galaxy,
) -> SpaceScene {
    // 星光背景：远球壳上的小发光方块
    let star_mat = emissive_mat(mats, Color::WHITE, 1.5);
    let star_mesh = meshes.add(Cuboid::new(1.4, 1.4, 1.4));
    let mut rnd = crate::rng::Rng::new(777);
    let stars_root = commands
        .spawn((Transform::IDENTITY, Visibility::default(), crate::InGame))
        .id();
    for _ in 0..400 {
        let mut v = Vec3::new(
            rnd.next() * 2.0 - 1.0,
            rnd.next() * 2.0 - 1.0,
            rnd.next() * 2.0 - 1.0,
        );
        if v.length_squared() < 1e-4 {
            v = Vec3::Y;
        }
        v = v.normalize() * 9000.0;
        let e = commands
            .spawn((
                Mesh3d(star_mesh.clone()),
                MeshMaterial3d(star_mat.clone()),
                Transform::from_translation(v),
                crate::InGame,
            ))
            .id();
        commands.entity(stars_root).add_child(e);
    }
    // 太阳
    let sun = commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(SUN_R))),
            MeshMaterial3d(emissive_mat(mats, Color::srgb(1.9, 1.5, 0.8), 3.0)),
            Transform::from_translation(SUN_POS),
            crate::InGame,
        ))
        .id();
    let sun_glow = commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(SUN_R * 3.2))),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.7, 0.3, 0.25),
                emissive: LinearRgba::new(1.0, 0.6, 0.25, 1.0) * 2.0,
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                ..default()
            })),
            Transform::from_translation(SUN_POS),
            crate::InGame,
        ))
        .id();
    // 太阳方向光
    let sun_dir = SUN_POS.normalize();
    let dir_light = commands
        .spawn((
            DirectionalLight {
                color: Color::srgb(1.0, 0.95, 0.82),
                illuminance: 20000.0,
                ..default()
            },
            Transform::from_rotation(Quat::from_rotation_arc(Vec3::NEG_Z, -sun_dir)),
            crate::InGame,
        ))
        .id();
    // 星球群
    let mut planets = Vec::new();
    for pd in &galaxy.planets {
        let tex = planet_texture(images, pd.biome, 1000 + pd.id as u32 * 137);
        let mat = mats.add(StandardMaterial {
            base_color_texture: Some(tex),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..default()
        });
        let root = commands
            .spawn((
                Mesh3d(meshes.add(Sphere::new(pd.radius))),
                MeshMaterial3d(mat),
                Transform::from_translation(Vec3::from(pd.pos)),
                crate::InGame,
            ))
            .id();
        let b = data::biome_by_key(pd.biome);
        let atmo = commands
            .spawn((
                Mesh3d(meshes.add(Sphere::new(pd.radius * 1.37))),
                MeshMaterial3d(mats.add(StandardMaterial {
                    base_color: Color::srgba(b.sky.0, b.sky.1, b.sky.2, 0.16),
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    cull_mode: None,
                    ..default()
                })),
                Transform::default(),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(atmo);
        planets.push(PlanetVis { def: pd.clone(), entity: root, atmo });
    }
    // 空间站
    let station = crate::station::spawn_station(commands, meshes, mats, Vec3::from(galaxy.station));
    // 小行星（不规则缩放球体）
    let mut asteroids = Vec::new();
    let mut ar = crate::rng::Rng::new(0xA57E);
    let rock_mat = metal_mat(mats, Color::srgb(0.42, 0.40, 0.38));
    let rock_mesh = meshes.add(Sphere::new(1.0));
    for _ in 0..26 {
        let ang = ar.next() * std::f32::consts::TAU;
        let dist = 500.0 + ar.next() * 2600.0;
        let el = (ar.next() - 0.5) * 1600.0;
        let pos = Vec3::new(ang.cos() * dist, el, ang.sin() * dist);
        let scale = 3.0 + ar.next() * 14.0;
        let e = commands
            .spawn((
                Mesh3d(rock_mesh.clone()),
                MeshMaterial3d(rock_mat.clone()),
                Transform::from_translation(pos).with_scale(Vec3::new(scale, scale * 0.7, scale * 0.9)),
                Asteroid {
                    spin: Vec3::new(ar.next() * 0.4 - 0.2, ar.next() * 0.4 - 0.2, 0.0),
                },
                crate::InGame,
            ))
            .id();
        asteroids.push(e);
    }
    SpaceScene {
        planets,
        station_pos: Vec3::from(galaxy.station),
        station,
        sun,
        sun_glow,
        asteroids,
        dir_light,
    }
}

pub fn despawn_space_scene(commands: &mut Commands, scene: &SpaceScene) {
    for p in &scene.planets {
        commands.entity(p.entity).despawn();
    }
    commands.entity(scene.station).despawn();
    commands.entity(scene.sun).despawn();
    commands.entity(scene.sun_glow).despawn();
    commands.entity(scene.dir_light).despawn();
    for a in &scene.asteroids {
        commands.entity(*a).despawn();
    }
}

// ---------- 输入 ----------

pub fn space_input_system(
    mouse: Res<AccumulatedMouseMotion>,
    keys: Res<ButtonInput<KeyCode>>,
    mut input: ResMut<SpaceInput>,
    mode: Res<FlightMode>,
    ui: Res<UiState>,
) {
    let flight = mode.flight_input();
    if ui.locked() || !flight {
        input.mouse_dx = 0.0;
        input.mouse_dy = 0.0;
        input.thrust = false;
        input.brake = false;
        input.boost = false;
        input.roll_left = false;
        input.roll_right = false;
        input.pulse = false;
        return;
    }
    let delta = mouse.delta;
    if delta.x.abs() < 200.0 && delta.y.abs() < 200.0 {
        input.mouse_dx += delta.x;
        input.mouse_dy += delta.y;
    }
    input.thrust = keys.pressed(KeyCode::KeyW);
    input.brake = keys.pressed(KeyCode::KeyS);
    input.boost = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    input.roll_left = keys.pressed(KeyCode::KeyA);
    input.roll_right = keys.pressed(KeyCode::KeyD);
    input.pulse = keys.pressed(KeyCode::KeyJ);
}

// ---------- 地面：登船 / 下船 ----------

#[allow(clippy::too_many_arguments)]
pub fn ship_interact_system(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut next_mode: ResMut<FlightMode>,
    mut player: Query<&mut Player>,
    mut ship_state: ResMut<ShipState>,
    mut game: ResMut<SpaceGame>,
    mut quests: ResMut<crate::quests::Quests>,
    mut flag_ev: MessageWriter<FlagEvent>,
    mut big_ev: MessageWriter<BigMessageEvent>,
    ui: Res<UiState>,
    world: Res<VoxelWorld>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    if *next_mode != FlightMode::Planet {
        return;
    }
    if !keys.just_pressed(KeyCode::KeyE) || ui.locked() {
        return;
    }
    let Ok(mut p) = player.single_mut() else { return };
    let ship = game.ship_pos;
    let dx = p.pos.x - ship.x;
    let dy = p.pos.y - ship.y;
    let dz = p.pos.z - ship.z;
    if dx * dx + dy * dy + dz * dz > 36.0 {
        return;
    }
    if !quests.flags.get("checkedShip").copied().unwrap_or(false) {
        quests.flags.insert("checkedShip".into(), true);
        flag_ev.write(FlagEvent { flag: "checkedShip".into() });
    }
    if p.creative() {
        board_ship(&mut next_mode, &mut p, &mut ship_state, &game, &world, &mut commands, &sfx);
        // 本次 E 已被登船消费：同帧链内 seated_system 不能再触发下船
        keys.clear_just_pressed(KeyCode::KeyE);
        return;
    }
    let repaired = quests.flags.get("shipRepaired").copied().unwrap_or(false);
    if !repaired {
        if quests.idx >= 6 && p.inv.count_item("iron") >= 10 && p.inv.count_item("carbon") >= 20 {
            p.inv.remove_item("iron", 10);
            p.inv.remove_item("carbon", 20);
            quests.flags.insert("shipRepaired".into(), true);
            flag_ev.write(FlagEvent { flag: "shipRepaired".into() });
            big_ev.write(BigMessageEvent {
                title: "推进器修复完毕".into(),
                sub: "按 E 登船 → W 点火起飞（需发射燃料）".into(),
                dur: 3.0,
            });
            crate::audio::play(&mut commands, sfx.craft.clone(), 0.6, None);
        } else {
            big_ev.write(BigMessageEvent {
                title: "飞船受损".into(),
                sub: "修复需要：铁锭×10 + 碳×20（完成前期任务解锁冶炼）".into(),
                dur: 3.2,
            });
            crate::audio::play(&mut commands, sfx.error.clone(), 0.5, None);
        }
        return;
    }
    board_ship(&mut next_mode, &mut p, &mut ship_state, &game, &world, &mut commands, &sfx);
    // 同上：清掉本帧的 E，避免链内 seated_system 同帧立即下船
    keys.clear_just_pressed(KeyCode::KeyE);
}

fn board_ship(
    next_mode: &mut FlightMode,
    p: &mut Player,
    ship_state: &mut ShipState,
    game: &SpaceGame,
    world: &VoxelWorld,
    commands: &mut Commands,
    sfx: &crate::audio::Sfx,
) {
    *next_mode = FlightMode::Seated;
    ship_state.board_yaw = ship_state.yaw;
    let ex = game.ship_pos.x + 2.2;
    let ez = game.ship_pos.z;
    p.pos = Vec3::new(
        ex,
        world.top_at(ex.floor() as i32, ez.floor() as i32) as f32 + 1.2,
        ez,
    );
    p.vel = Vec3::ZERO;
    p.mining = None;
    crate::audio::play(commands, sfx.click.clone(), 0.5, None);
    p.toast("已登船：W 点火起飞 · E 下船");
}

/// 座椅内：W 起飞 / E 下船。
#[allow(clippy::too_many_arguments)]
pub fn seated_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_mode: ResMut<FlightMode>,
    mut player: Query<&mut Player>,
    mut ship_state: ResMut<ShipState>,
    mut game: ResMut<SpaceGame>,
    mut flag_ev: MessageWriter<FlagEvent>,
    mut big_ev: MessageWriter<BigMessageEvent>,
    ui: Res<UiState>,
    world: Res<VoxelWorld>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    if *next_mode != FlightMode::Seated {
        return;
    }
    if ui.locked() {
        return;
    }
    let Ok(mut p) = player.single_mut() else { return };
    if keys.just_pressed(KeyCode::KeyE) {
        *next_mode = FlightMode::Planet;
        let ex = game.ship_pos.x + 2.5;
        let ez = game.ship_pos.z;
        p.pos = Vec3::new(
            ex,
            world.top_at(ex.floor() as i32, ez.floor() as i32) as f32 + 1.2,
            ez,
        );
        p.vel = Vec3::ZERO;
        p.yaw = ship_state.board_yaw + std::f32::consts::FRAC_PI_2;
        crate::audio::play(&mut commands, sfx.jump.clone(), 0.6, None);
        return;
    }
    if keys.just_pressed(KeyCode::KeyW) {
        attempt_takeoff(
            &mut next_mode, &mut p, &mut ship_state, &mut game, &mut flag_ev, &mut big_ev, &world,
            &mut commands, &sfx,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn attempt_takeoff(
    next_mode: &mut FlightMode,
    p: &mut Player,
    ship_state: &mut ShipState,
    game: &mut SpaceGame,
    flag_ev: &mut MessageWriter<FlagEvent>,
    big_ev: &mut MessageWriter<BigMessageEvent>,
    world: &VoxelWorld,
    commands: &mut Commands,
    sfx: &crate::audio::Sfx,
) {
    if p.creative() {
        launch(next_mode, ship_state, game, flag_ev, world, commands, sfx);
        return;
    }
    let on_pad = data::block_by_id(world.get(
        game.ship_pos.x.floor() as i32,
        game.ship_pos.y.floor() as i32 - 1,
        game.ship_pos.z.floor() as i32,
    ))
    .key
        == "launchpad";
    if game.fuel_loaded < 1 && !on_pad {
        if p.inv.count_item("fuel") > 0 {
            p.inv.remove_item("fuel", 1);
            game.fuel_loaded = 1;
            crate::audio::play(commands, sfx.pickup.clone(), 0.5, None);
            big_ev.write(BigMessageEvent {
                title: "燃料已加注".into(),
                sub: "再次按 W 点火起飞".into(),
                dur: 2.0,
            });
            return;
        }
        crate::audio::play(commands, sfx.error.clone(), 0.5, None);
        big_ev.write(BigMessageEvent {
            title: "燃料不足".into(),
            sub: "需要发射燃料×1（碳×25+氧×10 合成）或将飞船停在发射平台".into(),
            dur: 3.0,
        });
        return;
    }
    launch(next_mode, ship_state, game, flag_ev, world, commands, sfx);
}

fn launch(
    next_mode: &mut FlightMode,
    ship_state: &mut ShipState,
    game: &mut SpaceGame,
    flag_ev: &mut MessageWriter<FlagEvent>,
    world: &VoxelWorld,
    commands: &mut Commands,
    sfx: &crate::audio::Sfx,
) {
    if game.fuel_loaded > 0 {
        game.fuel_loaded -= 1;
    }
    flag_ev.write(FlagEvent { flag: "launched".into() });
    start_atmo(false, ship_state, game, world);
    *next_mode = FlightMode::Atmo;
    crate::audio::play(commands, sfx.laser_hit.clone(), 0.7, None);
}

/// 进入大气飞行（from_space: 太空再入）。
pub fn start_atmo(from_space: bool, ship: &mut ShipState, game: &SpaceGame, world: &VoxelWorld) {
    if from_space {
        let gy = world.top_at(ship.pos.x.floor() as i32, ship.pos.z.floor() as i32);
        ship.pos.y = HANDOFF_Y.max(gy as f32 + 40.0);
        ship.yaw = crate::rng::Rng::new(game.galaxy.seed ^ 0x11).next() * std::f32::consts::TAU;
        ship.pitch = -0.18;
        ship.speed = 24.0;
    } else {
        ship.pos = game.ship_pos;
        ship.yaw = ship.board_yaw;
        ship.pitch = 0.42;
        ship.speed = 14.0;
    }
    ship.roll = 0.0;
    ship.cam_roll = 0.0;
    ship.warmed = false;
    ship.presaved = false;
    ship.pitch_lim = 1.2;
    ship.pitch_floor = None;
    ship.pulsing = false;
    ship.pulse_charge = 0.0;
}

// ---------- 大气层飞行 ----------

fn ship_voxel_collision(ship: &mut ShipState, world: &VoxelWorld, dt: f32) {
    let p = ship.pos;
    let x0 = (p.x - SHIP_BOX[0]).floor() as i32;
    let x1 = (p.x + SHIP_BOX[0]).floor() as i32;
    let y0 = (p.y - SHIP_BOX[1]).floor() as i32;
    let y1 = (p.y + SHIP_BOX[1]).floor() as i32;
    let z0 = (p.z - SHIP_BOX[2]).floor() as i32;
    let z1 = (p.z + SHIP_BOX[2]).floor() as i32;
    let mut best: Option<(i32, f32, f32)> = None; // (axis, amt, push)
    let mut hit_below = false;
    for by in y0..=y1 {
        for bz in z0..=z1 {
            for bx in x0..=x1 {
                let d = data::block_by_id(world.get(bx, by, bz));
                if !d.solid || d.liquid || d.cross || d.key == "leaves" {
                    continue;
                }
                let top = match d.lowbox {
                    Some(h) => by as f32 + h,
                    None => by as f32 + 1.0,
                };
                let pen_x = (p.x + SHIP_BOX[0]).min(bx as f32 + 1.0) - (p.x - SHIP_BOX[0]).max(bx as f32);
                if pen_x <= 0.0 {
                    continue;
                }
                let pen_y = (p.y + SHIP_BOX[1]).min(top) - (p.y - SHIP_BOX[1]).max(by as f32);
                if pen_y <= 0.0 {
                    continue;
                }
                let pen_z = (p.z + SHIP_BOX[2]).min(bz as f32 + 1.0) - (p.z - SHIP_BOX[2]).max(bz as f32);
                if pen_z <= 0.0 {
                    continue;
                }
                let (axis, amt, push) = if pen_x <= pen_y && pen_x <= pen_z {
                    (0, pen_x, if p.x < bx as f32 + 0.5 { -pen_x } else { pen_x })
                } else if pen_y <= pen_z {
                    (1, pen_y, if p.y < by as f32 + (top - by as f32) * 0.5 { -pen_y } else { pen_y })
                } else {
                    (2, pen_z, if p.z < bz as f32 + 0.5 { -pen_z } else { pen_z })
                };
                if best.map(|b| amt < b.1).unwrap_or(true) {
                    best = Some((axis, amt, push));
                    if axis == 1 && push > 0.0 {
                        hit_below = true;
                    }
                }
            }
        }
    }
    if let Some((axis, _amt, push)) = best {
        match axis {
            0 => ship.pos.x += push,
            1 => ship.pos.y += push,
            _ => ship.pos.z += push,
        }
        if hit_below {
            let fwd = ship_forward(ship.yaw, ship.pitch);
            if fwd.y < -0.15 {
                ship.speed = (ship.speed * (1.0 - (dt * 2.5).min(1.0))).max(3.0);
                if ship.pitch < 0.0 {
                    ship.pitch += dt * 1.6;
                }
            }
        }
    }
}

pub fn ship_forward(yaw: f32, pitch: f32) -> Vec3 {
    Quat::from_euler(EulerRot::YXZ, pitch, yaw, 0.0) * Vec3::NEG_Z
}

pub fn ship_quat(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, pitch, yaw, roll)
}

/// E 键触发就地降落。
pub fn atmo_land_trigger_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_mode: ResMut<FlightMode>,
    mut ship: ResMut<ShipState>,
    mut land: ResMut<AtmoLand>,
    mut big_ev: MessageWriter<BigMessageEvent>,
    ui: Res<UiState>,
    world: Option<Res<VoxelWorld>>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    if *next_mode != FlightMode::Atmo || !keys.just_pressed(KeyCode::KeyE) || ui.locked() {
        return;
    }
    let Some(world) = world else { return };
    let x = ship.pos.x.floor() as i32;
    let z = ship.pos.z.floor() as i32;
    let gy = world.top_at(x, z);
    if data::block_by_id(world.get(x, gy, z)).liquid {
        big_ev.write(BigMessageEvent {
            title: "无法降落".into(),
            sub: "下方是液体表面，请寻找陆地".into(),
            dur: 2.2,
        });
        crate::audio::play(&mut commands, sfx.error.clone(), 0.5, None);
        return;
    }
    let landing_y = gy as f32 + 1.2;
    land.t = 0.0;
    land.from = ship.pos;
    land.to = Vec3::new(ship.pos.x, landing_y, ship.pos.z);
    *next_mode = FlightMode::AtmoLand;
    crate::audio::play(&mut commands, sfx.jump.clone(), 0.7, None);
}

#[allow(clippy::too_many_arguments)]
pub fn atmo_system(
    time: Res<Time>,
    mut next_mode: ResMut<FlightMode>,
    mut ship: ResMut<ShipState>,
    mut input: ResMut<SpaceInput>,
    game: Res<SpaceGame>,
    world: Option<Res<VoxelWorld>>,
    settings: Res<save::Settings>,
    mut player: Query<&mut Player>,
    mut flight_cam: ResMut<FlightCamera>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    if *next_mode != FlightMode::Atmo {
        return;
    }
    let dt = time.delta_secs();
    let Some(world) = world else { return };
    let sens = settings.mouse_sens * 0.0022;
    // 转向
    let d_roll = (if input.roll_left { 1.7 } else if input.roll_right { -1.7 } else { 0.0 }) * dt;
    let q = ship_quat(ship.yaw, ship.pitch, ship.cam_roll);
    let dq = Quat::from_euler(
        EulerRot::YXZ,
        input.mouse_dy * -sens,
        input.mouse_dx * -sens,
        d_roll,
    );
    let nq = (q * dq).normalize();
    let (ny, np, nr) = nq.to_euler(EulerRot::YXZ);
    ship.pitch_lim = (ship.pitch_lim - dt * 0.45).clamp(1.2, 1.55);
    ship.pitch = np.clamp(-ship.pitch_lim, ship.pitch_lim);
    ship.yaw = ny;
    ship.cam_roll = nr;
    if let Some(pf) = ship.pitch_floor {
        if ship.pitch < pf {
            ship.pitch = (ship.pitch + dt * (if ship.reentry_t > 0.0 { 2.4 } else { 1.6 })).min(pf);
        } else {
            ship.pitch_floor = None;
        }
    }
    let target_roll = input.mouse_dx * -0.04 * settings.mouse_sens;
    ship.roll += (target_roll - ship.roll) * (dt * 5.0).min(1.0);
    input.mouse_dx = 0.0;
    input.mouse_dy = 0.0;
    // 速度
    let max_s = if input.boost { 55.0 } else { 30.0 };
    let target = if input.thrust {
        max_s
    } else if input.brake {
        3.0
    } else {
        ship.speed.min(max_s)
    };
    ship.speed += (target - ship.speed) * (dt * 2.2).min(1.0);
    // 位移
    let fwd = ship_forward(ship.yaw, ship.pitch);
    let spd = ship.speed;
    ship.pos += fwd * spd * dt;
    // 星球是圆的：经纬环绕
    if ship.pos.x > WRAP_X / 2.0 {
        ship.pos.x -= WRAP_X;
    } else if ship.pos.x < -WRAP_X / 2.0 {
        ship.pos.x += WRAP_X;
    }
    if ship.pos.z > WRAP_Z / 2.0 {
        ship.pos.z -= WRAP_Z;
    } else if ship.pos.z < -WRAP_Z / 2.0 {
        ship.pos.z += WRAP_Z;
    }
    ship_voxel_collision(&mut ship, &world, dt);
    // 相机
    let cam_q = ship_quat(ship.yaw, ship.pitch, 0.0);
    let off = cam_q * Vec3::new(0.0, 3.2, 11.0);
    let mut cam_pos = ship.pos + off;
    if ship.reentry_t > 0.0 {
        ship.reentry_t -= dt;
        let shake = ship.reentry_t.min(1.0) * 0.35;
        cam_pos.x += (crate::rng::Rng::new((time.elapsed_secs() * 1000.0) as u32).next() - 0.5) * shake;
        cam_pos.y += (crate::rng::Rng::new((time.elapsed_secs() * 997.0) as u32).next() - 0.5) * shake;
    }
    *flight_cam = FlightCamera::set(cam_pos, cam_q, 72.0 + ship.speed * 0.15);
    // 玩家镜像（HUD/星空因子）
    if let Ok(mut p) = player.single_mut() {
        p.pos = ship.pos;
    }
    // 出大气
    if ship.pos.y > EXIT_Y {
        exit_to_space(&mut next_mode, &mut ship, &game, &mut commands, &sfx);
    }
}

#[allow(clippy::too_many_arguments)]
fn exit_to_space(
    mode: &mut FlightMode,
    ship: &mut ShipState,
    game: &SpaceGame,
    commands: &mut Commands,
    sfx: &crate::audio::Sfx,
) {
    let planet = game.planet();
    let s = voxel_scale(planet);
    let lon = ship.pos.x * 0.004;
    let lat = (ship.pos.z * 0.004).clamp(-1.15, 1.15);
    let dir = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
    let center = Vec3::from(planet.pos);
    ship.pos = center + dir * (planet.radius + (ship.pos.y - data::SEA_Y) * s);
    ship.speed = (ship.speed / s).max(12.0);
    ship.warmed = false;
    ship.presaved = false;
    *mode = FlightMode::Space;
    crate::audio::play(commands, sfx.laser_hit.clone(), 0.5, None);
}

// ---------- 太空飞行 ----------

/// space_system 参数组（超出 16 参数元组上限，聚合为 SystemParam）。
#[derive(bevy::ecs::system::SystemParam)]
pub struct SpaceSysParams<'w, 's> {
    pub time: Res<'w, Time>,
    pub next_mode: ResMut<'w, FlightMode>,
    pub ship: ResMut<'w, ShipState>,
    pub input: ResMut<'w, SpaceInput>,
    pub game: ResMut<'w, SpaceGame>,
    pub scene: Option<Res<'w, SpaceScene>>,
    pub settings: Res<'w, save::Settings>,
    pub player: Query<'w, 's, &'static mut Player>,
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub big_ev: MessageWriter<'w, BigMessageEvent>,
    pub flag_ev: MessageWriter<'w, FlagEvent>,
    pub land_ev: MessageWriter<'w, LandPlanetEvent>,
    pub ui_state: ResMut<'w, UiState>,
    pub flight_cam: ResMut<'w, FlightCamera>,
    pub commands: Commands<'w, 's>,
    pub sfx: Res<'w, crate::audio::Sfx>,
    pub warp_anim: ResMut<'w, WarpAnim>,
}

pub fn space_system(mut p: SpaceSysParams) {
    if *p.next_mode != FlightMode::Space {
        return;
    }
    let dt = p.time.delta_secs();
    let sens = p.settings.mouse_sens * 0.0022;
    // 姿态
    let d_roll = (if p.input.roll_left { 1.7 } else if p.input.roll_right { -1.7 } else { 0.0 }) * dt;
    let q = ship_quat(p.ship.yaw, p.ship.pitch, p.ship.roll);
    let dq = Quat::from_euler(
        EulerRot::YXZ,
        p.input.mouse_dy * -sens,
        p.input.mouse_dx * -sens,
        d_roll,
    );
    let nq = (q * dq).normalize();
    let (ny, np, nr) = nq.to_euler(EulerRot::YXZ);
    p.ship.pitch = np.clamp(-1.55, 1.55);
    p.ship.yaw = ny;
    p.ship.roll = nr;
    p.input.mouse_dx = 0.0;
    p.input.mouse_dy = 0.0;
    // 速度
    let max_s = if p.input.boost { BOOST_SPEED } else { MAX_SPEED };
    let mut target = 0.0;
    if p.input.thrust {
        target = max_s;
    }
    if p.input.brake {
        target = 4.0;
    }
    if !p.input.thrust && !p.input.brake {
        target = p.ship.speed.min(max_s);
    }
    // 脉冲引擎
    let mut tritium_use = 0;
    if p.input.pulse {
        let have = p.player.single().map(|pl| pl.inv.count_item("tritium") > 0).unwrap_or(false);
        if have {
            p.ship.pulse_charge = (p.ship.pulse_charge + dt * 0.8).min(1.0);
            if p.ship.pulse_charge >= 1.0 {
                if !p.ship.pulsing {
                    p.ship.pulsing = true;
                }
                target = PULSE_SPEED;
                p.ship.tritium_drain += dt;
                if p.ship.tritium_drain > 0.7 {
                    p.ship.tritium_drain = 0.0;
                    tritium_use += 1;
                }
            }
        }
    } else {
        p.ship.pulsing = false;
        p.ship.pulse_charge = (p.ship.pulse_charge - dt * 2.0).max(0.0);
    }
    if tritium_use > 0 {
        if let Ok(mut pl) = p.player.single_mut() {
            pl.inv.remove_item("tritium", tritium_use);
        }
    }
    p.ship.speed += (target - p.ship.speed) * (dt * (if p.ship.pulsing { 1.2 } else { 2.5 })).min(1.0);
    // 移动
    let fwd = ship_forward(p.ship.yaw, p.ship.pitch);
    let spd = p.ship.speed;
    p.ship.pos += fwd * spd * dt;
    if let Some(sc) = p.scene.as_ref() {
        crate::station::resolve_station_collision(&mut p.ship.pos, sc.station_pos);
        let sun_d = p.ship.pos.distance(SUN_POS);
        if sun_d < SUN_R + 40.0 {
            let mut v = p.ship.pos - SUN_POS;
            if v.length_squared() < 1e-4 {
                v = Vec3::Y;
            }
            p.ship.pos = SUN_POS + v.normalize() * (SUN_R + 40.0);
            p.ship.speed = p.ship.speed.min(24.0);
        }
        if sun_d < SUN_R * 2.2 {
            p.ship.sun_heat_t += dt;
            if p.ship.sun_heat_t > 0.6 {
                p.ship.sun_heat_t = 0.0;
                if let Ok(mut pl) = p.player.single_mut() {
                    if !pl.creative() && pl.stats.hp + pl.stats.shield > 1.0 {
                        pl.damage(1.0);
                    }
                }
                p.big_ev.write(BigMessageEvent {
                    title: "⚠ 恒星高温".into(),
                    sub: "船体过热，立即远离！".into(),
                    dur: 0.9,
                });
            }
        } else {
            p.ship.sun_heat_t = 0.0;
        }
    }
    // 相机
    let ship_q = ship_quat(p.ship.yaw, p.ship.pitch, p.ship.roll);
    let cam_off = ship_q * Vec3::new(0.0, 3.2, 11.0);
    let target_fov = 75.0 - 5.0 + (p.ship.speed / PULSE_SPEED) * 40.0 + if p.input.boost { 6.0 } else { 0.0 };
    *p.flight_cam = FlightCamera::set(p.ship.pos + cam_off, ship_q, target_fov);
    if let Ok(mut pl) = p.player.single_mut() {
        pl.pos = p.ship.pos;
    }
    // 星球无缝再入
    let mut entering: Option<usize> = None;
    if let Some(sc) = p.scene.as_ref() {
        for pv in &sc.planets {
            let center = Vec3::from(pv.def.pos);
            let d = p.ship.pos.distance(center) - pv.def.radius;
            if d < handoff_dist(&pv.def) {
                let to_center = (center - p.ship.pos).normalize();
                if fwd.dot(to_center) > -0.5 {
                    entering = Some(pv.def.id);
                }
                break;
            }
        }
    }
    if let Some(pid) = entering {
        enter_planet(
            &mut p.next_mode,
            &mut p.ship,
            &mut p.game,
            pid,
            &mut p.flag_ev,
            &mut p.big_ev,
            &mut p.land_ev,
            &mut p.commands,
            &p.sfx,
        );
        return;
    }
    // 空间站自动泊入（飞入泊入区即触发，与 JS Station.tryBegin 一致）
    if let Some(sc) = p.scene.as_ref() {
        if crate::station::in_dock_zone(&p.ship.pos, sc.station_pos) {
            let st = crate::station::begin_dock(&mut p.next_mode, &p.ship.pos, sc.station_pos);
            p.commands.insert_resource(st);
            crate::audio::play(&mut p.commands, p.sfx.click.clone(), 0.6, None);
            return;
        }
    }
    // 星系地图
    if p.keys.just_pressed(KeyCode::KeyM) {
        p.ui_state.panel = Panel::GalaxyMap;
    }
    // 扫描
    if p.keys.just_pressed(KeyCode::KeyC) {
        let mut best: Option<(String, f32)> = None;
        if let Some(sc) = p.scene.as_ref() {
            for pv in &sc.planets {
                let d = p.ship.pos.distance(Vec3::from(pv.def.pos)) - pv.def.radius;
                if best.as_ref().map(|b| d < b.1).unwrap_or(true) {
                    best = Some((pv.def.name.to_string(), d));
                }
            }
            let ds = p.ship.pos.distance(sc.station_pos);
            if best.as_ref().map(|b| ds < b.1).unwrap_or(true) {
                best = Some(("空间站".to_string(), ds));
            }
        }
        if let Some((name, d)) = best {
            p.big_ev.write(BigMessageEvent {
                title: format!("◈ {name}"),
                sub: format!("距离 {:.0} u", d),
                dur: 2.0,
            });
        }
    }
    // 曲速自动跃迁
    tick_warp_auto(
        &mut p.next_mode,
        &mut p.ship,
        &p.game,
        &mut p.player,
        &mut p.big_ev,
        &mut p.commands,
        &p.sfx,
        &mut p.warp_anim,
    );
}

#[allow(clippy::too_many_arguments)]
fn tick_warp_auto(
    mode: &mut FlightMode,
    ship: &mut ShipState,
    game: &SpaceGame,
    player: &mut Query<&mut Player>,
    big_ev: &mut MessageWriter<BigMessageEvent>,
    commands: &mut Commands,
    sfx: &crate::audio::Sfx,
    warp_anim: &mut WarpAnim,
) {
    let Some(lock) = game.warp_lock.clone() else { return };
    if lock.seed == game.galaxy.seed {
        return;
    }
    if !ship.pulsing || ship.speed < WARP_ENGAGE_SPEED {
        return;
    }
    let dir = galaxy_dir(lock.seed);
    let fwd = ship_forward(ship.yaw, ship.pitch);
    if fwd.dot(dir) < 0.94 {
        return;
    }
    let Ok(mut p) = player.single_mut() else { return };
    if p.inv.count_item("warpcell") < 1 {
        big_ev.write(BigMessageEvent {
            title: "缺少曲率电池".into(),
            sub: "跃迁需曲率电池×1 — 精炼厂合成或空间站购买".into(),
            dur: 2.6,
        });
        crate::audio::play(commands, sfx.error.clone(), 0.5, None);
        return;
    }
    p.inv.remove_item("warpcell", 1);
    ship.pulsing = false;
    ship.pulse_charge = 0.0;
    let yaw = (-dir.x).atan2(-dir.z);
    let pitch = dir.y.clamp(-1.0, 1.0).asin();
    *warp_anim = WarpAnim {
        active: true,
        t: 0.0,
        seed: lock.seed,
        yaw,
        pitch,
        v0: ship.speed,
    };
    *mode = FlightMode::Warping;
    big_ev.write(BigMessageEvent {
        title: "⟠ 曲速引擎点火".into(),
        sub: "脉冲全速突破 · 跃迁通道展开".into(),
        dur: 3.0,
    });
    crate::audio::play(commands, sfx.laser_hit.clone(), 0.8, None);
}

// ---------- 曲速动画 ----------

#[allow(clippy::too_many_arguments)]
pub fn warp_system(
    time: Res<Time>,
    mut next_mode: ResMut<FlightMode>,
    mut ship: ResMut<ShipState>,
    mut anim: ResMut<WarpAnim>,
    mut game: ResMut<SpaceGame>,
    mut flag_ev: MessageWriter<FlagEvent>,
    mut big_ev: MessageWriter<BigMessageEvent>,
    mut arrive_ev: MessageWriter<WarpArriveEvent>,
    mut flight_cam: ResMut<FlightCamera>,
) {
    if *next_mode != FlightMode::Warping {
        return;
    }
    if !anim.active {
        return;
    }
    let dt = time.delta_secs();
    anim.t += dt;
    let total = WARP_LAUNCH + WARP_RIDE;
    if anim.t < WARP_LAUNCH {
        let ak = (anim.t / (WARP_LAUNCH * 0.7)).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - ak) * (1.0 - ak);
        ship.yaw += (anim.yaw - ship.yaw) * ease * 0.3;
        ship.pitch += (anim.pitch - ship.pitch) * ease * 0.3;
        let vk = (anim.t / WARP_LAUNCH).clamp(0.0, 1.0);
        let ve = 1.0 - (1.0 - vk) * (1.0 - vk);
        ship.speed = anim.v0 + (4800.0 - anim.v0) * ve;
    } else {
        ship.yaw = anim.yaw;
        ship.pitch = anim.pitch;
        ship.speed = 4800.0;
    }
    ship.roll = 0.0;
    let q = ship_quat(ship.yaw, ship.pitch, ship.roll);
    let fwd = ship_forward(ship.yaw, ship.pitch);
    let sp = ship.speed;
    ship.pos += fwd * sp * dt;
    let cam_off = q * Vec3::new(0.0, 3.4, 12.0);
    *flight_cam = FlightCamera::set(ship.pos + cam_off, q, 111.0);
    if anim.t >= total {
        anim.active = false;
        finish_warp(&mut next_mode, &mut game, anim.seed, &mut flag_ev, &mut big_ev, &mut arrive_ev, &mut ship);
    }
}

fn finish_warp(
    next_mode: &mut FlightMode,
    game: &mut SpaceGame,
    target_seed: u32,
    flag_ev: &mut MessageWriter<FlagEvent>,
    big_ev: &mut MessageWriter<BigMessageEvent>,
    arrive_ev: &mut MessageWriter<WarpArriveEvent>,
    ship: &mut ShipState,
) {
    let prev_seed = game.galaxy.seed;
    let leaving_home = prev_seed == data::HOME_GALAXY_SEED;
    game.archives.insert(
        prev_seed,
        GalaxyArchive { planets: game.visited.clone(), marks: HashMap::new() },
    );
    let gal = if target_seed == data::HOME_GALAXY_SEED {
        data::home_galaxy()
    } else {
        data::generate_galaxy(target_seed)
    };
    let restored = game.archives.remove(&target_seed).unwrap_or_default();
    game.galaxy = gal;
    game.visited = restored.planets;
    game.current_planet = 0;
    game.landed_planet = -1;
    game.galaxy_count += 1;
    game.warp_lock = None;
    // 定位到第一个行星附近
    let p0 = game.galaxy.planets[0].clone();
    let center = Vec3::from(p0.pos);
    let dir = Vec3::new(0.1, 0.8, 0.6).normalize();
    ship.pos = center + dir * (p0.radius + 85.0);
    ship.speed = 20.0;
    ship.yaw = 0.0;
    ship.pitch = 0.0;
    ship.roll = 0.0;
    ship.pulsing = false;
    ship.pulse_charge = 0.0;
    flag_ev.write(FlagEvent { flag: "warpedOut".into() });
    if leaving_home {
        big_ev.write(BigMessageEvent {
            title: "第一章 完结".into(),
            sub: "起源星系在身后化为一粒尘埃。宇宙没有边界——继续前进吧，旅行者。".into(),
            dur: 5.0,
        });
    } else {
        big_ev.write(BigMessageEvent {
            title: format!("抵达 {}", game.galaxy.name),
            sub: format!("{} 颗星球等待探索", game.galaxy.planets.len()),
            dur: 4.0,
        });
    }
    arrive_ev.write(WarpArriveEvent);
    *next_mode = FlightMode::Space;
}

// ---------- 无缝再入星球 ----------

#[allow(clippy::too_many_arguments)]
fn enter_planet(
    mode: &mut FlightMode,
    ship: &mut ShipState,
    game: &mut SpaceGame,
    pid: usize,
    flag_ev: &mut MessageWriter<FlagEvent>,
    big_ev: &mut MessageWriter<BigMessageEvent>,
    land_ev: &mut MessageWriter<LandPlanetEvent>,
    commands: &mut Commands,
    sfx: &crate::audio::Sfx,
) {
    let Some(pd) = game.galaxy.planets.iter().find(|p| p.id == pid).cloned() else { return };
    let was_new = !game.visited.contains_key(&pid) && game.landed_planet != pid as i32;
    let s = voxel_scale(&pd);
    // 太空→体素换系
    let center = Vec3::from(pd.pos);
    let dir = (ship.pos - center).normalize();
    let lon = dir.z.atan2(dir.x);
    let lat = dir.y.clamp(-1.0, 1.0).asin();
    let ex = lon / 0.004;
    let ez = lat / 0.004;
    let alt = data::SEA_Y + (ship.pos.distance(center) - pd.radius) / s;
    // 保守兜底高度（同星球再入时 alt 已足够；新星球地形加载后由区块流式接管）
    let gy = data::SEA_Y;
    ship.pos = Vec3::new(ex, alt.max(gy as f32 + 40.0), ez);
    ship.pitch = ship.pitch.clamp(-1.55, 1.55);
    ship.pitch_lim = ship.pitch.abs().clamp(1.2, 1.55);
    ship.pitch_floor = Some(-0.4);
    ship.speed = (ship.speed / s).min(110.0);
    ship.roll = 0.0;
    ship.warmed = false;
    ship.presaved = false;
    ship.pulsing = false;
    ship.pulse_charge = 0.0;
    ship.reentry_t = 2.6;
    if was_new {
        flag_ev.write(FlagEvent { flag: "newPlanet".into() });
    }
    // 异球再入：发换球事件（planet_switch_system 用旧 current_planet 归档并重建场景）；
    // 同球再入：无需切换世界
    if pid != game.current_planet {
        land_ev.write(LandPlanetEvent { pid });
    } else {
        game.current_planet = pid;
    }
    game.landed_planet = pid as i32;
    *mode = FlightMode::Atmo;
    let b = data::biome_by_key(pd.biome);
    big_ev.write(BigMessageEvent {
        title: pd.name.to_string(),
        sub: format!("{} — E 就地降落", b.name),
        dur: 4.0,
    });
    crate::audio::play(commands, sfx.laser_hit.clone(), 0.5, None);
}

// ---------- 降落动画 ----------

#[allow(clippy::too_many_arguments)]
pub fn atmoland_system(
    time: Res<Time>,
    mut next_mode: ResMut<FlightMode>,
    mut ship: ResMut<ShipState>,
    mut land: ResMut<AtmoLand>,
    mut game: ResMut<SpaceGame>,
    mut flight_cam: ResMut<FlightCamera>,
    mut big_ev: MessageWriter<BigMessageEvent>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    if *next_mode != FlightMode::AtmoLand {
        return;
    }
    let dt = time.delta_secs();
    land.t += dt / 1.6;
    let t = land.t.min(1.0);
    let ease = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
    ship.pos = land.from.lerp(land.to, ease);
    ship.pitch = 0.0;
    ship.roll = 0.0;
    ship.cam_roll = 0.0;
    // 环绕镜头
    let cam_pos = Vec3::new(
        ship.pos.x + ship.yaw.sin() * 8.0,
        ship.pos.y + 4.0,
        ship.pos.z + ship.yaw.cos() * 8.0,
    );
    let cam_q = Quat::from_rotation_arc(Vec3::NEG_Z, (ship.pos - cam_pos).normalize());
    *flight_cam = FlightCamera::set(cam_pos, cam_q, 75.0);
    if t >= 1.0 {
        game.ship_pos = land.to;
        game.landed_planet = game.current_planet as i32;
        *next_mode = FlightMode::Seated;
        big_ev.write(BigMessageEvent {
            title: "降落完成".into(),
            sub: "E 下船 · W 再次起飞".into(),
            dur: 2.2,
        });
        crate::audio::play(&mut commands, sfx.jump.clone(), 0.6, None);
    }
}

// ---------- 相机驱动 ----------

/// 飞行相机（Atmo/Space/Warping/Station 由各自系统写 FlightCamera，本系统应用）。
pub fn flight_camera_system(
    mode: Res<FlightMode>,
    flight_cam: Res<FlightCamera>,
    mut cam: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<Player>)>,
) {
    if !mode.ship_cam() {
        return;
    }
    for (mut tf, mut proj) in &mut cam {
        tf.translation = flight_cam.pos;
        tf.rotation = flight_cam.rot;
        *proj = Projection::Perspective(PerspectiveProjection {
            fov: flight_cam.fov.to_radians(),
            ..default()
        });
    }
}

// ---------- 太空场景生命周期 ----------

/// 模式切换时创建/销毁太空场景。
#[allow(clippy::too_many_arguments)]
pub fn space_scene_sync_system(
    mode: Res<FlightMode>,
    scene: Option<ResMut<SpaceScene>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    game: Res<SpaceGame>,
    mut commands: Commands,
) {
    let want = mode.space_scene();
    match (want, scene) {
        (true, None) => {
            let sc = spawn_space_scene(&mut commands, &mut meshes, &mut images, &mut mats, &game.galaxy);
            commands.insert_resource(sc);
        }
        (false, Some(sc)) => {
            despawn_space_scene(&mut commands, &sc);
            commands.remove_resource::<SpaceScene>();
        }
        _ => {}
    }
}

/// 曲速抵达：重建太空场景（新星系）。
#[allow(clippy::too_many_arguments)]
pub fn warp_arrive_system(
    mut ev: MessageReader<WarpArriveEvent>,
    scene: Option<ResMut<SpaceScene>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    game: Res<SpaceGame>,
    mut commands: Commands,
) {
    for _ in ev.read() {
        if let Some(sc) = scene.as_deref() {
            despawn_space_scene(&mut commands, sc);
            commands.remove_resource::<SpaceScene>();
        }
        let sc = spawn_space_scene(&mut commands, &mut meshes, &mut images, &mut mats, &game.galaxy);
        commands.insert_resource(sc);
    }
}

// ---------- 船体实体同步 ----------

pub fn ship_sync_system(
    ship: Res<ShipState>,
    ship_asset: Res<ShipAsset>,
    mut q: Query<&mut Transform>,
    mode: Res<FlightMode>,
) {
    let Some(e) = ship_asset.entity else { return };
    let Ok(mut tf) = q.get_mut(e) else { return };
    tf.translation = ship.pos;
    tf.rotation = ship_quat(ship.yaw, ship.pitch, ship.roll);
    let flame_scale = 0.4 + ship.speed / MAX_SPEED * 0.8 + if ship.pulsing { 2.0 } else { 0.0 };
    for f in &ship_asset.flames {
        if let Ok(mut ftf) = q.get_mut(*f) {
            ftf.scale.z = flame_scale;
        }
    }
    let _ = mode;
}

/// 地面模式时飞船停泊在 ship_pos。
pub fn ship_parked_system(
    mode: Res<FlightMode>,
    game: Res<SpaceGame>,
    mut ship_state: ResMut<ShipState>,
    world: Option<Res<VoxelWorld>>,
) {
    if *mode != FlightMode::Planet && *mode != FlightMode::Seated {
        return;
    }
    // 降落/泊入后船位与状态资源同步
    ship_state.pos = game.ship_pos;
    ship_state.speed = 0.0;
    ship_state.pulsing = false;
    if let Some(w) = world {
        let gy = w.top_at(game.ship_pos.x.floor() as i32, game.ship_pos.z.floor() as i32);
        if (game.ship_pos.y - gy as f32).abs() > 3.0 {
            ship_state.pos.y = gy as f32 + 1.0;
        }
    }
}

// ---------- 初始飞船 ----------

pub fn spawn_initial_ship(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    world: &VoxelWorld,
    ship_data: &ShipData,
) -> (Entity, Vec<Entity>, Vec3) {
    let spawn = world.find_spawn(96, 96);
    let pos = Vec3::new(
        spawn.x + 4.0,
        world.top_at((spawn.x + 4.0) as i32, (spawn.z + 4.0) as i32) as f32 + 1.0,
        spawn.z + 4.0,
    );
    let cls = data::ship_class_by_key(&ship_data.cls);
    let (e, flames) = spawn_ship(commands, meshes, mats, pos, 0.0, cls);
    (e, flames, pos)
}

// ---------- 保存辅助 ----------

pub fn serialize_ship_state(ship: &ShipState) -> save::ShipStateSave {
    save::ShipStateSave {
        pos: [ship.pos.x, ship.pos.y, ship.pos.z],
        yaw: ship.yaw,
        pitch: ship.pitch,
        roll: ship.roll,
        speed: ship.speed,
    }
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn galaxy_generation_deterministic() {
        let g1 = data::generate_galaxy(12345);
        let g2 = data::generate_galaxy(12345);
        assert_eq!(g1.planets.len(), g2.planets.len());
        for (a, b) in g1.planets.iter().zip(g2.planets.iter()) {
            assert_eq!(a.biome, b.biome);
            assert_eq!(a.pos, b.pos);
            assert_eq!(a.radius, b.radius);
        }
        assert_eq!(g1.station, g2.station);
        assert_eq!(g1.market, g2.market);
    }

    #[test]
    fn galaxy_has_carbon_planet_and_station_clearance() {
        for seed in 1..200u32 {
            let g = data::generate_galaxy(seed);
            assert!(
                g.planets.iter().any(|p| matches!(p.biome, "lush" | "ocean" | "fungal" | "alien")),
                "seed {seed} 缺少富碳星球"
            );
            for p in &g.planets {
                let c = Vec3::from(p.pos);
                let st = Vec3::from(g.station);
                assert!(st.distance(c) > p.radius + 150.0, "seed {seed} 空间站太近");
            }
        }
    }

    #[test]
    fn neighbor_seeds_deterministic() {
        let a = neighbor_seeds(7777);
        let b = neighbor_seeds(7777);
        assert_eq!(a, b);
        assert!(!a.is_empty());
        let c = neighbor_seeds(999);
        assert!(c.contains(&data::HOME_GALAXY_SEED));
    }

    #[test]
    fn ship_class_roll() {
        assert_eq!(data::roll_ship_class(0.0).key, "C");
        assert_eq!(data::roll_ship_class(0.6).key, "B");
        assert_eq!(data::roll_ship_class(0.85).key, "A");
        assert_eq!(data::roll_ship_class(0.99).key, "S");
    }

    #[test]
    fn galaxy_name_home() {
        assert_eq!(data::galaxy_name(data::HOME_GALAXY_SEED), "起源星系");
    }

    #[test]
    fn handoff_roundtrip() {
        // 体素坐标 → 太空球面 → 体素坐标（无星球自转时精确互逆）
        let pd = data::DEFAULT_PLANETS[0].clone();
        let s = voxel_scale(&pd);
        let vx = 123.4f32;
        let vz = -45.6f32;
        let vy = 160.0f32;
        let lon = vx * 0.004;
        let lat = (vz * 0.004).clamp(-1.15, 1.15);
        let dir = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
        let center = Vec3::from(pd.pos);
        let sp = center + dir * (pd.radius + (vy - data::SEA_Y) * s);
        // 逆映射
        let d = (sp - center).normalize();
        let lon2 = d.z.atan2(d.x);
        let lat2 = d.y.clamp(-1.0, 1.0).asin();
        let x2 = lon2 / 0.004;
        let z2 = lat2 / 0.004;
        assert!((x2 - vx).abs() < 0.01, "x {x2} vs {vx}");
        assert!((z2 - vz).abs() < 0.01, "z {z2} vs {vz}");
    }
}
