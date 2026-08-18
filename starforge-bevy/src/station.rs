//! 空间站 — 泊入 / 站内行走 / 贸易 / 购船 / 离站。
//! Port of js/station.js + js/space.js 站体与碰撞部分。

use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy_world_serialization::prelude::WorldAssetRoot;

use crate::data;
use crate::player::Player;
use crate::quests::{BigMessageEvent, FlagEvent};
use crate::save::Appearance;
use crate::space::{FlightCamera, FlightMode, SHIP_R, ShipAsset, ShipState, SpaceGame};
use crate::ui::{Panel, UiState};

// ---------- 站体布局（DOCK_L 局部坐标） ----------

const DOCK_SLOT: [f32; 3] = [0.0, 10.0, 79.0];
const DOCK_INNER_WAIT: [f32; 3] = [0.0, 12.0, 44.0];
const DOCK_PAD: [f32; 3] = [20.0, 3.2, 30.0];
// The ship's nose is local -Z.  The landing pad faces the hangar interior
// (towards decreasing Z), so yaw 0 makes the ship enter nose-first and stop
// facing into the bay instead of backing in.
const DOCK_PAD_YAW: f32 = 0.0;
const DOCK_EXIT: [f32; 3] = [0.0, 12.0, 150.0];

/// 站内存档位置：机库出口（JS：站态存 dock exit，读档不再重泊入）。
pub fn station_exit_pos() -> [f32; 3] {
    DOCK_EXIT
}
const DOCK_TERMINAL: [f32; 3] = [0.0, 4.0, -3.0];
const DOCK_GARAGE: [f32; 3] = [30.0, 4.0, 24.0];
const BOUNDS_X: f32 = 30.0;
const BOUNDS_Z_MIN: f32 = -12.0;
const BOUNDS_Z_MAX: f32 = 74.0;
const PADS: [[f32; 3]; 4] = [
    [-20.0, 2.0, 30.0],
    [20.0, 2.0, 30.0],
    [-20.0, 2.0, 52.0],
    [20.0, 2.0, 52.0],
];
const VIS_PADS: [[f32; 3]; 3] = [[-20.0, 2.0, 30.0], [-20.0, 2.0, 52.0], [20.0, 2.0, 52.0]];

