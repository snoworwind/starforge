//! 太空 · 大气层飞行 · 曲速跃迁 — port of js/space.js + js/main.js flight code.
//! 空间站停靠/站内行走在 station.rs。

use bevy::gltf::GltfAssetLabel;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use bevy_world_serialization::prelude::WorldAssetRoot;
use std::collections::HashMap;
use std::time::Duration;

use crate::creatures::Creature;
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
/// 相机远平面：需覆盖远景地形（±1536 格）与太空天体（行星 1500~2600u、星光 9000u）
pub const CAM_FAR: f32 = 12000.0;
pub const WRAP_X: f32 = std::f32::consts::PI * 2.0 / 0.004; // ≈1570.8
pub const WRAP_Z: f32 = 2.3 / 0.004; // =575
// Conservative envelope for the largest ship's wings and for yaw/pitch
// rotations.  The old envelope only covered the central fuselage, allowing
// the wings to enter one-block walls during high-speed flight.
pub const SHIP_BOX: [f32; 3] = [3.8, 1.25, 3.8];
pub const SHIP_R: f32 = 3.0;

/// 飞船根节点中心相对地面方块索引的安全停泊高度。
///
/// `top_at` 返回的是最高方块的 y 索引，方块顶面在 `y + 1`。飞船碰撞
/// 包络的底面还要再向上留出 `SHIP_BOX[1]`，否则降落/起飞时会先与地面
/// 相交，导致船体陷入地形并被碰撞修正卡住。
pub fn parked_ship_y(ground_y: i32) -> f32 {
    ground_y as f32 + 1.0 + SHIP_BOX[1]
}

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
        matches!(
            self,
            Self::Seated
                | Self::Atmo
                | Self::AtmoLand
                | Self::Space
                | Self::Warping
                | Self::Station
        )
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
        matches!(
            self,
            Self::Planet | Self::Seated | Self::Atmo | Self::AtmoLand
        )
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

impl SpaceInput {
    /// 失焦清空全部输入（JS window.blur 移植，防 Alt-Tab 卡键）。
    pub fn clear(&mut self) {
        self.thrust = false;
        self.brake = false;
        self.boost = false;
        self.roll_left = false;
        self.roll_right = false;
        self.pulse = false;
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
    }
}

// ---------- 飞船状态 ----------

#[derive(Resource, Clone, Debug)]
pub struct ShipState {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub cam_roll: f32,
    /// NMS 式转向侧倾（太空：鼠标横向转向时的视觉银行，模型/相机整体携带）
    pub vis_bank: f32,
    pub speed: f32,
    pub pulse_charge: f32,
    pub pulsing: bool,
    pub tritium_drain: f32,
    pub board_yaw: f32,
    /// 座舱第三人称镜头累计时间。
    pub seated_t: f32,
    pub presaved: bool,
    /// 船体生命（JS VIS_HP：C20/B34/A52/S80）
    pub hp: f32,
    pub hp_max: f32,
    /// 开火冷却
    pub fire_cd: f32,
    /// 引擎循环音实体（飞行时播放）
    pub engine_snd: Option<Entity>,
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
            vis_bank: 0.0,
            speed: 0.0,
            pulse_charge: 0.0,
            pulsing: false,
            tritium_drain: 0.0,
            board_yaw: 0.0,
            seated_t: 0.0,
            presaved: false,
            hp: 20.0,
            hp_max: 20.0,
            fire_cd: 0.0,
            engine_snd: None,
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

/// A downloaded glTF scene's animation is attached after Bevy has materialized
/// its child `AnimationPlayer` entity.
#[derive(Clone, Component)]
struct ExternalAnimationSetup {
    model: &'static str,
}

#[derive(Resource, Default)]
pub struct ExternalAnimationLibrary {
    clips: HashMap<&'static str, (Handle<AnimationGraph>, AnimationNodeIndex)>,
}

/// Connect the single embedded animation clip carried by the external model
/// to an AnimationGraph and loop it. This is shared by ships and stations.
fn external_model_animation_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    setups: Query<&ExternalAnimationSetup>,
    mut players: Query<&mut AnimationPlayer>,
    asset_server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut library: ResMut<ExternalAnimationLibrary>,
) {
    let Ok(setup) = setups.get(ready.entity) else {
        return;
    };
    let (graph, clip) = if let Some(cached) = library.clips.get(setup.model) {
        (cached.0.clone(), cached.1)
    } else {
        let (graph, nodes) = AnimationGraph::from_clips([
            asset_server.load(GltfAssetLabel::Animation(0).from_asset(setup.model))
        ]);
        let graph = graphs.add(graph);
        let clip = nodes[0];
        library.clips.insert(setup.model, (graph.clone(), clip));
        (graph, clip)
    };
    for child in children.iter_descendants(ready.entity) {
        let Ok(mut player) = players.get_mut(child) else {
            continue;
        };
        let mut transitions = AnimationTransitions::new();
        transitions.play(&mut player, clip, Duration::ZERO).repeat();
        commands
            .entity(child)
            .insert((AnimationGraphHandle(graph.clone()), transitions));
    }
}

pub(crate) fn attach_external_animation(
    commands: &mut Commands,
    entity: Entity,
    model: &'static str,
) {
    commands
        .entity(entity)
        .insert(ExternalAnimationSetup { model })
        .observe(external_model_animation_ready);
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
    /// 该星球地图标记
    #[serde(default)]
    pub marks: Vec<Mark>,
    /// 该星球兽群（随星球档案持久化）
    #[serde(default)]
    pub creatures: Vec<crate::creatures::HerdSave>,
    /// 兽群细胞占用/被杀位图
    #[serde(default)]
    pub creature_cells: Vec<crate::creatures::CellSave>,
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
    /// 当前星球地图标记（JS mapMarks[pid]）
    pub marks: Vec<Mark>,
}

