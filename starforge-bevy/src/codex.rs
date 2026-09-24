//! Discovery codex: a persistent catalogue of blocks, items, biomes, wildlife
//! and machines. Entries unlock automatically as the player encounters them;
//! every fifth discovery grants research data, and completing a category pays
//! a credit bonus. Unlocks are stored in the world flags map so existing saves
//! pick the feature up without a schema change.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::collections::HashSet;

use crate::player::Player;
use crate::schedule::GameState;
use crate::ui::{Panel, UiState};

pub const CATEGORY_NAMES: [&str; 5] = ["方块", "物品", "生态", "生物", "机器"];
const REWARD_EVERY: usize = 5;
const CATEGORY_BONUS: i32 = 2500;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodexCategory {
    Block = 0,
    Item = 1,
    Biome = 2,
    Creature = 3,
    Machine = 4,
}

impl CodexCategory {
    pub const ALL: [CodexCategory; 5] = [
        CodexCategory::Block,
        CodexCategory::Item,
        CodexCategory::Biome,
        CodexCategory::Creature,
        CodexCategory::Machine,
    ];

    pub fn name(self) -> &'static str {
        CATEGORY_NAMES[self as usize]
    }
}

#[derive(Clone, Debug)]
pub struct CodexEntry {
    pub id: String,
    pub name: String,
    pub category: CodexCategory,
    pub detail: String,
    pub hint: String,
}

#[derive(Resource, Default)]
pub struct Codex {
    pub unlocked: HashSet<String>,
    pub scan_timer: f32,
    /// Newly unlocked entries queued for toast/reward processing.
    pub pending: Vec<String>,
    pub category: usize,
    pub selected: usize,
}

impl Codex {
    pub fn is_unlocked(&self, id: &str) -> bool {
        self.unlocked.contains(id)
    }

    pub fn count(&self, category: CodexCategory) -> (usize, usize) {
        let total = crate::codex::entries(category).len();
        let found = crate::codex::entries(category)
            .iter()
            .filter(|entry| self.unlocked.contains(&entry.id))
            .count();
        (found, total)
    }

    pub fn total(&self) -> (usize, usize) {
        let mut found = 0;
        let mut total = 0;
        for category in CodexCategory::ALL {
            let (f, t) = self.count(category);
            found += f;
            total += t;
        }
        (found, total)
    }
}

impl Codex {
    fn flag_key(id: &str) -> String {
        format!("codex:{id}")
    }
}