/// World-space position of one of the three visitor pads. The fourth pad is
/// permanently reserved for the player.
pub fn visitor_pad_world(station_pos: Vec3, index: usize, y: f32) -> Vec3 {
    let pad = VIS_PADS[index.min(VIS_PADS.len() - 1)];
    station_pos + Vec3::new(pad[0], y, pad[2])
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

// ---------- 站体碰撞（STATION_COLS） ----------

struct ColBox {
    min: [f32; 3],
    max: [f32; 3],
}

fn station_cols() -> Vec<ColBox> {
    let mut v = Vec::new();
    let push = |v: &mut Vec<ColBox>, min: [f32; 3], max: [f32; 3], sym: bool| {
        v.push(ColBox { min, max });
        if sym {
            v.push(ColBox {
                min: [-max[0], min[1], min[2]],
                max: [-min[0], max[1], max[2]],
            });
        }
    };
    push(&mut v, [-40.0, -4.0, -20.0], [40.0, 0.0, 82.0], false); // 库底板
    push(&mut v, [-40.0, 30.0, -20.0], [40.0, 34.0, 82.0], false); // 库顶板
    push(&mut v, [32.0, 0.0, -20.0], [40.0, 30.0, 82.0], true); // 侧墙
    push(&mut v, [-40.0, 0.0, -20.0], [40.0, 30.0, -14.0], false); // 后墙
    push(&mut v, [-40.0, 18.0, 76.0], [40.0, 30.0, 82.0], false); // 前墙·上段
    push(&mut v, [-40.0, 0.0, 76.0], [40.0, 2.0, 82.0], false); // 前墙·下段
    push(&mut v, [14.0, 2.0, 76.0], [40.0, 18.0, 82.0], true); // 前墙·侧段（中央即入口槽）
    v
}

/// 空间站实体碰撞（飞船按半径 SHIP_R 的球处理）。
pub fn resolve_station_collision(pos: &mut Vec3, station_pos: Vec3, shield_up: bool) -> bool {
    let mut corrected = false;
    let p = *pos - station_pos;
    // Active station shield: a 213-unit bubble centered slightly above and
    // behind the hangar. Ships already in the bay are not pushed out.
    if shield_up {
        let inside_bay = p.x.abs() < 32.0 && p.y > 0.0 && p.y < 30.0 && p.z > -14.0 && p.z < 78.0;
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
    // 主塔圆柱
    {
        let c = Vec3::new(0.0, 0.0, -72.0);
        let r = 46.0 + SHIP_R;
        let d = Vec3::new(p.x - c.x, 0.0, p.z - c.z).length();
        if d < r && p.y > -60.0 - SHIP_R && p.y < 74.0 + SHIP_R {
            if d > 1e-4 {
                let push = (r - d) / d;
                pos.x += (p.x - c.x) * push;
                pos.z += (p.z - c.z) * push;
                corrected = true;
            } else {
                pos.x += r;
                corrected = true;
            }
        }
    }
    for cb in station_cols() {
        let qx = p.x.clamp(cb.min[0], cb.max[0]);
        let qy = p.y.clamp(cb.min[1], cb.max[1]);
        let qz = p.z.clamp(cb.min[2], cb.max[2]);
        let d = Vec3::new(p.x - qx, p.y - qy, p.z - qz);
        let d2 = d.length_squared();
        if d2 >= SHIP_R * SHIP_R {
            continue;
        }
        if d2 > 1e-6 {
            let dn = d.normalize();
            let push = SHIP_R - d2.sqrt();
            pos.x += dn.x * push;
            pos.y += dn.y * push;
            pos.z += dn.z * push;
            corrected = true;
        } else {
            let pens = [
                p.x - cb.min[0],
                cb.max[0] - p.x,
                p.y - cb.min[1],
                cb.max[1] - p.y,
                p.z - cb.min[2],
                cb.max[2] - p.z,
            ];
            let mut mi = 0;
            for i in 1..6 {
                if pens[i] < pens[mi] {
                    mi = i;
                }
            }
            let push = pens[mi] + SHIP_R;
            match mi {
                0 => pos.x -= push,
                1 => pos.x += push,
                2 => pos.y -= push,
                3 => pos.y += push,
                4 => pos.z -= push,
                _ => pos.z += push,
            }
            corrected = true;
        }
    }
    corrected
}

/// 泊入触发区（inBay / inGate）。
pub fn in_dock_zone(ship_pos: &Vec3, station_pos: Vec3) -> bool {
    let v = *ship_pos - station_pos;
    let in_bay = v.x.abs() < 30.0 && v.y > 0.0 && v.y < 30.0 && v.z > -12.0 && v.z < 76.0;
    let in_gate = v.x.abs() < 12.0 && v.y > 3.0 && v.y < 17.0 && v.z > 82.0 && v.z < 100.0;
    in_bay || in_gate
}

// ---------- 站内状态 ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StationPhase {
    #[default]
    Idle,
    Dock,
    Parked,
    Walk,
    Leave,
}

#[derive(Clone, Debug)]
pub struct WalkState {
    pub pos: Vec3,
    pub board_cd: f32,
    /// 站内喷气背包垂直速度（JS station.js:354-370）
    pub vy: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StationNear {
    #[default]
    None,
    Ship,
    Terminal,
    Garage,
    Pilot(usize),
    Staff(usize),
}

#[derive(Clone, Debug)]
pub struct BuyOffer {
    pub cls: String,
    pub model: String,
    pub price: i32,
    pub pilot_name: String,
}

#[derive(Clone, Debug)]
pub struct StationDialog {
    pub name: String,
    pub lines: Vec<String>,
    pub idx: usize,
    pub chars: usize,
    pub buy: Option<usize>,
    pub owner: usize,
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
}

#[derive(Resource)]
pub struct StationState {
    pub phase: StationPhase,
    pub t: f32,
    pub curve: Vec<Vec3>,
    pub dur: f32,
    pub pad: Vec3,
    pub pad_yaw: f32,
    pub walk: Option<WalkState>,
    pub dlg: Option<StationDialog>,
    pub near: StationNear,
    pub offers: Vec<BuyOffer>,
    pub staff_positions: Vec<Vec3>,
    pub staff_talks: Vec<Vec<String>>,
    pub pilots: Vec<PilotNpc>,
    pub station_pos: Vec3,
    pub staff_entities: Vec<Entity>,
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
            walk: None,
            dlg: None,
            near: StationNear::None,
            offers: Vec::new(),
            staff_positions: Vec::new(),
            staff_talks: Vec::new(),
            pilots: Vec::new(),
            station_pos: Vec3::ZERO,
            staff_entities: Vec::new(),
        }
    }
}