impl SpaceGame {
    pub fn new(galaxy: Galaxy) -> Self {
        let galaxy = if galaxy.planets.is_empty() {
            // A generated galaxy is never empty, but this keeps a malformed
            // future save or test fixture from turning every planet lookup
            // into an underflow.
            data::home_galaxy()
        } else {
            galaxy
        };
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
            marks: Vec::new(),
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

/// 星球球面淡入（出大气/跃迁抵达时 0→1，约 1.2s，平滑 LOD 过渡）。
#[derive(Component)]
pub struct SphereFade {
    pub mat: Handle<StandardMaterial>,
    pub t: f32,
}

/// 星球球面淡入动画。
pub fn sphere_fade_system(
    time: Res<Time>,
    mut q: Query<&mut SphereFade>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    for mut f in &mut q {
        if f.t >= 1.2 {
            continue;
        }
        f.t = (f.t + dt).min(1.2);
        let k = f.t / 1.2;
        let a = k * k * (3.0 - 2.0 * k); // ease in-out
        if let Some(mut m) = mats.get_mut(f.mat.id()) {
            m.base_color.set_alpha(a);
        }
    }
}

#[derive(Component, Clone)]
pub struct LaserBolt {
    pub dir: Vec3,
    pub life: f32,
    pub speed: f32,
    pub dmg: f32,
    /// 大气弹与太空弹属于不同渲染/碰撞场景。
    pub space_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisitorPhase {
    Cruise,
    DockIn,
    Parked,
    DockOut,
}

/// 访客飞船：巡航、进港、停泊、离港与战斗目标共享同一实体。
#[derive(Component)]
pub struct VisitorShip {
    pub cls: &'static str,
    pub hp: f32,
    pub hp_max: f32,
    pub target: Vec3,
    pub speed: f32,
    pub phase: VisitorPhase,
    pub path: Vec<Vec3>,
    pub path_index: usize,
    pub pad: Option<usize>,
    pub timer: f32,
}

/// 太空掉落物（击碎小行星 / 击毁访客船）。
#[derive(Component)]
pub struct SpaceDrop {
    pub item: String,
    pub n: i32,
    pub vel: Vec3,
    pub age: f32,
}

/// 访客船补员计时。
#[derive(Resource, Default)]
pub struct VisitorRespawn {
    pub t: f32,
    pub initial_fill_done: bool,
}

#[derive(Resource, Default)]
pub struct VisitorTraffic {
    pub pads: [Option<Entity>; 3],
}

/// 武器参数（JS BOLT_SPECS 简化：(dmg, speedMul)）。
fn weapon_spec(cls_key: &str) -> (f32, f32) {
    match cls_key {
        "B" => (1.0, 1.15),
        "A" => (2.0, 1.9),
        "S" => (4.0, 1.05),
        _ => (1.0, 1.0),
    }
}

/// 访客船体强度（JS VIS_HP）。
pub fn vis_hp(cls_key: &str) -> f32 {
    match cls_key {
        "B" => 34.0,
        "A" => 52.0,
        "S" => 80.0,
        _ => 20.0,
    }
}

/// 击毁战利品（JS PIRATE_LOOT：(item, min, max)）。
fn pirate_loot(cls_key: &str) -> (i32, &'static [(&'static str, i32, i32)]) {
    match cls_key {
        "B" => (2500, &[("tritium", 8, 14), ("circuit", 2, 4)]),
        "A" => (
            6000,
            &[("tritium", 10, 16), ("data", 3, 5), ("gold_ore", 2, 3)],
        ),
        "S" => (
            15000,
            &[("data", 5, 8), ("gold_ore", 3, 5), ("warpcell", 1, 1)],
        ),
        _ => (800, &[("tritium", 6, 10)]),
    }
}

fn segment_hits_sphere(start: Vec3, end: Vec3, center: Vec3, radius: f32) -> bool {
    let segment = end - start;
    let length_squared = segment.length_squared();
    let t = if length_squared > 1e-8 {
        ((center - start).dot(segment) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (start + segment * t).distance_squared(center) <= radius * radius
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct BoltAux<'w, 's> {
    scene: Option<Res<'w, SpaceScene>>,
    defense: ResMut<'w, crate::station::StationDefense>,
    traffic: ResMut<'w, VisitorTraffic>,
    player: Query<'w, 's, &'static mut Player>,
    big_ev: MessageWriter<'w, BigMessageEvent>,
    sfx: Res<'w, crate::audio::Sfx>,
    creatures: Query<
        'w,
        's,
        (Entity, &'static mut Creature, &'static Transform),
        (
            With<Creature>,
            Without<LaserBolt>,
            Without<VisitorShip>,
            Without<Asteroid>,
        ),
    >,
}

/// 太空战斗：左键开火 + 弹道更新 + 命中（访客船/小行星）。
#[allow(clippy::too_many_arguments)]
pub fn bolt_system(
    time: Res<Time>,
    mode: Res<FlightMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut ship: ResMut<ShipState>,
    ship_asset: Res<ShipAsset>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut feedback: ResMut<crate::feedback::FeedbackAssets>,
    mut bolt_assets: Local<
        Option<(
            Handle<Mesh>,
            Handle<StandardMaterial>,
            Handle<StandardMaterial>,
        )>,
    >,
    mut bolts: Query<
        (Entity, &mut LaserBolt, &mut Transform),
        (Without<VisitorShip>, Without<Asteroid>, Without<Creature>),
    >,
    mut visitors: Query<
        (Entity, &mut VisitorShip, &Transform),
        (Without<LaserBolt>, Without<Asteroid>, Without<Creature>),
    >,
    asteroids: Query<
        (Entity, &Transform),
        (
            With<Asteroid>,
            Without<LaserBolt>,
            Without<VisitorShip>,
            Without<Creature>,
        ),
    >,
    mut aux: BoltAux,
) {
    if !matches!(*mode, FlightMode::Space | FlightMode::Atmo) {
        return;
    }
    let dt = time.delta_secs();
    ship.fire_cd = (ship.fire_cd - dt).max(0.0);
    if bolt_assets.is_none() {
        let mesh = meshes.add(Cuboid::new(0.16, 0.16, 1.6));
        let mat = mats.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.55, 0.25),
            emissive: LinearRgba::new(1.0, 0.4, 0.15, 1.0) * 2.5,
            unlit: true,
            ..default()
        });
        let drop_mat = mats.add(StandardMaterial {
            base_color: Color::srgb(0.55, 1.0, 0.85),
            emissive: LinearRgba::new(0.2, 0.9, 0.7, 1.0) * 1.8,
            unlit: true,
            ..default()
        });
        *bolt_assets = Some((mesh, mat, drop_mat));
    }
    let Some((bolt_mesh, bolt_mat, drop_mat)) = bolt_assets.clone() else {
        return;
    };
    // 开火（JS shoot：双发 ±0.9 偏移，聚向准星）
    if mouse.pressed(MouseButton::Left) && ship.fire_cd <= 0.0 {
        let (cooldown, offsets): (f32, &[f32]) = match ship_asset.data.cls.as_str() {
            "B" => (0.12, &[-0.55, 0.55]),      // rapid twin cannons
            "A" => (0.20, &[-0.95, 0.0, 0.95]), // tri-beam burst
            "S" => (0.48, &[0.0]),              // heavy lance
            _ => (0.22, &[-0.9, 0.9]),
        };
        ship.fire_cd = cooldown;
        let (dmg, smul) = weapon_spec(&ship_asset.data.cls);
        let q = ship_quat(ship.yaw, ship.pitch, ship.roll);
        let fwd = ship_forward(ship.yaw, ship.pitch);
        let right = q * Vec3::X;
        for &off in offsets {
            let origin = ship.pos + right * off + Vec3::Y * 0.4;
            commands.spawn((
                Mesh3d(bolt_mesh.clone()),
                MeshMaterial3d(bolt_mat.clone()),
                // The projectile mesh is modelled along local -Z, just like
                // the ship.  Keep its nose aligned with the actual travel
                // vector instead of leaving it in the world-axis rotation.
                Transform::from_translation(origin).with_rotation(bolt_rotation(fwd)),
                LaserBolt {
                    dir: fwd,
                    life: 1.6,
                    speed: 500.0 * smul,
                    dmg,
                    space_only: *mode == FlightMode::Space,
                },
                crate::InGame,
            ));
        }
        crate::audio::play(&mut commands, aux.sfx.laser_hit.clone(), 0.35, None);
    }
    // 弹道更新 + 命中（快照迭代避免嵌套可变借用）
    let snap: Vec<(Entity, LaserBolt, Vec3)> = bolts
        .iter()
        .map(|(e, b, tf)| (e, b.clone(), tf.translation))
        .collect();
    for (e, b, pos) in snap {
        // 场景交接时清掉另一套场景的弹体，避免大气弹残留到太空或地面。
        if b.space_only != (*mode == FlightMode::Space) {
            commands.entity(e).despawn();
            continue;
        }
        let np = pos + b.dir * b.speed * dt;
        let alive = b.life - dt > 0.0;
        // 访客船命中
        let mut hit_vis: Option<Entity> = None;
        for (ve, v, vt) in &visitors {
            if v.hp <= 0.0 {
                continue;
            }
            if segment_hits_sphere(pos, np, vt.translation, 3.4) {
                hit_vis = Some(ve);
                break;
            }
        }
        if let Some(ve) = hit_vis {
            let (cls, pos, pad) = {
                let Ok((_, v, vt)) = visitors.get(ve) else {
                    continue;
                };
                (v.cls, vt.translation, v.pad)
            };
            let dead = {
                let Ok((_, mut v, _)) = visitors.get_mut(ve) else {
                    continue;
                };
                v.hp -= b.dmg;
                v.hp <= 0.0
            };
            if dead {
                if let Some(pad) = pad {
                    aux.traffic.pads[pad] = None;
                }
                // 击毁：战利品直入货仓 + 信用点（JS destroyVisitor）
                let (cr, items) = pirate_loot(cls);
                if let Ok(mut p) = aux.player.single_mut() {
                    p.credits += cr;
                    for (item, a, bx) in items {
                        let n = a
                            + (crate::rng::Rng::new(
                                (pos.x as u32).wrapping_mul(31)
                                    ^ (pos.z as u32).wrapping_mul(57)
                                    ^ (pos.y as u32).wrapping_mul(97),
                            )
                            .next()
                                * (bx - a + 1) as f32) as i32;
                        let got = p.inv.add_item(item, n);
                        if got < n {
                            spawn_space_drop(
                                &mut commands,
                                &bolt_mesh,
                                &drop_mat,
                                pos + Vec3::Y * 2.0,
                                item,
                                n - got,
                            );
                        }
                    }
                }
                aux.big_ev.write(BigMessageEvent {
                    title: format!("☠ 击毁 {} 级访客船", cls),
                    sub: format!("战利品入舱 · 信用点 +{}", cr),
                    dur: 3.5,
                });
                crate::audio::play(&mut commands, aux.sfx.break_block.clone(), 0.7, None);
                commands.entity(ve).despawn();
            } else {
                crate::audio::play(&mut commands, aux.sfx.laser_hit.clone(), 0.3, None);
            }
            crate::feedback::spawn_block_burst(
                &mut commands,
                &mut feedback,
                &mut meshes,
                &mut mats,
                pos,
                crate::data::ids::METAL,
                time.elapsed_secs() as u32,
            );
            commands.entity(e).despawn();
            continue;
        }
        // 大气层射击也能命中地表生物：伤害交给统一 creature_despawn_system
        // 处理掉落与永久灭绝记录，避免飞船击杀绕过野生动物存档逻辑。
        let mut hit_creature: Option<(Entity, Vec3)> = None;
        for (ce, creature, creature_tf) in &aux.creatures {
            if *mode == FlightMode::Atmo
                && creature.hp > 0.0
                && segment_hits_sphere(
                    pos,
                    np,
                    creature_tf.translation + Vec3::Y * creature.height * 0.45,
                    creature.radius.max(0.6) * 1.8,
                )
            {
                hit_creature = Some((ce, creature_tf.translation));
                break;
            }
        }
        if let Some((ce, cpos)) = hit_creature {
            if let Ok((_, mut creature, _)) = aux.creatures.get_mut(ce) {
                // Use the same weapon damage as ship-vs-ship combat.  The
                // previous multiplier was not the cause of misses; the
                // overly small body sphere was.  A generous capsule proxy
                // makes fast atmospheric shots reliably register on the
                // visible body without requiring pixel-perfect aim.
                creature.hp -= b.dmg;
                creature.hit_t = 0.25;
                if creature.hp <= 0.0 {
                    crate::audio::play(&mut commands, aux.sfx.creature_die.clone(), 0.8, None);
                } else {
                    crate::audio::play(&mut commands, aux.sfx.creature_hit.clone(), 0.45, None);
                }
            }
            crate::feedback::spawn_block_burst(
                &mut commands,
                &mut feedback,
                &mut meshes,
                &mut mats,
                cpos,
                crate::data::ids::LEAVES,
                time.elapsed_secs() as u32,
            );
            commands.entity(e).despawn();
            continue;
        }
        // 小行星命中（JS：r10 击碎 → 氚 4-8 + 25% 金 1-2）
        let mut hit_ast: Option<(Entity, Vec3)> = None;
        for (ae, at) in &asteroids {
            if segment_hits_sphere(pos, np, at.translation, 10.0) {
                hit_ast = Some((ae, at.translation));
                break;
            }
        }
        if let Some((ae, apos)) = hit_ast {
            let mut rng = crate::rng::Rng::new(
                (np.x as u32).wrapping_mul(31)
                    ^ (np.z as u32).wrapping_mul(57)
                    ^ (time.elapsed_secs() as u32).wrapping_mul(97),
            );
            let trit = 4 + (rng.next() * 5.0) as i32;
            spawn_space_drop(&mut commands, &bolt_mesh, &drop_mat, apos, "tritium", trit);
            if rng.next() < 0.25 {
                let gold = 1 + (rng.next() * 2.0) as i32;
                spawn_space_drop(
                    &mut commands,
                    &bolt_mesh,
                    &drop_mat,
                    apos + Vec3::Y * 2.0,
                    "gold_ore",
                    gold,
                );
            }
            crate::audio::play(&mut commands, aux.sfx.break_block.clone(), 0.6, None);
            crate::feedback::spawn_block_burst(
                &mut commands,
                &mut feedback,
                &mut meshes,
                &mut mats,
                apos,
                crate::data::ids::CRYSTAL,
                time.elapsed_secs() as u32,
            );
            commands.entity(ae).despawn();
            commands.entity(e).despawn();
            continue;
        }
        // A stray shot that reaches the station raises its defensive bubble.
        // Once active, the larger shield boundary intercepts subsequent shots.
        if let Some(scene) = aux.scene.as_ref() {
            let center = scene.station_pos + Vec3::new(0.0, 20.0, -20.0);
            let radius = if aux.defense.active() { 213.0 } else { 150.0 };
            if segment_hits_sphere(pos, np, center, radius) {
                let first = aux.defense.raise();
                if first {
                    aux.big_ev.write(BigMessageEvent {
                        title: "⚠ 空间站防护盾激活".into(),
                        sub: "停止攻击 10 秒后恢复准入".into(),
                        dur: 3.0,
                    });
                    crate::audio::play(&mut commands, aux.sfx.alarm.clone(), 0.7, None);
                } else {
                    crate::audio::play(&mut commands, aux.sfx.laser_hit.clone(), 0.35, None);
                }
                commands.entity(e).despawn();
                continue;
            }
        }
        if alive {
            if let Ok((_, mut bq, mut bt)) = bolts.get_mut(e) {
                bq.life = b.life - dt;
                bt.translation = np;
            }
        } else {
            commands.entity(e).despawn();
        }
    }
}

/// 生成太空掉落物（发光小立方）。
fn spawn_space_drop(
    commands: &mut Commands,
    mesh: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    pos: Vec3,
    item: &str,
    n: i32,
) {
    if n <= 0 {
        return;
    }
    commands.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(pos),
        SpaceDrop {
            item: item.to_string(),
            n,
            vel: Vec3::ZERO,
            age: 0.0,
        },
        crate::InGame,
    ));
}

/// 太空掉落物：磁吸飞船拾取（JS 太空拾取）。
pub fn space_drop_system(
    time: Res<Time>,
    mode: Res<FlightMode>,
    mut commands: Commands,
    mut drops: Query<(Entity, &mut SpaceDrop, &mut Transform)>,
    ship: Res<ShipState>,
    mut player: Query<&mut Player>,
    sfx: Res<crate::audio::Sfx>,
) {
    if *mode != FlightMode::Space {
        return;
    }
    let dt = time.delta_secs();
    for (e, mut d, mut tf) in &mut drops {
        d.age += dt;
        if d.age > 180.0 {
            commands.entity(e).despawn();
            continue;
        }
        let dist = tf.translation.distance(ship.pos);
        if dist < 6.5 {
            let dir = (ship.pos - tf.translation).normalize_or_zero();
            tf.translation += dir * 24.0 * dt;
            if dist < 1.6
                && let Ok(mut p) = player.single_mut()
            {
                let added = p.inv.add_item(&d.item, d.n);
                if added > 0 {
                    d.n -= added;
                    crate::audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
                }
                if d.n <= 0 {
                    commands.entity(e).despawn();
                }
            }
        } else {
            tf.translation += d.vel * dt;
            d.vel *= (1.0 - dt).max(0.0);
        }
    }
}