/// Builds the full catalogue for a category. Cheap enough for UI and scanning
/// (called at most a few times per second).
pub fn entries(category: CodexCategory) -> Vec<CodexEntry> {
    match category {
        CodexCategory::Block => crate::data::BLOCKS
            .iter()
            .map(|block| {
                let detail = if let Some(machine) = block.machine {
                    format!(
                        "机器方块（{}）。硬度 {:.1}s，放置后接入电网或物流网络。",
                        crate::factory::MachineKind::from_block_key(machine).label(),
                        block.hard
                    )
                } else if block.ore {
                    format!("矿石方块。硬度 {:.1}s，挖掘后掉落矿物原料。", block.hard)
                } else if block.glow {
                    format!("发光方块。硬度 {:.1}s，可为基地提供照明。", block.hard)
                } else if block.cross {
                    "可采集植物。徒手或激光采集即可获得材料。".to_string()
                } else if block.liquid {
                    "液体方块。深处会缓降移动速度。".to_string()
                } else {
                    format!("建材方块。硬度 {:.1}s。", block.hard)
                };
                CodexEntry {
                    id: format!("block.{}", block.key),
                    name: block.name.to_string(),
                    category,
                    detail,
                    hint: "挖掘或放置后收录".to_string(),
                }
            })
            .collect(),
        CodexCategory::Item => crate::data::ITEMS
            .iter()
            .map(|item| {
                let detail = if let Some(bonus) = item.equipment {
                    format!(
                        "{} 装备（{} 槽，效果 {} +{:.0}）。",
                        item.desc, bonus.slot, bonus.effect, bonus.amount
                    )
                } else {
                    item.desc.to_string()
                };
                CodexEntry {
                    id: format!("item.{}", item.key),
                    name: item.name.to_string(),
                    category,
                    detail,
                    hint: "获得物品后收录".to_string(),
                }
            })
            .collect(),
        CodexCategory::Biome => crate::data::BIOMES
            .iter()
            .map(|biome| {
                let hazard = match biome.haz {
                    Some(_) => format!("危险：{}（{:.1}/s）。", biome.haz_name, biome.haz_rate),
                    None => "无环境危害。".to_string(),
                };
                let detail = format!(
                    "地形：{}；树木密度 {:.3}，矿物倍率 {:.2}。{}",
                    biome.terrain, biome.trees, biome.ore_mul, hazard
                );
                CodexEntry {
                    id: format!("biome.{}", biome.key),
                    name: biome.name.to_string(),
                    category,
                    detail,
                    hint: "降落或造访后收录".to_string(),
                }
            })
            .collect(),
        CodexCategory::Creature => creature_catalogue(),
        CodexCategory::Machine => crate::data::BLOCKS
            .iter()
            .filter_map(|block| block.machine)
            .map(|machine| {
                let kind = crate::factory::MachineKind::from_block_key(machine);
                let detail = match machine {
                    "furnace" => "使用燃料熔炼矿石，输入矿石与燃料后自动产出锭材。".to_string(),
                    "miner" => "挖掘正下方矿脉，把产物放入自身缓存等待物流取走。".to_string(),
                    "belt" => "以固定间隔向前输送物品；相邻传送带自动连接。".to_string(),
                    "assembler" => "按配方自动装配零件，需要持续供电。".to_string(),
                    "refinery" => "化学精炼高级材料，耗电较高。".to_string(),
                    "chest" => "24 格储物空间，可被传送带与收集点投料。".to_string(),
                    "reactor" => "消耗铀-235 稳定输出 100 kW 电力。".to_string(),
                    "solar" => "白天发电，输出随太阳高度变化。".to_string(),
                    "wind" => "随高度与阵风变化的清洁电力。".to_string(),
                    "burner" => "燃烧碳氢燃料驱动的小型发电机。".to_string(),
                    "battery" => "缓存电网能量，在需求高峰时放电。".to_string(),
                    "pipe" | "pump" | "tank" => "流体网络组件：输送、泵送并储存液体。".to_string(),
                    "turret" => "自动攻击 24 格内敌人，只反击已敌对目标。".to_string(),
                    "colony_core" => "扫描舱室规模并周期产出研究数据与信用点。".to_string(),
                    "medbay" => "附近生命值不足时消耗钠与氧气治疗。".to_string(),
                    "lumberbot" => "自主扫描、砍伐树木并把木材送到收集点。".to_string(),
                    "collector" => "收集附近掉落物并输出到物流线。".to_string(),
                    "geothermal" => "建在火山岩层上输出 45 kW 地热电力。".to_string(),
                    "splitter" => "轮转三向输出，均衡物流线路。".to_string(),
                    "filter" => "按配置物品筛选并定向输送。".to_string(),
                    "cable" => "组建独立局部电网，电缆之间不互相串电。".to_string(),
                    "launchpad" => "停靠其上的飞船可免发射燃料直接起飞。".to_string(),
                    "beacon" => "记录坐标并在全息地图上显示信标。".to_string(),
                    _ => "通用生产机器。".to_string(),
                };
                CodexEntry {
                    id: format!("machine.{}", machine),
                    name: kind.label().to_string(),
                    category,
                    detail,
                    hint: "建造后收录".to_string(),
                }
            })
            .collect(),
    }
}

fn creature_catalogue() -> Vec<CodexEntry> {
    const SPECIES: [(&str, &str, &str); 7] = [
        (
            "strider",
            "漫步兽",
            "性情温和的草原食草生物，受击后会短暂逃跑。",
        ),
        (
            "hopper",
            "跃行兽",
            "弹跳前进的敏捷生物，被激怒时会主动冲撞。",
        ),
        (
            "crab",
            "沙壳兽",
            "厚重甲壳的掘地生物，攻击性强，掉落下壳材料。",
        ),
        ("beetle", "晶甲兽", "缓慢的甲壳生物，可采集晶化甲壳素。"),
        ("manta", "霜绒兽", "漂浮滑行的低温生物，掉落低温晶体。"),
        ("blob", "荧沼软体", "半透明的沼地软体，携带活性酶。"),
        ("sentinel", "遗迹守卫", "守卫遗迹的机械体，炮塔会主动还击。"),
    ];
    SPECIES
        .iter()
        .map(|(kind, name, detail)| CodexEntry {
            id: format!("creature.{kind}"),
            name: name.to_string(),
            category: CodexCategory::Creature,
            detail: detail.to_string(),
            hint: "在野外目击后收录".to_string(),
        })
        .collect()
}

