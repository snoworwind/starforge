//! Creatures (simple wandering voxel animals) and dropped-item entities.

use crate::data;
use crate::inventory::Slot;
use crate::player::Player;
use crate::rng::Rng;
use crate::schedule::{GameSet, GameState, creature_mode};
use crate::ui::IconMaterials;
use crate::world::World;
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use bevy_world_serialization::prelude::WorldAssetRoot;
use std::collections::HashMap;
use std::time::Duration;

// ---------- Creatures（Minecraft 风格兽群系统，JS creatures.js 移植） ----------

/// 24m 生成网格（MC 同款）。
pub const CRE_CELL: f32 = 24.0;
/// 首次踏入一格时建立兽群的几率（≈ MC 区块生成生物的 1/10）。
pub const HERD_CHANCE: f32 = 0.30;
/// 玩家 128m 范围内活跃兽群数量目标（低于此值才周期补足 → 被杀后缓慢恢复）。
pub const TARGET_DENSITY: usize = 20;
/// 活跃生物上限（安全阀）。
pub const CRE_CAP: usize = 28;
/// 生成环内径：距玩家 < 24m 不生成（Minecraft 同款规则）。
pub const SPAWN_MIN: f32 = 24.0;
/// 生成环外径（Minecraft 同款 128m）。
pub const SPAWN_MAX: f32 = 128.0;
/// 距玩家 > 128m：兽群卸载休眠（保留位置/血量，不删除）。
pub const UNLOAD_D: f32 = 128.0;
/// 距玩家 < 96m：休眠兽群重载（迟滞带，避免边界抖动）。
pub const RELOAD_D: f32 = 96.0;
/// 周期生成间隔（秒），每次最多补 1 个兽群。
pub const SPAWN_INTERVAL: f32 = 0.8;
pub const FADE_IN_T: f32 = 1.0;
pub const FADE_OUT_T: f32 = 0.8;
/// 扫描细胞半径（覆盖 96m 物化半径 + 游荡）。
const BUCKET_R: i32 = 10;

#[derive(Component)]
pub struct Creature {
    pub hp: f32,
    pub radius: f32,
    pub height: f32,
    pub shoot_t: f32,
    /// 换向计时
    pub ai_t: f32,
    pub dir: Vec3,
    pub vel: Vec3,
    pub grounded: bool,
    pub home: Vec3,
    pub jump_t: f32,
    pub kind: &'static str,
    /// 模型基准缩放（spawn 时按实测包围盒换算；动画必须乘性应用，不能覆盖）
    pub scale: f32,
    /// 模型原点离脚底的偏移（贴地时 origin.y = 地面 + 1 + foot，脚底正好在方块顶面）
    pub foot: f32,
    /// 所属兽群 nid（None = 哨兵等独立生物）
    pub nid: Option<u64>,
    /// 实际移动速度（物种速度 × 兽群归一化倍率）
    pub speed: f32,
    /// 散步/休息状态（JS state：walk 2~7s → idle 1.5~4.5s 循环）
    pub walking: bool,
    /// 动画相位
    pub anim_t: f32,
    /// 淡入计时
    pub spawn_t: f32,
    /// 淡出中（卸载休眠，不视为死亡）
    pub fading: bool,
    pub fade_t: f32,
    /// 受击反馈计时
    pub hit_t: f32,
    /// Neutral creatures retaliate for a short period after being attacked.
    pub aggro_t: f32,
    pub attack_cd: f32,
}

/// A visual limb owned by a procedural creature body.
/// Static GLB animals in the original asset set have no skeleton, so limbs are
/// separate child entities and can be driven from the gameplay velocity.
#[derive(Component)]
struct CreatureLimb {
    owner: Entity,
    base_translation: Vec3,
    base_rotation: Quat,
    swing_axis: Vec3,
    phase: f32,
    amplitude: f32,
    lift: f32,
}

#[derive(Component)]
struct CreatureBodyPart {
    owner: Entity,
    base_translation: Vec3,
    phase: f32,
}

/// Animation graph prepared before a creature scene is instantiated.
///
/// The glTF scene owns the `AnimationPlayer`, so this component stays on the
/// gameplay root until `WorldInstanceReady` lets us find that player below it.
#[derive(Clone, Component)]
struct CreatureAnimationSetup {
    graph: Handle<AnimationGraph>,
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
}

#[derive(Resource, Default)]
pub struct CreatureAnimationLibrary {
    alpaca: Option<CreatureAnimationSetup>,
    deer: Option<CreatureAnimationSetup>,
    fox: Option<CreatureAnimationSetup>,
    wolf: Option<CreatureAnimationSetup>,
    sentinel: Option<CreatureAnimationSetup>,
}

/// Link an instantiated animation player back to its gameplay entity. This
/// avoids assuming that the player is a direct child of the creature root.
#[derive(Component)]
pub(crate) struct CreatureAnimationTarget {
    owner: Entity,
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
}

fn creature_animation_setup(
    kind: &str,
    asset_server: &AssetServer,
    graphs: &mut Assets<AnimationGraph>,
    library: &mut CreatureAnimationLibrary,
) -> Option<CreatureAnimationSetup> {
    let (cache, model, idle_index, walk_index) = match kind {
        // The asset keeps these clips in a stable order; use handles rather
        // than names so graph construction does not depend on glTF metadata.
        "blob" | "beetle" | "manta" => (
            &mut library.alpaca,
            "models/creatures/quaternius_alpaca.gltf",
            6,
            12,
        ),
        "strider" => (
            &mut library.deer,
            "models/creatures/quaternius_deer.gltf",
            6,
            12,
        ),
        "hopper" => (
            &mut library.fox,
            "models/creatures/quaternius_fox.gltf",
            5,
            11,
        ),
        "crab" => (
            &mut library.wolf,
            "models/creatures/quaternius_wolf.gltf",
            5,
            11,
        ),
        "sentinel" => (
            &mut library.sentinel,
            "models/creatures/sentinel.glb",
            40,
            90,
        ),
        _ => return None,
    };
    if let Some(animation) = cache {
        return Some(animation.clone());
    }
    let (graph, nodes) = AnimationGraph::from_clips([
        asset_server.load(GltfAssetLabel::Animation(idle_index).from_asset(model)),
        asset_server.load(GltfAssetLabel::Animation(walk_index).from_asset(model)),
    ]);
    let animation = CreatureAnimationSetup {
        graph: graphs.add(graph),
        idle: nodes[0],
        walk: nodes[1],
    };
    *cache = Some(animation.clone());
    Some(animation)
}

/// Attach the graph to the player generated inside a creature's glTF scene.
fn creature_animation_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    setups: Query<&CreatureAnimationSetup>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Ok(setup) = setups.get(ready.entity) else {
        return;
    };
    for child in children.iter_descendants(ready.entity) {
        let Ok(mut player) = players.get_mut(child) else {
            continue;
        };
        let mut transitions = AnimationTransitions::new();
        transitions
            .play(&mut player, setup.idle, Duration::ZERO)
            .repeat();
        commands.entity(child).insert((
            AnimationGraphHandle(setup.graph.clone()),
            transitions,
            CreatureAnimationTarget {
                owner: ready.entity,
                idle: setup.idle,
                walk: setup.walk,
            },
        ));
    }
}

/// Follow the existing creature AI state with skeletal idle/walk clips.
pub fn creature_animation_system(
    mut players: Query<(
        &mut AnimationPlayer,
        &mut AnimationTransitions,
        &CreatureAnimationTarget,
    )>,
    creatures: Query<&Creature>,
) {
    for (mut player, mut transitions, target) in &mut players {
        let Ok(creature) = creatures.get(target.owner) else {
            continue;
        };
        let desired = if creature.walking {
            target.walk
        } else {
            target.idle
        };
        if transitions.get_main_animation() != Some(desired) {
            transitions
                .play(&mut player, desired, Duration::from_millis(180))
                .repeat();
        }
    }
}