/// 访客舰队：巡航（行星轨道点之间）+ 补员（35-65s）。
#[allow(clippy::too_many_arguments)]
#[cfg(any())]
fn visitor_system_legacy(
    time: Res<Time>,
    mode: Res<FlightMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    game: Res<SpaceGame>,
    mut respawn: ResMut<VisitorRespawn>,
    mut visitors: Query<(Entity, &mut VisitorShip, &mut Transform)>,
) {
    if *mode != FlightMode::Space {
        return;
    }
    let dt = time.delta_secs();
    let count = visitors.iter().count();
    if count < 5 {
        respawn.t -= dt;
        if respawn.t <= 0.0 {
            let mut rng =
                crate::rng::Rng::new((time.elapsed_secs() as u32).wrapping_mul(7919) ^ 0x5EED);
            respawn.t = 35.0 + rng.next() * 30.0;
            // 出生点：当前星球轨道附近
            let pd = &game.galaxy.planets[game.current_planet.min(game.galaxy.planets.len() - 1)];
            let a = rng.next() * std::f32::consts::TAU;
            let el = (rng.next() - 0.5) * 0.8;
            let r = pd.radius + 220.0 + rng.next() * 300.0;
            let pos = Vec3::from(pd.pos)
                + Vec3::new(a.cos() * el.cos(), el.sin(), a.sin() * el.cos()) * r;
            let cls = crate::data::roll_ship_class(rng.next());
            let (ent, flames) = spawn_external_ship(
                &mut commands,
                &mut meshes,
                &mut mats,
                &asset_server,
                pos,
                a,
                cls,
                None,
            );
            for f in flames {
                commands.entity(f).despawn();
            }
            let mut target = Vec3::ZERO;
            let p2 =
                &game.galaxy.planets[(game.current_planet + 1).min(game.galaxy.planets.len() - 1)];
            target = Vec3::from(p2.pos)
                + Vec3::new(rng.next() - 0.5, (rng.next() - 0.5) * 0.5, rng.next() - 0.5)
                    .normalize()
                    * (p2.radius + 220.0 + rng.next() * 300.0);
            commands.entity(ent).insert(VisitorShip {
                cls: cls.key,
                hp: vis_hp(cls.key),
                hp_max: vis_hp(cls.key),
                target,
                speed: 30.0 + rng.next() * 20.0,
            });
        }
    }
    // 巡航
    for (_e, mut v, mut tf) in &mut visitors {
        let to = v.target - tf.translation;
        let d = to.length();
        if d < 25.0 {
            // 换目标：随机行星轨道点
            let mut rng = crate::rng::Rng::new(
                (tf.translation.x as u32).wrapping_mul(31)
                    ^ (time.elapsed_secs() as u32).wrapping_mul(97),
            );
            let pd = &game.galaxy.planets[((rng.next() * game.galaxy.planets.len() as f32)
                as usize)
                .min(game.galaxy.planets.len() - 1)];
            let a = rng.next() * std::f32::consts::TAU;
            let el = (rng.next() - 0.5) * 0.6;
            let r = pd.radius + 220.0 + rng.next() * 300.0;
            v.target = Vec3::from(pd.pos)
                + Vec3::new(a.cos() * el.cos(), el.sin(), a.sin() * el.cos()) * r;
        } else {
            let step = v.speed * dt / d;
            tf.translation += to * step;
            tf.look_to(to, Vec3::Y);
        }
    }
}

fn random_cruise_target(game: &SpaceGame, rng: &mut crate::rng::Rng) -> Vec3 {
    let planet = &game.galaxy.planets[((rng.next() * game.galaxy.planets.len() as f32) as usize)
        .min(game.galaxy.planets.len() - 1)];
    let angle = rng.next() * std::f32::consts::TAU;
    let elevation = (rng.next() - 0.5) * 0.7;
    let radius = planet.radius + 220.0 + rng.next() * 300.0;
    Vec3::from(planet.pos)
        + Vec3::new(
            angle.cos() * elevation.cos(),
            elevation.sin(),
            angle.sin() * elevation.cos(),
        ) * radius
}

fn move_visitor(transform: &mut Transform, target: Vec3, speed: f32, dt: f32) -> bool {
    let offset = target - transform.translation;
    let distance = offset.length();
    if distance < (speed * dt * 2.0).max(2.0) {
        transform.translation = target;
        return true;
    }
    let direction = offset / distance;
    transform.translation += direction * (speed * dt).min(distance);
    transform.look_to(direction, Vec3::Y);
    false
}

/// Visitor traffic: cruise between planets, claim one of three non-player
/// hangar pads, fly the full approach, park, and later depart.
#[allow(clippy::too_many_arguments)]
pub fn visitor_system(
    time: Res<Time>,
    mode: Res<FlightMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    game: Res<SpaceGame>,
    defense: Res<crate::station::StationDefense>,
    mut respawn: ResMut<VisitorRespawn>,
    mut traffic: ResMut<VisitorTraffic>,
    mut visitors: Query<(Entity, &mut VisitorShip, &mut Transform)>,
) {
    if !matches!(*mode, FlightMode::Space | FlightMode::Station) {
        return;
    }
    let dt = time.delta_secs();
    for pad in &mut traffic.pads {
        if pad.is_some_and(|entity| visitors.get(entity).is_err()) {
            *pad = None;
        }
    }
    let count = visitors.iter().count();
    if count >= 5 {
        respawn.initial_fill_done = true;
    } else {
        respawn.t -= dt;
        if respawn.t <= 0.0 {
            let mut rng = crate::rng::Rng::new(
                (time.elapsed_secs() * 1000.0) as u32 ^ (count as u32).wrapping_mul(7919) ^ 0x5EED,
            );
            let planet =
                &game.galaxy.planets[game.current_planet.min(game.galaxy.planets.len() - 1)];
            let angle = rng.next() * std::f32::consts::TAU;
            let elevation = (rng.next() - 0.5) * 0.8;
            let radius = planet.radius + 220.0 + rng.next() * 300.0;
            let position = Vec3::from(planet.pos)
                + Vec3::new(
                    angle.cos() * elevation.cos(),
                    elevation.sin(),
                    angle.sin() * elevation.cos(),
                ) * radius;
            let cls = crate::data::roll_ship_class(rng.next());
            let (entity, _flames) = spawn_external_ship(
                &mut commands,
                &mut meshes,
                &mut mats,
                &asset_server,
                position,
                angle,
                cls,
                None,
            );
            let target = random_cruise_target(&game, &mut rng);
            commands.entity(entity).insert(VisitorShip {
                cls: cls.key,
                hp: vis_hp(cls.key),
                hp_max: vis_hp(cls.key),
                target,
                speed: 30.0 + rng.next() * 20.0,
                phase: VisitorPhase::Cruise,
                path: Vec::new(),
                path_index: 0,
                pad: None,
                timer: 8.0 + rng.next() * 20.0,
            });
            if count + 1 >= 5 {
                respawn.initial_fill_done = true;
            }
            respawn.t = if respawn.initial_fill_done {
                35.0 + rng.next() * 30.0
            } else {
                0.15
            };
        }
    }

    let station = Vec3::from(game.galaxy.station);
    for (entity, mut visitor, mut transform) in &mut visitors {
        match visitor.phase {
            VisitorPhase::Cruise => {
                visitor.timer -= dt;
                let target = visitor.target;
                let speed = visitor.speed;
                if move_visitor(&mut transform, target, speed, dt) {
                    let mut rng = crate::rng::Rng::new(
                        entity.index().index().wrapping_mul(31)
                            ^ (time.elapsed_secs() * 100.0) as u32,
                    );
                    visitor.target = random_cruise_target(&game, &mut rng);
                }
                if visitor.timer <= 0.0 && !defense.active() {
                    let free = traffic.pads.iter().position(Option::is_none);
                    let mut rng = crate::rng::Rng::new(
                        entity.index().index().wrapping_mul(97)
                            ^ (time.elapsed_secs() * 10.0) as u32,
                    );
                    if let Some(pad) = free.filter(|_| rng.next() < 0.65) {
                        traffic.pads[pad] = Some(entity);
                        visitor.pad = Some(pad);
                        let pad_top = crate::station::visitor_pad_world(station, pad, 3.0);
                        visitor.path = vec![
                            station + Vec3::new(0.0, 12.0, 170.0),
                            station + Vec3::new(0.0, 10.0, 79.0),
                            station + Vec3::new(0.0, 12.0, 44.0),
                            pad_top + Vec3::Y * 7.0,
                            pad_top,
                        ];
                        visitor.path_index = 0;
                        visitor.phase = VisitorPhase::DockIn;
                    } else {
                        visitor.timer = 12.0 + rng.next() * 18.0;
                    }
                }
            }
            VisitorPhase::DockIn => {
                let target = visitor.path[visitor.path_index];
                let speed = if visitor.path_index + 1 == visitor.path.len() {
                    7.0
                } else {
                    24.0
                };
                if move_visitor(&mut transform, target, speed, dt) {
                    visitor.path_index += 1;
                    if visitor.path_index >= visitor.path.len() {
                        visitor.phase = VisitorPhase::Parked;
                        visitor.timer =
                            30.0 + (entity.index().index().wrapping_mul(17) % 40) as f32;
                    }
                }
            }
            VisitorPhase::Parked => {
                visitor.timer -= dt;
                transform.rotation = transform.rotation.slerp(
                    Quat::from_rotation_y(std::f32::consts::PI),
                    (dt * 2.0).min(1.0),
                );
                if visitor.timer <= 0.0 {
                    let pad = visitor.pad.unwrap_or(0);
                    let pad_top = crate::station::visitor_pad_world(station, pad, 3.0);
                    visitor.path = vec![
                        pad_top + Vec3::Y * 7.0,
                        station + Vec3::new(0.0, 12.0, 44.0),
                        station + Vec3::new(0.0, 10.0, 79.0),
                        station + Vec3::new(0.0, 12.0, 190.0),
                    ];
                    visitor.path_index = 0;
                    visitor.phase = VisitorPhase::DockOut;
                }
            }
            VisitorPhase::DockOut => {
                let target = visitor.path[visitor.path_index];
                let speed = if visitor.path_index == 0 { 8.0 } else { 26.0 };
                if move_visitor(&mut transform, target, speed, dt) {
                    visitor.path_index += 1;
                    if visitor.path_index >= visitor.path.len() {
                        if let Some(pad) = visitor.pad.take() {
                            traffic.pads[pad] = None;
                        }
                        let mut rng = crate::rng::Rng::new(
                            entity.index().index().wrapping_mul(193)
                                ^ (time.elapsed_secs() * 10.0) as u32,
                        );
                        visitor.target = random_cruise_target(&game, &mut rng);
                        visitor.timer = 18.0 + rng.next() * 30.0;
                        visitor.path.clear();
                        visitor.path_index = 0;
                        visitor.phase = VisitorPhase::Cruise;
                    }
                }
            }
        }
    }
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

#[derive(Resource, Default)]
pub struct WarpVisuals {
    entities: Vec<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
}

#[derive(Component)]
pub struct WarpStreak {
    local: Vec3,
    speed: f32,
    length: f32,
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

fn emissive_mat(
    mats: &mut Assets<StandardMaterial>,
    color: Color,
    mult: f32,
) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: color,
        emissive: color.to_linear() * mult,
        unlit: true,
        ..default()
    })
}