/// Periodically scans the surrounding world and unlocks new entries.
pub fn codex_scan_system(
    time: Res<Time>,
    mut codex: ResMut<Codex>,
    mut quests: ResMut<crate::quests::Quests>,
    player: Query<&Player>,
    world: Option<Res<crate::world::World>>,
    creatures: Query<(&crate::creatures::Creature, &Transform)>,
    machines: Query<&crate::factory::Machine>,
) {
    codex.scan_timer -= time.delta_secs();
    if codex.scan_timer > 0.0 {
        return;
    }
    codex.scan_timer = 0.6;
    let Ok(player) = player.single() else { return };
    let mut candidates: Vec<String> = Vec::new();
    // Biome.
    if let Some(world) = world.as_deref() {
        candidates.push(format!("biome.{}", world.biome().key));
    }
    // Inventory and equipment.
    for slot in player.inv.slots.iter().flatten() {
        candidates.push(format!("item.{}", slot.item));
        if let Some(item) = crate::data::item_by_key(&slot.item)
            && let Some(block) = item.block
        {
            candidates.push(format!("block.{block}"));
        }
    }
    for equipped in player.equipment.equipped() {
        candidates.push(format!("item.{equipped}"));
    }
    // Nearby wildlife.
    for (creature, transform) in &creatures {
        if creature.hp > 0.0 && transform.translation.distance(player.pos) < 52.0 {
            candidates.push(format!("creature.{}", creature.kind));
        }
    }
    // Machines and placed blocks: scan near machines, plus the block underfoot.
    for machine in &machines {
        candidates.push(format!("machine.{}", machine.kind.block_key()));
        candidates.push(format!("block.{}", machine.kind.block_key()));
    }
    // Block underfoot (records the surface the player is standing on).
    if let Some(world) = world.as_deref() {
        let x = player.pos.x.floor() as i32;
        let z = player.pos.z.floor() as i32;
        let below = crate::data::block_by_id(world.get(x, (player.pos.y - 0.5).floor() as i32, z));
        if below.id != crate::data::ids::AIR {
            candidates.push(format!("block.{}", below.key));
        }
    }
    let mut new_entries: Vec<String> = Vec::new();
    for id in candidates {
        if codex.unlocked.contains(&id) {
            continue;
        }
        // Only accept ids that actually exist in the catalogue.
        let exists = match id.split('.').next() {
            Some("block") => crate::data::BLOCKS
                .iter()
                .any(|b| id == format!("block.{}", b.key)),
            Some("item") => crate::data::item_by_key(id.trim_start_matches("item.")).is_some(),
            Some("biome") => crate::data::BIOMES
                .iter()
                .any(|b| id == format!("biome.{}", b.key)),
            Some("creature") => creature_catalogue().iter().any(|e| e.id == id),
            Some("machine") => crate::data::BLOCKS
                .iter()
                .any(|b| b.machine.is_some_and(|m| id == format!("machine.{m}"))),
            _ => false,
        };
        if !exists {
            continue;
        }
        codex.unlocked.insert(id.clone());
        quests.flags.insert(Codex::flag_key(&id), true);
        new_entries.push(id);
    }
    if new_entries.is_empty() {
        return;
    }
    for id in &new_entries {
        codex.pending.push(id.clone());
    }
    // Rewards are applied by `codex_reward_system`, which owns mutable player
    // access; the scan pass stays read-only so both can run in one chain.
}

