//! Machine blueprints: `Ctrl+C` copies the machines in a 5×5 footprint around
//! the targeted machine, `[` / `]` rotate the clipboard, and `Ctrl+V` pastes
//! it at the placement target, consuming one item per machine.
//!
//! Only machine blocks are copied (never raw terrain), so pasting can never
//! clobber a landscape and the item cost stays predictable.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::audio;
use crate::data;
use crate::player::{Player, W};
use crate::schedule::GameState;
use crate::ui::UiState;
use crate::world::World;

pub const COPY_RADIUS: i32 = 2;
pub const MAX_BLUEPRINT_BLOCKS: usize = 96;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlueprintBlock {
    pub offset: [i32; 3],
    pub key: &'static str,
    pub dir: u8,
}

#[derive(Resource, Default)]
pub struct BlueprintClipboard {
    pub blocks: Vec<BlueprintBlock>,
    /// Rotation in quarter turns (0..3), applied around the Y axis.
    pub rotation: u8,
}

/// Rotates an offset by `quarter_turns` around the Y axis.
pub fn rotate_offset(offset: [i32; 3], quarter_turns: u8) -> [i32; 3] {
    let (mut x, mut z) = (offset[0], offset[2]);
    for _ in 0..quarter_turns % 4 {
        // 90° clockwise when viewed from above: (x, z) -> (z, -x).
        let nx = z;
        let nz = -x;
        x = nx;
        z = nz;
    }
    [x, offset[1], z]
}

/// Rotates a facing (0=E,1=S,2=W,3=N) by quarter turns.
pub fn rotate_dir(dir: u8, quarter_turns: u8) -> u8 {
    (dir + quarter_turns) % 4
}

/// Collects the machines around `center` into a blueprint.
pub fn collect_blueprint(
    machines: impl Iterator<Item = ([i32; 3], &'static str, u8)>,
    center: [i32; 3],
) -> Vec<BlueprintBlock> {
    let mut blocks: Vec<BlueprintBlock> = Vec::new();
    for (pos, key, dir) in machines {
        let offset = [pos[0] - center[0], pos[1] - center[1], pos[2] - center[2]];
        if offset[0].abs() > COPY_RADIUS || offset[2].abs() > COPY_RADIUS || offset[1].abs() > 1 {
            continue;
        }
        blocks.push(BlueprintBlock { offset, key, dir });
        if blocks.len() >= MAX_BLUEPRINT_BLOCKS {
            break;
        }
    }
    blocks
}

/// The item that places a machine block, if any.
pub fn item_for_block(key: &str) -> Option<&'static str> {
    data::ITEMS
        .iter()
        .find(|item| item.block == Some(key))
        .map(|item| item.key)
}

