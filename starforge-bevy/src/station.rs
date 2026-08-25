//! 空间站 — 顶部悬停泊入 / 站内服务（贸易·购船·换船）/ 离站。

use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy_world_serialization::prelude::WorldAssetRoot;

use crate::data;
use crate::quests::{BigMessageEvent, FlagEvent};
use crate::space::{FlightCamera, FlightMode, SHIP_R, ShipAsset, ShipState, SpaceGame};
use crate::ui::{Panel, UiState};

// ---------- 站体停泊（模型顶部悬停泊入） ----------

const DOCK_PAD_YAW: f32 = 0.0;
/// 泊入触发半径（相对站顶停泊点）。
const STATION_DOCK_R: f32 = 48.0;
/// 悬停泊位高出站顶的高度。
const STATION_HOVER_MARGIN: f32 = 18.0;
/// 站顶离站/读档出口的偏移（相对站顶停泊点）。
const DOCK_EXIT: [f32; 3] = [0.0, 25.0, 0.0];

/// 站内存档位置：站顶停泊点上方（读档不再重泊入）。
pub fn station_exit_pos(station_pos: Vec3, seed: u32) -> Vec3 {
    let m = station_model_for_seed(seed);
    m.hover_point(station_pos) + Vec3::from(DOCK_EXIT)
}

/// 访客船停靠休息点：环绕站顶悬停区。
pub fn visitor_pad_world(station_pos: Vec3, index: usize, y: f32, seed: u32) -> Vec3 {
    let m = station_model_for_seed(seed);
    let hover = m.hover_point(station_pos);
    let a = index as f32 * std::f32::consts::TAU / 3.0 + 0.7;
    hover + Vec3::new(a.cos() * 46.0, y, a.sin() * 46.0)
}

#[derive(Resource, Default)]
pub struct StationDefense {
    pub remaining: f32,
    pub warn_cd: f32,
}

impl StationDefense {
    pub fn active(&self) -> bool {
        self.remaining > 0.0
    }

    /// Raise or refresh the shield. Returns true only for the initial alarm.
    pub fn raise(&mut self) -> bool {
        let first = !self.active();
        self.remaining = 10.0;
        first
    }
}

#[derive(Component)]
pub struct StationShield;

#[derive(Component)]
pub struct StationGateLight;

/// Visual-only modular station parts. Collision remains in `station_cols()`.
#[derive(Component)]
pub struct StationModule;

// ---------- 站体模型与碰撞（按模型实际包围盒） ----------

/// 空间站模型在场景空间（未施加生成旋转/缩放前）的包围盒。
/// 数值来自对 glTF 节点链的实测（POSITION 访问器场景空间 min/max）。
/// `bounds` 为总体包围盒（用于泊入判定/灯光）；`boxes` 为逐网格碰撞盒，
/// 让镂空/悬空区域可以自由穿越（不碰撞）。
pub struct StationModel {
    pub path: &'static str,
    pub scale: f32,
    pub rotation: Quat,
    pub bounds: ([f32; 3], [f32; 3]),
    pub boxes: &'static [([f32; 3], [f32; 3])],
}