// ---------- 场景构建 ----------

/// CC0 GLB 飞船模型（Kenney Space Kit，等级差异化映射）。
pub fn ship_model_for(cls_key: &str) -> &'static str {
    match cls_key {
        "B" => "models/ships/ship_b.glb",
        "A" => "models/ships/ship_a.glb",
        "S" => "models/ships/ship_s.glb",
        _ => "models/ships/ship_c.glb",
    }
}

/// 访客船模型（随机轮换）。
pub fn visitor_model_for(i: usize) -> &'static str {
    const V: [&str; 4] = [
        "models/ships/visitor1.glb",
        "models/ships/visitor2.glb",
        "models/ships/visitor3.glb",
        "models/ships/visitor4.glb",
    ];
    V[i % V.len()]
}

/// 统一风格的低多边形飞船。返回 (根实体, 动态尾焰实体)。
///
/// 这里使用程序化模块而不是继续混用多个免费 GLB：飞行、碰撞、存档仍使用
/// 原有 ShipState，视觉只依赖等级轮廓和少量材质，后续也方便替换成 Blender GLB。
pub const SHIP_SCALE: f32 = 1.0;

fn ship_part(
    commands: &mut Commands,
    root: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
) -> Entity {
    let entity = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            crate::InGame,
        ))
        .id();
    commands.entity(root).add_child(entity);
    entity
}

fn ship_profile(key: &str) -> (Vec3, f32, f32, f32, usize, Color) {
    match key {
        // 宽体探索船：更厚的机身、更大的翼面和额外货舱。
        "S" => (
            Vec3::new(3.4, 1.35, 4.6),
            2.9,
            5.8,
            2.3,
            3,
            Color::srgb(1.0, 0.72, 0.2),
        ),
        // 长航程船：长机身、紫色识别条。
        "A" => (
            Vec3::new(2.7, 1.05, 4.2),
            2.7,
            4.8,
            1.9,
            2,
            Color::srgb(0.72, 0.48, 1.0),
        ),
        // 拦截船：低矮、宽翼、双引擎。
        "B" => (
            Vec3::new(2.4, 0.82, 3.6),
            2.35,
            5.2,
            1.7,
            2,
            Color::srgb(0.12, 0.86, 0.9),
        ),
        // 初始小型调度船：方正、单体、易辨认。
        _ => (
            Vec3::new(2.25, 0.95, 3.2),
            2.2,
            3.9,
            1.55,
            1,
            Color::srgb(0.62, 0.7, 0.78),
        ),
    }
}

