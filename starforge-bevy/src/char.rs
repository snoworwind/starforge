//! Original voxel humanoids with a small articulated rig and role-specific gear.

use crate::save::Appearance;
use bevy::prelude::*;

/// The retired third-party models remain in the package as licensed reserves.
pub const RESERVED_NPC_MODELS: [&str; 8] = [
    "models/npc/adventurer_barbarian.glb",
    "models/npc/adventurer_knight.glb",
    "models/npc/adventurer_mage.glb",
    "models/npc/adventurer_rogue.glb",
    "models/npc/adventurer_rogue_hooded.glb",
    "models/npc/alien.glb",
    "models/npc/astronaut_a.glb",
    "models/npc/astronaut_b.glb",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcRole {
    Settler,
    Miner,
    Engineer,
    Botanist,
    Cartographer,
    Traveler,
}

impl NpcRole {
    pub const SHOWCASE: [Self; 5] = [
        Self::Settler,
        Self::Miner,
        Self::Engineer,
        Self::Botanist,
        Self::Cartographer,
    ];

    fn from_seed(seed: u32) -> Self {
        match seed % 5 {
            0 => Self::Settler,
            1 => Self::Miner,
            2 => Self::Engineer,
            3 => Self::Botanist,
            _ => Self::Cartographer,
        }
    }

    fn emblem(self) -> Color {
        match self {
            Self::Settler => Color::srgb_u8(0xd4, 0x91, 0x45),
            Self::Miner => Color::srgb_u8(0xe5, 0xa9, 0x39),
            Self::Engineer => Color::srgb_u8(0x42, 0xb7, 0xc7),
            Self::Botanist => Color::srgb_u8(0x67, 0xb8, 0x69),
            Self::Cartographer => Color::srgb_u8(0x9d, 0x83, 0xd8),
            Self::Traveler => Color::srgb_u8(0x55, 0xb5, 0xe8),
        }
    }

    fn work_gesture(self) -> f32 {
        match self {
            Self::Miner | Self::Engineer => 0.12,
            Self::Botanist | Self::Cartographer => 0.07,
            Self::Settler | Self::Traveler => 0.0,
        }
    }
}

#[derive(Resource)]
pub struct NpcArt {
    cube: Handle<Mesh>,
}

fn setup_npc_art(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(NpcArt {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
    });
}

/// A role in the old API: body-part entities now refer to rig joints.
pub struct HumanoidParts {
    pub root: Entity,
    pub head: Entity,
    pub torso: Entity,
    pub arm_l: Entity,
    pub arm_r: Entity,
    pub leg_l: Entity,
    pub leg_r: Entity,
}

#[derive(Component)]
pub struct NpcRig {
    head: Entity,
    torso: Entity,
    arm_l: Entity,
    arm_r: Entity,
    leg_l: Entity,
    leg_r: Entity,
    role: NpcRole,
    phase: f32,
    last_position: Vec3,
    gait: f32,
}

#[derive(Clone)]
struct Palette {
    skin: Handle<StandardMaterial>,
    hair: Handle<StandardMaterial>,
    coat: Handle<StandardMaterial>,
    trim: Handle<StandardMaterial>,
    pants: Handle<StandardMaterial>,
    boots: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    eye: Handle<StandardMaterial>,
    role: Handle<StandardMaterial>,
}

fn parse_color(hex: &str, fallback: [u8; 3]) -> Color {
    let value = hex.trim().trim_start_matches('#');
    let rgb = u32::from_str_radix(value, 16)
        .ok()
        .filter(|_| value.len() == 6);
    let (r, g, b) = rgb
        .map(|v| ((v >> 16) as u8, (v >> 8) as u8, v as u8))
        .unwrap_or((fallback[0], fallback[1], fallback[2]));
    Color::srgb_u8(r, g, b)
}

fn solid_material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 1.0,
        metallic: 0.0,
        reflectance: 0.08,
        ..default()
    })
}