impl Creature {
    pub fn new(home: Vec3) -> Self {
        Self {
            hp: 3.0,
            radius: 0.5,
            height: 1.0,
            shoot_t: 0.0,
            ai_t: 0.0,
            dir: Vec3::X,
            vel: Vec3::ZERO,
            grounded: false,
            home,
            jump_t: 0.0,
            kind: "strider",
            scale: 1.0,
            foot: 0.0,
            nid: None,
            speed: 1.8,
            walking: false,
            anim_t: 0.0,
            spawn_t: 1.0,
            fading: false,
            fade_t: 0.0,
            hit_t: 0.0,
            aggro_t: 0.0,
            attack_cd: 0.0,
        }
    }
}

/// 兽群存档记录（JS serialize herds）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HerdSave {
    pub cx: i32,
    pub cz: i32,
    pub cand: usize,
    pub x: f32,
    pub z: f32,
    pub hp: f32,
    pub home_x: f32,
    pub home_z: f32,
}

/// 细胞占用/被杀位图存档（JS removedMasks：被杀候选不复活）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CellSave {
    pub cx: i32,
    pub cz: i32,
    pub mask: u32,
}

/// 确定性候选点（含行为参数，JS registerCell 同序派生）。
#[derive(Clone)]
struct Cand {
    x: f32,
    z: f32,
    speed: f32,
    dir: f32,
    timer: f32,
    anim_t: f32,
}

#[derive(Clone)]
struct CellState {
    cands: Vec<Cand>,
    mask: u32,
    initialized: bool,
}

#[derive(Clone)]
struct Herd {
    nid: u64,
    cx: i32,
    cz: i32,
    cand: usize,
    x: f32,
    z: f32,
    hp: f32,
    home_x: f32,
    home_z: f32,
    speed: f32,
    dir: f32,
    timer: f32,
    anim_t: f32,
    entity: Option<Entity>,
}

/// 兽群生成器（MC 24m 网格：注册细胞 → 世界生成式掷骰 → 周期物化/卸载休眠）。
#[derive(Resource)]
pub struct CreatureSpawner {
    pub timer: f32,
    cells: HashMap<(i32, i32), CellState>,
    herds: HashMap<u64, Herd>,
}

impl Default for CreatureSpawner {
    fn default() -> Self {
        Self {
            timer: 0.0,
            cells: HashMap::new(),
            herds: HashMap::new(),
        }
    }
}

impl CreatureSpawner {
    /// 序列化（存档）：活跃实体回写最新位置/血量。
    pub fn serialize(
        &self,
        q: &Query<(Entity, &mut Creature, &Transform)>,
    ) -> (Vec<HerdSave>, Vec<CellSave>) {
        let mut herds: Vec<HerdSave> = Vec::new();
        for h in self.herds.values() {
            let mut x = h.x;
            let mut z = h.z;
            let mut hp = h.hp;
            if let Some(e) = h.entity
                && let Ok((_, c, tf)) = q.get(e)
            {
                x = tf.translation.x;
                z = tf.translation.z;
                hp = c.hp;
            }
            herds.push(HerdSave {
                cx: h.cx,
                cz: h.cz,
                cand: h.cand,
                x,
                z,
                hp,
                home_x: h.home_x,
                home_z: h.home_z,
            });
        }
        let cells = self
            .cells
            .iter()
            .map(|((cx, cz), c)| CellSave {
                cx: *cx,
                cz: *cz,
                mask: c.mask,
            })
            .collect();
        (herds, cells)
    }

    /// 读档恢复：兽群（位置/血量/领地）与击杀记录全部还原；被杀动物不会复活。
    /// 行为参数由 nid 确定性派生（JS herdParams）；物化时按当前生态取物种。
    pub fn restore(&mut self, world_seed: u32, herds: &[HerdSave], cells: &[CellSave]) {
        self.cells.clear();
        for c in cells.iter().take(100_000) {
            if c.cx.unsigned_abs() > 1_000_000 || c.cz.unsigned_abs() > 1_000_000 {
                continue;
            }
            self.cells
                .entry((c.cx, c.cz))
                .and_modify(|state| state.mask |= c.mask)
                .or_insert(CellState {
                    cands: Vec::new(),
                    mask: c.mask,
                    initialized: false,
                });
        }
        self.herds.clear();
        for h in herds.iter().take(100_000) {
            if h.cand >= u32::BITS as usize
                || h.cx.unsigned_abs() > 1_000_000
                || h.cz.unsigned_abs() > 1_000_000
            {
                continue;
            }
            let bit = 1u32 << h.cand;
            // A save can be captured after damage but before the regular
            // despawn pass records the kill. Treat non-positive saved HP as
            // dead and repair the cell mask instead of resurrecting it.
            if !h.hp.is_finite() || h.hp <= 0.0 {
                self.cells
                    .entry((h.cx, h.cz))
                    .and_modify(|state| state.mask |= bit)
                    .or_insert(CellState {
                        cands: Vec::new(),
                        mask: bit,
                        initialized: false,
                    });
                continue;
            }
            if self
                .cells
                .get(&(h.cx, h.cz))
                .is_some_and(|state| state.mask & bit != 0)
            {
                continue;
            }
            let nid = crate::rng::batch_seed(world_seed, h.cx, h.cz) as u64 * 64 + h.cand as u64;
            let mut rnd = Rng::new(nid as u32);
            let speed = 1.8 * (0.5 + rnd.next());
            let dir = rnd.next() * std::f32::consts::TAU;
            let timer = 1.0 + rnd.next() * 3.0;
            let anim_t = rnd.next() * 10.0;
            self.herds.insert(
                nid,
                Herd {
                    nid,
                    cx: h.cx,
                    cz: h.cz,
                    cand: h.cand,
                    x: if h.x.is_finite() { h.x } else { 0.0 },
                    z: if h.z.is_finite() { h.z } else { 0.0 },
                    hp: h.hp,
                    home_x: if h.home_x.is_finite() { h.home_x } else { 0.0 },
                    home_z: if h.home_z.is_finite() { h.home_z } else { 0.0 },
                    speed,
                    dir,
                    timer,
                    anim_t,
                    entity: None,
                },
            );
        }
        println!(
            "CREATURE restore herds={} cells={}",
            herds.len(),
            cells.len()
        );
    }
}

/// 模型参数：(模型路径, 缩放, 脚底偏移)。模型来自带骨骼动画的 Quaternius glTF。
/// 这些模型的脚底在 y≈0，统一缩放到约 2 格高，适配原有命中盒和地形贴地逻辑。
pub fn creature_model(kind: &str) -> (&'static str, f32, f32) {
    match kind {
        "strider" => ("models/creatures/quaternius_deer.gltf", 0.52, 0.0),
        "hopper" => ("models/creatures/quaternius_fox.gltf", 0.52, 0.0),
        "crab" => ("models/creatures/quaternius_wolf.gltf", 0.5, 0.0),
        "beetle" | "manta" => ("models/creatures/quaternius_alpaca.gltf", 0.5, 0.0),
        "blob" => ("models/creatures/quaternius_alpaca.gltf", 0.44, 0.0),
        _ => ("models/creatures/quaternius_deer.gltf", 0.52, 0.0),
    }
}

fn species_speed(kind: &str) -> f32 {
    match kind {
        "crab" | "beetle" => 0.7,
        "hopper" => 1.45,
        "manta" => 1.05,
        "blob" => 0.35,
        _ => 1.8,
    }
}

fn rgb_color(value: u32) -> Color {
    Color::srgb_u8(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

fn creature_part(
    commands: &mut Commands,
    root: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
) -> Entity {
    let e = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            crate::InGame,
        ))
        .id();
    commands.entity(root).add_child(e);
    e
}

fn creature_body_part(
    commands: &mut Commands,
    root: Entity,
    owner: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    translation: Vec3,
    scale: Vec3,
    phase: f32,
) {
    let e = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(translation).with_scale(scale),
            CreatureBodyPart {
                owner,
                base_translation: translation,
                phase,
            },
            crate::InGame,
        ))
        .id();
    commands.entity(root).add_child(e);
}

fn creature_limb(
    commands: &mut Commands,
    root: Entity,
    owner: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    translation: Vec3,
    rotation: Quat,
    size: Vec3,
    swing_axis: Vec3,
    phase: f32,
    amplitude: f32,
    lift: f32,
) {
    let e = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(translation)
                .with_rotation(rotation)
                .with_scale(size),
            CreatureLimb {
                owner,
                base_translation: translation,
                base_rotation: rotation,
                swing_axis,
                phase,
                amplitude,
                lift,
            },
            crate::InGame,
        ))
        .id();
    commands.entity(root).add_child(e);
}