#[allow(clippy::too_many_arguments)]
pub fn blueprint_system(
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<UiState>,
    mut clipboard: ResMut<BlueprintClipboard>,
    mut player: Query<&mut Player>,
    mut world: ResMut<World>,
    machines: Query<&crate::factory::Machine>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
    mut fx: crate::particles::ParticleSystem,
) {
    if ui.locked() {
        return;
    }
    let Ok(mut player) = player.single_mut() else {
        return;
    };
    if player.dead {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    // ---------- Rotate clipboard ----------
    if !clipboard.blocks.is_empty() {
        if keys.just_pressed(KeyCode::BracketLeft) {
            clipboard.rotation = (clipboard.rotation + 3) % 4;
            audio::play(&mut commands, sfx.click.clone(), 0.35, Some(1.2));
            player.toast(format!("蓝图朝向：{}°", clipboard.rotation as i32 * 90));
        }
        if keys.just_pressed(KeyCode::BracketRight) {
            clipboard.rotation = (clipboard.rotation + 1) % 4;
            audio::play(&mut commands, sfx.click.clone(), 0.35, Some(1.2));
            player.toast(format!("蓝图朝向：{}°", clipboard.rotation as i32 * 90));
        }
    }

    // ---------- Copy ----------
    if ctrl && keys.just_pressed(KeyCode::KeyC) {
        let origin = player.eye();
        let dir = player.look_dir();
        let hit = world.raycast(origin, dir, 8.0);
        let Some((cell, _, _)) = hit else {
            player.toast("蓝图复制：请对准一台机器");
            return;
        };
        let center = cell;
        let mut collected: Vec<([i32; 3], &'static str, u8)> = Vec::new();
        for machine in &machines {
            let position = machine.pos;
            let key = machine.kind.block_key();
            collected.push((position, key, machine.dir));
        }
        let blocks = collect_blueprint(collected.into_iter(), center);
        if blocks.is_empty() {
            player.toast("蓝图复制：附近没有机器");
            return;
        }
        let count = blocks.len();
        clipboard.blocks = blocks;
        clipboard.rotation = 0;
        audio::play(&mut commands, sfx.craft.clone(), 0.6, Some(1.3));
        player.toast(format!(
            "已复制蓝图：{count} 台机器（[ / ] 旋转，Ctrl+V 粘贴）"
        ));
        return;
    }

    // ---------- Paste ----------
    if ctrl && keys.just_pressed(KeyCode::KeyV) {
        if clipboard.blocks.is_empty() {
            player.toast("蓝图剪贴板为空（Ctrl+C 复制目标机器）");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
            return;
        }
        let origin = player.eye();
        let dir = player.look_dir();
        let Some((cell, normal, _)) = world.raycast(origin, dir, 6.0) else {
            player.toast("蓝图粘贴：请对准地面或方块");
            return;
        };
        let base = [
            cell[0] + normal[0],
            cell[1] + normal[1],
            cell[2] + normal[2],
        ];
        // Count required items first so a partial paste never eats materials.
        let mut required: Vec<(&'static str, i32)> = Vec::new();
        for block in &clipboard.blocks {
            let Some(item) = item_for_block(block.key) else {
                continue;
            };
            if let Some(entry) = required.iter_mut().find(|(key, _)| *key == item) {
                entry.1 += 1;
            } else {
                required.push((item, 1));
            }
        }
        let missing: Vec<(&'static str, i32)> = required
            .iter()
            .filter(|(item, need)| player.inv.count_item(item) < *need)
            .copied()
            .collect();
        if !missing.is_empty() {
            let list = missing
                .iter()
                .map(|(item, need)| {
                    format!(
                        "{}×{}",
                        data::item_by_key(item).map(|i| i.name).unwrap_or(item),
                        need
                    )
                })
                .collect::<Vec<_>>()
                .join("、");
            player.toast(format!("材料不足：{list}"));
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
            return;
        }
        let mut placed = 0;
        for block in clipboard.blocks.clone() {
            let rotated = rotate_offset(block.offset, clipboard.rotation);
            let target = [
                base[0] + rotated[0],
                base[1] + rotated[1],
                base[2] + rotated[2],
            ];
            if !(1..data::WORLD_H).contains(&target[1]) {
                continue;
            }
            if world.get(target[0], target[1], target[2]) != data::ids::AIR {
                continue;
            }
            // Keep the player from being entombed by their own blueprint.
            let bx0 = (player.pos.x - W).floor() as i32;
            let bx1 = (player.pos.x + W).floor() as i32;
            let by0 = player.pos.y.floor() as i32;
            let by1 = (player.pos.y + crate::player::H).floor() as i32;
            let bz0 = (player.pos.z - W).floor() as i32;
            let bz1 = (player.pos.z + W).floor() as i32;
            if target[0] >= bx0
                && target[0] <= bx1
                && target[1] >= by0
                && target[1] <= by1
                && target[2] >= bz0
                && target[2] <= bz1
            {
                continue;
            }
            let Some(item) = item_for_block(block.key) else {
                continue;
            };
            if player.inv.count_item(item) < 1 || !player.inv.remove_item(item, 1) {
                continue;
            }
            let block_def = data::block_by_key(block.key);
            world.set(target[0], target[1], target[2], block_def.id);
            let rotated_dir = rotate_dir(block.dir, clipboard.rotation);
            if block_def.machine.is_some() {
                crate::factory::spawn_machine(&mut commands, target, block.key, rotated_dir);
            }
            placed += 1;
            fx.emit(
                crate::particles::ParticleStyle::Glow,
                Vec3::new(
                    target[0] as f32 + 0.5,
                    target[1] as f32 + 0.5,
                    target[2] as f32 + 0.5,
                ),
                crate::particles::EmitOptions::default()
                    .count(3)
                    .speed(0.5, 2.0)
                    .spread(2.0)
                    .size_scale(0.7),
            );
        }
        if placed > 0 {
            audio::play(&mut commands, sfx.place.clone(), 0.7, Some(1.1));
            player.toast(format!("蓝图粘贴完成：{placed} 台机器"));
        } else {
            player.toast("蓝图粘贴：目标位置被占用");
        }
    }
}

/// Hint shown in the HUD while a blueprint is in the clipboard.
pub fn blueprint_hint_system(
    mut contexts: EguiContexts,
    ui: Res<UiState>,
    clipboard: Res<BlueprintClipboard>,
) {
    if ui.locked() || clipboard.blocks.is_empty() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("blueprint_hint"),
    ));
    let viewport = ctx.viewport_rect();
    painter.text(
        egui::pos2(viewport.right() - 16.0, viewport.bottom() - 150.0),
        egui::Align2::RIGHT_BOTTOM,
        format!(
            "蓝图 {} 台 · 朝向 {}° · Ctrl+V 粘贴",
            clipboard.blocks.len(),
            clipboard.rotation as i32 * 90
        ),
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(0x7f, 0x9d, 0xb0),
    );
}

pub struct BlueprintPlugin;

impl Plugin for BlueprintPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlueprintClipboard>()
            .add_systems(
                Update,
                blueprint_system.run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                blueprint_hint_system
                    .in_set(crate::schedule::GameSet::HudMain)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_cycles_four_ways() {
        let offset = [2, 0, 0];
        assert_eq!(rotate_offset(offset, 0), [2, 0, 0]);
        assert_eq!(rotate_offset(offset, 1), [0, 0, -2]);
        assert_eq!(rotate_offset(offset, 2), [-2, 0, 0]);
        assert_eq!(rotate_offset(offset, 3), [0, 0, 2]);
        assert_eq!(rotate_offset(offset, 4), [2, 0, 0]);
    }

    #[test]
    fn dir_rotation_wraps() {
        assert_eq!(rotate_dir(0, 0), 0);
        assert_eq!(rotate_dir(3, 1), 0);
        assert_eq!(rotate_dir(3, 3), 2);
    }

    #[test]
    fn collect_filters_by_radius_and_cap() {
        let center = [0, 0, 0];
        let machines = vec![
            ([1, 0, 1], "furnace", 0),
            ([40, 0, 40], "furnace", 0),
            ([0, 0, -2], "belt", 1),
        ];
        let collected = collect_blueprint(machines.into_iter(), center);
        assert_eq!(collected.len(), 2);
        assert!(collected.iter().all(|b| b.offset[0].abs() <= COPY_RADIUS));
    }

    #[test]
    fn every_machine_block_has_a_placing_item() {
        for block in crate::data::BLOCKS.iter().filter(|b| b.machine.is_some()) {
            assert!(
                item_for_block(block.key).is_some(),
                "{} has no item",
                block.key
            );
        }
    }

    #[test]
    fn blueprint_size_is_bounded() {
        let machines: Vec<([i32; 3], &'static str, u8)> = (0..200)
            .map(|i| ([i % 5 - 2, 0, i / 5 % 5 - 2], "belt", 0))
            .collect();
        let collected = collect_blueprint(machines.into_iter(), [0, 0, 0]);
        assert!(collected.len() <= MAX_BLUEPRINT_BLOCKS);
    }
}
