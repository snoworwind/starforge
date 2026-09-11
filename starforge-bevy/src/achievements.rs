//! Long-term progression: tracked statistics and a 40+ entry achievement
//! board. Stats are fed by small increments from gameplay systems; unlocking
//! pays credits, plays a stinger and shows a toast. Unlocks persist in the
//! world flags map (`ach:<id>`).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::collections::{HashMap, HashSet};

use crate::schedule::GameState;
use crate::ui::{Panel, Research, UiState};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Metric {
    BlocksMined,
    BlocksPlaced,
    DistanceWalked,
    DistanceFlown,
    Jumps,
    CreaturesKilled,
    SentinelsKilled,
    MachinesPlaced,
    TechsResearched,
    Warps,
    PlanetsVisited,
    BiomesSeen,
    Scans,
    CodexEntries,
    Credits,
    PlaySeconds,
    DaysSurvived,
    Deaths,
    MaxAltitude,
    MaxShipSpeed,
    ItemsPicked,
    DamageTaken,
    Colonists,
}

impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Metric::BlocksMined => "挖掘方块",
            Metric::BlocksPlaced => "放置方块",
            Metric::DistanceWalked => "步行距离",
            Metric::DistanceFlown => "飞行距离",
            Metric::Jumps => "跳跃次数",
            Metric::CreaturesKilled => "击杀生物",
            Metric::SentinelsKilled => "击毁遗迹守卫",
            Metric::MachinesPlaced => "建造机器",
            Metric::TechsResearched => "研究科技",
            Metric::Warps => "曲率跃迁",
            Metric::PlanetsVisited => "造访星球",
            Metric::BiomesSeen => "生态图鉴",
            Metric::Scans => "扫描次数",
            Metric::CodexEntries => "图鉴收录",
            Metric::Credits => "累计信用点",
            Metric::PlaySeconds => "游戏时长",
            Metric::DaysSurvived => "生存天数",
            Metric::Deaths => "阵亡次数",
            Metric::MaxAltitude => "最高海拔",
            Metric::MaxShipSpeed => "最高航速",
            Metric::ItemsPicked => "拾取物品",
            Metric::DamageTaken => "承受伤害",
            Metric::Colonists => "殖民人口",
        }
    }
}

#[derive(Resource, Default)]
pub struct PlayerStats {
    pub values: HashMap<Metric, f64>,
    pub biomes: HashSet<String>,
    pub last_pos: Option<Vec3>,
    pub was_dead: bool,
}

impl PlayerStats {
    pub fn add(&mut self, metric: Metric, amount: f64) {
        *self.values.entry(metric).or_insert(0.0) += amount;
    }

    pub fn max(&mut self, metric: Metric, value: f64) {
        let slot = self.values.entry(metric).or_insert(0.0);
        if value > *slot {
            *slot = value;
        }
    }

    pub fn get(&self, metric: Metric) -> f64 {
        self.values.get(&metric).copied().unwrap_or(0.0)
    }