/// Spawn a modular animal body. The old deer/crab assets are single static
/// meshes, so this gives every visible leg its own transform and animation.
fn spawn_procedural_creature(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
    kind: &str,
    body_hex: u32,
    legs_hex: u32,
    eye_hex: u32,
) {
    let body = materials.add(StandardMaterial {
        base_color: rgb_color(body_hex),
        perceptual_roughness: 0.82,
        metallic: if matches!(kind, "crab" | "beetle") {
            0.2
        } else {
            0.0
        },
        ..default()
    });
    let legs = materials.add(StandardMaterial {
        base_color: rgb_color(legs_hex),
        perceptual_roughness: 0.74,
        metallic: 0.12,
        ..default()
    });
    let eye = materials.add(StandardMaterial {
        base_color: rgb_color(eye_hex),
        emissive: rgb_color(eye_hex).to_linear() * 2.0,
        unlit: true,
        ..default()
    });
    let shell = materials.add(StandardMaterial {
        base_color: rgb_color(body_hex),
        perceptual_roughness: 0.46,
        metallic: 0.42,
        ..default()
    });

    match kind {
        "crab" | "beetle" => {
            creature_body_part(
                commands,
                root,
                root,
                meshes.add(Sphere::new(1.0)),
                body.clone(),
                Vec3::new(0.0, 0.72, 0.0),
                Vec3::new(1.05, 0.5, 0.85),
                0.0,
            );
            creature_body_part(
                commands,
                root,
                root,
                meshes.add(Sphere::new(1.0)),
                shell,
                Vec3::new(0.0, 1.02, 0.08),
                Vec3::new(0.82, 0.3, 0.63),
                0.8,
            );
            for side in [-1.0f32, 1.0] {
                for (i, z) in [-0.58f32, 0.0, 0.58].into_iter().enumerate() {
                    creature_limb(
                        commands,
                        root,
                        root,
                        meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                        legs.clone(),
                        Vec3::new(side * 0.78, 0.38, z),
                        Quat::from_rotation_y(side * 0.82),
                        Vec3::new(0.18, 0.18, 0.68),
                        Vec3::Y,
                        (i as f32 * 1.9)
                            + if side < 0.0 {
                                0.0
                            } else {
                                std::f32::consts::PI
                            },
                        0.45,
                        0.06,
                    );
                }
                creature_part(
                    commands,
                    root,
                    meshes.add(Sphere::new(0.18)),
                    legs.clone(),
                    Transform::from_xyz(side * 0.52, 0.92, -0.76),
                );
                creature_part(
                    commands,
                    root,
                    meshes.add(Sphere::new(0.1)),
                    eye.clone(),
                    Transform::from_xyz(side * 0.32, 1.22, -0.68),
                );
            }
        }
        "manta" => {
            creature_body_part(
                commands,
                root,
                root,
                meshes.add(Sphere::new(1.0)),
                body.clone(),
                Vec3::new(0.0, 0.82, 0.0),
                Vec3::new(1.35, 0.38, 0.8),
                0.0,
            );
            for side in [-1.0f32, 1.0] {
                for (i, z) in [-0.45f32, 0.45].into_iter().enumerate() {
                    creature_limb(
                        commands,
                        root,
                        root,
                        meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                        legs.clone(),
                        Vec3::new(side * 0.94, 0.58, z),
                        Quat::from_rotation_z(side * 0.55),
                        Vec3::new(0.72, 0.14, 0.28),
                        Vec3::Y,
                        (i as f32 * std::f32::consts::PI) + if side < 0.0 { 0.0 } else { 1.4 },
                        0.28,
                        0.04,
                    );
                }
                creature_part(
                    commands,
                    root,
                    meshes.add(Sphere::new(0.1)),
                    eye.clone(),
                    Transform::from_xyz(side * 0.38, 1.05, -0.55),
                );
            }
        }
        "blob" => {
            creature_body_part(
                commands,
                root,
                root,
                meshes.add(Sphere::new(1.0)),
                body.clone(),
                Vec3::new(0.0, 0.78, 0.0),
                Vec3::new(0.9, 0.72, 0.9),
                0.0,
            );
            for (i, (x, z)) in [
                (-0.48f32, -0.42f32),
                (0.48, -0.42),
                (-0.52, 0.38),
                (0.52, 0.38),
            ]
            .into_iter()
            .enumerate()
            {
                creature_limb(
                    commands,
                    root,
                    root,
                    meshes.add(Sphere::new(1.0)),
                    legs.clone(),
                    Vec3::new(x, 0.34, z),
                    Quat::IDENTITY,
                    Vec3::splat(0.28),
                    Vec3::X,
                    i as f32 * 1.57,
                    0.25,
                    0.08,
                );
            }
            for side in [-1.0f32, 1.0] {
                creature_part(
                    commands,
                    root,
                    meshes.add(Sphere::new(0.1)),
                    eye.clone(),
                    Transform::from_xyz(side * 0.3, 1.0, -0.7),
                );
            }
        }
        _ => {
            // Strider / hopper：四足身体，脚相位交错，移动时形成真正的对角步态。
            let body_scale = if kind == "hopper" {
                Vec3::new(0.82, 0.72, 1.05)
            } else {
                Vec3::new(0.72, 0.92, 0.82)
            };
            creature_body_part(
                commands,
                root,
                root,
                meshes.add(Sphere::new(1.0)),
                body.clone(),
                Vec3::new(0.0, 0.98, 0.0),
                body_scale,
                0.0,
            );
            creature_body_part(
                commands,
                root,
                root,
                meshes.add(Sphere::new(1.0)),
                shell,
                Vec3::new(0.0, 1.12, -0.66),
                Vec3::new(0.45, 0.48, 0.48),
                0.7,
            );
            for side in [-1.0f32, 1.0] {
                for (i, z) in [-0.48f32, 0.48].into_iter().enumerate() {
                    creature_limb(
                        commands,
                        root,
                        root,
                        meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                        legs.clone(),
                        Vec3::new(side * 0.5, 0.38, z),
                        Quat::from_rotation_z(side * 0.12),
                        Vec3::new(0.2, if kind == "hopper" { 0.95 } else { 1.15 }, 0.2),
                        Vec3::X,
                        (i as f32 * std::f32::consts::PI)
                            + if side < 0.0 {
                                0.0
                            } else {
                                std::f32::consts::PI
                            },
                        0.42,
                        0.1,
                    );
                }
                creature_part(
                    commands,
                    root,
                    meshes.add(Sphere::new(0.1)),
                    eye.clone(),
                    Transform::from_xyz(side * 0.2, 1.38, -0.96),
                );
            }
        }
    }
}

fn ease_in_out(t: f32) -> f32 {
    if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else {
        t * t * (3.0 - 2.0 * t)
    }
}

