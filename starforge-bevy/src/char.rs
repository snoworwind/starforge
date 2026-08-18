//! 角色外观（捏人）与第三人称人形模型 — 使用 CC0 GLB 角色。
//! 模型来源：KayKit Character Pack: Adventurers（CC0）+ Kenney Space Kit 宇航员/外星人（CC0），
//! 许可证存于 assets/licenses/。用于：空间站 NPC / 村庄村民 / 角色创建预览。

use crate::save::Appearance;
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use bevy_world_serialization::prelude::WorldAssetRoot;
use std::collections::HashMap;

/// 可用 NPC 模型（按位置哈希轮换，保证同位置同外观）。
pub const NPC_MODELS: [&str; 8] = [
    "models/npc/adventurer_barbarian.glb",
    "models/npc/adventurer_knight.glb",
    "models/npc/adventurer_mage.glb",
    "models/npc/adventurer_rogue.glb",
    "models/npc/adventurer_rogue_hooded.glb",
    "models/npc/alien.glb",
    "models/npc/astronaut_a.glb",
    "models/npc/astronaut_b.glb",
];

/// 各 NPC 模型的 (缩放, 脚底对齐 y 偏移)：按实测包围盒换算，目标总高 1.9 格（原版体素人形）。
/// 资产已去除 Kenney 建模残留的根节点 t(2,0,1.5) 平移。
fn npc_scale(model: &str) -> (f32, f32) {
    match model {
        // KayKit 冒险者：高 3.31~3.44、脚底 y=-1.12
        "models/npc/adventurer_barbarian.glb"
        | "models/npc/adventurer_knight.glb"
        | "models/npc/adventurer_mage.glb"
        | "models/npc/adventurer_rogue.glb"
        | "models/npc/adventurer_rogue_hooded.glb" => (1.9 / 3.35, 1.12 * (1.9 / 3.35)),
        // Kenney 外星人：高 1.78、脚底 y=-0.39
        "models/npc/alien.glb" => (1.9 / 1.78, 0.39 * (1.9 / 1.78)),
        // Kenney 宇航员：高 1.58、脚底 y=-0.39
        _ => (1.9 / 1.58, 0.39 * (1.9 / 1.58)),
    }
}

/// 人形各部件（供动画/高亮使用；GLB 模型下 root 即全部）。
pub struct HumanoidParts {
    pub root: Entity,
    pub head: Entity,
    pub torso: Entity,
    pub arm_l: Entity,
    pub arm_r: Entity,
    pub leg_l: Entity,
    pub leg_r: Entity,
}

/// KayKit NPC scenes contain a complete armature and animation library, but
/// glTF scene instantiation does not automatically choose a clip.  Without
/// this setup every adventurer stays in the authored T-pose (most visibly with
/// both arms held straight out).
#[derive(Component)]
struct NpcAnimationSetup {
    model: &'static str,
}

#[derive(Resource, Default)]
pub struct NpcAnimationLibrary {
    adventurer: HashMap<&'static str, (Handle<AnimationGraph>, AnimationNodeIndex)>,
}

fn npc_animation_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    setups: Query<&NpcAnimationSetup>,
    mut players: Query<&mut AnimationPlayer>,
    asset_server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut library: ResMut<NpcAnimationLibrary>,
) {
    let Ok(setup) = setups.get(ready.entity) else {
        return;
    };
    // The low-poly astronaut/alien assets are authored in a usable static
    // pose.  The KayKit adventurers need their idle clip to leave the T-pose.
    let Some((graph, idle)) = (setup.model.starts_with("models/npc/adventurer_")).then(|| {
        if let Some((graph, idle)) = library.adventurer.get(setup.model) {
            return (graph.clone(), *idle);
        }
        let (graph, nodes) = AnimationGraph::from_clips([
            asset_server.load(GltfAssetLabel::Animation(36).from_asset(setup.model))
        ]);
        let graph = graphs.add(graph);
        let idle = nodes[0];
        library
            .adventurer
            .insert(setup.model, (graph.clone(), idle));
        (graph, idle)
    }) else {
        return;
    };
    for child in children.iter_descendants(ready.entity) {
        let Ok(mut player) = players.get_mut(child) else {
            continue;
        };
        let mut transitions = AnimationTransitions::new();
        transitions
            .play(&mut player, idle, std::time::Duration::ZERO)
            .repeat();
        commands
            .entity(child)
            .insert((AnimationGraphHandle(graph.clone()), transitions));
    }
}

#[derive(Component)]
pub struct NpcIdle {
    pub phase: f32,
    pub base_y: f32,
    pub base_rotation: Quat,
}

/// 生成 NPC 人形（GLB 角色模型，按模型实测尺寸缩放 + 脚底对齐）。
pub fn spawn_humanoid(
    commands: &mut Commands,
    asset_server: &AssetServer,
    _appearance: &Appearance,
    pos: Vec3,
    yaw: f32,
) -> HumanoidParts {
    let idx = ((pos.x as i32).wrapping_mul(31) ^ (pos.z as i32).wrapping_mul(57)).unsigned_abs()
        as usize
        % NPC_MODELS.len();
    let model = NPC_MODELS[idx];
    let (scale, y_off) = npc_scale(model);
    let root = commands
        .spawn((
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(model))),
            Transform::from_translation(pos + Vec3::Y * y_off)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(scale)),
            Visibility::default(),
            NpcIdle {
                phase: pos.x * 0.17 + pos.z * 0.11,
                base_y: pos.y + y_off,
                base_rotation: Quat::from_rotation_y(yaw),
            },
            NpcAnimationSetup { model },
            crate::InGame,
        ))
        .observe(npc_animation_ready)
        .id();
    HumanoidParts {
        root,
        head: root,
        torso: root,
        arm_l: root,
        arm_r: root,
        leg_l: root,
        leg_r: root,
    }
}

/// Small breathing/weight-shift loop for all GLB NPC roots. It remains useful
/// even for assets whose embedded animation clips are not exposed by Bevy.
pub fn npc_idle_system(time: Res<Time>, mut q: Query<(&mut NpcIdle, &mut Transform)>) {
    for (mut idle, mut transform) in &mut q {
        idle.phase += time.delta_secs();
        let wave = idle.phase * 1.7;
        transform.translation.y = idle.base_y + wave.sin() * 0.025;
        transform.rotation = idle.base_rotation * Quat::from_rotation_y(wave.cos() * 0.012);
    }
}

/// 随机外观（角色创建默认）。
pub fn random_appearance(seed: u32) -> Appearance {
    Appearance::random(seed)
}