/// 按星系种子取空间站模型参数（与 spawn_station_model 同一映射，单一事实来源）。
pub fn station_model_for_seed(seed: u32) -> StationModel {
    if seed == data::HOME_GALAXY_SEED {
        StationModel {
            path: "models/external/stations/space_station/scene.gltf",
            scale: 11.0,
            rotation: Quat::IDENTITY,
            bounds: ([-7.44, -14.64, -10.37], [7.08, 7.80, 7.14]),
            // 环形站：外环 + 内环 + 中央塔 + 上下壳体，中心镂空区不碰撞。
            boxes: &[
                ([-7.10, -1.75, -7.20], [7.10, 1.75, 7.20]), // 外环（上下）
                ([-4.65, -2.25, -4.75], [4.65, 2.25, 4.75]), // 内环
                ([-5.80, -10.60, -6.20], [5.80, 1.05, 5.25]), // 中部壳体
                ([-4.45, -7.85, -4.10], [4.45, 7.85, 4.10]), // 核心塔
                ([-6.85, -14.50, -8.60], [7.10, 1.40, 6.70]), // 下部基座
                ([-1.00, -6.90, -1.20], [1.00, 6.90, 1.20]), // 顶部塔柱
            ],
        }
    } else {
        match seed % 3 {
            0 => StationModel {
                path: "models/external/stations/space_station_3/scene.gltf",
                scale: 30.0,
                rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                bounds: ([-10.82, -11.67, -9.84], [9.69, 11.16, 9.40]),
                // 主壳体按 72% 收缩，外围细杆/天线不参与碰撞。
                boxes: &[([-7.80, -8.30, -7.00], [6.70, 7.80, 6.50])],
            },
            1 => StationModel {
                path: "models/external/stations/space_station_4/scene.gltf",
                scale: 5.0,
                rotation: Quat::IDENTITY,
                bounds: ([-24.83, -18.70, -21.10], [26.89, 15.51, 20.06]),
                // 主船体收缩到约 70%，周边小部件（推进器/天线）不参与碰撞，
                // 站体轮廓内的镂空/悬空区域可自由飞越。
                boxes: &[
                    ([-17.00, -13.60, -14.90], [19.10, 10.40, 13.90]),
                    ([-5.70, -4.80, -4.80], [5.70, 4.80, 4.80]), // 侧翼组件
                ],
            },
            _ => StationModel {
                path: "models/external/stations/helveta/scene.gltf",
                scale: 0.28,
                rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                bounds: ([-3.48, -0.26, -1.73], [2.67, 0.60, 1.34]),
                boxes: &[([-3.48, -0.26, -1.73], [2.67, 0.60, 1.34])],
            },
        }
    }
}

impl StationModel {
    /// 站体世界空间包围盒中心与半尺寸（随模型旋转/缩放）。
    fn world_box(&self, station_pos: Vec3) -> (Vec3, Vec3) {
        let mn = Vec3::from(self.bounds.0);
        let mx = Vec3::from(self.bounds.1);
        let center = station_pos + self.rotation * ((mn + mx) * 0.5 * self.scale);
        let half = (mx - mn) * 0.5 * self.scale;
        (center, half)
    }

    /// 顶部停泊点：站顶上方悬停位置。
    pub fn hover_point(&self, station_pos: Vec3) -> Vec3 {
        let (center, half) = self.world_box(station_pos);
        center + self.rotation * Vec3::Y * (half.y + STATION_HOVER_MARGIN)
    }
}

