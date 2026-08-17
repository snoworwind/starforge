//! Creatures (simple wandering voxel animals) and dropped-item entities.

use crate::data;
use crate::inventory::Slot;
use crate::player::Player;
use crate::rng::Rng;
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
pub const HERD_CHANCE: f32 = 0.18;
/// 玩家 128m 范围内活跃兽群数量目标（低于此值才周期补足 → 被杀后缓慢恢复）。
pub const TARGET_DENSITY: usize = 12;
/// 活跃生物上限（安全阀）。
pub const CRE_CAP: usize = 16;
/// 生成环内径：距玩家 < 24m 不生成（Minecraft 同款规则）。
pub const SPAWN_MIN: f32 = 24.0;
/// 生成环外径（Minecraft 同款 128m）。
pub const SPAWN_MAX: f32 = 128.0;
/// 距玩家 > 128m：兽群卸载休眠（保留位置/血量，不删除）。
pub const UNLOAD_D: f32 = 128.0;
/// 距玩家 < 96m：休眠兽群重载（迟滞带，避免边界抖动）。
pub const RELOAD_D: f32 = 96.0;
/// 周期生成间隔（秒），每次最多补 1 个兽群。
pub const SPAWN_INTERVAL: f32 = 1.2;
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
    blob: Option<CreatureAnimationSetup>,
    sentinel: Option<CreatureAnimationSetup>,
}

/// Link an instantiated animation player back to its gameplay entity. This
/// avoids assuming that the player is a direct child of the creature root.
#[derive(Component)]
struct CreatureAnimationTarget {
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
        "blob" => (&mut library.blob, "models/creatures/blob.glb", 2, 3),
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
        q: &Query<(Entity, &Creature, &Transform)>,
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
        self.herds.clear();
        for h in herds {
            if h.cand >= u32::BITS as usize {
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
                    hp: if h.hp.is_finite() { h.hp.max(1.0) } else { 3.0 },
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
        self.cells.clear();
        for c in cells {
            self.cells.insert(
                (c.cx, c.cz),
                CellState {
                    cands: Vec::new(),
                    mask: c.mask,
                    initialized: false,
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

/// 模型参数：(模型路径, 缩放, 脚底偏移)。尺寸为**最终渲染尺寸**（含 GLB 节点变换）。
/// 缩放口径与 JS buildCreature 一致：模型最长边 = max(w,h,d) × 2.2（strider 2.42 / crab 1.54 / blob 1.54）。
pub fn creature_model(kind: &str) -> (&'static str, f32, f32) {
    match kind {
        "crab" => (
            "models/creatures/crab.glb",
            1.54 / 19.42,
            8.64 * (1.54 / 19.42),
        ),
        "blob" => ("models/creatures/blob.glb", 1.54 / 2.0, 0.0),
        _ => (
            "models/creatures/strider.glb",
            2.42 / 135.37,
            67.52 * (2.42 / 135.37),
        ),
    }
}

fn species_speed(kind: &str) -> f32 {
    match kind {
        "crab" => 0.7,
        "blob" => 0.35,
        _ => 1.8,
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
            let gy = world.top_at(ix, iz);
            let dd = data::block_by_id(world.get(ix, gy, iz));
            ok = !dd.liquid && dd.key != "log" && dd.key != "leaves";
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
    let h = h.clone();
    // 地形被破坏（水/树）→ 保持休眠，等恢复
    let ix = h.x.floor() as i32;
    let iz = h.z.floor() as i32;
    let gy = world.top_at(ix, iz);
    let dd = data::block_by_id(world.get(ix, gy, iz));
    if dd.liquid || dd.key == "log" || dd.key == "leaves" {
        return false;
    }
    let kind = data::biome_animal_kind(world.biome().key);
    let (model, scale, y_off) = creature_model(kind);
    // 命中盒随模型尺寸（JS radius = max(w,h,d)*1.3）
    let (w, hh, d) = match kind {
        "crab" => (0.55, 1.1, 0.7),
        "blob" => (0.7, 1.0, 0.7),
        _ => (0.35, 2.2, 0.35),
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
    let _ = (w, d);
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
    }

    // 3. 卸载休眠（>128m：回写位置/血量并淡出；实体淡出完成后由 despawn 系统清空 entity）/ 重载（<96m）
    let mut to_reload: Vec<u64> = Vec::new();
    for (nid, h) in &mut spawner.herds {
        let d = ((h.x - p.pos.x).powi(2) + (h.z - p.pos.z).powi(2)).sqrt();
        if let Some(e) = h.entity {
            if d > UNLOAD_D
                && let Ok((_, mut c, tf)) = creatures.get_mut(e)
            {
                if c.fading {
                    continue; // 已在淡出卸载中，由 despawn 系统收尾
                }
                h.x = tf.translation.x;
                h.z = tf.translation.z;
                h.hp = c.hp;
                c.fading = true; // 淡出 0.8s 后卸载（数据已回写，随时可重载）
                c.fade_t = 0.0;
                println!("CREATURE unload herd {nid} d={d:.0} (fade-out)");
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
    player: Query<&Player>,
) {
    let dt = time.delta_secs();
    let Ok(p) = player.single() else { return };
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
        if c.walking {
            pos += c.vel * dt;
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
        let ground = world.top_at(pos.x.floor() as i32, pos.z.floor() as i32);
        let floor_y = ground as f32 + 1.0 + c.foot;
        if pos.y <= floor_y + 0.01 {
            pos.y = floor_y + 0.01;
            c.vel.y = 0.0;
            c.grounded = true;
        } else {
            c.grounded = false;
        }
        // avoid walking into player
        if (pos - p.pos).xz().length() < 1.0 && (pos.y - p.pos.y).abs() < 2.0 {
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
                // 有腿生物：行走前后摆动
                let tilt = (c.anim_t * 10.0).sin() * 0.08;
                tf.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(tilt);
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
        let breath = 1.0 + (c.anim_t * 3.0).sin() * 0.03;
        let hit = if c.hit_t > 0.0 {
            1.0 + (c.hit_t / 0.25) * 0.18
        } else {
            1.0
        };
        tf.scale = Vec3::splat((c.scale * fade_in * breath * hit * fade_out).max(0.001));
    }
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
            crate::audio::play(&mut commands, sfx.break_block.clone(), 0.7, None);
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
            crate::audio::play(&mut commands, sfx.break_block.clone(), 0.5, None);
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
        for s in &world.g.structures {
            if let crate::world::Structure::Ruin { x, z, .. } = s {
                let dx = ppos.x - *x as f32;
                let dz = ppos.z - *z as f32;
                let d = (dx * dx + dz * dz).sqrt();
                if d < 40.0 && nearest.map(|(_, bd)| d < bd).unwrap_or(true) {
                    nearest = Some(([*x, 0, *z], d));
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
                let top = world.top_at(x, z);
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
            tf.translation += dir * 4.7 * dt; // speed 1.8 × 2.6 追击
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
    if snap.len() > DROP_CAP {
        let mut order: Vec<usize> = (0..snap.len()).collect();
        order.sort_by(|a, b| {
            snap[*b]
                .1
                .age
                .partial_cmp(&snap[*a].1.age)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &idx in order.iter().take(snap.len() - DROP_CAP) {
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
                mask: 0b1000,
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
}