/// Applies the rewards queued by the scan system (kept separate so the scan
/// system only needs read access to the player).
pub fn codex_reward_system(
    mut codex: ResMut<Codex>,
    mut player: Query<&mut Player>,
    mut big_ev: MessageWriter<crate::quests::BigMessageEvent>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
    mut screen: Option<ResMut<crate::screen_fx::ScreenFx>>,
) {
    if codex.pending.is_empty() {
        return;
    }
    let Ok(mut player) = player.single_mut() else {
        return;
    };
    let discoveries = codex.pending.len();
    let mut names: Vec<String> = Vec::new();
    for id in codex.pending.drain(..) {
        if let Some(entry) = find_entry(&id) {
            names.push(entry.name);
        }
    }
    // Show a compact toast for the batch.
    if discoveries == 1 {
        player.toast(format!(
            "图鉴收录：{}",
            names.first().cloned().unwrap_or_default()
        ));
    } else if discoveries <= 3 {
        player.toast(format!(
            "图鉴收录：{} 等 {discoveries} 项",
            names.join("、")
        ));
    } else {
        player.toast(format!("图鉴收录 {discoveries} 项新发现"));
    }
    crate::audio::play(&mut commands, sfx.research.clone(), 0.4, Some(1.15));
    if let Some(screen) = screen.as_deref_mut() {
        screen.discover();
    }
    // Every fifth total discovery: research data.
    let total = codex.unlocked.len();
    if total.is_multiple_of(REWARD_EVERY) && player.inv.add_item("data", 1) == 1 {
        player.toast("里程碑：图鉴奖励 +1 研究数据");
    }
    // Category completion bonus (checked once per batch).
    for category in CodexCategory::ALL {
        let (found, count) = codex.count(category);
        if found == count
            && count > 0
            && !codex
                .unlocked
                .contains(&format!("bonus.{}", category.name()))
        {
            codex.unlocked.insert(format!("bonus.{}", category.name()));
            player.credits = player.credits.saturating_add(CATEGORY_BONUS);
            big_ev.write(crate::quests::BigMessageEvent {
                title: format!("图鉴完成：{}", category.name()),
                sub: format!("奖励 +{CATEGORY_BONUS} 信用点"),
                dur: 3.4,
            });
        }
    }
}

fn find_entry(id: &str) -> Option<CodexEntry> {
    for category in CodexCategory::ALL {
        if let Some(entry) = entries(category).into_iter().find(|entry| entry.id == id) {
            return Some(entry);
        }
    }
    None
}

/// K toggles the codex panel (L/T/M style).
pub fn codex_hotkey_system(keys: Res<ButtonInput<KeyCode>>, mut ui_state: ResMut<UiState>) {
    if keys.just_pressed(KeyCode::KeyK) {
        if ui_state.panel == Panel::Codex {
            ui_state.close_panel();
        } else if !ui_state.locked() {
            ui_state.panel = Panel::Codex;
        }
    }
}