/// 空间站实体碰撞（飞船按半径 SHIP_R 的球处理，站体为模型 OBB）。
pub fn resolve_station_collision(
    pos: &mut Vec3,
    station_pos: Vec3,
    shield_up: bool,
    seed: u32,
) -> bool {
    let mut corrected = false;
    // 主动防护盾：站体上方气泡（沿用旧逻辑）。
    if shield_up {
        let m = station_model_for_seed(seed);
        let (mn, mx) = m.bounds;
        let local = m.rotation.inverse() * (*pos - station_pos) / m.scale;
        let inside_bay = local.x >= mn[0]
            && local.x <= mx[0]
            && local.y >= mn[1]
            && local.y <= mx[1]
            && local.z >= mn[2]
            && local.z <= mx[2];
        if !inside_bay {
            let center = station_pos + Vec3::new(0.0, 20.0, -20.0);
            let delta = *pos - center;
            let distance = delta.length();
            if distance < 213.0 && distance > 1e-4 {
                *pos = center + delta * (213.0 / distance);
                return true;
            }
        }
    }
    // 模型逐网格碰撞盒：飞船位置变换到站体局部空间，逐盒做 球 vs AABB 推挤。
    let m = station_model_for_seed(seed);
    let inv_rot = m.rotation.inverse();
    let r = SHIP_R / m.scale;
    for &(mn, mx) in m.boxes {
        let local = inv_rot * (*pos - station_pos) / m.scale;
        let qx = local.x.clamp(mn[0], mx[0]);
        let qy = local.y.clamp(mn[1], mx[1]);
        let qz = local.z.clamp(mn[2], mx[2]);
        let d = Vec3::new(local.x - qx, local.y - qy, local.z - qz);
        let d2 = d.length_squared();
        if d2 >= r * r {
            continue;
        }
        let push = if d2 > 1e-9 {
            d.normalize() * (r - d2.sqrt())
        } else {
            let pens = [
                local.x - mn[0],
                mx[0] - local.x,
                local.y - mn[1],
                mx[1] - local.y,
                local.z - mn[2],
                mx[2] - local.z,
            ];
            let mut mi = 0;
            for i in 1..6 {
                if pens[i] < pens[mi] {
                    mi = i;
                }
            }
            let mut v = Vec3::ZERO;
            match mi {
                0 => v.x = -(pens[0] + r),
                1 => v.x = pens[1] + r,
                2 => v.y = -(pens[2] + r),
                3 => v.y = pens[3] + r,
                4 => v.z = -(pens[4] + r),
                _ => v.z = pens[5] + r,
            }
            v
        };
        let pushed = m.rotation * ((local + push) * m.scale);
        *pos = station_pos + pushed;
        corrected = true;
    }
    corrected
}

/// 靠近空间站顶部即触发泊入（无需进入机库）。
pub fn station_dock_zone(ship_pos: &Vec3, station_pos: Vec3, seed: u32) -> bool {
    let m = station_model_for_seed(seed);
    ship_pos.distance(m.hover_point(station_pos)) < STATION_DOCK_R
}

// ---------- 站内状态 ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StationPhase {
    #[default]
    Idle,
    Dock,
    Parked,
    Leave,
}

#[derive(Clone, Debug)]
pub struct BuyOffer {
    pub cls: String,
    pub model: String,
    pub price: i32,
    pub pilot_name: String,
}

#[derive(Clone, Debug)]
pub struct PilotNpc {
    pub pos: Vec3,
    pub name: String,
    pub cls: String,
    pub model: String,
    pub price: i32,
    pub entity: Option<Entity>,
    pub ship_entity: Option<Entity>,
    /// 停靠休息剩余秒数；归零即离站。
    pub rest: f32,
}

#[derive(Resource)]
pub struct StationState {
    pub phase: StationPhase,
    pub t: f32,
    pub curve: Vec<Vec3>,
    pub dur: f32,
    pub pad: Vec3,
    pub pad_yaw: f32,
    pub offers: Vec<BuyOffer>,
    pub pilots: Vec<PilotNpc>,
    pub station_pos: Vec3,
    pub seed: u32,
    /// 游商船离站后的补位倒计时。
    pub pilot_respawn: f32,
}

impl Default for StationState {
    fn default() -> Self {
        Self {
            phase: StationPhase::Idle,
            t: 0.0,
            curve: Vec::new(),
            dur: 1.0,
            pad: Vec3::ZERO,
            pad_yaw: DOCK_PAD_YAW,
            offers: Vec::new(),
            pilots: Vec::new(),
            station_pos: Vec3::ZERO,
            seed: 0,
            pilot_respawn: 0.0,
        }
    }
}

/// 构建泊入状态：飞船靠近站顶即泊入，悬停在站顶上方（不强制进机库/落地）。
pub fn begin_dock(
    mode: &mut FlightMode,
    ship_pos: &Vec3,
    yaw: f32,
    station_pos: Vec3,
    seed: u32,
) -> StationState {
    let m = station_model_for_seed(seed);
    let hover = m.hover_point(station_pos);
    let mut st = StationState {
        phase: StationPhase::Dock,
        t: 0.0,
        station_pos,
        seed,
        pad_yaw: yaw,
        ..default()
    };
    st.pad = hover;
    st.curve = vec![*ship_pos, hover];
    st.dur = 1.6;
    st.offers = roll_offers(station_pos);
    st.pilots = build_pilots(&st.offers, station_pos, seed);
    *mode = FlightMode::Station;
    st
}