/// 注册一个候选细胞：出生点与行为参数全部确定性（JS registerCell 同序 RNG 消耗）。
/// 世界生成式兽群掷骰：掷中即产生兽群记录（永久占据名额并计入密度）。
fn register_cell(spawner: &mut CreatureSpawner, world: &World, cx: i32, cz: i32, count: i32) {
    let key = (cx, cz);
    if spawner.cells.contains_key(&key) {
        return;
    }
    let mut rnd = Rng::new(crate::rng::batch_seed(world.seed, cx, cz));
    let ccx = cx as f32 * CRE_CELL + CRE_CELL / 2.0;
    let ccz = cz as f32 * CRE_CELL + CRE_CELL / 2.0;
    let mut cands: Vec<Cand> = Vec::new();
    for _ in 0..count.min(22) {
        // 出生点：细胞中心 12~92 格随机环（确定性），避开水体与树木顶端
        let mut wx = 0.0f32;
        let mut wz = 0.0f32;
        let mut ok = false;
        for _ in 0..8 {
            let ang = rnd.next() * std::f32::consts::TAU;
            let dist = 12.0 + rnd.next() * 80.0;
            wx = ccx + ang.cos() * dist;
            wz = ccz + ang.sin() * dist;
            let ix = wx.floor() as i32;
            let iz = wz.floor() as i32;
            ok = world.creature_ground_at(ix, iz).is_some();
            if ok {
                break;
            }
        }
        // 行为参数（RNG 消耗顺序与 JS 一致）
        let speed = 1.8 * (0.5 + rnd.next());
        let dir = rnd.next() * std::f32::consts::TAU;
        let timer = 1.0 + rnd.next() * 3.0;
        let anim_t = rnd.next() * 10.0;
        if ok {
            cands.push(Cand {
                x: wx,
                z: wz,
                speed,
                dir,
                timer,
                anim_t,
            });
        }
    }
    let roll = rnd.next();
    let herd_roll = !cands.is_empty() && roll < HERD_CHANCE;
    let herd_idx = if cands.is_empty() {
        0
    } else {
        rnd.range(cands.len())
    };
    let cands = if herd_roll {
        // 保留候选供兽群引用（避免 move 后借用冲突）
        cands
    } else {
        cands
    };
    spawner.cells.insert(
        key,
        CellState {
            cands,
            mask: 0,
            initialized: true,
        },
    );
    if herd_roll {
        let nid = crate::rng::batch_seed(world.seed, cx, cz) as u64 * 64 + herd_idx as u64;
        if !spawner.herds.contains_key(&nid) {
            let c = spawner.cells[&key].cands[herd_idx].clone();
            spawner.herds.insert(
                nid,
                Herd {
                    nid,
                    cx,
                    cz,
                    cand: herd_idx,
                    x: c.x,
                    z: c.z,
                    hp: 4.0,
                    home_x: c.x,
                    home_z: c.z,
                    speed: c.speed,
                    dir: c.dir,
                    timer: c.timer,
                    anim_t: c.anim_t,
                    entity: None,
                },
            );
        }
    }
}

/// 物化兽群（首次生成或卸载后重载）：重校验地形，淡入出场，保留血量。
#[allow(clippy::too_many_arguments)]
fn materialize_herd(
    spawner: &mut CreatureSpawner,
    nid: u64,
    commands: &mut Commands,
    asset_server: &AssetServer,
    graphs: &mut Assets<AnimationGraph>,
    library: &mut CreatureAnimationLibrary,
    world: &World,
) -> bool {
    let Some(h) = spawner.herds.get(&nid) else {
        return false;
    };
    // 物化通过 Commands 延迟写入 ECS，但 Herd.entity 会立即记录返回的
    // Entity。这个幂等保护覆盖同一帧的补足/重载路径，杜绝重复兽群实体。
    if h.entity.is_some() {
        return false;
    }
    let h = h.clone();
    // 地形被破坏（水/树）→ 保持休眠，等恢复
    let ix = h.x.floor() as i32;
    let iz = h.z.floor() as i32;
    let Some(gy) = world.creature_ground_at(ix, iz) else {
        return false;
    };
    let kind = data::biome_animal_kind(world.biome().key);
    let (model, scale, y_off) = creature_model(kind);
    // 命中盒按新模型的统一目标高度估算，避免旧静态网格的尺寸参数残留。
    let hh = match kind {
        "crab" | "beetle" => 1.95,
        "blob" | "manta" => 1.75,
        _ => 2.2,
    };
    let animation = creature_animation_setup(kind, asset_server, graphs, library);
    let e = commands
        .spawn((
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(model))),
            Transform::from_translation(Vec3::new(h.x, gy as f32 + 1.0 + y_off, h.z))
                .with_scale(Vec3::splat(0.001)), // 淡入起点
            Visibility::default(),
            Creature {
                hp: h.hp.max(1.0),
                radius: 0.6,
                height: hh + 0.3,
                shoot_t: 0.0,
                ai_t: h.timer,
                dir: Vec3::new(h.dir.cos(), 0.0, h.dir.sin()),
                vel: Vec3::ZERO,
                grounded: false,
                home: Vec3::new(h.home_x, gy as f32 + 1.0, h.home_z),
                jump_t: 0.0,
                kind,
                scale,
                foot: y_off,
                nid: Some(nid),
                speed: species_speed(kind) * h.speed,
                walking: false, // JS 出生先休息（timer = 1~4s）再开始散步
                anim_t: h.anim_t,
                spawn_t: 0.0,
                fading: false,
                fade_t: 0.0,
                hit_t: 0.0,
                aggro_t: 0.0,
                attack_cd: 0.0,
            },
            crate::InGame,
        ))
        .id();
    if let Some(animation) = animation {
        commands
            .entity(e)
            .insert(animation)
            .observe(creature_animation_ready);
    }
    if let Some(hh) = spawner.herds.get_mut(&nid) {
        hh.entity = Some(e);
    }
    println!(
        "CREATURE materialize herd {nid} kind={kind} at ({:.1},{:.1})",
        h.x, h.z
    );
    true
}

/// Minecraft 风格兽群维护：注册候选细胞 → 周期补足密度 → 卸载休眠/重载。
#[allow(clippy::too_many_arguments)]
pub fn creature_spawn_system(
    time: Res<Time>,
    mut spawner: ResMut<CreatureSpawner>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut animation_library: ResMut<CreatureAnimationLibrary>,
    mut creatures: Query<(Entity, &mut Creature, &Transform)>,
    world: Res<World>,
    player: Query<&Player>,
) {
    let dt = time.delta_secs();
    let Ok(p) = player.single() else { return };
    let Some(animal) = world.biome().animal else {
        return;
    };
    let pcx = (p.pos.x / CRE_CELL).floor() as i32;
    let pcz = (p.pos.z / CRE_CELL).floor() as i32;
    // 1. 注册玩家周边 BUCKET_R 格内的候选细胞（确定性；已注册的跳过）
    for dx in -BUCKET_R..=BUCKET_R {
        for dz in -BUCKET_R..=BUCKET_R {
            let (cx, cz) = (pcx + dx, pcz + dz);
            let needs_init = spawner
                .cells
                .get(&(cx, cz))
                .map(|cell| !cell.initialized)
                .unwrap_or(true);
            if needs_init {
                let saved_mask = spawner
                    .cells
                    .remove(&(cx, cz))
                    .map(|cell| cell.mask)
                    .unwrap_or(0);
                register_cell(&mut spawner, &world, cx, cz, animal.4);
                if saved_mask != 0
                    && let Some(cell) = spawner.cells.get_mut(&(cx, cz))
                {
                    cell.mask = saved_mask;
                }
            }
        }
    }
    // 2. 周期补足（1.2s）：活跃数低于目标密度时，物化最近的 24~128m 内休眠兽群
    spawner.timer -= dt;
    // Commands::spawn 在本系统返回后才会进入 Query；记录本帧刚提交的兽群，
    // 避免下面的句柄校验把它误判为“实体已丢失”并重复物化。
    let mut spawned_this_tick: Vec<u64> = Vec::new();
    if spawner.timer <= 0.0 {
        spawner.timer = SPAWN_INTERVAL;
        let active = creatures.iter().filter(|(_, c, _)| !c.fading).count();
        println!(
            "CREATURE tick player=({:.0},{:.0}) active={active} herds={}",
            p.pos.x,
            p.pos.z,
            spawner.herds.len()
        );
        if active < TARGET_DENSITY.min(CRE_CAP) {
            let mut best: Option<(f32, u64)> = None;
            for (nid, h) in &spawner.herds {
                if h.entity.is_some() {
                    continue;
                }
                let d = ((h.x - p.pos.x).powi(2) + (h.z - p.pos.z).powi(2)).sqrt();
                if (SPAWN_MIN..=SPAWN_MAX).contains(&d)
                    && best.map(|(bd, _)| d < bd).unwrap_or(true)
                {
                    best = Some((d, *nid));
                }
            }
            if let Some((d, nid)) = best {
                println!("CREATURE topup active={active} spawn herd {nid} d={d:.0}");
                if materialize_herd(
                    &mut spawner,
                    nid,
                    &mut commands,
                    &asset_server,
                    &mut graphs,
                    &mut animation_library,
                    &world,
                ) {
                    spawned_this_tick.push(nid);
                }
            }
        }
    }

    // 3. 卸载休眠（>128m：回写位置/血量并淡出；实体淡出完成后由 despawn 系统清空 entity）/ 重载（<96m）
    let mut to_reload: Vec<u64> = Vec::new();
    for (nid, h) in &mut spawner.herds {
        if spawned_this_tick.contains(nid) {
            continue;
        }
        let d = ((h.x - p.pos.x).powi(2) + (h.z - p.pos.z).powi(2)).sqrt();
        if let Some(e) = h.entity {
            match creatures.get_mut(e) {
                Ok((_, mut c, tf)) => {
                    if d < RELOAD_D && c.fading {
                        // 高速穿越时玩家可能在淡出完成前折返；取消卸载，
                        // 否则生物会在眼前消失且要等下一轮生成才能回来。
                        c.fading = false;
                        c.fade_t = 0.0;
                    } else if d > UNLOAD_D && !c.fading {
                        h.x = tf.translation.x;
                        h.z = tf.translation.z;
                        h.hp = c.hp;
                        c.fading = true; // 淡出 0.8s 后卸载（数据已回写）
                        c.fade_t = 0.0;
                        println!("CREATURE unload herd {nid} d={d:.0} (fade-out)");
                    }
                }
                Err(_) => {
                    // 场景切换/区块快速流式时实体可能已被命令队列删除。
                    // 清掉僵尸 Entity 句柄，否则该兽群会永久被认为“已物化”，
                    // 这正是生物偶发消失后不再刷新的根因。
                    h.entity = None;
                    if d < RELOAD_D {
                        to_reload.push(*nid);
                    }
                }
            }
        } else if d < RELOAD_D {
            to_reload.push(*nid);
        }
    }
    for nid in to_reload {
        materialize_herd(
            &mut spawner,
            nid,
            &mut commands,
            &asset_server,
            &mut graphs,
            &mut animation_library,
            &world,
        );
    }
}