fn make_palette(
    materials: &mut Assets<StandardMaterial>,
    appearance: &Appearance,
    role: NpcRole,
) -> Palette {
    Palette {
        skin: solid_material(materials, parse_color(&appearance.skin, [216, 174, 133])),
        hair: solid_material(materials, parse_color(&appearance.hair, [67, 48, 35])),
        coat: solid_material(materials, parse_color(&appearance.suit, [68, 90, 108])),
        trim: solid_material(materials, parse_color(&appearance.trim, [53, 176, 190])),
        pants: solid_material(materials, parse_color(&appearance.pants, [48, 58, 66])),
        boots: solid_material(materials, parse_color(&appearance.boots, [35, 36, 37])),
        dark: solid_material(materials, Color::srgb_u8(35, 41, 45)),
        eye: solid_material(materials, Color::srgb_u8(27, 33, 38)),
        role: solid_material(materials, role.emblem()),
    }
}

fn box_part(
    commands: &mut Commands,
    art: &NpcArt,
    parent: Entity,
    material: &Handle<StandardMaterial>,
    center: Vec3,
    size: Vec3,
) -> Entity {
    commands
        .spawn((
            Mesh3d(art.cube.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(center).with_scale(size),
            ChildOf(parent),
            crate::InGame,
        ))
        .id()
}

fn joint(commands: &mut Commands, parent: Entity, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Visibility::default(),
            ChildOf(parent),
            crate::InGame,
        ))
        .id()
}

fn make_head(
    commands: &mut Commands,
    art: &NpcArt,
    head: Entity,
    palette: &Palette,
    appearance: &Appearance,
    role: NpcRole,
) {
    box_part(
        commands,
        art,
        head,
        &palette.skin,
        Vec3::ZERO,
        Vec3::new(0.5, 0.5, 0.48),
    );
    // Pixel eyes and a small, readable nose give the face a distinct silhouette.
    for x in [-0.115, 0.115] {
        box_part(
            commands,
            art,
            head,
            &palette.eye,
            Vec3::new(x, 0.035, 0.251),
            Vec3::new(0.065, 0.075, 0.025),
        );
        box_part(
            commands,
            art,
            head,
            &palette.hair,
            Vec3::new(x, 0.115, 0.25),
            Vec3::new(0.09, 0.04, 0.028),
        );
    }
    box_part(
        commands,
        art,
        head,
        &palette.skin,
        Vec3::new(0.0, -0.075, 0.31),
        Vec3::new(0.13, 0.18, 0.16),
    );

    if appearance.helmet {
        box_part(
            commands,
            art,
            head,
            &palette.coat,
            Vec3::new(0.0, 0.285, -0.005),
            Vec3::new(0.54, 0.13, 0.53),
        );
        box_part(
            commands,
            art,
            head,
            &palette.trim,
            Vec3::new(0.0, 0.22, 0.24),
            Vec3::new(0.43, 0.075, 0.055),
        );
    } else {
        let hair = appearance.hair_style.as_str();
        box_part(
            commands,
            art,
            head,
            &palette.hair,
            Vec3::new(0.0, 0.255, -0.015),
            Vec3::new(0.54, 0.12, 0.5),
        );
        match hair {
            "long" => {
                for x in [-0.23, 0.23] {
                    box_part(
                        commands,
                        art,
                        head,
                        &palette.hair,
                        Vec3::new(x, -0.07, -0.11),
                        Vec3::new(0.09, 0.38, 0.3),
                    );
                }
            }
            "pony" | "bun" => {
                box_part(
                    commands,
                    art,
                    head,
                    &palette.hair,
                    Vec3::new(0.0, 0.18, -0.3),
                    Vec3::new(0.18, 0.18, 0.18),
                );
                if hair == "pony" {
                    box_part(
                        commands,
                        art,
                        head,
                        &palette.hair,
                        Vec3::new(0.13, 0.02, -0.38),
                        Vec3::new(0.11, 0.28, 0.12),
                    );
                }
            }
            "mohawk" => {
                box_part(
                    commands,
                    art,
                    head,
                    &palette.hair,
                    Vec3::new(0.0, 0.34, 0.015),
                    Vec3::new(0.14, 0.16, 0.34),
                );
            }
            _ => {}
        }
    }

    // Different head gear makes role silhouettes legible before the player gets close.
    match role {
        NpcRole::Miner => {
            box_part(
                commands,
                art,
                head,
                &palette.role,
                Vec3::new(0.0, 0.29, 0.28),
                Vec3::new(0.13, 0.1, 0.07),
            );
        }
        NpcRole::Engineer => {
            for x in [-0.27, 0.27] {
                box_part(
                    commands,
                    art,
                    head,
                    &palette.dark,
                    Vec3::new(x, 0.0, 0.0),
                    Vec3::new(0.07, 0.18, 0.19),
                );
            }
        }
        NpcRole::Botanist => {
            box_part(
                commands,
                art,
                head,
                &palette.role,
                Vec3::new(-0.2, 0.31, -0.02),
                Vec3::new(0.2, 0.11, 0.18),
            );
            box_part(
                commands,
                art,
                head,
                &palette.trim,
                Vec3::new(-0.27, 0.4, -0.02),
                Vec3::new(0.12, 0.1, 0.12),
            );
        }
        NpcRole::Cartographer => {
            box_part(
                commands,
                art,
                head,
                &palette.role,
                Vec3::new(0.0, 0.31, 0.0),
                Vec3::new(0.61, 0.1, 0.55),
            );
            box_part(
                commands,
                art,
                head,
                &palette.dark,
                Vec3::new(0.0, 0.05, 0.264),
                Vec3::new(0.39, 0.045, 0.035),
            );
        }
        NpcRole::Settler | NpcRole::Traveler => {}
    }
}