/// 构建泊入状态（space_system 调 begin_dock 后 insert_resource）。
pub fn begin_dock(mode: &mut FlightMode, ship_pos: &Vec3, station_pos: Vec3) -> StationState {
    let v = *ship_pos - station_pos;
    let in_bay = v.x.abs() < 30.0 && v.y > 0.0 && v.y < 30.0 && v.z > -12.0 && v.z < 76.0;
    let mut st = StationState {
        phase: StationPhase::Dock,
        t: 0.0,
        station_pos,
        ..default()
    };
    st.pad = station_pos + Vec3::from(DOCK_PAD);
    let over = st.pad + Vec3::Y * 7.0;
    let slot = station_pos + Vec3::from(DOCK_SLOT);
    let inner = station_pos + Vec3::from(DOCK_INNER_WAIT);
    if in_bay {
        st.curve = vec![*ship_pos, inner, over];
        st.dur = 2.2;
    } else {
        st.curve = vec![
            *ship_pos,
            slot + Vec3::new(0.0, 1.0, 16.0),
            slot,
            inner,
            over,
        ];
        st.dur = 4.2;
    }
    st.offers = roll_offers(station_pos);
    st.staff_positions = vec![
        station_pos + Vec3::new(-8.0, 3.1, -6.0),
        station_pos + Vec3::new(0.0, 3.1, -8.0),
        station_pos + Vec3::new(8.0, 3.1, -6.0),
    ];
    st.staff_talks = vec![
        vec!["欢迎来到贸易站。".into(), "需要补给的话去终端看看。".into()],
        vec!["停机坪有三位游商。".into(), "他们的船都保养得不错。".into()],
        vec![
            "想买蓝图吗？终端上有售。".into(),
            "科技是最大的财富。".into(),
        ],
    ];
    st.pilots = build_pilots(&st.offers, station_pos);
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

fn build_pilots(offers: &[BuyOffer], station_pos: Vec3) -> Vec<PilotNpc> {
    let mut out = Vec::new();
    for (i, o) in offers.iter().enumerate() {
        let pad = station_pos + Vec3::from(VIS_PADS[i % VIS_PADS.len()]);
        let beside = pad + Vec3::new(-3.5, 2.0, 0.0);
        out.push(PilotNpc {
            pos: beside,
            name: o.pilot_name.clone(),
            cls: o.cls.clone(),
            model: o.model.clone(),
            price: o.price,
            entity: None,
            ship_entity: None,
        });
    }
    out
}

// ---------- 站体模型 ----------

pub fn spawn_station(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    pos: Vec3,
) -> Entity {
    let hull = mats.add(StandardMaterial {
        base_color: Color::srgb(0.38, 0.44, 0.5),
        perceptual_roughness: 0.7,
        metallic: 0.6,
        cull_mode: None, // 机库内部可见
        ..default()
    });
    let dark = mats.add(StandardMaterial {
        base_color: Color::srgb(0.24, 0.28, 0.32),
        perceptual_roughness: 0.8,
        metallic: 0.5,
        cull_mode: None,
        ..default()
    });
    let glow_c = mats.add(StandardMaterial {
        base_color: Color::srgb(0.21, 0.88, 0.91),
        emissive: LinearRgba::new(0.1, 0.6, 0.7, 1.0) * 2.0,
        unlit: true,
        ..default()
    });
    let glow_a = mats.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.7, 0.28),
        emissive: LinearRgba::new(0.7, 0.4, 0.1, 1.0) * 2.0,
        unlit: true,
        ..default()
    });
    let gate_glow = mats.add(StandardMaterial {
        base_color: Color::srgb(0.21, 0.88, 0.91),
        emissive: LinearRgba::new(0.1, 0.6, 0.7, 1.0) * 2.0,
        unlit: true,
        ..default()
    });
    let root = commands
        .spawn((
            Transform::from_translation(pos),
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
             z: f32|
     -> Entity {
        let e = commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(w, h, d))),
                MeshMaterial3d(m.clone()),
                Transform::from_xyz(x, y, z),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(e);
        e
    };
    // 主塔
    let tower = commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::new(46.0, 134.0))),
            MeshMaterial3d(hull.clone()),
            Transform::from_xyz(0.0, 7.0, -72.0),
            crate::InGame,
        ))
        .id();
    commands.entity(root).add_child(tower);
    // 巨环桁架
    let ring = commands
        .spawn((
            Mesh3d(meshes.add(Torus::new(6.0, 70.0))),
            MeshMaterial3d(dark.clone()),
            Transform::from_xyz(0.0, -20.0, -72.0)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            crate::InGame,
        ))
        .id();
    commands.entity(root).add_child(ring);
    // 视觉层：把原本的“大圆柱 + 大圆环”拆成分段工业结构。
    // 这些实体只负责渲染，碰撞仍由 station_cols() 独立维护，方便后续继续迭代模型。
    let spine_dark = mats.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.16, 0.2),
        perceptual_roughness: 0.62,
        metallic: 0.78,
        cull_mode: None,
        ..default()
    });
    let panel_blue = mats.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.28, 0.36),
        perceptual_roughness: 0.48,
        metallic: 0.72,
        cull_mode: None,
        ..default()
    });
    let panel_orange = mats.add(StandardMaterial {
        base_color: Color::srgb(0.46, 0.23, 0.1),
        perceptual_roughness: 0.5,
        metallic: 0.62,
        cull_mode: None,
        ..default()
    });
    let station_light = mats.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.9, 0.92),
        emissive: LinearRgba::new(0.08, 0.72, 0.78, 1.0) * 3.0,
        unlit: true,
        ..default()
    });
    let warning_light = mats.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.44, 0.12),
        emissive: LinearRgba::new(0.95, 0.18, 0.03, 1.0) * 2.5,
        unlit: true,
        ..default()
    });

    // 主轴分段和外露维护环，避免主塔成为一根没有层级的光滑柱体。
    for y in [-43.0f32, -18.0, 7.0, 32.0, 57.0] {
        b(
            commands,
            meshes,
            root,
            56.0,
            2.2,
            9.0,
            &spine_dark,
            0.0,
            y,
            -72.0,
        );
        b(
            commands,
            meshes,
            root,
            38.0,
            0.65,
            3.0,
            &station_light,
            0.0,
            y + 1.45,
            -72.0,
        );
    }
    // 环形居住舱与桁架支撑：用重复模块制造远近都可辨认的轮廓。
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let x = a.cos() * 63.0;
        let y = -20.0 + a.sin() * 63.0;
        let pod = b(
            commands,
            meshes,
            root,
            18.0,
            5.0,
            10.0,
            if i % 2 == 0 {
                &panel_blue
            } else {
                &panel_orange
            },
            x,
            y,
            -72.0,
        );
        commands.entity(pod).insert(StationModule);
        let lamp = b(
            commands,
            meshes,
            root,
            8.0,
            0.35,
            0.45,
            &station_light,
            x,
            y + 3.0,
            -72.0,
        );
        commands.entity(lamp).insert(StationModule);
    }
    // 四条停泊臂与外侧端舱。
    for (x, z, rot) in [
        (-58.0f32, -72.0f32, 0.0f32),
        (58.0, -72.0, 0.0),
        (0.0, -130.0, std::f32::consts::FRAC_PI_2),
        (0.0, -14.0, std::f32::consts::FRAC_PI_2),
    ] {
        let arm = commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(12.0, 5.0, 46.0))),
                MeshMaterial3d(spine_dark.clone()),
                Transform::from_xyz(x, -20.0, z).with_rotation(Quat::from_rotation_y(rot)),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(arm);
        let arm_light = commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(2.0, 0.35, 38.0))),
                MeshMaterial3d(station_light.clone()),
                Transform::from_xyz(x, -16.8, z).with_rotation(Quat::from_rotation_y(rot)),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(arm_light);
    }
    // 机库门框和跑道灯：强化当前可交互区域的视觉指向。
    for x in [-18.0f32, 18.0] {
        b(
            commands,
            meshes,
            root,
            3.0,
            24.0,
            3.0,
            &spine_dark,
            x,
            13.0,
            79.0,
        );
        b(
            commands,
            meshes,
            root,
            3.8,
            0.8,
            3.0,
            &station_light,
            x,
            26.0,
            79.0,
        );
        for z in [12.0f32, 24.0, 36.0, 48.0, 60.0] {
            b(
                commands,
                meshes,
                root,
                1.0,
                0.28,
                3.0,
                &station_light,
                x,
                2.2,
                z,
            );
        }
    }
    // 维护天线和顶部警示灯，让站体在远处不再只是一个灰色几何体。
    b(
        commands,
        meshes,
        root,
        2.5,
        36.0,
        2.5,
        &spine_dark,
        0.0,
        76.0,
        -72.0,
    );
    b(
        commands,
        meshes,
        root,
        12.0,
        1.2,
        2.0,
        &panel_blue,
        0.0,
        92.0,
        -72.0,
    );
    b(
        commands,
        meshes,
        root,
        2.2,
        2.2,
        2.2,
        &warning_light,
        0.0,
        96.0,
        -72.0,
    );
    // 低成本局部灯光：只照亮机库入口和中央轴，不给每个模块创建灯。
    for (translation, color) in [
        (Vec3::new(0.0, 18.0, 72.0), Color::srgb(0.15, 0.75, 0.9)),
        (Vec3::new(-24.0, 5.0, 34.0), Color::srgb(0.1, 0.55, 0.9)),
        (Vec3::new(24.0, 5.0, 34.0), Color::srgb(0.95, 0.42, 0.16)),
    ] {
        let light = commands
            .spawn((
                PointLight {
                    color,
                    intensity: 850.0,
                    range: 32.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(translation),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(light);
    }
    // 机库
    b(
        commands, meshes, root, 80.0, 4.0, 102.0, &dark, 0.0, -2.0, 31.0,
    );
    b(
        commands, meshes, root, 80.0, 4.0, 102.0, &dark, 0.0, 32.0, 31.0,
    );
    b(
        commands, meshes, root, 8.0, 30.0, 102.0, &hull, -36.0, 15.0, 31.0,
    );
    b(
        commands, meshes, root, 8.0, 30.0, 102.0, &hull, 36.0, 15.0, 31.0,
    );
    b(
        commands, meshes, root, 80.0, 30.0, 6.0, &hull, 0.0, 15.0, -17.0,
    );
    b(
        commands, meshes, root, 80.0, 12.0, 6.0, &hull, 0.0, 24.0, 79.0,
    );
    b(
        commands, meshes, root, 80.0, 2.0, 6.0, &hull, 0.0, 1.0, 79.0,
    );
    b(
        commands, meshes, root, 26.0, 16.0, 6.0, &hull, -27.0, 10.0, 79.0,
    );
    b(
        commands, meshes, root, 26.0, 16.0, 6.0, &hull, 27.0, 10.0, 79.0,
    );
    // 停机坪
    for p in PADS {
        b(
            commands,
            meshes,
            root,
            12.0,
            2.0,
            12.0,
            &dark,
            p[0],
            p[1] + 1.0,
            p[2],
        );
    }
    // 大厅平台（z≤6 抬升）
    b(
        commands, meshes, root, 64.0, 3.0, 26.0, &hull, 0.0, 1.5, -7.0,
    );
    // 门口引导灯
    let gate_left = b(
        commands, meshes, root, 2.0, 0.4, 0.4, &gate_glow, -14.0, 10.0, 79.4,
    );
    let gate_right = b(
        commands, meshes, root, 2.0, 0.4, 0.4, &gate_glow, 14.0, 10.0, 79.4,
    );
    commands.entity(gate_left).insert(StationGateLight);
    commands.entity(gate_right).insert(StationGateLight);
    // 交易终端（发光屏）
    b(
        commands,
        meshes,
        root,
        2.6,
        4.2,
        1.4,
        &dark,
        DOCK_TERMINAL[0],
        DOCK_TERMINAL[1] + 1.0,
        DOCK_TERMINAL[2],
    );
    b(
        commands,
        meshes,
        root,
        2.2,
        2.0,
        0.2,
        &glow_c,
        DOCK_TERMINAL[0],
        DOCK_TERMINAL[1] + 1.0,
        DOCK_TERMINAL[2] - 0.72,
    );
    // 换船电脑
    b(
        commands,
        meshes,
        root,
        2.2,
        3.0,
        1.2,
        &dark,
        DOCK_GARAGE[0],
        DOCK_GARAGE[1] + 0.5,
        DOCK_GARAGE[2],
    );
    b(
        commands,
        meshes,
        root,
        1.8,
        1.4,
        0.2,
        &glow_a,
        DOCK_GARAGE[0],
        DOCK_GARAGE[1] + 0.5,
        DOCK_GARAGE[2] - 0.62,
    );
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
    root
}

/// 外部空间站模型。模型按星系种子轮换：家园使用基础站，后续星系
/// 使用其余 CC-BY 模型；停靠路径和碰撞仍使用本文件中的逻辑尺寸。
pub fn spawn_station_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    pos: Vec3,
    galaxy_seed: u32,
) -> Entity {
    let (path, scale, rotation) = if galaxy_seed == data::HOME_GALAXY_SEED {
        (
            "models/external/stations/space_station/scene.gltf",
            11.0,
            Quat::IDENTITY,
        )
    } else {
        match galaxy_seed % 3 {
            0 => (
                "models/external/stations/space_station_3/scene.gltf",
                30.0,
                Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            ),
            1 => (
                "models/external/stations/space_station_4/scene.gltf",
                5.0,
                Quat::IDENTITY,
            ),
            _ => (
                "models/external/stations/helveta/scene.gltf",
                0.28,
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            ),
        }
    };
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
        crate::audio::play(&mut commands, sfx.scan.clone(), 0.45, None);
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
                let mut fresh = crate::save::ShipSave {
                    model: e.model.clone(),
                    cls: e.cls.clone(),
                    name,
                    inv: vec![None; data::ship_class_by_key(&e.cls).slots],
                };
                fresh.inv = old.inv.clone();
                fresh.inv.truncate(data::ship_class_by_key(&e.cls).slots);
                fresh
                    .inv
                    .resize(data::ship_class_by_key(&e.cls).slots, None);
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
        game.ship_inv = ship_asset.data.inv.clone();
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
    mut player: Query<&mut Player>,
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut flight_cam: ResMut<FlightCamera>,
    mut flag_ev: MessageWriter<FlagEvent>,
    mut big_ev: MessageWriter<BigMessageEvent>,
    mut switch_ev: MessageWriter<ShipSwitchEvent>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    if *next_mode != FlightMode::Station {
        ui_state.prompt = None;
        // 离站清理：NPC/访客飞船/对话全清
        for pil in st.pilots.iter_mut() {
            if let Some(e) = pil.entity.take() {
                commands.entity(e).despawn();
            }
            if let Some(e) = pil.ship_entity.take() {
                commands.entity(e).despawn();
            }
        }
        for e in st.staff_entities.drain(..) {
            commands.entity(e).despawn();
        }
        if st.phase != StationPhase::Idle {
            st.phase = StationPhase::Idle;
            st.dlg = None;
            st.near = StationNear::None;
            st.walk = None;
        }
        return;
    }
    let dt = time.delta_secs();
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
                    title: "泊入完成".into(),
                    sub: "E 下船 · W 离站".into(),
                    dur: 2.4,
                });
            }
        }
        StationPhase::Parked => {
            ship.pos = st.pad;
            ship.yaw = st.pad_yaw;
            ui_state.prompt = Some("[E] 下船 · W 离站".into());
            if ui_state.locked() {
                ui_state.prompt = None;
                return;
            }
            if keys.just_pressed(KeyCode::KeyE) {
                disembark(&mut st, &mut player);
                crate::audio::play(&mut commands, sfx.jump.clone(), 0.5, None);
            }
            if keys.just_pressed(KeyCode::KeyW) {
                st.curve = vec![
                    st.pad,
                    st.pad + Vec3::Y * 7.0,
                    st.station_pos + Vec3::from(DOCK_EXIT),
                ];
                st.dur = 2.4;
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
                ship.speed = 30.0; // JS: 出口速度 30
                *next_mode = FlightMode::Space;
                crate::audio::play(&mut commands, sfx.laser_hit.clone(), 0.5, None);
            }
        }
        StationPhase::Walk => {
            walk_tick(
                dt,
                &mut st,
                &mut player,
                &mut ship,
                &keys,
                &mut ui_state,
                &mut flight_cam,
                &mut big_ev,
                &mut switch_ev,
                &mut commands,
                &sfx,
            );
        }
        StationPhase::Idle => {}
    }
}