/// Creature AI: 游荡 / 跳跃 / 淡入淡出 / 行走动画（Minecraft 风格：不因距离消失，由兽群系统卸载休眠）。
pub fn creature_system(
    time: Res<Time>,
    mut q: Query<(&mut Creature, &mut Transform)>,
    world: Res<World>,
    mut player: Query<&mut Player>,
) {
    let dt = time.delta_secs();
    let Ok(mut p) = player.single_mut() else {
        return;
    };
    let player_pos = p.pos;
    for (mut c, mut tf) in &mut q {
        if c.hp <= 0.0 {
            continue;
        }
        // 守卫由 sentinel_system 单独驱动
        if c.kind == "sentinel" {
            continue;
        }
        // 淡入 / 淡出计时
        c.spawn_t = (c.spawn_t + dt).min(FADE_IN_T + 1.0);
        if c.fading {
            c.fade_t += dt;
            if c.fade_t >= FADE_OUT_T {
                c.hp = -1.0; // 由 despawn 系统收尾（不视为击杀）
                continue;
            }
        }
        c.hit_t = (c.hit_t - dt).max(0.0);
        c.aggro_t = (c.aggro_t - dt).max(0.0);
        c.attack_cd = (c.attack_cd - dt).max(0.0);
        // 散步/休息状态机（JS tickOne 同口径）：walk 2~7s → idle 1.5~4.5s 循环，
        // 每次开始散步只做小角度转向（±0.75 rad），不再每 1~4s 乱转
        c.ai_t -= dt;
        if c.ai_t <= 0.0 {
            let mut rng =
                Rng::new((tf.translation.x as u32).wrapping_mul(31) + time.elapsed_secs() as u32);
            if c.walking {
                c.walking = false;
                c.ai_t = 1.5 + rng.next() * 3.0; // 休息
                c.vel = Vec3::ZERO;
            } else {
                c.walking = true;
                c.ai_t = 2.0 + rng.next() * 5.0; // 散步
                let turn = (rng.next() - 0.5) * 1.5;
                let yaw = c.dir.x.atan2(c.dir.z) + turn;
                c.dir = Vec3::new(yaw.cos(), 0.0, yaw.sin());
                c.vel = c.dir * c.speed;
                // strider 跳跃：低概率、初速 4.6（约 0.5 格，避免过高）
                if c.kind == "strider" && rng.next() < 0.05 {
                    c.vel.y = 4.6;
                }
            }
        }
        let mut pos = tf.translation;
        let current_ground = world.creature_ground_at(pos.x.floor() as i32, pos.z.floor() as i32);
        let mut ground = current_ground;
        if c.walking {
            // Horizontal movement is accepted only when the destination is a
            // loaded, dry, non-tree surface and the step is small enough.  The
            // old code sampled `top_at` after moving, so a tree canopy or a
            // water column became an instant elevator.
            let candidate = pos + Vec3::new(c.vel.x * dt, 0.0, c.vel.z * dt);
            let next_ground =
                world.creature_ground_at(candidate.x.floor() as i32, candidate.z.floor() as i32);
            let step_ok = match (current_ground, next_ground) {
                (Some(from), Some(to)) => (to - from).abs() <= 1,
                _ => false,
            };
            if step_ok {
                pos.x = candidate.x;
                pos.z = candidate.z;
                ground = next_ground;
            } else {
                // Turn away from an impassable column without changing the
                // creature's vertical state.  A short cooldown avoids
                // repeatedly pushing into the same tree/water edge.
                c.dir = -c.dir;
                c.vel.x = c.dir.x * c.speed;
                c.vel.z = c.dir.z * c.speed;
                c.ai_t = c.ai_t.min(0.35);
            }
        }
        let player_delta = player_pos - pos;
        let player_dist = player_delta.xz().length();
        if c.aggro_t > 0.0 && matches!(c.kind, "crab" | "beetle" | "hopper") {
            c.walking = true;
            c.dir = Vec3::new(player_delta.x, 0.0, player_delta.z).normalize_or_zero();
            c.vel.x = c.dir.x * c.speed * 1.8;
            c.vel.z = c.dir.z * c.speed * 1.8;
            if player_dist < 1.5 && c.attack_cd <= 0.0 {
                c.attack_cd = 1.2;
                p.damage(if c.kind == "beetle" { 2.0 } else { 1.0 });
            }
        } else if matches!(c.kind, "manta" | "blob") && player_dist < 4.0 {
            // Timid fauna flees rather than sharing the same passive wander AI.
            c.walking = true;
            let away = pos - player_pos;
            c.dir = Vec3::new(away.x, 0.0, away.z).normalize_or_zero();
            c.vel.x = c.dir.x * c.speed * 1.5;
            c.vel.z = c.dir.z * c.speed * 1.5;
        }
        // home 领地（JS 野生生物 26 格外折返）
        if (pos - c.home).xz().length() > 26.0 {
            c.dir = (c.home - pos).normalize_or_zero();
            c.vel = c.dir * c.speed * 0.8;
        }
        if !c.grounded {
            c.vel.y -= 22.0 * dt;
        }
        pos += Vec3::Y * c.vel.y * dt;
        // 贴地：原点 = 地面 + 1 + foot（脚底正好在方块顶面）——
        // 修复旧实现把原点钳到 地面+1，导致 origin 在脚底上方 0.5+ 格的模型（鹿/蟹）半身陷入地下
        if ground.is_none() {
            ground = world.creature_ground_at(pos.x.floor() as i32, pos.z.floor() as i32);
        }
        let Some(ground) = ground else {
            // The streaming ring is still catching up.  Do not snap to an
            // unloaded column (which would otherwise look like a terrain
            // teleport); hold position until the real chunk is available.
            pos.x = tf.translation.x;
            pos.z = tf.translation.z;
            c.vel.y = c.vel.y.min(0.0);
            tf.translation = pos;
            continue;
        };
        let floor_y = ground as f32 + 1.0 + c.foot;
        if pos.y <= floor_y + 0.01 {
            pos.y = floor_y + 0.01;
            c.vel.y = 0.0;
            c.grounded = true;
        } else {
            c.grounded = false;
        }
        // avoid walking into player
        if (pos - player_pos).xz().length() < 1.0 && (pos.y - player_pos.y).abs() < 2.0 {
            pos -= c.dir * dt * 2.0;
        }
        tf.translation = pos;
        // 朝向 + 动画（休息时无行走摆动，仅呼吸）
        let moving = c.walking && c.vel.xz().length_squared() > 0.01;
        if moving {
            let yaw = c.vel.x.atan2(c.vel.z);
            c.anim_t += dt * 2.0;
            if c.kind == "blob" {
                // 史莱姆：弹跳移动
                tf.translation.y += (c.anim_t * 6.0).sin().abs() * 0.1;
                tf.rotation = Quat::from_rotation_y(yaw);
            } else {
                // The glTF walk clip already animates the skeleton.  Tilting
                // the gameplay root here as well makes the whole body sway
                // twice, which is especially noticeable on deer/wolves.
                tf.rotation = Quat::from_rotation_y(yaw);
            }
        } else {
            c.anim_t += dt;
            let yaw = c.dir.x.atan2(c.dir.z);
            tf.rotation = Quat::from_rotation_y(yaw);
        }
        // 合成缩放：基准 × 淡入 × 呼吸 × 受击脉冲 × 淡出（全部乘性，不覆盖基准）
        let fade_in = ease_in_out((c.spawn_t / FADE_IN_T).min(1.0));
        let fade_out = if c.fading {
            (1.0 - ease_in_out((c.fade_t / FADE_OUT_T).min(1.0))).max(0.001)
        } else {
            1.0
        };
        // Keep a subtle idle pulse, but never scale a walking skeleton on the
        // gameplay root: the clip owns its own body motion.
        let breath = if moving {
            1.0
        } else {
            1.0 + (c.anim_t * 3.0).sin() * 0.015
        };
        let hit = if c.hit_t > 0.0 {
            1.0 + (c.hit_t / 0.25) * 0.18
        } else {
            1.0
        };
        tf.scale = Vec3::splat((c.scale * fade_in * breath * hit * fade_out).max(0.001));
    }
}

