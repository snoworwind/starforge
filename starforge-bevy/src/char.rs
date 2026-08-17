//! 角色外观（捏人）与第三人称人形模型 — 使用 CC0 GLB 角色。
//! 模型来源：KayKit Character Pack: Adventurers（CC0）+ Kenney Space Kit 宇航员/外星人（CC0），
//! 许可证存于 assets/licenses/。用于：空间站 NPC / 村庄村民 / 角色创建预览。

use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy_world_serialization::prelude::WorldAssetRoot;
use crate::save::Appearance;

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

/// 生成 NPC 人形（GLB 角色模型，脚底在 y=0）。
pub fn spawn_humanoid(
    commands: &mut Commands,
    asset_server: &AssetServer,
    _appearance: &Appearance,
    pos: Vec3,
    yaw: f32,
) -> HumanoidParts {
    let idx = ((pos.x as i32).wrapping_mul(31) ^ (pos.z as i32).wrapping_mul(57)).unsigned_abs() as usize
        % NPC_MODELS.len();
    let root = commands
        .spawn((
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(NPC_MODELS[idx]))),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            crate::InGame,
        ))
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

/// 随机外观（角色创建默认）。
pub fn random_appearance(seed: u32) -> Appearance {
    Appearance::random(seed)
}