fn disembark(st: &mut StationState, player: &mut Query<&mut Player>) {
    let sx = st.station_pos.x + 8.0;
    let sz = st.station_pos.z + 24.0;
    let fy = st.station_pos.y + floor_at(8.0, 24.0) + 0.1;
    st.walk = Some(WalkState {
        pos: Vec3::new(sx, fy, sz),
        board_cd: 0.6,
        vy: 0.0,
    });
    if let Ok(mut p) = player.single_mut() {
        let term = st.station_pos + Vec3::from(DOCK_TERMINAL);
        p.yaw = (-(term.x - sx)).atan2(-(term.z - sz));
        p.pitch = 0.0;
        if let Some(walk) = st.walk.as_ref() {
            p.pos = walk.pos;
        }
        p.vel = Vec3::ZERO;
    }
    st.phase = StationPhase::Walk;
    st.near = StationNear::None;
}

/// 站内地板高度（局部）：停机坪 4、大厅 3、其余 0。
fn floor_at(lx: f32, lz: f32) -> f32 {
    for p in PADS {
        if (lx - p[0]).abs() < 7.0 && (lz - p[2]).abs() < 7.0 {
            return p[1] + 2.0;
        }
    }
    if lz <= 6.0 { 3.0 } else { 0.0 }
}