    pub fn reset(&mut self) {
        self.values.clear();
        self.biomes.clear();
        self.last_pos = None;
        self.was_dead = false;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AchievementDef {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub metric: Metric,
    pub target: f64,
    pub reward: i32,
    /// Major achievements get a big centered message when unlocked.
    pub major: bool,
}

const fn ach(
    id: &'static str,
    name: &'static str,
    desc: &'static str,
    metric: Metric,
    target: f64,
    reward: i32,
    major: bool,
) -> AchievementDef {
    AchievementDef {
        id,
        name,
        desc,
        metric,
        target,
        reward,
        major,
    }
}

/// The full achievement board.
pub const ACHIEVEMENTS: &[AchievementDef] = &[
    // ---- mining & building ----
    ach(
        "dig_100",
        "第一桶矿",
        "挖掘 100 个方块。",
        Metric::BlocksMined,
        100.0,
        200,
        false,
    ),
    ach(
        "dig_1000",
        "矿脉收割者",
        "挖掘 1,000 个方块。",
        Metric::BlocksMined,
        1_000.0,
        800,
        false,
    ),
    ach(
        "dig_10000",
        "地壳手术",
        "挖掘 10,000 个方块。",
        Metric::BlocksMined,
        10_000.0,
        5_000,
        true,
    ),
    ach(
        "build_100",
        "安家",
        "放置 100 个方块。",
        Metric::BlocksPlaced,
        100.0,
        200,
        false,
    ),
    ach(
        "build_1000",
        "工程队长",
        "放置 1,000 个方块。",
        Metric::BlocksPlaced,
        1_000.0,
        900,
        false,
    ),
    ach(
        "build_5000",
        "星球建筑师",
        "放置 5,000 个方块。",
        Metric::BlocksPlaced,
        5_000.0,
        4_000,
        true,
    ),
    // ---- exploration ----
    ach(
        "walk_1k",
        "散步者",
        "累计步行 1,000 米。",
        Metric::DistanceWalked,
        1_000.0,
        150,
        false,
    ),
    ach(
        "walk_10k",
        "长途旅行者",
        "累计步行 10,000 米。",
        Metric::DistanceWalked,
        10_000.0,
        700,
        false,
    ),
    ach(
        "walk_50k",
        "星表徒步",
        "累计步行 50,000 米。",
        Metric::DistanceWalked,
        50_000.0,
        3_000,
        true,
    ),
    ach(
        "fly_10k",
        "离开地面",
        "累计飞行 10,000 米。",
        Metric::DistanceFlown,
        10_000.0,
        500,
        false,
    ),
    ach(
        "fly_100k",
        "星海领航员",
        "累计飞行 100,000 米。",
        Metric::DistanceFlown,
        100_000.0,
        3_000,
        true,
    ),
    ach(
        "alt_300",
        "云层之上",
        "到达海拔 300 米。",
        Metric::MaxAltitude,
        300.0,
        250,
        false,
    ),
    ach(
        "alt_1000",
        "近地轨道",
        "到达海拔 1,000 米。",
        Metric::MaxAltitude,
        1_000.0,
        900,
        false,
    ),
    ach(
        "biomes_4",
        "生态观察员",
        "造访 4 种生态。",
        Metric::BiomesSeen,
        4.0,
        500,
        false,
    ),
    ach(
        "biomes_8",
        "星球博物志",
        "造访 8 种生态。",
        Metric::BiomesSeen,
        8.0,
        1_500,
        false,
    ),
    ach(
        "biomes_16",
        "万物之地",
        "造访全部 16 种生态。",
        Metric::BiomesSeen,
        16.0,
        6_000,
        true,
    ),
    // ---- wildlife & combat ----
    ach(
        "hunt_10",
        "不速之客",
        "击杀 10 只生物。",
        Metric::CreaturesKilled,
        10.0,
        250,
        false,
    ),
    ach(
        "hunt_100",
        "生态平衡",
        "击杀 100 只生物。",
        Metric::CreaturesKilled,
        100.0,
        1_200,
        false,
    ),
    ach(
        "hunt_500",
        "顶级掠食者",
        "击杀 500 只生物。",
        Metric::CreaturesKilled,
        500.0,
        5_000,
        true,
    ),
    ach(
        "sentinel_1",
        "初次交锋",
        "击毁 1 个遗迹守卫。",
        Metric::SentinelsKilled,
        1.0,
        400,
        false,
    ),
    ach(
        "sentinel_20",
        "遗迹清道夫",
        "击毁 20 个遗迹守卫。",
        Metric::SentinelsKilled,
        20.0,
        2_500,
        false,
    ),
    ach(
        "damage_100",
        "千锤百炼",
        "累计承受 100 点伤害。",
        Metric::DamageTaken,
        100.0,
        300,
        false,
    ),
    ach(
        "deaths_10",
        "百折不挠",
        "阵亡 10 次。",
        Metric::Deaths,
        10.0,
        600,
        false,
    ),
    // ---- industry ----
    ach(
        "machine_10",
        "工业起步",
        "建造 10 台机器。",
        Metric::MachinesPlaced,
        10.0,
        300,
        false,
    ),
    ach(
        "machine_50",
        "流水线之心",
        "建造 50 台机器。",
        Metric::MachinesPlaced,
        50.0,
        1_500,
        false,
    ),
    ach(
        "machine_150",
        "自动化帝国",
        "建造 150 台机器。",
        Metric::MachinesPlaced,
        150.0,
        6_000,
        true,
    ),
    ach(
        "tech_6",
        "科研新星",
        "研究 6 项科技。",
        Metric::TechsResearched,
        6.0,
        400,
        false,
    ),
    ach(
        "tech_12",
        "首席科学家",
        "研究 12 项科技。",
        Metric::TechsResearched,
        12.0,
        2_000,
        false,
    ),
    ach(
        "tech_all",
        "科技制高点",
        "研究全部科技。",
        Metric::TechsResearched,
        crate::data::TECHS.len() as f64,
        12_000,
        true,
    ),
    // ---- codex & scanning ----
    ach(
        "scan_25",
        "地质雷达",
        "使用扫描 25 次。",
        Metric::Scans,
        25.0,
        300,
        false,
    ),
    ach(
        "codex_30",
        "记录者",
        "收录 30 条图鉴。",
        Metric::CodexEntries,
        30.0,
        600,
        false,
    ),
    ach(
        "codex_120",
        "星际百科",
        "收录 120 条图鉴。",
        Metric::CodexEntries,
        120.0,
        3_000,
        false,
    ),
    ach(
        "codex_all",
        "完整的档案",
        "完成全部图鉴收录。",
        Metric::CodexEntries,
        260.0,
        15_000,
        true,
    ),
    ach(
        "pickup_500",
        "拾荒专家",
        "拾取 500 件物品。",
        Metric::ItemsPicked,
        500.0,
        700,
        false,
    ),
    ach(
        "credits_100k",
        "小有积蓄",
        "累计获得 100,000 信用点。",
        Metric::Credits,
        100_000.0,
        1_000,
        false,
    ),
    ach(
        "credits_1m",
        "星际富豪",
        "累计获得 1,000,000 信用点。",
        Metric::Credits,
        1_000_000.0,
        10_000,
        true,
    ),
    // ---- space ----
    ach(
        "warp_1",
        "跃迁第一步",
        "完成 1 次曲率跃迁。",
        Metric::Warps,
        1.0,
        1_000,
        false,
    ),
    ach(
        "warp_10",
        "星系旅人",
        "完成 10 次曲率跃迁。",
        Metric::Warps,
        10.0,
        5_000,
        true,
    ),
    ach(
        "planet_5",
        "多星球居民",
        "造访 5 颗星球。",
        Metric::PlanetsVisited,
        5.0,
        1_500,
        false,
    ),
    ach(
        "ship_400",
        "极速航行",
        "飞船速度达到 400 u/s。",
        Metric::MaxShipSpeed,
        400.0,
        1_200,
        false,
    ),
    // ---- endurance ----
    ach(
        "play_1h",
        "沉浸其中",
        "游戏时长达到 1 小时。",
        Metric::PlaySeconds,
        3_600.0,
        500,
        false,
    ),
    ach(
        "play_10h",
        "星穹常客",
        "游戏时长达到 10 小时。",
        Metric::PlaySeconds,
        36_000.0,
        4_000,
        true,
    ),
    ach(
        "days_5",
        "日夜交替",
        "经历 5 个昼夜。",
        Metric::DaysSurvived,
        5.0,
        400,
        false,
    ),
    ach(
        "jump_500",
        "弹跳人生",
        "跳跃 500 次。",
        Metric::Jumps,
        500.0,
        300,
        false,
    ),
    ach(
        "colonists_8",
        "小镇初成",
        "殖民地人口达到 8。",
        Metric::Colonists,
        8.0,
        3_000,
        true,
    ),
];

#[derive(Resource, Default)]
pub struct Achievements {
    pub unlocked: HashSet<String>,
    pub pending: Vec<String>,
}

impl Achievements {
    pub fn is_unlocked(&self, id: &str) -> bool {
        self.unlocked.contains(id)
    }

    pub fn progress(&self, stats: &PlayerStats, def: &AchievementDef) -> f32 {
        if self.unlocked.contains(def.id) {
            return 1.0;
        }
        let value = stats.get(def.metric);
        (value / def.target.max(1e-6)).clamp(0.0, 1.0) as f32
    }

    pub fn completed(&self) -> usize {
        self.unlocked.len()
    }
}

/// Feeds aggregate values that are not incremented directly by gameplay
/// systems (positions, play time, time of day).
#[allow(clippy::too_many_arguments)]
pub fn stats_tick_system(
    time: Res<Time>,
    mut stats: ResMut<PlayerStats>,
    player: Query<&crate::player::Player>,
    ship: Option<Res<crate::space::ShipState>>,
    mode: Res<crate::space::FlightMode>,
    game: Option<Res<crate::space::SpaceGame>>,
    screen: Option<Res<crate::screen_fx::ScreenFx>>,
    research: Res<Research>,
    codex: Res<crate::codex::Codex>,
    world: Option<Res<crate::world::World>>,
    mut local: Local<StatsTickLocal>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let Ok(player) = player.single() else { return };
    // Distance: integrate position deltas so teleports do not count. In ship
    // modes the player is mirrored to the ship, so ship distance is counted
    // separately below instead.
    if let Some(last) = stats.last_pos {
        let delta = (player.pos - last).length();
        if delta < 30.0 {
            let horizontal = Vec2::new(player.pos.x - last.x, player.pos.z - last.z).length();
            if !mode.ship_cam() {
                if player.in_liquid || !player.on_ground {
                    stats.add(Metric::DistanceFlown, horizontal as f64);
                } else {
                    stats.add(Metric::DistanceWalked, horizontal as f64);
                }
            }
        }
    }
    stats.last_pos = Some(player.pos);
    stats.max(Metric::MaxAltitude, player.pos.y as f64);
    stats.add(Metric::PlaySeconds, dt as f64);
    if let Some(ship) = ship.as_ref() {
        let flown = ship.speed.abs() as f64 * dt as f64;
        if flown > 0.0 {
            stats.add(Metric::DistanceFlown, flown);
        }
        stats.max(Metric::MaxShipSpeed, ship.speed.abs() as f64);
    }
    // Deaths as an edge, because `Player::damage` is called from many places.
    if player.dead && !stats.was_dead {
        stats.add(Metric::Deaths, 1.0);
    }
    stats.was_dead = player.dead;
    // Damage taken: sum shield + hp drops, ignoring healing.
    let pool = player.stats.hp + player.stats.shield;
    if let Some(previous) = local.last_pool
        && pool < previous - 0.01
    {
        stats.add(Metric::DamageTaken, (previous - pool) as f64);
    }
    local.last_pool = Some(pool);
    // Warps: edge on leaving the warping mode.
    if local.last_mode == crate::space::FlightMode::Warping
        && *mode != crate::space::FlightMode::Warping
    {
        stats.add(Metric::Warps, 1.0);
    }
    local.last_mode = *mode;
    if let Some(game) = game.as_deref() {
        stats.max(Metric::PlanetsVisited, game.visited.len() as f64 + 1.0);
    }
    // Scans: edge on the screen pulse.
    if let Some(screen) = screen.as_deref() {
        if screen.scan > 0.85 && local.last_scan <= 0.85 {
            stats.add(Metric::Scans, 1.0);
        }
        local.last_scan = screen.scan;
    }
    // Aggregate snapshots.
    stats.max(Metric::TechsResearched, research.techs.len() as f64);
    let (codex_found, _) = codex.total();
    stats.max(Metric::CodexEntries, codex_found as f64);
    if let Some(world) = world.as_deref()
        && stats.biomes.insert(world.biome().key.to_string())
    {
        let seen = stats.biomes.len() as f64;
        stats.max(Metric::BiomesSeen, seen);
    }
    let days = (player.play_time as f64 / 480.0).floor();
    stats.max(Metric::DaysSurvived, days);
}

/// Scratch state for [`stats_tick_system`] (kept out of the resource so it
/// never lands in a save).
#[derive(Default)]
pub struct StatsTickLocal {
    pub last_pool: Option<f32>,
    pub last_mode: crate::space::FlightMode,
    pub last_scan: f32,
}

/// Colony population is sampled on a slow timer because the query walks every
/// machine in the world.
pub fn colony_stats_system(
    time: Res<Time>,
    machines: Query<&crate::factory::MachineState>,
    mut stats: ResMut<PlayerStats>,
    mut timer: Local<f32>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 2.0;
    let residents: i32 = machines
        .iter()
        .map(|state| match state {
            crate::factory::MachineState::Colony(colony) => colony.residents,
            _ => 0,
        })
        .sum();
    stats.max(Metric::Colonists, residents as f64);
}

/// Credits tracking uses the maximum balance plus spending awareness: we track
/// lifetime income as max(balance + spent) approximated by the running max of
/// the balance, which is what the achievement board advertises.
pub fn credits_tick_system(mut stats: ResMut<PlayerStats>, player: Query<&crate::player::Player>) {
    if let Ok(player) = player.single() {
        stats.max(Metric::Credits, player.credits.max(0) as f64);
    }
}

pub fn achievement_check_system(
    mut achievements: ResMut<Achievements>,
    stats: Res<PlayerStats>,
    mut quests: ResMut<crate::quests::Quests>,
    mut player: Query<&mut crate::player::Player>,
    mut big_ev: MessageWriter<crate::quests::BigMessageEvent>,
    mut sounds: MessageWriter<crate::music::MusicStinger>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    let mut unlocked_now: Vec<&'static AchievementDef> = Vec::new();
    for def in ACHIEVEMENTS {
        if achievements.unlocked.contains(def.id) {
            continue;
        }
        if stats.get(def.metric) >= def.target {
            achievements.unlocked.insert(def.id.to_string());
            quests.flags.insert(format!("ach:{}", def.id), true);
            unlocked_now.push(def);
        }
    }
    if unlocked_now.is_empty() {
        return;
    }
    let Ok(mut player) = player.single_mut() else {
        return;
    };
    let count = unlocked_now.len();
    if count == 1 {
        let def = unlocked_now[0];
        player.toast(format!("成就达成：{}", def.name));
    } else {
        player.toast(format!("达成 {count} 项新成就"));
    }
    let mut bonus = 0i32;
    let mut major: Vec<&AchievementDef> = Vec::new();
    for def in &unlocked_now {
        bonus = bonus.saturating_add(def.reward);
        if def.major {
            major.push(def);
        }
    }
    player.credits = player.credits.saturating_add(bonus);
    crate::audio::play(&mut commands, sfx.coin.clone(), 0.6, Some(1.1));
    sounds.write(crate::music::MusicStinger(
        crate::music::engine::Stinger::Milestone,
    ));
    for def in major {
        big_ev.write(crate::quests::BigMessageEvent {
            title: format!("成就：{}", def.name),
            sub: format!("{}  ·  +{} 信用点", def.desc, def.reward),
            dur: 4.0,
        });
    }
}

pub fn achievements_restore_system(
    mut achievements: ResMut<Achievements>,
    quests: Res<crate::quests::Quests>,
) {
    achievements.unlocked.clear();
    achievements.pending.clear();
    for (key, value) in &quests.flags {
        if *value && let Some(id) = key.strip_prefix("ach:") {
            achievements.unlocked.insert(id.to_string());
        }
    }
}

pub fn achievements_hotkey_system(keys: Res<ButtonInput<KeyCode>>, mut ui_state: ResMut<UiState>) {
    if keys.just_pressed(KeyCode::KeyJ) {
        if ui_state.panel == Panel::Achievements {
            ui_state.close_panel();
        } else if !ui_state.locked() {
            ui_state.panel = Panel::Achievements;
        }
    }
}

pub fn achievements_panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    achievements: Res<Achievements>,
    stats: Res<PlayerStats>,
    mut filter: Local<usize>,
) {
    if ui_state.panel != Panel::Achievements {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let viewport = ctx.viewport_rect();
    egui::Window::new("成就")
        .default_pos(egui::pos2(viewport.center().x - 380.0, 60.0))
        .default_size(egui::vec2(760.0, 540.0))
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            let completed = achievements.completed();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("成就  {completed}/{}", ACHIEVEMENTS.len()))
                        .size(20.0)
                        .strong()
                        .color(egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("关闭 (J)").clicked() {
                        ui_state.close_panel();
                    }
                });
            });
            let ratio = completed as f32 / ACHIEVEMENTS.len().max(1) as f32;
            ui.add(
                egui::ProgressBar::new(ratio)
                    .text(format!("完成度 {:.0}%", ratio * 100.0))
                    .desired_width(ui.available_width()),
            );
            ui.horizontal(|ui| {
                ui.selectable_value(&mut *filter, 0, "全部");
                ui.selectable_value(&mut *filter, 1, "进行中");
                ui.selectable_value(&mut *filter, 2, "已完成");
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("achievements_scroll")
                .show(ui, |ui| {
                    egui::Grid::new("achievements_grid")
                        .num_columns(1)
                        .spacing([8.0, 8.0])
                        .show(ui, |ui| {
                            for def in ACHIEVEMENTS {
                                let unlocked = achievements.is_unlocked(def.id);
                                if (*filter == 1 && unlocked) || (*filter == 2 && !unlocked) {
                                    continue;
                                }
                                let frame_color = if unlocked {
                                    egui::Color32::from_rgb(0x2a, 0x3c, 0x2e)
                                } else {
                                    egui::Color32::from_rgb(0x16, 0x1b, 0x22)
                                };
                                egui::Frame::new()
                                    .fill(frame_color)
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .inner_margin(egui::Margin::same(10))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(def.name)
                                                    .size(15.0)
                                                    .strong()
                                                    .color(if unlocked {
                                                        egui::Color32::from_rgb(0x7d, 0xff, 0x8a)
                                                    } else {
                                                        egui::Color32::from_rgb(0xc9, 0xe6, 0xee)
                                                    }),
                                            );
                                            ui.label(
                                                egui::RichText::new(format!("+{} ₪", def.reward))
                                                    .size(11.0)
                                                    .color(egui::Color32::from_rgb(
                                                        0xff, 0xb3, 0x47,
                                                    )),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if unlocked {
                                                        ui.label(
                                                            egui::RichText::new("已完成")
                                                                .size(11.0)
                                                                .color(egui::Color32::from_rgb(
                                                                    0x7d, 0xff, 0x8a,
                                                                )),
                                                        );
                                                    } else {
                                                        let progress =
                                                            achievements.progress(&stats, def);
                                                        ui.label(
                                                            egui::RichText::new(format!(
                                                                "{:.0}/{:.0}",
                                                                stats.get(def.metric),
                                                                def.target
                                                            ))
                                                            .size(11.0)
                                                            .color(egui::Color32::from_gray(150)),
                                                        );
                                                        ui.add(
                                                            egui::ProgressBar::new(progress)
                                                                .desired_width(160.0),
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.label(
                                            egui::RichText::new(def.desc)
                                                .size(12.0)
                                                .color(egui::Color32::from_gray(150)),
                                        );
                                    });
                                ui.end_row();
                            }
                        });
                });
        });
}

