//! 角色外观（捏人）与第三人称体素人形模型 — port of js/humanoid.js + SVG 人形外观。
//! 用于：空间站 NPC / 角色创建预览 / 站内第三人称。

use bevy::prelude::*;
use crate::save::Appearance;

fn hex_color(hex: &str) -> Color {
    crate::space::parse_hex(hex).unwrap_or(Color::srgb(0.8, 0.8, 0.8))
}

/// 人形各部件（供动画/高亮使用）。
pub struct HumanoidParts {
    pub root: Entity,
    pub head: Entity,
    pub torso: Entity,
    pub arm_l: Entity,
    pub arm_r: Entity,
    pub leg_l: Entity,
    pub leg_r: Entity,
}

/// 构建体素人形（总高约 1.9 格，脚底在 y=0）。
pub fn spawn_humanoid(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    appearance: &Appearance,
    pos: Vec3,
    yaw: f32,
    extra: impl FnOnce(&mut Commands, &mut Assets<Mesh>, &mut Assets<StandardMaterial>, Entity),
) -> HumanoidParts {
    let skin = hex_color(&appearance.skin);
    let suit = hex_color(&appearance.suit);
    let trim = hex_color(&appearance.trim);
    let pants = hex_color(&appearance.pants);
    let boots = hex_color(&appearance.boots);
    let hair = hex_color(&appearance.hair);
    let visor = hex_color(&appearance.visor);

    let mut mk = |mats: &mut Assets<StandardMaterial>, c: Color| -> Handle<StandardMaterial> {
        mats.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: 0.9,
            metallic: 0.0,
            ..default()
        })
    };
    let mat_skin = mk(mats, skin);
    let mat_suit = mk(mats, suit);
    let mat_trim = mk(mats, trim);
    let mat_pants = mk(mats, pants);
    let mat_boots = mk(mats, boots);
    let mat_hair = mk(mats, hair);
    let mat_visor = mk(mats, visor);

    let root = commands
        .spawn((
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            crate::InGame,
        ))
        .id();
    let mut part = |mats: &mut Assets<StandardMaterial>,
                    root: Entity,
                    w: f32,
                    h: f32,
                    d: f32,
                    m: Handle<StandardMaterial>,
                    x: f32,
                    y: f32,
                    z: f32|
     -> Entity {
        let e = commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(w, h, d))),
                MeshMaterial3d(m),
                Transform::from_xyz(x, y, z),
                crate::InGame,
            ))
            .id();
        commands.entity(root).add_child(e);
        e
    };
    // 腿（裤装）+ 靴
    for sx in [-0.14f32, 0.14] {
        part(mats, root, 0.24, 0.72, 0.26, mat_pants.clone(), sx, 0.76, 0.0);
        part(mats, root, 0.26, 0.16, 0.28, mat_boots.clone(), sx, 0.32, 0.0);
    }
    // 躯干（制服）+ 饰条
    part(mats, root, 0.62, 0.66, 0.3, mat_suit.clone(), 0.0, 1.45, 0.0);
    part(mats, root, 0.64, 0.08, 0.32, mat_trim.clone(), 0.0, 1.62, 0.0);
    // 手臂
    for sx in [-0.41f32, 0.41] {
        part(mats, root, 0.16, 0.56, 0.22, mat_suit.clone(), sx, 1.38, 0.0);
        part(mats, root, 0.15, 0.16, 0.2, mat_skin.clone(), sx, 1.02, 0.0);
    }
    // 头
    let head = part(mats, root, 0.42, 0.42, 0.42, mat_skin.clone(), 0.0, 2.03, 0.0);
    // 目镜
    part(mats, root, 0.34, 0.09, 0.06, mat_visor.clone(), 0.0, 2.08, -0.22);
    // 头盔
    if appearance.helmet {
        part(mats, root, 0.46, 0.24, 0.46, mat_suit.clone(), 0.0, 2.2, 0.0);
        part(mats, root, 0.3, 0.1, 0.3, mat_trim.clone(), 0.0, 2.33, 0.0);
    } else {
        // 发型
        match appearance.hair_style.as_str() {
            "none" => {}
            "short" => {
                part(mats, root, 0.44, 0.14, 0.44, mat_hair.clone(), 0.0, 2.26, 0.0);
            }
            "long" => {
                part(mats, root, 0.44, 0.14, 0.44, mat_hair.clone(), 0.0, 2.26, 0.0);
                part(mats, root, 0.44, 0.34, 0.16, mat_hair.clone(), 0.0, 2.08, 0.26);
            }
            "pony" => {
                part(mats, root, 0.44, 0.12, 0.44, mat_hair.clone(), 0.0, 2.26, 0.0);
                part(mats, root, 0.14, 0.4, 0.14, mat_hair.clone(), 0.0, 2.14, 0.3);
            }
            "mohawk" => {
                part(mats, root, 0.1, 0.2, 0.44, mat_hair.clone(), 0.0, 2.36, 0.0);
            }
            "bun" => {
                part(mats, root, 0.44, 0.1, 0.44, mat_hair.clone(), 0.0, 2.26, 0.0);
                part(mats, root, 0.16, 0.16, 0.16, mat_hair.clone(), 0.0, 2.38, -0.1);
            }
            _ => {
                part(mats, root, 0.44, 0.14, 0.44, mat_hair.clone(), 0.0, 2.26, 0.0);
            }
        }
    }
    extra(commands, meshes, mats, root);
    HumanoidParts {
        root,
        head,
        torso: Entity::PLACEHOLDER,
        arm_l: Entity::PLACEHOLDER,
        arm_r: Entity::PLACEHOLDER,
        leg_l: Entity::PLACEHOLDER,
        leg_r: Entity::PLACEHOLDER,
    }
}

/// 随机外观（角色创建默认）。
pub fn random_appearance(seed: u32) -> Appearance {
    Appearance::random(seed)
}