pub fn codex_panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut codex: ResMut<Codex>,
    player: Query<&Player>,
) {
    if ui_state.panel != Panel::Codex {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let viewport = ctx.viewport_rect();
    egui::Window::new("图鉴")
        .default_pos(egui::pos2(viewport.center().x - 380.0, 60.0))
        .default_size(egui::vec2(760.0, 540.0))
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            let (found, total) = codex.total();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("星际图鉴  {found}/{total}"))
                        .size(20.0)
                        .strong()
                        .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("关闭 (K)").clicked() {
                        ui_state.close_panel();
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                for (index, category) in CodexCategory::ALL.iter().enumerate() {
                    let (f, t) = codex.count(*category);
                    let selected = codex.category == index;
                    let label = format!("{} {f}/{t}", category.name());
                    if ui.selectable_label(selected, label).clicked() {
                        codex.category = index;
                        codex.selected = 0;
                    }
                }
            });
            ui.separator();
            let category = CodexCategory::ALL
                .get(codex.category)
                .copied()
                .unwrap_or(CodexCategory::Block);
            let catalogue = entries(category);
            let unlocked_here = catalogue
                .iter()
                .filter(|entry| codex.is_unlocked(&entry.id))
                .count();
            let ratio = if catalogue.is_empty() {
                0.0
            } else {
                unlocked_here as f32 / catalogue.len() as f32
            };
            ui.add(
                egui::ProgressBar::new(ratio)
                    .text(format!("{}：{unlocked_here}/{}", category.name(), catalogue.len()))
                    .desired_width(ui.available_width()),
            );
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                // Entry list.
                egui::ScrollArea::vertical()
                    .id_salt("codex_list")
                    .max_height(400.0)
                    .show(ui, |ui| {
                        ui.set_min_width(280.0);
                        for (index, entry) in catalogue.iter().enumerate() {
                            let known = codex.is_unlocked(&entry.id);
                            let label = if known {
                                egui::RichText::new(&entry.name).color(egui::Color32::from_rgb(
                                    0xc9, 0xe6, 0xee,
                                ))
                            } else {
                                egui::RichText::new("？？？").color(egui::Color32::from_gray(110))
                            };
                            if ui.selectable_label(codex.selected == index, label).clicked() {
                                codex.selected = index;
                            }
                        }
                    });
                ui.separator();
                // Detail pane.
                if !catalogue.is_empty()
                    && let Some(entry) = catalogue.get(codex.selected.min(catalogue.len() - 1))
                {
                    let known = codex.is_unlocked(&entry.id);
                    ui.vertical(|ui| {
                        if known {
                            ui.label(
                                egui::RichText::new(&entry.name)
                                    .size(18.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
                            );
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&entry.detail).size(13.0));
                        } else {
                            ui.label(
                                egui::RichText::new("尚未收录")
                                    .size(18.0)
                                    .color(egui::Color32::from_gray(120)),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!("收录方式：{}", entry.hint))
                                    .size(13.0)
                                    .color(egui::Color32::from_gray(150)),
                            );
                        }
                    });
                }
            });
            ui.separator();
            let (_found, _) = codex.total();
            if let Ok(player) = player.single() {
                ui.label(
                    egui::RichText::new(format!(
                        "每 {REWARD_EVERY} 项收录奖励研究数据；完成分类奖励 {CATEGORY_BONUS} 信用点。当前信用点：{}",
                        player.credits
                    ))
                    .size(11.0)
                    .color(egui::Color32::from_gray(140)),
                );
            }
        });
}

/// Restores unlocks from the world flags when a save finishes loading.
pub fn codex_restore_system(mut codex: ResMut<Codex>, quests: Res<crate::quests::Quests>) {
    codex.unlocked.clear();
    codex.pending.clear();
    for (key, value) in &quests.flags {
        if *value && let Some(id) = key.strip_prefix("codex:") {
            codex.unlocked.insert(id.to_string());
        }
    }
}

pub struct CodexPlugin;

impl Plugin for CodexPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Codex>()
            .add_systems(OnEnter(GameState::Playing), codex_restore_system)
            .add_systems(
                Update,
                (
                    codex_scan_system,
                    codex_reward_system,
                    codex_hotkey_system,
                    codex_panel_system,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_covers_every_data_entry() {
        let blocks = entries(CodexCategory::Block);
        assert_eq!(blocks.len(), crate::data::BLOCKS.len());
        let items = entries(CodexCategory::Item);
        assert_eq!(items.len(), crate::data::ITEMS.len());
        let biomes = entries(CodexCategory::Biome);
        assert_eq!(biomes.len(), crate::data::BIOMES.len());
        let machines = entries(CodexCategory::Machine);
        let machine_blocks = crate::data::BLOCKS
            .iter()
            .filter(|block| block.machine.is_some())
            .count();
        assert_eq!(machines.len(), machine_blocks);
        let creatures = entries(CodexCategory::Creature);
        assert_eq!(creatures.len(), 7);
    }

    #[test]
    fn entry_ids_are_unique() {
        let mut ids: Vec<String> = Vec::new();
        for category in CodexCategory::ALL {
            for entry in entries(category) {
                assert!(!ids.contains(&entry.id), "duplicate {}", entry.id);
                ids.push(entry.id);
            }
        }
    }

    #[test]
    fn category_counts_track_unlocks() {
        let mut codex = Codex::default();
        let (found, total) = codex.count(CodexCategory::Biome);
        assert_eq!(found, 0);
        assert!(total >= 16);
        codex.unlocked.insert("biome.lush".into());
        let (found, _) = codex.count(CodexCategory::Biome);
        assert_eq!(found, 1);
    }

    #[test]
    fn flag_keys_are_prefixed() {
        assert_eq!(Codex::flag_key("item.carbon"), "codex:item.carbon");
    }
}