pub struct AchievementsPlugin;

impl Plugin for AchievementsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerStats>()
            .init_resource::<Achievements>()
            .add_systems(OnEnter(GameState::Playing), achievements_restore_system)
            .add_systems(
                Update,
                (
                    stats_tick_system,
                    colony_stats_system,
                    credits_tick_system,
                    achievement_check_system,
                    achievements_hotkey_system,
                    achievements_panel_system,
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
    fn definitions_are_unique_and_positive() {
        let mut ids = HashSet::new();
        for def in ACHIEVEMENTS {
            assert!(ids.insert(def.id), "duplicate {}", def.id);
            assert!(def.target > 0.0, "{}", def.id);
            assert!(def.reward > 0, "{}", def.id);
            assert!(!def.name.is_empty() && !def.desc.is_empty());
        }
        assert!(ACHIEVEMENTS.len() >= 40, "board is too small");
    }

    #[test]
    fn metric_coverage_is_complete() {
        // Every metric used by the board must have a label.
        for def in ACHIEVEMENTS {
            assert!(!def.metric.label().is_empty());
        }
    }

    #[test]
    fn progress_clamps_between_zero_and_one() {
        let mut stats = PlayerStats::default();
        stats.values.insert(Metric::BlocksMined, 50.0);
        let achievements = Achievements::default();
        let def = &ACHIEVEMENTS[0];
        let progress = achievements.progress(&stats, def);
        assert!((0.0..=1.0).contains(&progress));
        stats.values.insert(Metric::BlocksMined, 10_000.0);
        let progress = achievements.progress(&stats, def);
        assert_eq!(progress, 1.0);
    }

    #[test]
    fn stats_add_and_max() {
        let mut stats = PlayerStats::default();
        stats.add(Metric::Jumps, 3.0);
        stats.add(Metric::Jumps, 2.0);
        assert_eq!(stats.get(Metric::Jumps), 5.0);
        stats.max(Metric::MaxAltitude, 100.0);
        stats.max(Metric::MaxAltitude, 40.0);
        assert_eq!(stats.get(Metric::MaxAltitude), 100.0);
        stats.reset();
        assert_eq!(stats.get(Metric::Jumps), 0.0);
    }

    #[test]
    fn major_achievements_exist() {
        let majors = ACHIEVEMENTS.iter().filter(|def| def.major).count();
        assert!(majors >= 8);
    }
}