/// Low-volume ambient calls keep the wildlife space feeling inhabited without
/// attaching a separate audio source to every animal.
pub fn creature_sound_system(
    time: Res<Time>,
    mut commands: Commands,
    creatures: Query<(&Creature, &Transform)>,
    sfx: Res<crate::audio::Sfx>,
    mut next_call: Local<f32>,
) {
    let dt = time.delta_secs();
    *next_call -= dt;
    let Some(position) = creatures
        .iter()
        .find_map(|(c, tf)| (c.hp > 0.0 && c.walking).then_some(tf.translation))
    else {
        return;
    };
    if *next_call > 0.0 {
        return;
    }
    *next_call = 5.0 + (time.elapsed_secs().sin().abs() * 4.0);
    crate::audio::play_spatial(
        &mut commands,
        sfx.creature_hit.clone(),
        position,
        0.08,
        Some(0.78 + time.elapsed_secs().cos().abs() * 0.34),
    );
}

/// Despawn dead creatures（击杀 → 掉落 + 兽群标记不复活；淡出完成 → 仅卸载休眠）。
pub fn creature_despawn_system(
    creatures: Query<(Entity, &Creature, &Transform)>,
    mut spawner: ResMut<CreatureSpawner>,
    mut commands: Commands,
    world: Res<World>,
    icons: Res<crate::ui::IconMaterials>,
    sfx: Res<crate::audio::Sfx>,
) {
    for (e, c, tf) in &creatures {
        if c.hp > 0.0 {
            continue;
        }
        // 淡出完成（卸载休眠）：兽群保留，可重载；无掉落
        if c.fading {
            if let Some(nid) = c.nid {
                if let Some(h) = spawner.herds.get_mut(&nid) {
                    h.entity = None;
                }
                println!("CREATURE fade-done herd {nid} dormant");
            }
            commands.entity(e).despawn();
            continue;
        }
        let mut rng = crate::rng::Rng::new(
            (tf.translation.x as u32).wrapping_mul(31) ^ (tf.translation.z as u32).wrapping_mul(57),
        );
        if c.kind == "sentinel" {
            // 遗迹守卫（JS）：电路板×1 + 装甲板×1(50%)
            spawn_drop(
                &mut commands,
                &world,
                &icons,
                tf.translation + Vec3::Y * 0.4,
                Vec3::new(0.0, 2.2, 0.0),
                "circuit".into(),
                1,
                0.4,
            );
            if rng.next() < 0.5 {
                spawn_drop(
                    &mut commands,
                    &world,
                    &icons,
                    tf.translation + Vec3::Y * 0.8,
                    Vec3::new(0.0, 2.2, 0.0),
                    "plate".into(),
                    1,
                    0.4,
                );
            }
            crate::audio::play_spatial(
                &mut commands,
                sfx.break_block.clone(),
                tf.translation,
                0.7,
                None,
            );
        } else {
            let n = 1 + (rng.next() * 2.0) as i32;
            spawn_drop(
                &mut commands,
                &world,
                &icons,
                tf.translation + Vec3::Y * 0.4,
                Vec3::new(0.0, 2.2, 0.0),
                "carbon".into(),
                n,
                0.4,
            );
            let biome_loot = match c.kind {
                "hopper" => Some(("spores", 1 + (rng.next() * 2.0) as i32)),
                "crab" => Some(("chitin", 1 + (rng.next() * 2.0) as i32)),
                "beetle" => Some(("chitin", 2 + (rng.next() * 2.0) as i32)),
                "manta" => Some(("cryocrystal", 1)),
                "blob" => Some(("enzyme", 1)),
                "strider" if rng.next() < 0.45 => Some(("resin", 1)),
                _ => None,
            };
            if let Some((item, amount)) = biome_loot {
                spawn_drop(
                    &mut commands,
                    &world,
                    &icons,
                    tf.translation + Vec3::new(0.25, 0.8, 0.0),
                    Vec3::new(0.5, 2.4, 0.0),
                    item.into(),
                    amount,
                    0.4,
                );
            }
            crate::audio::play_spatial(
                &mut commands,
                sfx.creature_die.clone(),
                tf.translation,
                0.5,
                None,
            );
            // 击杀：细胞掩码记录永久消失（读档不重生）+ 从兽群表移除（补员/重载绝不复活）
            if let Some(nid) = c.nid {
                println!("CREATURE kill herd {nid} (permanent, mask set)");
                if let Some(h) = spawner.herds.get(&nid) {
                    let (cx, cz, cand) = (h.cx, h.cz, h.cand);
                    if let Some(st) = spawner.cells.get_mut(&(cx, cz)) {
                        st.mask |= 1 << cand;
                    }
                }
                spawner.herds.remove(&nid);
            }
        }
        commands.entity(e).despawn();
    }
}

/// 遗迹守卫生成计时。
#[derive(Resource, Default)]
pub struct SentinelSpawner {
    pub timer: f32,
}