fn roll_offers(station_pos: Vec3) -> Vec<BuyOffer> {
    let mut rnd = crate::rng::Rng::new(station_pos.x.to_bits() ^ 0xC0FFEE);
    let mut out = Vec::new();
    let names = [
        "游商·卡洛",
        "飞手·薇拉",
        "老练的走私客",
        "星途旅人·顿",
        "佣兵·赤羽",
        "货运队长·穆",
    ];
    for _ in 0..3 {
        let cls = data::roll_ship_class(rnd.next());
        let model = data::SHIP_MODEL_NAMES[rnd.range(data::SHIP_MODEL_NAMES.len())].0;
        let price =
            ((cls.price as f32 * (0.88 + rnd.next() * 0.28) / 100.0).round() * 100.0) as i32;
        out.push(BuyOffer {
            cls: cls.key.to_string(),
            model: model.to_string(),
            price,
            pilot_name: names[rnd.range(names.len())].to_string(),
        });
    }
    out
}

fn build_pilots(offers: &[BuyOffer], station_pos: Vec3, seed: u32) -> Vec<PilotNpc> {
    let m = station_model_for_seed(seed);
    let hover = m.hover_point(station_pos);
    let mut out = Vec::new();
    for (i, o) in offers.iter().enumerate() {
        // 停靠休息点：环绕站顶悬停区
        let a = i as f32 * std::f32::consts::TAU / 3.0 + 0.7;
        let rest = hover + Vec3::new(a.cos() * 46.0, 6.0 + i as f32 * 5.0, a.sin() * 46.0);
        out.push(PilotNpc {
            pos: rest,
            name: o.pilot_name.clone(),
            cls: o.cls.clone(),
            model: o.model.clone(),
            price: o.price,
            entity: None,
            ship_entity: None,
            rest: 45.0 + i as f32 * 15.0,
        });
    }
    out
}