pub fn spawn_ship(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    _asset_server: &AssetServer,
    pos: Vec3,
    yaw: f32,
    cls: &ShipClass,
) -> (Entity, Vec<Entity>) {
    let (body_size, nose_z, wing_span, wing_depth, engine_count, accent) = ship_profile(cls.key);
    let hull_color = match cls.key {
        "S" => Color::srgb(0.3, 0.25, 0.19),
        "A" => Color::srgb(0.22, 0.2, 0.3),
        "B" => Color::srgb(0.16, 0.27, 0.31),
        _ => Color::srgb(0.32, 0.38, 0.44),
    };
    let hull = mats.add(StandardMaterial {
        base_color: hull_color,
        perceptual_roughness: 0.52,
        metallic: 0.72,
        ..default()
    });
    let hull_dark = mats.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.1, 0.13),
        perceptual_roughness: 0.72,
        metallic: 0.62,
        ..default()
    });
    let canopy = mats.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.28, 0.34),
        emissive: LinearRgba::new(0.02, 0.16, 0.2, 1.0) * 1.2,
        perceptual_roughness: 0.22,
        metallic: 0.3,
        ..default()
    });
    let accent_mat = mats.add(StandardMaterial {
        base_color: accent,
        emissive: accent.to_linear() * 1.5,
        unlit: true,
        ..default()
    });
    let engine_mat = mats.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.72, 0.9),
        emissive: LinearRgba::new(0.03, 0.55, 0.9, 1.0) * 3.0,
        unlit: true,
        ..default()
    });
    let nav_red = mats.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.14, 0.08),
        emissive: LinearRgba::new(1.0, 0.04, 0.01, 1.0) * 2.5,
        unlit: true,
        ..default()
    });
    let nav_green = mats.add(StandardMaterial {
        base_color: Color::srgb(0.2, 1.0, 0.42),
        emissive: LinearRgba::new(0.05, 0.8, 0.18, 1.0) * 2.5,
        unlit: true,
        ..default()
    });
    let root = commands
        .spawn((
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            crate::InGame,
        ))
        .id();

    // 机身、前鼻和座舱：椭球座舱是远处最容易辨认的视觉锚点。
    ship_part(
        commands,
        root,
        meshes.add(Cuboid::new(body_size.x, body_size.y, body_size.z)),
        hull.clone(),
        Transform::from_xyz(0.0, 0.0, 0.35),
    );
    ship_part(
        commands,
        root,
        meshes.add(Sphere::new(1.0)),
        hull.clone(),
        Transform::from_xyz(0.0, 0.0, -nose_z * 0.76).with_scale(Vec3::new(
            body_size.x * 0.52,
            body_size.y * 0.7,
            body_size.z * 0.52,
        )),
    );
    ship_part(
        commands,
        root,
        meshes.add(Sphere::new(1.0)),
        canopy,
        Transform::from_xyz(0.0, body_size.y * 0.48, -0.55).with_scale(Vec3::new(
            body_size.x * 0.27,
            body_size.y * 0.23,
            body_size.z * 0.34,
        )),
    );

    // 翼面和尾鳍，等级通过宽度、数量和角度形成轮廓差异。
    let wing_y = -body_size.y * 0.1;
    for side in [-1.0f32, 1.0] {
        let wing_x = side * (wing_span * 0.32);
        let wing = ship_part(
            commands,
            root,
            meshes.add(Cuboid::new(wing_span * 0.58, 0.22, wing_depth)),
            hull.clone(),
            Transform::from_xyz(wing_x, wing_y, 0.25)
                .with_rotation(Quat::from_rotation_y(side * 0.18)),
        );
        if cls.key == "S" {
            // S 级为宽体货船，外挂舱让侧面轮廓更有分量。
            ship_part(
                commands,
                root,
                meshes.add(Cuboid::new(0.9, 0.7, 1.75)),
                hull_dark.clone(),
                Transform::from_xyz(side * (wing_span * 0.48), -0.1, 0.75),
            );
        }
        let _ = wing;
    }
    ship_part(
        commands,
        root,
        meshes.add(Cuboid::new(0.28, 1.1, 1.55)),
        hull_dark.clone(),
        Transform::from_xyz(0.0, body_size.y * 0.7, 1.55),
    );
    ship_part(
        commands,
        root,
        meshes.add(Cuboid::new(body_size.x * 0.72, 0.16, 0.28)),
        accent_mat.clone(),
        Transform::from_xyz(0.0, body_size.y * 0.54, -0.1),
    );

    // 引擎舱、喷口和武器挂点。
    let engine_x = if engine_count == 1 {
        0.0
    } else {
        body_size.x * 0.32
    };
    for i in 0..engine_count {
        let side = if engine_count == 1 {
            0.0
        } else if i == 0 {
            -1.0
        } else {
            1.0
        };
        let x = side * engine_x;
        ship_part(
            commands,
            root,
            meshes.add(Cylinder::new(0.34, 1.3)),
            hull_dark.clone(),
            Transform::from_xyz(x, -0.1, 1.8)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        );
        ship_part(
            commands,
            root,
            meshes.add(Cylinder::new(0.2, 0.15)),
            engine_mat.clone(),
            Transform::from_xyz(x, -0.1, 2.48)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        );
    }
    for side in [-1.0f32, 1.0] {
        ship_part(
            commands,
            root,
            meshes.add(Cuboid::new(0.16, 0.16, 0.72)),
            accent_mat.clone(),
            Transform::from_xyz(side * body_size.x * 0.42, 0.12, -1.45),
        );
    }
    ship_part(
        commands,
        root,
        meshes.add(Sphere::new(0.11)),
        nav_red,
        Transform::from_xyz(-wing_span * 0.52, 0.05, -0.3),
    );
    ship_part(
        commands,
        root,
        meshes.add(Sphere::new(0.11)),
        nav_green,
        Transform::from_xyz(wing_span * 0.52, 0.05, -0.3),
    );

    // 尾焰：仍返回实体列表，现有 ship_sync_system 会按速度和脉冲引擎动态拉伸。
    let flame_mat = mats.add(StandardMaterial {
        base_color: Color::srgba(0.18, 0.78, 1.0, 0.72),
        emissive: LinearRgba::new(0.08, 0.6, 1.0, 1.0) * 3.0,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let mut flames = Vec::new();
    let flame_count = if engine_count == 1 { 1 } else { 2 };
    for i in 0..flame_count {
        let x = if flame_count == 1 {
            0.0
        } else if i == 0 {
            -engine_x
        } else {
            engine_x
        };
        flames.push(ship_part(
            commands,
            root,
            meshes.add(Cuboid::new(0.34, 0.34, 1.65)),
            flame_mat.clone(),
            Transform::from_xyz(x, -0.1, 3.05),
        ));
    }
    (root, flames)
}

/// Spawn a licensed glTF ship from `assets/models/external`.
///
/// The old procedural builder above is kept as a reference for the original
/// silhouette, but all live ship call sites use this asset-backed path. The
/// logical save-game model names are mapped to the downloaded models so old
/// saves keep working without migration.
pub fn spawn_external_ship(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    pos: Vec3,
    yaw: f32,
    cls: &ShipClass,
    model: Option<&str>,
) -> (Entity, Vec<Entity>) {
    let (path, scale, model_rotation, flame_z, flame_count) = match model {
        Some("ship_striker") => (
            "models/external/ships/space_ship_torb/scene.gltf",
            0.18,
            Quat::IDENTITY,
            2.2,
            2,
        ),
        Some("ship_dispatcher") => (
            "models/external/ships/space_ship_b/scene.gltf",
            1.0,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            3.0,
            2,
        ),
        Some("ship_insurgent") => (
            "models/external/ships/supermatic_sky_cruiser/scene.gltf",
            1.1,
            Quat::IDENTITY,
            3.2,
            2,
        ),
        Some("ship") => (
            "models/external/ships/space_ship_c/scene.gltf",
            0.45,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            3.0,
            1,
        ),
        _ => match cls.key {
            "S" => (
                "models/external/ships/unsa_destroyer/scene.gltf",
                0.00028,
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                5.0,
                2,
            ),
            "A" => (
                "models/external/ships/supermatic_sky_cruiser/scene.gltf",
                1.1,
                Quat::IDENTITY,
                3.2,
                2,
            ),
            "B" => (
                "models/external/ships/space_ship_b/scene.gltf",
                1.0,
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                3.0,
                2,
            ),
            _ => (
                "models/external/ships/space_ship_c/scene.gltf",
                0.45,
                Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                3.0,
                1,
            ),
        },
    };
    let root = commands
        .spawn((
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            crate::InGame,
        ))
        .id();
    let scene = commands
        .spawn((
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(path))),
            Transform::from_rotation(model_rotation).with_scale(Vec3::splat(scale)),
            crate::InGame,
        ))
        .id();
    attach_external_animation(commands, scene, path);
    commands.entity(root).add_child(scene);

    let flame_mat = mats.add(StandardMaterial {
        base_color: Color::srgba(0.18, 0.78, 1.0, 0.72),
        emissive: LinearRgba::new(0.08, 0.6, 1.0, 1.0) * 3.0,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let flame_mesh = meshes.add(Cuboid::new(0.34, 0.34, 1.65));
    let mut flames = Vec::new();
    for i in 0..flame_count {
        let x = if flame_count == 1 {
            0.0
        } else if i == 0 {
            -0.9
        } else {
            0.9
        };
        flames.push(ship_part(
            commands,
            root,
            flame_mesh.clone(),
            flame_mat.clone(),
            Transform::from_xyz(x, -0.1, flame_z),
        ));
    }
    (root, flames)
}

/// 程序化星球贴图（128×256，噪声着色 + 极冠）。
pub fn planet_texture(
    images: &mut Assets<Image>,
    biome_key: &str,
    seed: u32,
    world: Option<&VoxelWorld>,
) -> Handle<Image> {
    let b = data::biome_by_key(biome_key);
    let noise = crate::rng::Noise2::new(seed);
    let w = 512usize;
    let h = 1024usize;
    let mut buf = vec![0u8; w * h * 4];
    let tint = (
        ((b.tint >> 16) & 0xFF) as f32 / 255.0,
        ((b.tint >> 8) & 0xFF) as f32 / 255.0,
        (b.tint & 0xFF) as f32 / 255.0,
    );
    let wt = b.water_tint;
    let water_rgb = [
        ((wt >> 16) & 0xFF) as f32 / 255.0,
        ((wt >> 8) & 0xFF) as f32 / 255.0,
        (wt & 0xFF) as f32 / 255.0,
    ];
    let seab = world.map(|wd| wd.g.sea() as f32).unwrap_or(32.0);
    // 球面 UV ↔ 体素坐标（与 exit_to_space 的 lon/lat 映射一致：lon = x*0.004, lat = z*0.004）。
    // 有当前星球体素世界时采样真实地形（高度/地表覆盖/海平面），
    // 飞出大气后星球贴图与刚离开的地表一致（JS "整球回绘" 的近似）；否则程序化噪声回退。
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        let lat = (0.5 - v) * std::f32::consts::PI;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let lon = (u - 0.5) * std::f32::consts::TAU;
            let wx = (lon / 0.004).floor();
            let wz = (lat / 0.004).floor();
            let (mut r, mut g, mut bl) = if let Some(wd) = world {
                let hgt = wd.g.height_at(wx, wz) as f32;
                let mut grd = wd.g.biome.grass;
                if let Some((k, _, _)) = wd.g.sub_at(wx, wz)
                    && !k.is_empty()
                {
                    grd = k;
                }
                let grass = match grd {
                    "sand" => (0.80, 0.74, 0.55),
                    "snow" => (0.92, 0.94, 0.96),
                    "basalt" | "ash" | "rust" => (0.45, 0.44, 0.42),
                    "salt" => (0.85, 0.85, 0.88),
                    "obsidian" => (0.18, 0.18, 0.22),
                    "hive" | "amber" => (0.55, 0.42, 0.25),
                    "alien" => (0.45, 0.30, 0.55),
                    "murk" => (0.35, 0.42, 0.30),
                    "redmoss" => (0.55, 0.25, 0.25),
                    _ => tint,
                };
                if hgt < seab && !b.dry {
                    let depth = (seab - hgt).clamp(0.0, 12.0);
                    let sh = (0.78 - depth * 0.02).clamp(0.15, 1.0);
                    (water_rgb[0] * sh, water_rgb[1] * sh, water_rgb[2] * sh)
                } else {
                    let (cr, cg, cb) = if hgt < seab + 1.0
                        && !matches!(
                            b.grass,
                            "sand"
                                | "basalt"
                                | "ash"
                                | "salt"
                                | "obsidian"
                                | "rust"
                                | "hive"
                                | "amber"
                        ) {
                        (0.80, 0.74, 0.55)
                    } else {
                        grass
                    };
                    let sh = (0.72 + (hgt - 14.0) * 0.012).clamp(0.35, 1.35);
                    (cr * sh, cg * sh, cb * sh)
                }
            } else {
                // 程序化回退：大陆/海洋分形 + 生态色调 + 高度阴影 + 极冠
                let n = noise.fbm2(lon.cos() * 3.0, lat.sin() * 3.0, 4, 2.0, 0.5);
                let n2 = noise.fbm2(lon.sin() * 5.0 + 17.0, lat.cos() * 5.0 + 3.0, 3, 2.0, 0.5);
                let land = n * 0.65 + n2 * 0.35;
                let shade = (0.82 + n * 0.22 + n2 * 0.10).clamp(0.5, 1.2);
                if land < 0.42 {
                    let d = (0.42 - land) * 4.0;
                    (
                        water_rgb[0] * (0.55 - d * 0.3),
                        water_rgb[1] * (0.55 - d * 0.3),
                        water_rgb[2] * (0.55 - d * 0.3),
                    )
                } else {
                    (tint.0 * shade, tint.1 * shade, tint.2 * shade)
                }
            };
            let ice = ((lat.abs() - 1.15).max(0.0) * 8.0).min(1.0);
            r = r * (1.0 - ice) + 0.95 * ice;
            g = g * (1.0 - ice) + 0.96 * ice;
            bl = bl * (1.0 - ice) + 0.98 * ice;
            let i = (y * w + x) * 4;
            buf[i] = (r * 255.0).min(255.0) as u8;
            buf[i + 1] = (g * 255.0).min(255.0) as u8;
            buf[i + 2] = (bl * 255.0).min(255.0) as u8;
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
    // 线性过滤：修复旧版 128×256 + 最近邻的“像素大色块”简陋贴图
    img.sampler = bevy::image::ImageSampler::linear();
    images.add(img)
}

/// 构建太空场景（恒星/星球/星空/小行星/空间站）。
/// `world` 为当前星球的体素世界（星球贴图采样真实地形用），`current_planet` 为其星球 id。
#[allow(clippy::too_many_arguments)]
pub fn spawn_space_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    mats: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    galaxy: &Galaxy,
    world: Option<&VoxelWorld>,
    current_planet: usize,
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
        // 当前星球：采样真实体素地形；其他星球：程序化回退
        let sample_world = if pd.id == current_planet { world } else { None };
        let tex = planet_texture(images, pd.biome, 1000 + pd.id as u32 * 137, sample_world);
        // 星球淡入（出大气/跃迁抵达时球面 0→1 平滑出现，避免贴图瞬间突变）
        let mat = mats.add(StandardMaterial {
            base_color_texture: Some(tex),
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.0),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let root = commands
            .spawn((
                Mesh3d(meshes.add(Sphere::new(pd.radius))),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(Vec3::from(pd.pos)),
                SphereFade {
                    mat: mat.clone(),
                    t: 0.0,
                },
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
        planets.push(PlanetVis {
            def: pd.clone(),
            entity: root,
            atmo,
        });
    }
    // 空间站
    let station = crate::station::spawn_station_model(
        commands,
        meshes,
        mats,
        asset_server,
        Vec3::from(galaxy.station),
        galaxy.seed,
    );
    // 小行星（CC0 陨石模型，随机缩放）
    let mut asteroids = Vec::new();
    let mut ar = crate::rng::Rng::new(0xA57E);
    let meteor_a =
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/asteroids/meteor.glb"));
    let meteor_b = asset_server
        .load(GltfAssetLabel::Scene(0).from_asset("models/asteroids/meteor_detailed.glb"));
    for i in 0..26 {
        let ang = ar.next() * std::f32::consts::TAU;
        let dist = 500.0 + ar.next() * 2600.0;
        let el = (ar.next() - 0.5) * 1600.0;
        let pos = Vec3::new(ang.cos() * dist, el, ang.sin() * dist);
        // 陨石模型实测半径约 0.435 单位，按原版球体半径 3~17 换算缩放
        let scale = (3.0 + ar.next() * 14.0) / 0.435;
        let model = if i % 3 == 0 {
            meteor_b.clone()
        } else {
            meteor_a.clone()
        };
        let e = commands
            .spawn((
                WorldAssetRoot(model),
                Transform::from_translation(pos).with_scale(Vec3::new(
                    scale,
                    scale * 0.7,
                    scale * 0.9,
                )),
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

/// 小行星自转 + 补员（击碎后维持 ≥8 颗，远离飞船生成）。
#[allow(clippy::too_many_arguments)]
pub fn asteroid_spin_system(
    time: Res<Time>,
    mut q: Query<(&Asteroid, &mut Transform)>,
    scene: Option<ResMut<SpaceScene>>,
    ship: Res<ShipState>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (a, mut tf) in &mut q {
        tf.rotate(Quat::from_scaled_axis(a.spin * dt));
    }
    if let Some(mut sc) = scene
        && sc.asteroids.len() < 8
    {
        let mut rng =
            crate::rng::Rng::new((time.elapsed_secs() as u32).wrapping_mul(7919) ^ 0xA57E);
        let meteor_a =
            asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/asteroids/meteor.glb"));
        for _ in 0..16 {
            let ang = rng.next() * std::f32::consts::TAU;
            let dist = 900.0 + rng.next() * 2000.0;
            let el = (rng.next() - 0.5) * 1600.0;
            let pos = ship.pos + Vec3::new(ang.cos() * dist, el, ang.sin() * dist);
            if pos.distance(ship.pos) > 800.0 {
                // 陨石模型实测半径约 0.435 单位，按原版球体半径 3~17 换算缩放
                let scale = (3.0 + rng.next() * 14.0) / 0.435;
                let e = commands
                    .spawn((
                        WorldAssetRoot(meteor_a.clone()),
                        Transform::from_translation(pos).with_scale(Vec3::new(
                            scale,
                            scale * 0.7,
                            scale * 0.9,
                        )),
                        Asteroid {
                            spin: Vec3::new(rng.next() * 0.4 - 0.2, rng.next() * 0.4 - 0.2, 0.0),
                        },
                        crate::InGame,
                    ))
                    .id();
                sc.asteroids.push(e);
                break;
            }
        }
    }
}

pub fn despawn_space_scene(
    commands: &mut Commands,
    scene: &SpaceScene,
    extras: Query<Entity, Or<(With<VisitorShip>, With<SpaceDrop>, With<LaserBolt>)>>,
) {
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
    for e in &extras {
        commands.entity(e).despawn();
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
    game: Res<SpaceGame>,
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
    let Ok(mut p) = player.single_mut() else {
        return;
    };
    let ship = game.ship_pos;
    let dx = p.pos.x - ship.x;
    let dy = p.pos.y - ship.y;
    let dz = p.pos.z - ship.z;
    // JS: distanceTo(shipPos) < 4.5
    if dx * dx + dy * dy + dz * dz > 20.25 {
        return;
    }
    if !quests.flags.get("checkedShip").copied().unwrap_or(false) {
        quests.flags.insert("checkedShip".into(), true);
        flag_ev.write(FlagEvent {
            flag: "checkedShip".into(),
        });
    }
    if p.creative() {
        board_ship(
            &mut next_mode,
            &mut p,
            &mut ship_state,
            &game,
            &world,
            &mut commands,
            &sfx,
        );
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
            flag_ev.write(FlagEvent {
                flag: "shipRepaired".into(),
            });
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
    board_ship(
        &mut next_mode,
        &mut p,
        &mut ship_state,
        &game,
        &world,
        &mut commands,
        &sfx,
    );
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
    ship_state.seated_t = 0.0;
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
    let Ok(mut p) = player.single_mut() else {
        return;
    };
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
            &mut next_mode,
            &mut p,
            &mut ship_state,
            &mut game,
            &mut flag_ev,
            &mut big_ev,
            &world,
            &mut commands,
            &sfx,
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
    .key == "launchpad";
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
    flag_ev.write(FlagEvent {
        flag: "launched".into(),
    });
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
    ship.vis_bank = 0.0;
    ship.warmed = false;
    ship.presaved = false;
    ship.pitch_lim = 1.2;
    ship.pitch_floor = None;
    ship.pulsing = false;
    ship.pulse_charge = 0.0;
}

// ---------- 大气层飞行 ----------

fn ship_voxel_collision(ship: &mut ShipState, world: &VoxelWorld, _dt: f32) -> bool {
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
                let pen_x =
                    (p.x + SHIP_BOX[0]).min(bx as f32 + 1.0) - (p.x - SHIP_BOX[0]).max(bx as f32);
                if pen_x <= 0.0 {
                    continue;
                }
                let pen_y = (p.y + SHIP_BOX[1]).min(top) - (p.y - SHIP_BOX[1]).max(by as f32);
                if pen_y <= 0.0 {
                    continue;
                }
                let pen_z =
                    (p.z + SHIP_BOX[2]).min(bz as f32 + 1.0) - (p.z - SHIP_BOX[2]).max(bz as f32);
                if pen_z <= 0.0 {
                    continue;
                }
                let (axis, amt, push) = if pen_x <= pen_y && pen_x <= pen_z {
                    (0, pen_x, if p.x < bx as f32 + 0.5 { -pen_x } else { pen_x })
                } else if pen_y <= pen_z {
                    (
                        1,
                        pen_y,
                        if p.y < by as f32 + (top - by as f32) * 0.5 {
                            -pen_y
                        } else {
                            pen_y
                        },
                    )
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
        let normal = match axis {
            0 => Vec3::new(push.signum(), 0.0, 0.0),
            1 => Vec3::new(0.0, push.signum(), 0.0),
            _ => Vec3::new(0.0, 0.0, push.signum()),
        };
        match axis {
            0 => ship.pos.x += push,
            1 => ship.pos.y += push,
            _ => ship.pos.z += push,
        }
        let fwd = ship_forward(ship.yaw, ship.pitch);
        // A correction without cancelling the component that entered the
        // block produces the classic one-frame forward/backward jitter.  Stop
        // at the contact plane; the pilot can turn away and resume smoothly.
        if fwd.dot(normal) < -0.05 {
            ship.speed = 0.0;
            if hit_below && ship.pitch < 0.0 {
                ship.pitch = 0.0;
            }
        }
        true
    } else {
        false
    }
}

pub fn ship_forward(yaw: f32, pitch: f32) -> Vec3 {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * Vec3::NEG_Z
}

fn bolt_rotation(dir: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::NEG_Z, dir.normalize_or_zero())
}

/// 机体姿态四元数。注意：glam 的 `from_euler(YXZ, a, b, c)` = Ry(a)·Rx(b)·Rz(c)，
/// 第一个参数绕 Y（偏航）、第二个绕 X（俯仰）——与 three.js Euler(pitch,yaw,roll,'YXZ')
/// 的参数含义相反，因此这里必须传 (yaw, pitch, roll) 才能得到 Ry(yaw)·Rx(pitch)·Rz(roll)。
pub fn ship_quat(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll)
}

/// NMS 式姿态积分（与 JS `Space.update` / `updateAtmo` 同口径）：
/// 鼠标俯仰/偏航与 A/D 滚转合成到机体本地轴（`q * dq`，delta 作用于机体局部坐标）。
/// 输入当前 (yaw, pitch, roll) 与增量 (d_yaw, d_pitch, d_roll)，返回新的 (pitch, yaw, roll)。
pub fn integrate_attitude(
    yaw: f32,
    pitch: f32,
    roll: f32,
    d_yaw: f32,
    d_pitch: f32,
    d_roll: f32,
) -> (f32, f32, f32) {
    let q = ship_quat(yaw, pitch, roll);
    let dq = Quat::from_euler(EulerRot::YXZ, d_yaw, d_pitch, d_roll);
    let nq = (q * dq).normalize();
    // to_euler(YXZ) 返回 (yaw, pitch, roll)（与 from_euler 参数顺序一致）
    let (ny, np, nr) = nq.to_euler(EulerRot::YXZ);
    (np, ny, nr)
}

/// A/D 滚转微调缓慢自动回正（JS `updateAtmo` / `Space.update` 的 camRoll 配平）。
/// `steer` 为本帧鼠标移动量（|dx|+|dy|），转向时加速配平以掩盖回正动作。
fn decay_cam_roll(cam_roll: &mut f32, dt: f32, steer: f32, strong: bool) {
    let mut k = 0.08 + (steer * 0.02).min(0.7);
    if strong && cam_roll.abs() > 0.5 {
        k += 1.2;
    }
    *cam_roll -= *cam_roll * (dt * k).min(1.0);
    if cam_roll.abs() < 0.0008 {
        *cam_roll = 0.0;
    }
}

/// E 键触发就地降落。
pub fn atmo_land_trigger_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_mode: ResMut<FlightMode>,
    ship: Res<ShipState>,
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
    let landing_y = parked_ship_y(gy);
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
    // 转向：NMS 式——鼠标俯仰/偏航 + A/D 绕前进轴滚转，均作用于机体本地轴
    // （JS updateAtmo 同口径：滚转存入 camRoll，模型/相机整体携带、缓慢自动回正）
    let d_roll = (if input.roll_left {
        1.7
    } else if input.roll_right {
        -1.7
    } else {
        0.0
    }) * dt;
    let (np, ny, nr) = integrate_attitude(
        ship.yaw,
        ship.pitch,
        ship.cam_roll,
        input.mouse_dx * -sens,
        input.mouse_dy * -sens,
        d_roll,
    );
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
    // 鼠标侧倾（视觉银行）存入真实 roll —— 模型反向携带（JS：Euler z = -atmo.roll）
    let target_roll = input.mouse_dx * -0.04 * settings.mouse_sens;
    ship.roll += (target_roll - ship.roll) * (dt * 5.0).min(1.0);
    // A/D 滚转微调缓慢自动回正（转向时加速配平）
    let steer = input.mouse_dx.abs() + input.mouse_dy.abs();
    decay_cam_roll(&mut ship.cam_roll, dt, steer, true);
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
    // 位移：细分步进防高速穿墙（单步 ≤ 0.5 格）。船体 AABB 半宽 1.6/1.1/1.9，
    // 高速（Boost 110 格/秒）单帧位移可达 1.8+ 格，终点碰撞检测会跳过 1 格厚的墙。
    let fwd = ship_forward(ship.yaw, ship.pitch);
    let dist = ship.speed * dt;
    let steps = (dist / 0.5).ceil().max(1.0) as u32;
    for _ in 0..steps {
        ship.pos += fwd * (dist / steps as f32);
        if ship_voxel_collision(&mut ship, &world, dt) && ship.speed <= 0.0 {
            break;
        }
    }
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
    // 相机：携带 A/D 滚转微调（模型与镜头整体横滚，缓慢自动回正——JS camQ * trim 同口径）
    let cam_q = ship_quat(ship.yaw, ship.pitch, 0.0) * Quat::from_rotation_z(ship.cam_roll);
    let off = cam_q * Vec3::new(0.0, 3.2, 11.0);
    let mut cam_pos = ship.pos + off;
    if ship.reentry_t > 0.0 {
        ship.reentry_t -= dt;
        let shake = ship.reentry_t.min(1.0) * 0.35;
        cam_pos.x +=
            (crate::rng::Rng::new((time.elapsed_secs() * 1000.0) as u32).next() - 0.5) * shake;
        cam_pos.y +=
            (crate::rng::Rng::new((time.elapsed_secs() * 997.0) as u32).next() - 0.5) * shake;
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
    // JS: Space.shipState.speed = Math.max(24, atmo.speed * s)
    ship.speed = (ship.speed * s).max(24.0);
    ship.warmed = false;
    ship.presaved = false;
    // 换系：A/D 滚转（camRoll）延续为太空真实滚转；鼠标侧倾（roll）不携带
    // （JS finishLaunch 把 Euler(pitch,yaw,camRoll) 映射进太空姿态，bank 被 setAttitude 覆盖）
    ship.roll = ship.cam_roll;
    ship.cam_roll = 0.0;
    ship.vis_bank = 0.0;
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
    pub station_defense: ResMut<'w, crate::station::StationDefense>,
}

pub fn space_system(mut p: SpaceSysParams) {
    if *p.next_mode != FlightMode::Space {
        return;
    }
    let dt = p.time.delta_secs();
    let sens = p.settings.mouse_sens * 0.0022;
    // 姿态：NMS 式——鼠标俯仰/偏航 + A/D 绕前进轴滚转，均作用于机体本地轴
    let d_roll = (if p.input.roll_left {
        1.7
    } else if p.input.roll_right {
        -1.7
    } else {
        0.0
    }) * dt;
    let (np, ny, nr) = integrate_attitude(
        p.ship.yaw,
        p.ship.pitch,
        p.ship.roll,
        p.input.mouse_dx * -sens,
        p.input.mouse_dy * -sens,
        d_roll,
    );
    p.ship.pitch = np.clamp(-1.55, 1.55);
    p.ship.yaw = ny;
    p.ship.roll = nr;
    // 视觉侧倾（NMS 式转向银行）与换系滚转微调 —— 模型/相机整体携带（JS Space.update 同口径）
    p.ship.vis_bank +=
        (p.input.mouse_dx * -0.045 * p.settings.mouse_sens - p.ship.vis_bank) * (dt * 5.0).min(1.0);
    let steer = p.input.mouse_dx.abs() + p.input.mouse_dy.abs();
    decay_cam_roll(&mut p.ship.cam_roll, dt, steer, false);
    p.input.mouse_dx = 0.0;
    p.input.mouse_dy = 0.0;
    // 速度
    let max_s = if p.input.boost {
        BOOST_SPEED
    } else {
        MAX_SPEED
    };
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
        let have = p
            .player
            .single()
            .map(|pl| pl.inv.count_item("tritium") > 0)
            .unwrap_or(false);
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
    if tritium_use > 0
        && let Ok(mut pl) = p.player.single_mut()
    {
        pl.inv.remove_item("tritium", tritium_use);
    }
    p.ship.speed +=
        (target - p.ship.speed) * (dt * (if p.ship.pulsing { 1.2 } else { 2.5 })).min(1.0);
    // 移动
    let fwd = ship_forward(p.ship.yaw, p.ship.pitch);
    let spd = p.ship.speed;
    p.ship.pos += fwd * spd * dt;
    if let Some(sc) = p.scene.as_ref() {
        let station_hit = crate::station::resolve_station_collision(
            &mut p.ship.pos,
            sc.station_pos,
            p.station_defense.active(),
        );
        if station_hit {
            // The next frame must not immediately re-enter the same wall or
            // shield boundary, otherwise high-speed flight visibly jitters.
            p.ship.speed = 0.0;
        }
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
                if let Ok(mut pl) = p.player.single_mut()
                    && !pl.creative()
                    && pl.stats.hp + pl.stats.shield > 1.0
                {
                    pl.damage(1.0);
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
    // 相机（真实滚转 + 转向银行 + 换系滚转微调整体携带，与 JS Space.update 一致）
    let ship_q = ship_quat(p.ship.yaw, p.ship.pitch, p.ship.roll)
        * Quat::from_rotation_z(p.ship.cam_roll + p.ship.vis_bank);
    let cam_off = ship_q * Vec3::new(0.0, 3.2, 11.0);
    let target_fov =
        75.0 - 5.0 + (p.ship.speed / PULSE_SPEED) * 40.0 + if p.input.boost { 6.0 } else { 0.0 };
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
    if let Some(sc) = p.scene.as_ref()
        && crate::station::in_dock_zone(&p.ship.pos, sc.station_pos)
    {
        if p.station_defense.active() {
            if p.station_defense.warn_cd <= 0.0 {
                p.station_defense.warn_cd = 2.5;
                p.big_ev.write(BigMessageEvent {
                    title: "⛔ 泊入请求被拒绝".into(),
                    sub: "空间站防护盾激活中——停止攻击 10 秒后恢复准入".into(),
                    dur: 2.2,
                });
                crate::audio::play(&mut p.commands, p.sfx.error.clone(), 0.5, None);
            }
            return;
        }
        let st = crate::station::begin_dock(&mut p.next_mode, &p.ship.pos, sc.station_pos);
        p.commands.insert_resource(st);
        crate::audio::play(&mut p.commands, p.sfx.click.clone(), 0.6, None);
        return;
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
    let Some(lock) = game.warp_lock.clone() else {
        return;
    };
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
    let Ok(mut p) = player.single_mut() else {
        return;
    };
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
    ship.cam_roll = 0.0;
    ship.vis_bank = 0.0;
    let q = ship_quat(ship.yaw, ship.pitch, ship.roll);
    let fwd = ship_forward(ship.yaw, ship.pitch);
    let sp = ship.speed;
    ship.pos += fwd * sp * dt;
    let cam_off = q * Vec3::new(0.0, 3.4, 12.0);
    *flight_cam = FlightCamera::set(ship.pos + cam_off, q, 111.0);
    if anim.t >= total {
        anim.active = false;
        finish_warp(
            &mut next_mode,
            &mut game,
            anim.seed,
            &mut flag_ev,
            &mut big_ev,
            &mut arrive_ev,
            &mut ship,
        );
    }
}

/// 曲速星线：在飞船局部空间循环 180 条发光细线，随加速阶段逐渐拉长。
pub fn warp_visual_system(
    time: Res<Time>,
    mode: Res<FlightMode>,
    ship: Res<ShipState>,
    anim: Res<WarpAnim>,
    mut visuals: ResMut<WarpVisuals>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut streaks: Query<(&mut WarpStreak, &mut Transform)>,
) {
    if *mode != FlightMode::Warping {
        for entity in visuals.entities.drain(..) {
            commands.entity(entity).despawn();
        }
        return;
    }
    let mesh = visuals
        .mesh
        .get_or_insert_with(|| meshes.add(Cuboid::new(0.035, 0.035, 1.0)))
        .clone();
    let material = visuals
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.45, 0.9, 1.0, 0.78),
                emissive: LinearRgba::new(1.5, 4.0, 6.0, 1.0),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })
        })
        .clone();
    if visuals.entities.is_empty() {
        let mut rng = crate::rng::Rng::new(anim.seed ^ 0x57A2_11CE);
        for _ in 0..180 {
            let angle = rng.next() * std::f32::consts::TAU;
            let radius = 1.5 + rng.next().powf(0.45) * 25.0;
            let local = Vec3::new(
                angle.cos() * radius,
                angle.sin() * radius,
                -15.0 - rng.next() * 190.0,
            );
            let speed = 38.0 + rng.next() * 105.0;
            let length = 4.0 + rng.next() * 18.0;
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                    WarpStreak {
                        local,
                        speed,
                        length,
                    },
                    crate::InGame,
                ))
                .id();
            visuals.entities.push(entity);
        }
    }
    let q = ship_quat(ship.yaw, ship.pitch, 0.0);
    let stretch = (anim.t / WARP_LAUNCH).clamp(0.12, 1.0);
    for (mut streak, mut transform) in &mut streaks {
        streak.local.z += streak.speed * time.delta_secs() * (0.55 + stretch * 2.5);
        if streak.local.z > 8.0 {
            streak.local.z = -205.0;
        }
        transform.translation = ship.pos + q * streak.local;
        transform.rotation = q;
        transform.scale = Vec3::new(1.0, 1.0, streak.length * stretch);
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
    // 旧星系：档案含该星系全部星球 + 当前星球标记（JS galaxyArchives[seed] + _marks）
    let mut prev_archive = GalaxyArchive {
        planets: game.visited.clone(),
        marks: HashMap::new(),
    };
    prev_archive
        .marks
        .insert(game.current_planet, std::mem::take(&mut game.marks));
    game.archives.insert(prev_seed, prev_archive);
    let gal = if target_seed == data::HOME_GALAXY_SEED {
        data::home_galaxy()
    } else {
        data::generate_galaxy(target_seed)
    };
    let restored = game.archives.remove(&target_seed).unwrap_or_default();
    game.galaxy = gal;
    game.visited = restored.planets;
    game.marks = restored.marks.get(&0).cloned().unwrap_or_default();
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
    ship.cam_roll = 0.0;
    ship.vis_bank = 0.0;
    ship.pulsing = false;
    ship.pulse_charge = 0.0;
    flag_ev.write(FlagEvent {
        flag: "warpedOut".into(),
    });
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
    let Some(pd) = game.galaxy.planets.iter().find(|p| p.id == pid).cloned() else {
        return;
    };
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
    ship.pos = Vec3::new(ex, alt.max(gy + 40.0), ez);
    ship.pitch = ship.pitch.clamp(-1.55, 1.55);
    ship.pitch_lim = ship.pitch.abs().clamp(1.2, 1.55);
    ship.pitch_floor = Some(-0.4);
    ship.speed = (ship.speed / s).min(110.0);
    // 换系：太空滚转延续为大气滚转微调（JS 再入把 Space 姿态映射进 atmo.camRoll，缓慢回正）
    ship.cam_roll = ship.roll;
    ship.roll = 0.0;
    ship.vis_bank = 0.0;
    ship.warmed = false;
    ship.presaved = false;
    ship.pulsing = false;
    ship.pulse_charge = 0.0;
    ship.reentry_t = 2.6;
    if was_new {
        flag_ev.write(FlagEvent {
            flag: "newPlanet".into(),
        });
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
    mut player: Query<&mut Player>,
) {
    if *next_mode != FlightMode::AtmoLand {
        return;
    }
    let dt = time.delta_secs();
    land.t += dt / 1.6;
    let t = land.t.min(1.0);
    let ease = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
    ship.pos = land.from.lerp(land.to, ease);
    // 让地面玩家实体与降落中的飞船保持同一坐标；否则流式中心和相机在
    // AtmoLand → Seated 的交接帧仍使用登船前的旧位置。
    if let Ok(mut p) = player.single_mut() {
        p.pos = ship.pos;
        p.vel = Vec3::ZERO;
    }
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
        // JS: atmoLandStart 提前 boardYaw = atmo.yaw（下船朝向与降落航向一致）
        ship.board_yaw = ship.yaw;
        ship.seated_t = 0.0;
        *next_mode = FlightMode::Seated;
        big_ev.write(BigMessageEvent {
            title: "降落完成".into(),
            sub: "E 下船 · W 再次起飞".into(),
            dur: 2.2,
        });
        crate::audio::play(&mut commands, sfx.jump.clone(), 0.6, None);
    }
}

/// 降落完成后保持 JS 版的座舱第三人称镜头，直到玩家按 E 下船。
/// 不能让 Planet/Seated 的第一人称相机系统在交接期间抢写相机，否则会出现
/// 一帧落地镜头、一帧玩家镜头来回切换的抖动。
pub fn seated_camera_system(
    time: Res<Time>,
    mode: Res<FlightMode>,
    mut ship: ResMut<ShipState>,
    mut flight_cam: ResMut<FlightCamera>,
) {
    if *mode != FlightMode::Seated {
        return;
    }
    ship.seated_t += time.delta_secs();
    let cam_q = ship_quat(ship.board_yaw, -0.12, 0.0);
    let cam_off = cam_q * Vec3::new(0.0, 2.9 + (ship.seated_t * 1.4).sin() * 0.05, 9.2);
    *flight_cam = FlightCamera::set(ship.pos + cam_off, cam_q, 75.0);
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
            far: CAM_FAR,
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
    asset_server: Res<AssetServer>,
    game: Res<SpaceGame>,
    world: Option<Res<VoxelWorld>>,
    extras: Query<Entity, Or<(With<VisitorShip>, With<SpaceDrop>, With<LaserBolt>)>>,
    mut commands: Commands,
) {
    let want = mode.space_scene();
    match (want, scene) {
        (true, None) => {
            let sc = spawn_space_scene(
                &mut commands,
                &mut meshes,
                &mut images,
                &mut mats,
                &asset_server,
                &game.galaxy,
                world.as_deref(),
                game.current_planet,
            );
            commands.insert_resource(sc);
        }
        (false, Some(sc)) => {
            despawn_space_scene(&mut commands, &sc, extras);
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
    asset_server: Res<AssetServer>,
    game: Res<SpaceGame>,
    world: Option<Res<VoxelWorld>>,
    extras: Query<Entity, Or<(With<VisitorShip>, With<SpaceDrop>, With<LaserBolt>)>>,
    mut commands: Commands,
) {
    for _ in ev.read() {
        if let Some(sc) = scene.as_deref() {
            despawn_space_scene(&mut commands, sc, extras);
            commands.remove_resource::<SpaceScene>();
        }
        let sc = spawn_space_scene(
            &mut commands,
            &mut meshes,
            &mut images,
            &mut mats,
            &asset_server,
            &game.galaxy,
            world.as_deref(),
            game.current_planet,
        );
        commands.insert_resource(sc);
    }
}

// ---------- 船体实体同步 ----------

pub fn ship_sync_system(
    ship: Res<ShipState>,
    ship_asset: Res<ShipAsset>,
    mut q: Query<(
        Entity,
        &mut Transform,
        Option<&mut MeshMaterial3d<StandardMaterial>>,
    )>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mode: Res<FlightMode>,
    time: Res<Time>,
) {
    let Some(e) = ship_asset.entity else { return };
    let Ok((_, mut tf, _)) = q.get_mut(e) else {
        return;
    };
    tf.translation = ship.pos;
    // 模型姿态与相机同源（JS shipGroup.quaternion 口径）：
    // 大气层：Euler(pitch, yaw, roll)（鼠标侧倾银行：右转右倾、左转左倾，与太空模式一致）
    //        + A/D 滚转微调整体携带；
    // 太空：真实滚转 + 转向银行 + 换系滚转微调。
    // 注：JS 原版 atmo 模型对 roll 取反（Euler z = -atmo.roll），导致大气层内转向时
    // 机身朝转弯反方向侧倾（太空模式却不取反，朝转弯方向侧倾）——移植版修正为与太空一致。
    tf.rotation = match *mode {
        FlightMode::Atmo | FlightMode::AtmoLand => {
            ship_quat(ship.yaw, ship.pitch, ship.roll) * Quat::from_rotation_z(ship.cam_roll)
        }
        FlightMode::Space | FlightMode::Warping => {
            ship_quat(ship.yaw, ship.pitch, ship.roll)
                * Quat::from_rotation_z(ship.cam_roll + ship.vis_bank)
        }
        FlightMode::Seated => {
            let base = ship_quat(ship.board_yaw, -0.12, 0.0);
            base * Quat::from_rotation_z((ship.seated_t * 2.2).sin() * 0.006)
        }
        _ => ship_quat(ship.yaw, ship.pitch, ship.roll),
    };
    // 引擎尾焰（JS Space.update 同口径）：长度随速度 + 脉冲引擎 2.0 加成；
    // 逐帧随机闪烁 ±10%（0.9~1.1 倍）并随速度做透明度动画 0.4~0.9 —— 修复静态无动画的尾焰。
    let flame_scale = 0.4 + ship.speed / MAX_SPEED * 0.8 + if ship.pulsing { 2.0 } else { 0.0 };
    let flick = 0.9 + crate::rng::Rng::new((time.elapsed_secs() * 1000.0) as u32).next() * 0.2;
    let opacity = 0.4 + (ship.speed / 100.0).min(0.5);
    for f in &ship_asset.flames {
        if let Ok((_, mut ftf, mat)) = q.get_mut(*f) {
            ftf.scale.z = flame_scale * flick;
            if let Some(m) = mat
                && let Some(mut mm) = mats.get_mut(m.0.id())
            {
                mm.base_color.set_alpha(opacity);
            }
        }
    }
    let _ = mode;
}

/// 地面模式时飞船停泊在 ship_pos。
pub fn ship_parked_system(
    mode: Res<FlightMode>,
    mut game: ResMut<SpaceGame>,
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
        let gy = w.top_at(
            game.ship_pos.x.floor() as i32,
            game.ship_pos.z.floor() as i32,
        );
        let safe_y = parked_ship_y(gy);
        if game.ship_pos.y < safe_y {
            // 修复旧存档/旧版本落点过低的问题，并同步游戏状态；只修正
            // 下陷位置，不强行抬高合法的发射平台或特殊停泊点。
            game.ship_pos.y = safe_y;
            ship_state.pos.y = safe_y;
        }
    }
}

// ---------- 初始飞船 ----------

pub fn spawn_initial_ship(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    world: &VoxelWorld,
    ship_data: &ShipData,
) -> (Entity, Vec<Entity>, Vec3) {
    let spawn = world.find_spawn(96, 96);
    let pos = Vec3::new(
        spawn.x + 4.0,
        parked_ship_y(world.top_at((spawn.x + 4.0) as i32, (spawn.z + 4.0) as i32)),
        spawn.z + 4.0,
    );
    let cls = data::ship_class_by_key(&ship_data.cls);
    let (e, flames) = spawn_external_ship(
        commands,
        meshes,
        mats,
        asset_server,
        pos,
        0.0,
        cls,
        Some(&ship_data.model),
    );
    (e, flames, pos)
}

// ---------- 飞船引擎循环音 ----------

/// 大气/太空飞行时循环播放引擎音（JS Sound.loops.engine 移植）。
pub fn engine_loop_system(
    mode: Res<FlightMode>,
    mut ship: ResMut<ShipState>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    let flying = matches!(
        *mode,
        FlightMode::Atmo | FlightMode::AtmoLand | FlightMode::Space | FlightMode::Warping
    );
    if flying && ship.engine_snd.is_none() {
        let e = crate::audio::play_loop(&mut commands, sfx.engine_loop.clone(), 0.32);
        ship.engine_snd = Some(e);
    } else if !flying && let Some(e) = ship.engine_snd.take() {
        commands.entity(e).despawn();
    }
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
                g.planets
                    .iter()
                    .any(|p| matches!(p.biome, "lush" | "ocean" | "fungal" | "alien")),
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
    fn parked_ship_clears_ground_collision_envelope() {
        let ground_y = 10;
        let y = parked_ship_y(ground_y);
        let ground_top = ground_y as f32 + 1.0;
        assert!(y - SHIP_BOX[1] >= ground_top);
    }

    #[test]
    fn glam_euler_yxz_matches_threejs() {
        // three.js r128 实测：Euler(pitch=0.4, yaw=0.5, roll=0.2, 'YXZ')
        // 生成 q = (0.215738, 0.222044, 0.045896, 0.949762)（= Ry(yaw)*Rx(pitch)*Rz(roll)）
        // glam from_euler(YXZ, yaw, pitch, roll) 必须与之逐分量一致，否则模型滚转方向/合成顺序与 JS 不一致。
        let q = ship_quat(0.5, 0.4, 0.2);
        let expected = Quat::from_xyzw(0.215738, 0.222044, 0.045896, 0.949762);
        let d = (q - expected).length();
        assert!(
            d < 1e-4,
            "glam YXZ mismatch: got {:?} expected {:?} (d={})",
            q,
            expected,
            d
        );
        // 反向：roll 取负（JS 模型 Euler z = -roll）后与 three.js 对照
        let q2 = ship_quat(0.5, 0.4, -0.2);
        let expected2 = Quat::from_xyzw(0.167325, 0.260478, -0.143708, 0.939948);
        let d2 = (q2 - expected2).length();
        assert!(
            d2 < 1e-4,
            "glam YXZ(-roll) mismatch: got {:?} expected {:?} (d={})",
            q2,
            expected2,
            d2
        );
    }

    #[test]
    fn galaxy_name_home() {
        assert_eq!(data::galaxy_name(data::HOME_GALAXY_SEED), "起源星系");
    }

    #[test]
    fn projectile_nose_follows_travel_direction() {
        for dir in [Vec3::NEG_Z, Vec3::X, Vec3::Y, Vec3::new(1.0, 2.0, -3.0)] {
            let dir = dir.normalize();
            let nose = bolt_rotation(dir) * Vec3::NEG_Z;
            assert!(nose.distance(dir) < 1e-5, "{nose:?} vs {dir:?}");
        }
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

    #[test]
    fn attitude_roll_is_pure_roll() {
        // A/D 滚转：水平姿态下是纯 roll，不污染俯仰/偏航
        let (np, ny, nr) = integrate_attitude(0.0, 0.0, 0.0, 0.0, 0.0, 0.1);
        assert!(
            np.abs() < 1e-5 && ny.abs() < 1e-5 && (nr - 0.1).abs() < 1e-4,
            "{np} {ny} {nr}"
        );
        // 已有横滚/俯仰时继续滚转仍是纯 roll
        let (np, ny, nr) = integrate_attitude(0.5, 0.3, 0.7, 0.0, 0.0, 0.1);
        assert!(
            (np - 0.3).abs() < 1e-4 && (ny - 0.5).abs() < 1e-4 && (nr - 0.8).abs() < 1e-3,
            "{np} {ny} {nr}"
        );
    }

    #[test]
    fn attitude_mouse_acts_on_local_axes() {
        // 鼠标俯仰作用于机体本地轴：水平姿态下保持正交
        let (np, ny, nr) = integrate_attitude(0.0, 0.0, 0.0, 0.05, 0.1, 0.0);
        assert!(
            (np - 0.1).abs() < 1e-4 && (ny - 0.05).abs() < 1e-4 && nr.abs() < 1e-4,
            "{np} {ny} {nr}"
        );
        // 横滚 90° 后（左滚），本地俯仰增量在欧拉分解中表现为偏航（机头向左）——而非污染 roll
        let (np, ny, nr) =
            integrate_attitude(0.0, 0.0, std::f32::consts::FRAC_PI_2, 0.0, 0.05, 0.0);
        assert!((ny - 0.05).abs() < 2e-3, "yaw {ny}");
        assert!(np.abs() < 2e-3, "pitch {np}");
        assert!((nr - std::f32::consts::FRAC_PI_2).abs() < 2e-3, "roll {nr}");
    }

    #[test]
    fn quat_convention_probe() {
        // 回归：glam `from_euler(YXZ, a, b, c)` = Ry(a)·Rx(b)·Rz(c) ——
        // ship_quat 必须传 (yaw, pitch, roll) 才能与 three.js Euler(pitch,yaw,roll,'YXZ') 一致。
        // （此前传 (pitch, yaw, roll) 导致鼠标俯仰/偏航轴互换、飞行操控错乱）
        let q = Quat::from_euler(EulerRot::YXZ, 0.05, 0.0, 0.0);
        let (axis, ang) = q.to_axis_angle();
        assert!(
            axis.distance(Vec3::Y) < 1e-4 && (ang - 0.05).abs() < 1e-4,
            "{axis:?} {ang}"
        );
        let q2 = Quat::from_euler(EulerRot::YXZ, 0.0, 0.05, 0.0);
        let (axis2, ang2) = q2.to_axis_angle();
        assert!(
            axis2.distance(Vec3::X) < 1e-4 && (ang2 - 0.05).abs() < 1e-4,
            "{axis2:?} {ang2}"
        );
        let q3 = Quat::from_euler(EulerRot::YXZ, 0.0, 0.0, 0.05);
        let (axis3, ang3) = q3.to_axis_angle();
        assert!(
            axis3.distance(Vec3::Z) < 1e-4 && (ang3 - 0.05).abs() < 1e-4,
            "{axis3:?} {ang3}"
        );
        // ship_quat：机头方向与地面 look_dir 公式一致（yaw 绕世界 Y、pitch 绕本地 X）
        let fwd = ship_quat(0.3, 0.2, 0.0) * Vec3::NEG_Z;
        let expect = Vec3::new(
            -0.3f32.sin() * 0.2f32.cos(),
            0.2f32.sin(),
            -0.3f32.cos() * 0.2f32.cos(),
        );
        assert!(fwd.distance(expect) < 1e-4, "{fwd:?} vs {expect:?}");
    }

    #[test]
    fn attitude_mouse_delta_signs() {
        // 鼠标右移 → 偏航减小（与地面 look_system 方向一致）；鼠标上移 → 俯仰增大
        let (_, ny, _) = integrate_attitude(0.0, 0.0, 0.0, -0.01, 0.0, 0.0);
        assert!(ny < 0.0, "right mouse should decrease yaw, got {ny}");
        let (np, _, _) = integrate_attitude(0.0, 0.0, 0.0, 0.0, 0.01, 0.0);
        assert!(np > 0.0, "mouse up should increase pitch, got {np}");
        // A（rollLeft）正 → 向左滚转；D（rollRight）负 → 向右滚转
        let (_, _, nr_a) = integrate_attitude(0.0, 0.0, 0.0, 0.0, 0.0, 0.01);
        let (_, _, nr_d) = integrate_attitude(0.0, 0.0, 0.0, 0.0, 0.0, -0.01);
        assert!(
            nr_a > 0.0 && nr_d < 0.0,
            "A positive roll, D negative roll: {nr_a} {nr_d}"
        );
    }
}