#[allow(clippy::too_many_arguments)]
fn walk_tick(
    dt: f32,
    st: &mut StationState,
    player: &mut Query<&mut Player>,
    ship: &mut ShipState,
    keys: &ButtonInput<KeyCode>,
    ui_state: &mut UiState,
    flight_cam: &mut FlightCamera,
    big_ev: &mut MessageWriter<BigMessageEvent>,
    switch_ev: &mut MessageWriter<ShipSwitchEvent>,
    commands: &mut Commands,
    sfx: &crate::audio::Sfx,
) {
    let Ok(mut p) = player.single_mut() else {
        return;
    };
    let o = st.station_pos;
    let Some(w) = st.walk.as_mut() else {
        ui_state.prompt = None;
        return;
    };
    w.board_cd -= dt;
    if ui_state.locked() {
        // 面板打开时视角维持
        let cam_pos = p.eye();
        let rot = Quat::from_rotation_y(p.yaw) * Quat::from_rotation_x(p.pitch);
        *flight_cam = FlightCamera::set(cam_pos, rot, 75.0);
        st.near = StationNear::None;
        ui_state.prompt = None;
        return;
    }
    let f = Vec3::new(-p.yaw.sin(), 0.0, -p.yaw.cos());
    let r = Vec3::new(-f.z, 0.0, f.x);
    let mut wish = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        wish += f;
    }
    if keys.pressed(KeyCode::KeyS) {
        wish -= f;
    }
    if keys.pressed(KeyCode::KeyD) {
        wish += r;
    }
    if keys.pressed(KeyCode::KeyA) {
        wish -= r;
    }
    let sp = if keys.pressed(KeyCode::ShiftLeft) {
        7.0
    } else {
        4.4
    };
    if wish.length_squared() > 0.0 {
        wish = wish.normalize();
    }
    w.pos += wish * sp * dt;
    let lx = (w.pos.x - o.x).clamp(-BOUNDS_X + 0.5, BOUNDS_X - 0.5);
    let lz = (w.pos.z - o.z).clamp(BOUNDS_Z_MIN + 0.5, BOUNDS_Z_MAX - 0.5);
    // 站内喷气背包（JS：重力 20、喷气 +46 上限 8.5、耗 22/s、回充 16/s、天花板 +28）
    let mut vy = w.vy - 20.0 * dt;
    if keys.pressed(KeyCode::Space) && p.stats.jet > 0.0 {
        vy = (vy + 46.0 * dt).min(8.5);
        p.stats.jet = (p.stats.jet - 22.0 * dt).max(0.0);
    }
    let floor_y = o.y + floor_at(lx, lz) + 0.1;
    let ceil_y = o.y + 28.0;
    let mut ny = w.pos.y + vy * dt;
    if ny <= floor_y {
        ny = floor_y;
        vy = 0.0;
        p.stats.jet = (p.stats.jet + 16.0 * dt).min(100.0);
    } else if ny >= ceil_y {
        ny = ceil_y;
        vy = 0.0;
    }
    w.vy = vy;
    w.pos = Vec3::new(o.x + lx, ny, o.z + lz);
    p.pos = w.pos;
    // 附近交互目标（JS 固定优先级：ship 7.5 > garage 3.6 > terminal 4.2 > pilot 3.4 > staff 3.4）
    let mut near = StationNear::None;
    if w.board_cd <= 0.0 {
        let d = w.pos.distance(st.pad + Vec3::Y * 1.5);
        if d < 7.5 {
            near = StationNear::Ship;
        }
    }
    if near == StationNear::None {
        let gar = st.station_pos + Vec3::from(DOCK_GARAGE);
        if w.pos.distance(gar + Vec3::Y * 1.2) < 3.6 {
            near = StationNear::Garage;
        }
    }
    if near == StationNear::None {
        let term = st.station_pos + Vec3::from(DOCK_TERMINAL);
        if w.pos.distance(term + Vec3::Y * 1.5) < 4.2 {
            near = StationNear::Terminal;
        }
    }
    if near == StationNear::None {
        for (i, pil) in st.pilots.iter().enumerate() {
            if w.pos.distance(pil.pos + Vec3::Y * 1.0) < 3.4 {
                near = StationNear::Pilot(i);
                break;
            }
        }
    }
    if near == StationNear::None {
        for (i, sp_) in st.staff_positions.iter().enumerate() {
            if w.pos.distance(*sp_) < 3.4 {
                near = StationNear::Staff(i);
                break;
            }
        }
    }
    st.near = near;
    ui_state.prompt = match near {
        StationNear::None => None,
        StationNear::Ship => Some("[E] 登船".to_string()),
        StationNear::Terminal => Some("[E] 打开贸易终端".to_string()),
        StationNear::Garage => Some("[E] 打开换船电脑".to_string()),
        StationNear::Pilot(i) => Some(format!(
            "[E] 与{}交谈",
            st.pilots.get(i).map(|x| x.name.as_str()).unwrap_or("游商")
        )),
        StationNear::Staff(_) => Some("[E] 与站员交谈".to_string()),
    };
    let cam_pos = p.eye();
    let rot = Quat::from_rotation_y(p.yaw) * Quat::from_rotation_x(p.pitch);
    *flight_cam = FlightCamera::set(cam_pos, rot, 75.0);
    if !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    if let Some(d) = st.dlg.as_mut() {
        let cur_len = d.lines[d.idx].chars().count();
        let fully = d.chars >= cur_len;
        if !fully {
            d.chars = cur_len;
            return;
        }
        if d.idx + 1 < d.lines.len() {
            d.idx += 1;
            d.chars = 0;
            return;
        }
        let buy = d.buy;
        let ok = match buy.and_then(|bi| st.offers.get(bi).cloned()) {
            Some(o) => {
                let credits = p.credits;
                if credits >= o.price {
                    p.credits -= o.price;
                    switch_ev.write(ShipSwitchEvent {
                        cls: o.cls.clone(),
                        model: o.model.clone(),
                        garage_idx: None,
                    });
                    big_ev.write(BigMessageEvent {
                        title: "成交！".into(),
                        sub: format!("已购入 {} 级飞船", o.cls),
                        dur: 2.4,
                    });
                    true
                } else {
                    big_ev.write(BigMessageEvent {
                        title: "信用点不足".into(),
                        sub: format!("需要 ₪{}", o.price),
                        dur: 2.2,
                    });
                    crate::audio::play(commands, sfx.error.clone(), 0.5, None);
                    false
                }
            }
            None => true,
        };
        if ok {
            st.dlg = None;
        }
        return;
    }
    match near {
        StationNear::Ship => {
            st.phase = StationPhase::Parked;
            st.walk = None;
            p.pos = st.pad + Vec3::Y * 2.0;
            ship.pos = st.pad;
            ship.yaw = st.pad_yaw;
        }
        StationNear::Terminal => {
            ui_state.panel = Panel::Trade;
            crate::audio::play(commands, sfx.click.clone(), 0.5, None);
        }
        StationNear::Garage => {
            ui_state.panel = Panel::Garage;
            crate::audio::play(commands, sfx.click.clone(), 0.5, None);
        }
        StationNear::Pilot(i) => {
            let Some(offer) = st.offers.get(i).cloned() else {
                return;
            };
            let cls = data::ship_class_by_key(&offer.cls);
            let model_name = data::SHIP_MODEL_NAMES
                .iter()
                .find(|(k, _)| *k == offer.model)
                .map(|(_, n)| *n)
                .unwrap_or("飞船");
            st.dlg = Some(StationDialog {
                name: offer.pilot_name.clone(),
                lines: vec![
                    format!("看什么？哦——我这艘「{model_name}」啊。"),
                    format!(
                        "等级 {} 级 · 武装「{}」 · 货仓 {} 格。",
                        cls.key, cls.weapon_name, cls.slots
                    ),
                    format!("出价 ₪{}，一口价。想要的话，再按一次 E 成交。", offer.price),
                ],
                idx: 0,
                chars: 0,
                buy: Some(i),
                owner: i,
            });
            crate::audio::play(commands, sfx.click.clone(), 0.5, None);
        }
        StationNear::Staff(i) => {
            let lines = st
                .staff_talks
                .get(i)
                .cloned()
                .unwrap_or_else(|| vec!["你好，旅行者。".into()]);
            st.dlg = Some(StationDialog {
                name: "站员".into(),
                lines,
                idx: 0,
                chars: 0,
                buy: None,
                owner: i,
            });
            crate::audio::play(commands, sfx.click.clone(), 0.5, None);
        }
        StationNear::None => {}
    }
}