/// 遗迹守卫（JS sentinel）：遗迹附近生成；16 格内追击玩家、接触伤害 2（1.15s CD）；
/// 远离 40 格后消失。生成在 world.g.structures 的 Ruin 处。
#[allow(clippy::too_many_arguments)]
pub fn sentinel_system(
    time: Res<Time>,
    mut spawner: ResMut<SentinelSpawner>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut animation_library: ResMut<CreatureAnimationLibrary>,
    world: Res<World>,
    mut player: Query<&mut Player>,
    mut creatures: Query<(Entity, &mut Creature, &mut Transform)>,
    mut dmg_cd: Local<f32>,
) {
    let dt = time.delta_secs();
    let ppos = player.single().map(|p| p.pos).unwrap_or(Vec3::ZERO);
    // 生成
    spawner.timer -= dt;
    if spawner.timer <= 0.0 {
        spawner.timer = 2.0;
        let mut nearest: Option<([i32; 3], f32)> = None;
        let (px, pz) = (ppos.x as i32, ppos.z as i32);
        for s in world
            .g
            .structures_in_rect(px - 48, pz - 48, px + 48, pz + 48)
        {
            if let crate::world::Structure::Ruin { x, z, .. } = s {
                let dx = ppos.x - x as f32;
                let dz = ppos.z - z as f32;
                let d = (dx * dx + dz * dz).sqrt();
                if d < 40.0 && nearest.map(|(_, bd)| d < bd).unwrap_or(true) {
                    nearest = Some(([x, 0, z], d));
                }
            }
        }
        if let Some((cell, _)) = nearest {
            let has = creatures.iter().any(|(_, c, tf)| {
                c.kind == "sentinel"
                    && tf
                        .translation
                        .distance(Vec3::new(cell[0] as f32, 0.0, cell[2] as f32))
                        < 30.0
            });
            if !has {
                let x = cell[0];
                let z = cell[2];
                let Some(top) = world.creature_ground_at(x, z) else {
                    return;
                };
                let animation = creature_animation_setup(
                    "sentinel",
                    &asset_server,
                    &mut graphs,
                    &mut animation_library,
                );
                let entity = commands
                    .spawn((
                        WorldAssetRoot(asset_server.load(
                            GltfAssetLabel::Scene(0).from_asset("models/creatures/sentinel.glb"),
                        )),
                        // 骷髅实测高 2.17，目标 1.9 格（脚底即模型原点，无需偏移）
                        Transform::from_translation(Vec3::new(
                            x as f32 + 0.5,
                            top as f32 + 1.0,
                            z as f32 + 0.5,
                        ))
                        .with_scale(Vec3::splat(1.9 / 2.17)),
                        Creature {
                            hp: 10.0,
                            radius: 0.6,
                            height: 1.8,
                            shoot_t: 0.0,
                            ai_t: 0.0,
                            dir: Vec3::X,
                            vel: Vec3::ZERO,
                            grounded: true,
                            home: Vec3::new(x as f32 + 0.5, top as f32 + 1.0, z as f32 + 0.5),
                            jump_t: 0.0,
                            kind: "sentinel",
                            scale: 1.9 / 2.17,
                            foot: 0.0,
                            nid: None,
                            speed: 2.4,
                            walking: true,
                            anim_t: 0.0,
                            spawn_t: 1.0,
                            fading: false,
                            fade_t: 0.0,
                            hit_t: 0.0,
                            aggro_t: 0.0,
                            attack_cd: 0.0,
                        },
                        crate::InGame,
                    ))
                    .id();
                if let Some(animation) = animation {
                    commands
                        .entity(entity)
                        .insert(animation)
                        .observe(creature_animation_ready);
                }
            }
        }
    }
    // 守卫 AI：追击 + 接触伤害 + 远离消失
    for (_e, mut c, mut tf) in &mut creatures {
        if c.kind != "sentinel" {
            continue;
        }
        let dist = tf.translation.distance(ppos);
        if dist < 16.0 {
            let dir = (ppos - tf.translation).normalize_or_zero();
            let next = tf.translation + dir * 4.7 * dt; // speed 1.8 × 2.6 追击
            let from_ground = world.creature_ground_at(
                tf.translation.x.floor() as i32,
                tf.translation.z.floor() as i32,
            );
            let to_ground = world.creature_ground_at(next.x.floor() as i32, next.z.floor() as i32);
            if let (Some(from), Some(to)) = (from_ground, to_ground)
                && (to - from).abs() <= 1
            {
                tf.translation.x = next.x;
                tf.translation.z = next.z;
                tf.translation.y = to as f32 + 1.0;
            }
            let yaw = dir.x.atan2(dir.z);
            tf.rotation = Quat::from_rotation_y(yaw);
            if dist < 1.9 {
                *dmg_cd -= dt;
                if *dmg_cd <= 0.0 {
                    *dmg_cd = 1.15;
                    if let Ok(mut pp) = player.single_mut() {
                        pp.damage(2.0);
                    }
                }
            }
        } else if dist > 40.0 {
            c.hp = -1.0; // 标记消失
        }
    }
}

// ---------- Dropped items ----------

#[derive(Component)]
pub struct DropItem {
    pub item: String,
    pub n: i32,
    pub age: f32,
    pub vel: Vec3,
    pub pick_delay: f32,
    pub base_y: f32,
    pub resting: bool,
    pub no_space_t: f32,
}

pub const DROP_CAP: usize = 90;

/// Spawn a dropped item.
pub fn spawn_drop(
    commands: &mut Commands,
    world: &World,
    icon_materials: &IconMaterials,
    pos: Vec3,
    vel: Vec3,
    item: String,
    n: i32,
    pick_delay: f32,
) {
    if n <= 0 {
        return;
    }
    let mat = icon_materials
        .map
        .get(&item)
        .cloned()
        .unwrap_or_else(|| icon_materials.fallback.clone());
    let ground = world.top_at(pos.x.floor() as i32, pos.z.floor() as i32);
    commands.spawn((
        Mesh3d(icon_materials.quad.clone()),
        MeshMaterial3d(mat),
        Transform::from_translation(pos),
        Visibility::default(),
        DropItem {
            item,
            n,
            age: 0.0,
            vel,
            pick_delay,
            base_y: ground as f32 + 1.0 + 0.3,
            resting: false,
            no_space_t: 0.0,
        },
        crate::InGame,
    ));
}