/// 外部空间站模型。模型按星系种子轮换：家园使用基础站，后续星系
/// 使用其余 CC-BY 模型；碰撞/泊入按模型实际包围盒（station_model_for_seed）。
pub fn spawn_station_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    pos: Vec3,
    galaxy_seed: u32,
) -> Entity {
    let m = station_model_for_seed(galaxy_seed);
    let (path, scale, rotation) = (m.path, m.scale, m.rotation);
    let root = commands
        .spawn((
            Transform::from_translation(pos),
            Visibility::default(),
            crate::InGame,
        ))
        .id();
    let model = commands
        .spawn((
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(path))),
            Transform::from_rotation(rotation).with_scale(Vec3::splat(scale)),
            crate::InGame,
        ))
        .id();
    crate::space::attach_external_animation(commands, model, path);
    commands.entity(root).add_child(model);

    // 交互系统使用独立的防护盾实体，不依赖外部模型的材质节点。
    let shield = commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(213.0))),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: Color::srgba(0.25, 0.65, 1.0, 0.0),
                emissive: LinearRgba::new(0.1, 0.45, 1.0, 1.0),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                ..default()
            })),
            Transform::from_xyz(0.0, 20.0, -20.0),
            Visibility::Hidden,
            StationShield,
            crate::InGame,
        ))
        .id();
    commands.entity(root).add_child(shield);

    // 站体氛围灯：外部模型自发光偏弱，补几盏彩色灯让站体更生动。
    let (center, half) = m.world_box(pos);
    let radius = half.x.max(half.z);
    for (dx, dz, color) in [
        (0.7f32, 0.0f32, Color::srgb(0.15, 0.75, 0.9)),
        (-0.7, 0.0, Color::srgb(0.95, 0.42, 0.16)),
        (0.0, 0.7, Color::srgb(0.35, 0.9, 0.55)),
        (0.0, -0.7, Color::srgb(0.9, 0.7, 0.3)),
    ] {
        let lp = center + m.rotation * Vec3::new(half.x * dx, 0.0, half.z * dz);
        let light = commands
            .spawn((
                PointLight {
                    color,
                    intensity: 6000.0,
                    range: radius * 1.6,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(lp),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(light);
    }
    // 停泊区顶部照明
    let top_light = commands
        .spawn((
            PointLight {
                color: Color::srgb(0.6, 0.85, 1.0),
                intensity: 8000.0,
                range: radius * 1.2,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(m.hover_point(pos) + Vec3::Y * 4.0),
            crate::InGame,
        ))
        .id();
    commands.entity(root).add_child(top_light);
    root
}

/// Shield countdown and visual state. Gate lights switch to a flashing red
/// warning while docking clearance is suspended.
pub fn station_defense_system(
    time: Res<Time>,
    mut defense: ResMut<StationDefense>,
    mut shield: Query<(&MeshMaterial3d<StandardMaterial>, &mut Visibility), With<StationShield>>,
    gates: Query<&MeshMaterial3d<StandardMaterial>, With<StationGateLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut big_ev: MessageWriter<BigMessageEvent>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
    station: Res<StationState>,
) {
    let was_active = defense.active();
    if defense.remaining > 0.0 {
        defense.remaining = (defense.remaining - time.delta_secs()).max(0.0);
    }
    defense.warn_cd = (defense.warn_cd - time.delta_secs()).max(0.0);
    let active = defense.active();
    let pulse = 0.12 + (time.elapsed_secs() * 6.0).sin() * 0.035;
    for (material, mut visibility) in &mut shield {
        *visibility = if active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if let Some(mut mat) = materials.get_mut(material.0.id()) {
            mat.base_color.set_alpha(if active { pulse } else { 0.0 });
        }
    }
    for material in &gates {
        if let Some(mut mat) = materials.get_mut(material.0.id()) {
            if active {
                let flash = 0.55 + 0.45 * (time.elapsed_secs() * 9.0).sin();
                mat.base_color = Color::srgb(1.0, 0.18 * flash, 0.12 * flash);
                mat.emissive = LinearRgba::new(1.0, 0.04, 0.02, 1.0) * 2.5;
            } else {
                mat.base_color = Color::srgb(0.21, 0.88, 0.91);
                mat.emissive = LinearRgba::new(0.1, 0.6, 0.7, 1.0) * 2.0;
            }
        }
    }
    if was_active && !active {
        big_ev.write(BigMessageEvent {
            title: "空间站防护盾解除".into(),
            sub: "准入已恢复".into(),
            dur: 2.2,
        });
        crate::audio::play_spatial(
            &mut commands,
            sfx.scan.clone(),
            station.station_pos + Vec3::new(0.0, 20.0, -20.0),
            0.45,
            None,
        );
    }
}

// ---------- Catmull-Rom ----------

fn catmull_rom(points: &[Vec3], t: f32) -> Vec3 {
    let n = points.len();
    if n == 1 {
        return points[0];
    }
    if n == 2 {
        return points[0].lerp(points[1], t);
    }
    let seg = (t * (n - 1) as f32).floor().min(n as f32 - 2.0) as usize;
    let lt = t * (n - 1) as f32 - seg as f32;
    let p0 = points[seg.saturating_sub(1)];
    let p1 = points[seg];
    let p2 = points[(seg + 1).min(n - 1)];
    let p3 = points[(seg + 2).min(n - 1)];
    let lt2 = lt * lt;
    let lt3 = lt2 * lt;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * lt
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * lt2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * lt3)
}

// ---------- 换船 ----------

#[derive(Message)]
pub struct ShipSwitchEvent {
    pub cls: String,
    pub model: String,
    /// Some(i)：与车库第 i 艘交换；None：购买新船（旧船入库）
    pub garage_idx: Option<usize>,
}

pub fn ship_switch_system(
    mut ev: MessageReader<ShipSwitchEvent>,
    mut ship_asset: ResMut<ShipAsset>,
    mut ship_state: ResMut<ShipState>,
    mut game: ResMut<SpaceGame>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    for e in ev.read() {
        let old = ship_asset.data.clone();
        let new_data = match e.garage_idx {
            Some(i) => {
                let Some(stored) = game.garage.get(i).cloned() else {
                    continue;
                };
                if let Some(slot) = game.garage.get_mut(i) {
                    *slot = old.clone();
                }
                stored
            }
            None => {
                let name = data::SHIP_MODEL_NAMES
                    .iter()
                    .find(|(k, _)| *k == e.model)
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| "新飞船".into());
                let fresh = crate::save::ShipSave {
                    model: e.model.clone(),
                    cls: e.cls.clone(),
                    name,
                    inv: vec![None; data::ship_class_by_key(&e.cls).slots],
                };
                // Cargo belongs to its ship. Buying a new hull stores the old
                // ship (with its cargo) in the garage and starts with an empty
                // hold instead of duplicating the old cargo into both ships.
                game.garage.push(old.clone());
                fresh
            }
        };
        let cls = data::ship_class_by_key(&e.cls);
        let pos = ship_state.pos;
        let yaw = ship_state.yaw;
        if let Some(old_e) = ship_asset.entity.take() {
            commands.entity(old_e).despawn();
        }
        for f in ship_asset.flames.drain(..) {
            commands.entity(f).despawn();
        }
        let (ent, flames) = crate::space::spawn_external_ship(
            &mut commands,
            &mut meshes,
            &mut mats,
            &asset_server,
            pos,
            yaw,
            cls,
            Some(&new_data.model),
        );
        ship_asset.entity = Some(ent);
        ship_asset.flames = flames;
        ship_asset.data = new_data;
        // 船体生命随等级（JS VIS_HP）
        ship_state.hp = crate::space::vis_hp(&e.cls);
        ship_state.hp_max = crate::space::vis_hp(&e.cls);
        ship_state.fire_cd = 0.0;
    }
}

// ---------- 主系统 ----------

#[allow(clippy::too_many_arguments)]
pub fn station_system(
    time: Res<Time>,
    mut next_mode: ResMut<FlightMode>,
    mut ship: ResMut<ShipState>,
    mut st: ResMut<StationState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut flight_cam: ResMut<FlightCamera>,
    mut flag_ev: MessageWriter<FlagEvent>,
    mut big_ev: MessageWriter<BigMessageEvent>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    if *next_mode != FlightMode::Station {
        ui_state.prompt = None;
        // 离站清理：游商船全清
        for pil in st.pilots.iter_mut() {
            if let Some(e) = pil.ship_entity.take() {
                commands.entity(e).despawn();
            }
        }
        if st.phase != StationPhase::Idle {
            st.phase = StationPhase::Idle;
        }
        return;
    }
    let dt = time.delta_secs();
    // 游商船停靠休息：休息结束离站，稍后补位再来
    st.pilot_respawn -= dt;
    let mut need_respawn = false;
    for pil in st.pilots.iter_mut() {
        if let Some(e) = pil.ship_entity {
            pil.rest -= dt;
            if pil.rest <= 0.0 {
                commands.entity(e).despawn();
                pil.ship_entity = None;
                need_respawn = true;
            }
        }
    }
    if need_respawn {
        st.pilot_respawn = 55.0;
    }
    match st.phase {
        StationPhase::Dock => {
            ui_state.prompt = None;
            st.t += dt / st.dur;
            let t = st.t.min(1.0);
            let ease = t * t * (3.0 - 2.0 * t);
            ship.pos = catmull_rom(&st.curve, ease);
            ship.yaw = st.pad_yaw;
            ship.pitch = 0.0;
            ship.roll = 0.0;
            let q = crate::space::ship_quat(ship.yaw, ship.pitch, ship.roll);
            let cam_off = q * Vec3::new(0.0, 3.2, 11.0);
            *flight_cam = FlightCamera::set(ship.pos + cam_off, q, 75.0);
            if t >= 1.0 {
                ship.pos = st.pad;
                st.phase = StationPhase::Parked;
                flag_ev.write(FlagEvent {
                    flag: "docked".into(),
                });
                big_ev.write(BigMessageEvent {
                    title: "已停泊".into(),
                    sub: "E 空间站服务 · W 离站".into(),
                    dur: 2.4,
                });
            }
        }
        StationPhase::Parked => {
            ship.pos = st.pad;
            ship.yaw = st.pad_yaw;
            ui_state.prompt = Some("[E] 空间站服务 · W 离站".into());
            if ui_state.locked() {
                ui_state.prompt = None;
                return;
            }
            if keys.just_pressed(KeyCode::KeyE) {
                ui_state.panel = Panel::Station;
                crate::audio::play(&mut commands, sfx.click.clone(), 0.5, None);
            }
            if keys.just_pressed(KeyCode::KeyW) {
                // 离站升到泊入判定区（半径 48）之外，避免立即再次触发泊入
                st.curve = vec![st.pad, st.pad + Vec3::Y * 70.0];
                st.dur = 1.8;
                st.t = 0.0;
                st.phase = StationPhase::Leave;
            }
        }
        StationPhase::Leave => {
            ui_state.prompt = None;
            st.t += dt / st.dur;
            let t = st.t.min(1.0);
            let ease = t * t * (3.0 - 2.0 * t);
            ship.pos = catmull_rom(&st.curve, ease);
            ship.yaw = st.pad_yaw;
            let q = crate::space::ship_quat(ship.yaw, ship.pitch, 0.0);
            let cam_off = q * Vec3::new(0.0, 3.2, 11.0);
            *flight_cam = FlightCamera::set(ship.pos + cam_off, q, 75.0);
            if t >= 1.0 {
                st.phase = StationPhase::Idle;
                ship.speed = 30.0;
                *next_mode = FlightMode::Space;
                crate::audio::play(&mut commands, sfx.laser_hit.clone(), 0.5, None);
            }
        }
        StationPhase::Idle => {}
    }
}

/// 站内游商船：停靠站顶周围休息（不再卖船，也不再生成人形 NPC）。
pub fn station_npc_spawn_system(
    mode: Res<FlightMode>,
    mut st: ResMut<StationState>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if *mode != FlightMode::Station || st.phase != StationPhase::Parked {
        return;
    }
    if st.pilot_respawn > 0.0 {
        return;
    }
    if !st.pilots.iter().any(|p| p.ship_entity.is_none()) {
        return;
    }
    let pilots = st.pilots.clone();
    for (i, pil) in pilots.iter().enumerate() {
        let orig = &mut st.pilots[i];
        if orig.ship_entity.is_some() {
            continue;
        }
        let cls = data::ship_class_by_key(&pil.cls);
        let (ship_e, flames) = crate::space::spawn_external_ship(
            &mut commands,
            &mut meshes,
            &mut mats,
            &asset_server,
            pil.pos,
            std::f32::consts::PI,
            cls,
            Some(&pil.model),
        );
        orig.ship_entity = Some(ship_e);
        orig.rest = 45.0 + i as f32 * 15.0;
        for f in flames {
            commands.entity(f).despawn();
        }
    }
}

// ---------- Plugin ----------

/// Station plugin: docking state machine, defense, visitors and ship switching.
pub struct StationPlugin;

impl Plugin for StationPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ShipSwitchEvent>()
            .init_resource::<StationState>()
            .init_resource::<StationDefense>()
            .add_systems(
                Update,
                (
                    station_system,
                    station_defense_system,
                    station_npc_spawn_system,
                    ship_switch_system,
                )
                    .chain()
                    .in_set(crate::schedule::GameSet::LateStation)
                    .run_if(in_state(crate::schedule::GameState::Playing)),
            );
    }
}