// ---------- 对话打字机 ----------

pub fn station_dialog_system(time: Res<Time>, mut st: ResMut<StationState>) {
    if let Some(d) = st.dlg.as_mut() {
        let dt = time.delta_secs();
        // JS 打字机 26 字符/秒
        d.chars += (dt * 26.0) as usize;
        if let Some(cur) = d.lines.get(d.idx)
            && d.chars > cur.chars().count() + 8
        {
            d.chars = cur.chars().count();
        }
    }
}

// ---------- 站内 NPC 实体 ----------

#[allow(clippy::too_many_arguments)]
pub fn station_npc_spawn_system(
    mode: Res<FlightMode>,
    mut st: ResMut<StationState>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if *mode != FlightMode::Station {
        return;
    }
    if st.phase != StationPhase::Walk {
        return;
    }
    if !st.pilots.iter().any(|p| p.entity.is_none()) {
        return;
    }
    let mut rnd = crate::rng::Rng::new(st.station_pos.x.to_bits() ^ 0x5EED);
    let station_pos = st.station_pos;
    let pilots = st.pilots.clone();
    for (i, pil) in pilots.iter().enumerate() {
        let orig = &mut st.pilots[i];
        if orig.entity.is_some() {
            continue;
        }
        let app = Appearance::random(rnd.next().to_bits());
        let human = crate::char::spawn_humanoid(
            &mut commands,
            &asset_server,
            &app,
            pil.pos,
            std::f32::consts::PI,
        );
        orig.entity = Some(human.root);
        let pad = station_pos + Vec3::from(VIS_PADS[i % VIS_PADS.len()]);
        let cls = data::ship_class_by_key(&pil.cls);
        let (ship_e, flames) = crate::space::spawn_external_ship(
            &mut commands,
            &mut meshes,
            &mut mats,
            &asset_server,
            pad + Vec3::new(0.0, 2.0, 0.0),
            std::f32::consts::PI,
            cls,
            Some(&pil.model),
        );
        orig.ship_entity = Some(ship_e);
        for f in flames {
            commands.entity(f).despawn();
        }
    }
    // 站员
    if st.staff_entities.is_empty() {
        for sp in st.staff_positions.clone() {
            let app = Appearance::random(rnd.next().to_bits());
            let human = crate::char::spawn_humanoid(
                &mut commands,
                &asset_server,
                &app,
                sp,
                std::f32::consts::PI,
            );
            st.staff_entities.push(human.root);
        }
    }
}