fn add_tool(commands: &mut Commands, art: &NpcArt, hand: Entity, palette: &Palette, role: NpcRole) {
    match role {
        NpcRole::Miner => {
            box_part(
                commands,
                art,
                hand,
                &palette.dark,
                Vec3::new(0.0, -0.27, 0.08),
                Vec3::new(0.075, 0.55, 0.075),
            );
            box_part(
                commands,
                art,
                hand,
                &palette.role,
                Vec3::new(0.0, -0.55, 0.08),
                Vec3::new(0.36, 0.11, 0.12),
            );
        }
        NpcRole::Engineer => {
            box_part(
                commands,
                art,
                hand,
                &palette.dark,
                Vec3::new(0.0, -0.22, 0.08),
                Vec3::new(0.075, 0.42, 0.075),
            );
            box_part(
                commands,
                art,
                hand,
                &palette.role,
                Vec3::new(0.0, -0.42, 0.08),
                Vec3::new(0.25, 0.1, 0.1),
            );
        }
        NpcRole::Botanist => {
            box_part(
                commands,
                art,
                hand,
                &palette.dark,
                Vec3::new(0.0, -0.2, 0.08),
                Vec3::new(0.22, 0.24, 0.2),
            );
            box_part(
                commands,
                art,
                hand,
                &palette.role,
                Vec3::new(0.0, -0.06, 0.08),
                Vec3::new(0.12, 0.22, 0.12),
            );
            box_part(
                commands,
                art,
                hand,
                &palette.trim,
                Vec3::new(0.12, 0.03, 0.08),
                Vec3::new(0.12, 0.1, 0.09),
            );
        }
        NpcRole::Cartographer => {
            box_part(
                commands,
                art,
                hand,
                &palette.role,
                Vec3::new(0.0, -0.2, 0.12),
                Vec3::new(0.3, 0.25, 0.035),
            );
            box_part(
                commands,
                art,
                hand,
                &palette.trim,
                Vec3::new(0.0, -0.2, 0.145),
                Vec3::new(0.18, 0.035, 0.012),
            );
        }
        NpcRole::Settler | NpcRole::Traveler => {
            box_part(
                commands,
                art,
                hand,
                &palette.role,
                Vec3::new(0.0, -0.19, 0.1),
                Vec3::new(0.16, 0.22, 0.15),
            );
        }
    }
}