/// Drop physics: gravity, landing, magnet pickup, despawn, merge & cap.
pub fn drops_system(
    time: Res<Time>,
    mut commands: Commands,
    mut drops: Query<(Entity, &mut DropItem, &mut Transform)>,
    mut player: Query<&mut Player>,
    world: Res<World>,
    sfx: Res<crate::audio::Sfx>,
) {
    let dt = time.delta_secs();
    let Ok(mut p) = player.single_mut() else {
        return;
    };
    let player_chest = p.pos - Vec3::Y * 1.0;
    let mut pickup_sound = false;
    let all: Vec<Entity> = drops.iter().map(|(e, _, _)| e).collect();
    let mut snap: Vec<(Entity, DropItem, Vec3)> = Vec::new();
    for e in &all {
        if let Ok((_, d, tf)) = drops.get(*e) {
            snap.push((
                *e,
                DropItem {
                    item: d.item.clone(),
                    n: d.n,
                    age: d.age,
                    vel: d.vel,
                    pick_delay: d.pick_delay,
                    base_y: d.base_y,
                    resting: d.resting,
                    no_space_t: d.no_space_t,
                },
                tf.translation,
            ));
        }
    }
    // 同类合并（JS: dist²<1.2 合并，n 相加、age 重置）
    let mut merged: Vec<usize> = Vec::new();
    for i in 0..snap.len() {
        if merged.contains(&i) {
            continue;
        }
        for j in (i + 1)..snap.len() {
            if merged.contains(&j) {
                continue;
            }
            let (_, di, pi) = &snap[i];
            let (_, dj, pj) = &snap[j];
            if di.item == dj.item && di.pick_delay <= 0.0 && dj.pick_delay <= 0.0 {
                let d2 = (pi.x - pj.x).powi(2) + (pi.y - pj.y).powi(2) + (pi.z - pj.z).powi(2);
                if d2 < 1.44 {
                    snap[i].1.n += dj.n;
                    snap[i].1.age = 0.0;
                    commands.entity(snap[j].0).despawn();
                    merged.push(j);
                }
            }
        }
    }
    // 掉落上限（JS DROP_CAP 90：超限最旧入包）
    let surviving_count = snap.len().saturating_sub(merged.len());
    if surviving_count > DROP_CAP {
        // Entries already merged into an earlier drop are pending despawn.
        // Counting/collecting one of those entries again can duplicate its
        // items because the surviving entry already includes its quantity.
        let mut order: Vec<usize> = (0..snap.len())
            .filter(|idx| !merged.contains(idx))
            .collect();
        order.sort_by(|a, b| {
            snap[*b]
                .1
                .age
                .partial_cmp(&snap[*a].1.age)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &idx in order.iter().take(surviving_count - DROP_CAP) {
            let (e, d, _) = &snap[idx];
            let added = p.inv.add_item(&d.item, d.n);
            if added >= d.n {
                commands.entity(*e).despawn();
            } else if added > 0
                && let Ok((_, mut dd, _)) = drops.get_mut(*e)
            {
                dd.n -= added;
            }
        }
    }
    // 合并结果写回实体（幸存者数量/年龄）
    for (e, d, _) in &snap {
        if let Ok((_, mut dd, _)) = drops.get_mut(*e) {
            dd.n = d.n;
            dd.age = d.age;
        }
    }
    for (e, mut d, mut tf) in &mut drops {
        d.age += dt;
        if d.age > 240.0 {
            commands.entity(e).despawn();
            continue;
        }
        if d.vel.length_squared() > 0.0001 {
            d.vel.y -= 16.0 * dt;
            let mut np = tf.translation + d.vel * dt;
            // land
            let below = data::block_by_id(world.get(
                np.x.floor() as i32,
                (np.y - 0.28).floor() as i32,
                np.z.floor() as i32,
            ));
            if below.solid {
                let fy = (np.y - 0.28).floor();
                d.base_y = fy + 1.0 + 0.3;
                np.y = d.base_y;
                d.vel = Vec3::ZERO;
                d.resting = true;
            }
            if np.y < -8.0 {
                let top = world.top_at(np.x.floor() as i32, np.z.floor() as i32);
                np.y = top as f32 + 0.4;
                d.vel = Vec3::ZERO;
                d.resting = true;
                d.base_y = np.y;
            }
            tf.translation = np;
        }
        if d.resting {
            // re-fall if support removed
            let below = world.get(
                tf.translation.x.floor() as i32,
                (d.base_y - 0.4).floor() as i32,
                tf.translation.z.floor() as i32,
            );
            if !data::block_by_id(below).solid {
                d.vel.y = -0.5;
                d.resting = false;
            } else {
                let bob = d.base_y + (d.age * 2.2).sin() * 0.06 + 0.06;
                tf.translation.y = bob;
            }
        }
        // spin + billboard toward player
        tf.rotate_y(dt * 1.6);
        // pickup
        if d.age > d.pick_delay && d.no_space_t <= 0.0 {
            let dist = tf.translation.distance(player_chest);
            if dist < 6.5 {
                let room = p.inv.room_for(&d.item);
                if room <= 0 {
                    d.no_space_t = 1.5;
                } else if dist > 1.05 {
                    let dir = (player_chest - tf.translation).normalize();
                    let spd = (8.0 + (6.5 - dist) * 4.0).min(26.0);
                    tf.translation += dir * spd * dt;
                    d.resting = false;
                } else {
                    let take = d.n.min(room);
                    let added = p.inv.add_item(&d.item, take);
                    d.n -= added;
                    if d.n <= 0 {
                        commands.entity(e).despawn();
                        pickup_sound = true;
                        continue;
                    }
                }
            }
        } else if d.no_space_t > 0.0 {
            d.no_space_t -= dt;
        }
    }
    if pickup_sound {
        crate::audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
    }
    let _ = &mut p;
}

/// Empty helper for callers that need a Slot (keeps API symmetry).
pub fn slot(item: &str, n: i32) -> Slot {
    Slot {
        item: item.to_string(),
        n,
    }
}

// ---------- Plugin ----------

/// Creatures plugin: herd spawner/AI, drops, sentinel spawner and animation cache.
pub struct CreaturesPlugin;

impl Plugin for CreaturesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatureAnimationLibrary>()
            .init_resource::<SentinelSpawner>()
            .add_systems(
                Update,
                (
                    creature_spawn_system.run_if(creature_mode),
                    creature_system.run_if(creature_mode),
                    creature_sound_system.run_if(creature_mode),
                    creature_animation_system.run_if(creature_mode),
                    sentinel_system.run_if(creature_mode),
                )
                    .chain()
                    .in_set(GameSet::GroundCreatures)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                (
                    creature_despawn_system.run_if(creature_mode),
                    drops_system.run_if(creature_mode),
                )
                    .chain()
                    .in_set(GameSet::LateCreatures)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world(seed: u32) -> World {
        let biome = data::biome_by_key("lush");
        World::new(seed, biome.key, 6)
    }

    #[test]
    fn batch_seed_deterministic() {
        let a = crate::rng::batch_seed(7777, 3, -4);
        let b = crate::rng::batch_seed(7777, 3, -4);
        let c = crate::rng::batch_seed(7777, 3, -5);
        assert_eq!(a, b);
        assert_ne!(a, c);
        // 与 JS creatures.js batchSeedOf 黄金值一致（node 实测）
        assert_eq!(crate::rng::batch_seed(7777, 0, 0), 0xE28E_0574);
        assert_eq!(crate::rng::batch_seed(7777, 3, -4), 0xAB3A_6E5C);
    }

    #[test]
    fn register_cell_deterministic_and_herd_roll() {
        let world = test_world(4242);
        let mut s1 = CreatureSpawner::default();
        let mut s2 = CreatureSpawner::default();
        // 同一世界同一种子：细胞注册结果完全一致（候选 + 兽群掷骰）
        for (cx, cz) in [(0, 0), (1, 0)] {
            register_cell(&mut s1, &world, cx, cz, 10);
        }
        for (cx, cz) in [(0, 0), (1, 0)] {
            register_cell(&mut s2, &world, cx, cz, 10);
        }
        assert_eq!(s1.cells.len(), s2.cells.len());
        assert_eq!(s1.herds.len(), s2.herds.len());
        // 兽群 nid 一致
        let nids1: Vec<u64> = s1.herds.keys().copied().collect();
        let nids2: Vec<u64> = s2.herds.keys().copied().collect();
        assert_eq!(nids1, nids2);
        // 候选点非空且与兽群位置一致
        for nid in &nids1 {
            let h = &s1.herds[nid];
            assert!(!s1.cells[&(h.cx, h.cz)].cands.is_empty());
            assert_eq!(s1.cells[&(h.cx, h.cz)].cands[h.cand].x, h.x);
        }
    }

    #[test]
    fn herd_serialize_restore_roundtrip() {
        let world = test_world(1234);
        // 手动构造存档数据（serialize 的 Query 部分由集成验证）
        let herds = vec![
            HerdSave {
                cx: 0,
                cz: 0,
                cand: 1,
                x: 10.5,
                z: 20.5,
                hp: 3.5,
                home_x: 12.0,
                home_z: 18.0,
            },
            HerdSave {
                cx: 2,
                cz: -1,
                cand: 3,
                x: -40.25,
                z: 66.75,
                hp: 4.0,
                home_x: -40.25,
                home_z: 66.75,
            },
        ];
        let cells = vec![
            CellSave {
                cx: 0,
                cz: 0,
                mask: 0b101,
            },
            CellSave {
                cx: 2,
                cz: -1,
                mask: 0b0100,
            },
        ];
        let mut s = CreatureSpawner::default();
        s.restore(world.seed, &herds, &cells);
        assert_eq!(s.herds.len(), 2);
        assert_eq!(s.cells.len(), 2);
        for (k, v) in &s.cells {
            let c = cells.iter().find(|c| c.cx == k.0 && c.cz == k.1).unwrap();
            assert_eq!(v.mask, c.mask);
        }
        for h in &herds {
            let nid = crate::rng::batch_seed(world.seed, h.cx, h.cz) as u64 * 64 + h.cand as u64;
            let h2 = &s.herds[&nid];
            assert_eq!(h.x, h2.x);
            assert_eq!(h.z, h2.z);
            assert_eq!(h.hp, h2.hp);
            assert_eq!(h.home_x, h2.home_x);
            assert_eq!(h.cand, h2.cand);
            assert!(h2.entity.is_none());
        }
        // 确定性：同一存档恢复两次结果一致
        let mut s3 = CreatureSpawner::default();
        s3.restore(world.seed, &herds, &cells);
        for (nid, h) in &s.herds {
            let h3 = &s3.herds[nid];
            assert_eq!(h.speed, h3.speed);
            assert_eq!(h.timer, h3.timer);
            assert_eq!(h.anim_t, h3.anim_t);
        }
    }

    #[test]
    fn restore_does_not_resurrect_dead_or_masked_herds() {
        let world = test_world(1234);
        let herds = vec![
            HerdSave {
                cx: 0,
                cz: 0,
                cand: 1,
                x: 1.0,
                z: 1.0,
                hp: 0.0,
                home_x: 1.0,
                home_z: 1.0,
            },
            HerdSave {
                cx: 0,
                cz: 0,
                cand: 2,
                x: 2.0,
                z: 2.0,
                hp: 3.0,
                home_x: 2.0,
                home_z: 2.0,
            },
        ];
        let cells = vec![CellSave {
            cx: 0,
            cz: 0,
            mask: 1 << 2,
        }];
        let mut spawner = CreatureSpawner::default();
        spawner.restore(world.seed, &herds, &cells);
        assert!(spawner.herds.is_empty());
        assert_eq!(spawner.cells[&(0, 0)].mask & 0b110, 0b110);
    }
}