fn build_body(
    commands: &mut Commands,
    art: &NpcArt,
    root: Entity,
    materials: &mut Assets<StandardMaterial>,
    appearance: &Appearance,
    role: NpcRole,
) -> HumanoidParts {
    let palette = make_palette(materials, appearance, role);
    let torso = joint(commands, root, Vec3::new(0.0, 1.02, 0.0));
    let head = joint(commands, root, Vec3::new(0.0, 1.51, 0.0));
    let arm_l = joint(commands, torso, Vec3::new(-0.37, 0.27, 0.0));
    let arm_r = joint(commands, torso, Vec3::new(0.37, 0.27, 0.0));
    let leg_l = joint(commands, root, Vec3::new(-0.16, 0.82, 0.0));
    let leg_r = joint(commands, root, Vec3::new(0.16, 0.82, 0.0));

    // Layered tunic, collar, apron, belt, pockets, shoulder plates and a pack.
    box_part(
        commands,
        art,
        torso,
        &palette.coat,
        Vec3::new(0.0, 0.02, 0.0),
        Vec3::new(0.59, 0.68, 0.39),
    );
    box_part(
        commands,
        art,
        torso,
        &palette.trim,
        Vec3::new(0.0, 0.36, 0.02),
        Vec3::new(0.39, 0.12, 0.41),
    );
    box_part(
        commands,
        art,
        torso,
        &palette.pants,
        Vec3::new(0.0, -0.29, 0.0),
        Vec3::new(0.62, 0.12, 0.4),
    );
    box_part(
        commands,
        art,
        torso,
        &palette.role,
        Vec3::new(0.0, -0.1, 0.204),
        Vec3::new(0.33, 0.35, 0.035),
    );
    box_part(
        commands,
        art,
        torso,
        &palette.trim,
        Vec3::new(0.0, 0.19, 0.211),
        Vec3::new(0.1, 0.26, 0.026),
    );
    for x in [-0.2, 0.2] {
        box_part(
            commands,
            art,
            torso,
            &palette.role,
            Vec3::new(x, 0.34, 0.0),
            Vec3::new(0.22, 0.16, 0.43),
        );
    }
    box_part(
        commands,
        art,
        torso,
        &palette.dark,
        Vec3::new(0.0, 0.0, -0.245),
        Vec3::new(0.36, 0.4, 0.16),
    );
    box_part(
        commands,
        art,
        torso,
        &palette.trim,
        Vec3::new(0.0, -0.17, -0.34),
        Vec3::new(0.38, 0.09, 0.05),
    );
    box_part(
        commands,
        art,
        torso,
        &palette.role,
        Vec3::new(0.21, -0.11, 0.23),
        Vec3::new(0.18, 0.2, 0.07),
    );
    box_part(
        commands,
        art,
        torso,
        &palette.dark,
        Vec3::new(0.0, -0.28, 0.23),
        Vec3::new(0.12, 0.12, 0.065),
    );

    for arm in [arm_l, arm_r] {
        box_part(
            commands,
            art,
            arm,
            &palette.coat,
            Vec3::new(0.0, -0.15, 0.0),
            Vec3::new(0.2, 0.34, 0.22),
        );
        box_part(
            commands,
            art,
            arm,
            &palette.trim,
            Vec3::new(0.0, -0.32, 0.0),
            Vec3::new(0.205, 0.08, 0.225),
        );
        box_part(
            commands,
            art,
            arm,
            &palette.coat,
            Vec3::new(0.0, -0.43, 0.0),
            Vec3::new(0.17, 0.2, 0.18),
        );
        box_part(
            commands,
            art,
            arm,
            &palette.skin,
            Vec3::new(0.0, -0.57, 0.035),
            Vec3::new(0.16, 0.16, 0.17),
        );
    }
    for leg in [leg_l, leg_r] {
        box_part(
            commands,
            art,
            leg,
            &palette.pants,
            Vec3::new(0.0, -0.21, 0.0),
            Vec3::new(0.22, 0.42, 0.25),
        );
        box_part(
            commands,
            art,
            leg,
            &palette.pants,
            Vec3::new(0.0, -0.52, 0.0),
            Vec3::new(0.2, 0.26, 0.22),
        );
        box_part(
            commands,
            art,
            leg,
            &palette.boots,
            Vec3::new(0.0, -0.75, 0.055),
            Vec3::new(0.27, 0.17, 0.34),
        );
        box_part(
            commands,
            art,
            leg,
            &palette.role,
            Vec3::new(0.0, -0.62, 0.116),
            Vec3::new(0.205, 0.055, 0.025),
        );
    }

    make_head(commands, art, head, &palette, appearance, role);
    add_tool(commands, art, arm_r, &palette, role);

    HumanoidParts {
        root,
        head,
        torso,
        arm_l,
        arm_r,
        leg_l,
        leg_r,
    }
}

pub fn spawn_humanoid(
    commands: &mut Commands,
    art: &NpcArt,
    materials: &mut Assets<StandardMaterial>,
    appearance: &Appearance,
    pos: Vec3,
    yaw: f32,
    role: NpcRole,
) -> HumanoidParts {
    let seed = (pos.x.floor() as i32 as u32).wrapping_mul(31)
        ^ (pos.z.floor() as i32 as u32).wrapping_mul(57);
    let phase = (seed % 1000) as f32 * 0.006283;
    let root = commands
        .spawn((
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            crate::InGame,
        ))
        .id();
    let parts = build_body(commands, art, root, materials, appearance, role);
    commands.entity(root).insert(NpcRig {
        head: parts.head,
        torso: parts.torso,
        arm_l: parts.arm_l,
        arm_r: parts.arm_r,
        leg_l: parts.leg_l,
        leg_r: parts.leg_r,
        role,
        phase,
        last_position: pos,
        gait: 0.0,
    });
    parts
}

pub fn spawn_villager(
    commands: &mut Commands,
    art: &NpcArt,
    materials: &mut Assets<StandardMaterial>,
    appearance: &Appearance,
    pos: Vec3,
    yaw: f32,
    village_seed: u32,
) -> HumanoidParts {
    spawn_humanoid(
        commands,
        art,
        materials,
        appearance,
        pos,
        yaw,
        NpcRole::from_seed(village_seed),
    )
}

pub fn npc_animation_system(
    time: Res<Time>,
    mut rigs: Query<(&mut NpcRig, &Transform)>,
    mut bones: Query<&mut Transform, Without<NpcRig>>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    if dt <= 0.0 {
        return;
    }
    for (mut rig, root) in &mut rigs {
        let speed = root.translation.distance(rig.last_position) / dt;
        rig.last_position = root.translation;
        let target_gait = (speed / 2.4).clamp(0.0, 1.0);
        rig.gait += (target_gait - rig.gait) * (dt * 8.0).min(1.0);
        rig.phase += dt * (1.15 + rig.gait * 5.2);
        let breathe = (rig.phase * 1.7).sin();
        let swing = (rig.phase * 1.35).sin() * 0.55 * rig.gait;
        let gesture = (rig.phase * 2.1).sin() * rig.role.work_gesture() * (1.0 - rig.gait);

        if let Ok(mut head) = bones.get_mut(rig.head) {
            head.translation.y = 1.51 + breathe * 0.012;
            head.rotation = Quat::from_rotation_y((rig.phase * 0.37).sin() * 0.12);
        }
        if let Ok(mut torso) = bones.get_mut(rig.torso) {
            torso.translation.y = 1.02 + breathe * 0.008;
            torso.rotation = Quat::from_rotation_z((rig.phase * 0.8).sin() * 0.025);
        }
        if let Ok(mut arm) = bones.get_mut(rig.arm_l) {
            arm.rotation = Quat::from_rotation_x(swing + gesture * 0.35);
        }
        if let Ok(mut arm) = bones.get_mut(rig.arm_r) {
            arm.rotation = Quat::from_rotation_x(-swing - gesture);
        }
        if let Ok(mut leg) = bones.get_mut(rig.leg_l) {
            leg.rotation = Quat::from_rotation_x(-swing * 0.8);
        }
        if let Ok(mut leg) = bones.get_mut(rig.leg_r) {
            leg.rotation = Quat::from_rotation_x(swing * 0.8);
        }
    }
}

pub struct CharPlugin;

impl Plugin for CharPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_npc_art).add_systems(
            Update,
            npc_animation_system
                .in_set(crate::schedule::GameSet::CommonNpc)
                .run_if(in_state(crate::schedule::GameState::Playing)),
        );
    }
}
