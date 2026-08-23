//! 任务线（21 步主线 + 村庄支线）— port of main.js QUESTS / checkQuest / announceQuest.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::data::{self, QuestType};
use crate::player::Player;
use crate::schedule::{GameSet, GameState};

/// 主线旗标（checkQuest 消费）。事件置位与立即检查解耦：event 旗标在事件里
/// 先写入 flags，再由每帧的 quest_tick 检查推进。
#[derive(Resource)]
pub struct Quests {
    pub flags: HashMap<String, bool>,
    pub idx: usize,
    pub side: Option<SideQuest>,
    pub placed: HashMap<String, i32>,
    /// 主线对话框（打字机）
    pub dialog: Option<QuestDialog>,
    /// 村庄支线对话（可交易/领取奖励）
    pub side_dialog: Option<QuestDialog>,
    pub announce_t: f32,
    pub announce: Option<(String, String, f32)>,
    pub done_t: f32,
    /// 村庄支线 NPC（靠近村庄时生成）
    pub villager: Option<Entity>,
    pub villager_pos: Option<Vec3>,
}

impl Default for Quests {
    fn default() -> Self {
        Self {
            flags: HashMap::new(),
            idx: 0,
            side: None,
            placed: HashMap::new(),
            dialog: None,
            side_dialog: None,
            announce_t: 0.0,
            announce: None,
            done_t: 0.0,
            villager: None,
            villager_pos: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SideQuest {
    pub item: String,
    pub need: i32,
    pub reward: i32,
    pub x: i32,
    pub z: i32,
    pub done: bool,
}

#[derive(Clone, Debug)]
pub struct QuestDialog {
    pub name: String,
    pub lines: Vec<String>,
    pub idx: usize,
    pub chars: usize,
    pub on_close: Option<DialogAction>,
}

#[derive(Clone, Debug)]
pub enum DialogAction {
    /// 主线对话播完：设置旗标并检查任务推进
    Flag(String),
    /// 支线对话：交物品领取奖励
    SideReward,
}

/// 方块放置事件（quest placedCount 累计）。
#[derive(Message)]
pub struct PlacedEvent {
    pub block: String,
}

/// 主线旗标事件。
#[derive(Message)]
pub struct FlagEvent {
    pub flag: String,
}

/// 显示大字提示（任务完成/新任务）。
#[derive(Message)]
pub struct BigMessageEvent {
    pub title: String,
    pub sub: String,
    pub dur: f32,
}

impl Quests {
    pub fn current_quest(&self) -> Option<&'static data::Quest> {
        data::QUESTS.get(self.idx)
    }

    pub fn creative(&self, p: &Player) -> bool {
        p.creative()
    }

    pub fn progress(&self, p: &Player) -> Option<String> {
        let q = self.current_quest()?;
        match q.qtype {
            QuestType::Collect => {
                let item = q.item?;
                let have = p.inv.count_item(item);
                Some(format!("{}/{}", have.min(q.n), q.n))
            }
            QuestType::Place if q.n > 1 => {
                let block = q.block?;
                Some(format!(
                    "{}/{}",
                    self.placed.get(block).copied().unwrap_or(0),
                    q.n
                ))
            }
            _ => None,
        }
    }

    /// 检查当前任务是否完成；完成则推进。返回完成的任务标题（用于提示）。
    pub fn check(&mut self, p: &Player, techs: &[String]) -> Option<&'static data::Quest> {
        if p.creative() {
            return None;
        }
        let q = self.current_quest()?;
        let done = match q.qtype {
            QuestType::Collect => q.item.map(|i| p.inv.count_item(i) >= q.n).unwrap_or(false),
            QuestType::Place => {
                let need = if q.n > 0 { q.n } else { 1 };
                q.block
                    .map(|b| self.placed.get(b).copied().unwrap_or(0) >= need)
                    .unwrap_or(false)
            }
            QuestType::Tech => q
                .tech
                .map(|t| techs.iter().any(|x| x == t))
                .unwrap_or(false),
            QuestType::Event => q
                .flag
                .map(|f| self.flags.get(f).copied().unwrap_or(false))
                .unwrap_or(false),
        };
        if done {
            self.idx += 1;
            // q_explore 激活即作废旧旗标（提前着陆不能瞬时完成「新世界」）
            if let Some(nq) = data::QUESTS.get(self.idx)
                && nq.id == "q_explore"
            {
                self.flags.insert("newPlanet".into(), false);
            }
            self.announce = Some((
                format!("任务完成：{}", q.title),
                format!("奖励 ₪{}", 50 + self.idx as i32 * 25),
                3.2,
            ));
            self.announce_t = 2.6; // 2.6s 后广播新任务
            Some(q)
        } else {
            None
        }
    }

    /// 每帧推进 announce 计时：任务完成后延迟播报新任务。
    pub fn tick_announce(&mut self, dt: f32, p: &Player) {
        if self.announce_t > 0.0 {
            self.announce_t -= dt;
            if self.announce_t <= 0.0 {
                if let Some(q) = self.current_quest() {
                    if let Some(d) = q.dialog {
                        self.announce = Some((format!("◈ {}", q.title), d.to_string(), 5.2));
                    } else {
                        self.announce = Some((
                            "◈ 新任务".to_string(),
                            format!("{} — {}", q.title, q.desc),
                            4.0,
                        ));
                    }
                } else if !p.creative() {
                    self.announce = Some((
                        "◈ 第一章 完结".into(),
                        "宇宙没有边界。旅行者，继续前进吧。".into(),
                        5.0,
                    ));
                }
            }
        }
    }
}

/// 科技是否已研究（研究状态在 ui::Research 里，但 Quest 检查需要在无 Research
/// 资源的上下文中可用——通过 Player 上的科技标记）。这里由外部调用方传入。
fn research_has(techs: &[String], tech: &str) -> bool {
    techs.iter().any(|t| t == tech)
}

/// 每帧任务轮询：collect/place/tech 型任务的完成检测 + 播报计时。
pub fn quest_tick_system(
    time: Res<Time>,
    mut quests: ResMut<Quests>,
    mut player: Query<&mut Player>,
    research: Res<crate::ui::Research>,
    mut placed_ev: MessageReader<PlacedEvent>,
    mut flag_ev: MessageReader<FlagEvent>,
    mut big_ev: MessageWriter<BigMessageEvent>,
) {
    let dt = time.delta_secs();
    for ev in placed_ev.read() {
        let n = quests.placed.entry(ev.block.clone()).or_insert(0);
        *n += 1;
    }
    for ev in flag_ev.read() {
        quests.flags.insert(ev.flag.clone(), true);
    }
    let mut announced: Vec<(String, String, f32)> = Vec::new();
    if let Ok(mut p) = player.single_mut() {
        if let Some(q) = quests.check(&p, &research.techs) {
            p.credits += 50 + quests.idx as i32 * 25;
            announced.push((
                format!("任务完成：{}", q.title),
                format!("奖励 ₪{}", 50 + quests.idx as i32 * 25),
                3.2,
            ));
        }
        quests.tick_announce(dt, &p);
    }
    // 主线对话框打字机（JS 26 字符/秒）
    if let Some(d) = quests.dialog.as_mut() {
        d.chars += (dt * 26.0) as usize;
        let cur = &d.lines[d.idx];
        if d.chars > cur.chars().count() + 8 {
            d.chars = cur.chars().count();
        }
    }
    if let Some(d) = quests.side_dialog.as_mut() {
        d.chars += (dt * 26.0) as usize;
        let cur = &d.lines[d.idx];
        if d.chars > cur.chars().count() + 8 {
            d.chars = cur.chars().count();
        }
    }
    // 大字提示
    if let Some((t, s, dur)) = quests.announce.take() {
        announced.push((t, s, dur));
    }
    for (t, s, dur) in announced {
        big_ev.write(BigMessageEvent {
            title: t,
            sub: s,
            dur,
        });
    }
}

/// 村庄支线：玩家靠近村庄时若有村民 NPC，按 E 领取/交付。
pub fn side_quest_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut quests: ResMut<Quests>,
    mut player: Query<&mut Player>,
    ui: Res<crate::ui::UiState>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    let Some(d) = quests.side_dialog.as_ref() else {
        return;
    };
    // 主线对话框优先（同帧不应有两个对话框）
    if quests.dialog.is_some() {
        return;
    }
    let cur = &d.lines[d.idx];
    let fully_shown = d.chars >= cur.chars().count();
    let advance = if keys.just_pressed(KeyCode::KeyE) && !ui.locked() {
        if fully_shown {
            if d.idx + 1 < d.lines.len() {
                true
            } else {
                // 最后一句：执行结算动作
                let action = d.on_close.clone();
                quests.side_dialog = None;
                if let Some(action) = action {
                    match action {
                        DialogAction::SideReward => {
                            if let Ok(mut p) = player.single_mut()
                                && let Some(sq) = quests.side.as_ref()
                                && !sq.done
                            {
                                let have = p.inv.count_item(&sq.item);
                                if have >= sq.need {
                                    p.inv.remove_item(&sq.item, sq.need);
                                    p.credits += sq.reward;
                                    p.toast(format!("村庄感谢你！+₪{}", sq.reward));
                                    if let Some(side) = quests.side.as_mut() {
                                        side.done = true;
                                    }
                                    crate::audio::play(
                                        &mut commands,
                                        sfx.pickup.clone(),
                                        0.5,
                                        None,
                                    );
                                }
                            }
                        }
                        DialogAction::Flag(f) => {
                            quests.flags.insert(f, true);
                        }
                    }
                }
                return;
            }
        } else {
            false
        }
    } else if keys.just_pressed(KeyCode::Escape) && !ui.locked() {
        quests.side_dialog = None;
        return;
    } else {
        false
    };
    if advance && let Some(d) = quests.side_dialog.as_mut() {
        d.idx += 1;
        d.chars = 0;
    }
}

// ---------- 村庄支线（NPC 生成 + 领取委托） ----------

/// 靠近村庄时生成村民 NPC；按 E 领取支线委托。
#[allow(clippy::too_many_arguments)]
pub fn village_side_quest_system(
    mode: Res<crate::space::FlightMode>,
    keys: Res<ButtonInput<KeyCode>>,
    mut quests: ResMut<Quests>,
    player: Query<&Player>,
    world: Option<Res<crate::world::World>>,
    ui: Res<crate::ui::UiState>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    if *mode != crate::space::FlightMode::Planet {
        return;
    }
    let Some(world) = world else { return };
    let Ok(p) = player.single() else { return };
    // 找最近村庄（无限地表：按格哈希查玩家附近区域）
    let mut best: Option<(i32, i32, i32, f32)> = None;
    let (px, pz) = (p.pos.x as i32, p.pos.z as i32);
    for s in world
        .g
        .structures_in_rect(px - 32, pz - 32, px + 32, pz + 32)
    {
        if let crate::world::Structure::Village { x, z, h, .. } = s {
            let dx = p.pos.x - (x as f32 + 0.5);
            let dz = p.pos.z - (z as f32 + 0.5);
            let d = (dx * dx + dz * dz).sqrt();
            if d < 28.0 && best.map(|b| d < b.3).unwrap_or(true) {
                best = Some((x, z, h, d));
            }
        }
    }
    let Some((vx, vz, vh, _d)) = best else {
        // 远离村庄：移除村民
        if let Some(e) = quests.villager.take() {
            commands.entity(e).despawn();
        }
        quests.villager_pos = None;
        return;
    };
    // 村民生成
    let villager_pos = Vec3::new(vx as f32 + 0.5, vh as f32 + 1.0, vz as f32 + 0.5);
    if quests.villager.is_none() {
        let app = crate::save::Appearance::random((vx as u32) ^ (vz as u32));
        let human = crate::char::spawn_humanoid(
            &mut commands,
            &asset_server,
            &app,
            villager_pos,
            std::f32::consts::PI,
        );
        quests.villager = Some(human.root);
        quests.villager_pos = Some(villager_pos);
    }
    // 星球切换保护：村民位置距上次太远（旧星球实体未清理）
    if let Some(op) = quests.villager_pos
        && op.distance(villager_pos) > 100.0
    {
        if let Some(e) = quests.villager.take() {
            commands.entity(e).despawn();
        }
        quests.villager_pos = None;
        return;
    }
    // 对话进行中：不重复开
    if quests.dialog.is_some() || quests.side_dialog.is_some() || ui.locked() {
        return;
    }
    let d = p.pos.distance(villager_pos);
    if d > 3.5 {
        return;
    }
    if !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    match quests.side.as_ref().map(|s| s.done) {
        None => {
            // 生成委托
            let pool = [
                "sodium",
                "carbon",
                "oxygen",
                "coal",
                "iron_ore",
                "copper_ore",
                "stone",
            ];
            let item = pool
                [crate::rng::Rng::new((vx as u32) ^ 0x5EED ^ (vz as u32)).range(pool.len())]
            .to_string();
            let need = 3 + (crate::rng::Rng::new((vz as u32) ^ 0x77).next() * 6.0) as i32;
            let reward = 100 + need * 25;
            quests.side = Some(SideQuest {
                item: item.clone(),
                need,
                reward,
                x: vx,
                z: vz,
                done: false,
            });
            let name = data::item_by_key(&item).map(|i| i.name).unwrap_or(&item);
            quests.side_dialog = Some(QuestDialog {
                name: "村民".into(),
                lines: vec![
                    format!("旅行者！我们村庄急需 {name} ×{need}。"),
                    format!("带回来给你 ₪{reward} 报酬。"),
                    "再按一次 E 交付。".into(),
                ],
                idx: 0,
                chars: 0,
                on_close: Some(DialogAction::SideReward),
            });
        }
        Some(true) => {
            quests.side_dialog = Some(QuestDialog {
                name: "村民".into(),
                lines: vec!["谢谢！".into(), "村庄永远不会忘记你。".into()],
                idx: 0,
                chars: 0,
                on_close: None,
            });
        }
        Some(false) => {
            // 委托进行中：提醒
            let sq = quests.side.as_ref().unwrap();
            quests.side_dialog = Some(QuestDialog {
                name: "村民".into(),
                lines: vec![
                    format!("还差 {} 个，我在这里等你。", sq.item),
                    "采够了再按一次 E 交付。".into(),
                ],
                idx: 0,
                chars: 0,
                on_close: Some(DialogAction::SideReward),
            });
        }
    }
}

/// Quests plugin: quest state machine + the game messages it owns.
pub struct QuestsPlugin;

impl Plugin for QuestsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Quests>()
            .add_message::<PlacedEvent>()
            .add_message::<FlagEvent>()
            .add_message::<BigMessageEvent>()
            .add_systems(
                Update,
                (
                    quest_tick_system,
                    side_quest_system,
                    village_side_quest_system,
                )
                    .chain()
                    .in_set(GameSet::CommonQuests)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_player(creative: bool) -> Player {
        let mut p = Player::new(if creative {
            crate::data::Difficulty::Creative
        } else {
            crate::data::Difficulty::Normal
        });
        p.inv.add_item("carbon", 30);
        p.inv.add_item("sodium", 10);
        p.inv.add_item("stone", 20);
        p
    }

    #[test]
    fn quest_chain_collect_progression() {
        let mut q = Quests::default();
        let p = test_player(false);
        // q_wake 是事件旗标任务
        q.flags.insert("checkedShip".into(), true);
        assert!(q.check(&p, &[]).is_some());
        assert_eq!(q.idx, 1);
        // q_carbon 需 carbon×15（已有 30 → 直接完成）
        assert!(q.check(&p, &[]).is_some());
        assert_eq!(q.idx, 2);
        // q_sodium 需 8
        assert!(q.check(&p, &[]).is_some());
        assert_eq!(q.idx, 3);
        // q_stone 需 12
        assert!(q.check(&p, &[]).is_some());
        assert_eq!(q.idx, 4);
        // q_furnace 是放置任务：未放置不推进
        assert!(q.check(&p, &[]).is_none());
        assert_eq!(q.idx, 4);
    }

    #[test]
    fn quest_event_flag_advances() {
        let mut q = Quests::default();
        let mut p = test_player(false);
        q.flags.insert("checkedShip".into(), true);
        assert!(q.check(&p, &[]).is_some());
        // q_carbon 未满足（清空碳）不推进
        p.inv.remove_item("carbon", 30);
        assert!(q.check(&p, &[]).is_none());
        assert_eq!(q.idx, 1);
    }

    #[test]
    fn creative_skips_quests() {
        let mut q = Quests::default();
        let p = test_player(true);
        q.flags.insert("checkedShip".into(), true);
        assert!(q.check(&p, &[]).is_none());
        assert_eq!(q.idx, 0);
    }

    #[test]
    fn tech_quest_needs_research() {
        let mut q = Quests {
            idx: 7, // q_tech（研究 metallurgy）
            ..Default::default()
        };
        let p = test_player(false);
        assert!(q.check(&p, &[]).is_none());
        assert!(q.check(&p, &["metallurgy".to_string()]).is_some());
        assert_eq!(q.idx, 8);
    }

    #[test]
    fn place_quest_counts_blocks() {
        let mut q = Quests {
            idx: 4, // q_furnace：放置熔炉
            ..Default::default()
        };
        let p = test_player(false);
        assert!(q.check(&p, &[]).is_none());
        q.placed.insert("furnace".into(), 1);
        assert!(q.check(&p, &[]).is_some());
    }

    #[test]
    fn side_quest_reward_consumes_items() {
        let mut q = Quests::default();
        let mut p = test_player(false);
        q.side = Some(SideQuest {
            item: "carbon".into(),
            need: 10,
            reward: 150,
            x: 0,
            z: 0,
            done: false,
        });
        // 模拟侧任务对话框关闭动作（交物品）
        q.side_dialog = Some(QuestDialog {
            name: "村民".into(),
            lines: vec!["交付".into()],
            idx: 0,
            chars: 0,
            on_close: Some(DialogAction::SideReward),
        });
        let before = p.inv.count_item("carbon");
        let d = q.side_dialog.clone().unwrap();
        if let Some(DialogAction::SideReward) = d.on_close {
            let sq = q.side.as_ref().unwrap();
            if !sq.done && p.inv.count_item(&sq.item) >= sq.need {
                p.inv.remove_item(&sq.item, sq.need);
                p.credits += sq.reward;
            }
        }
        assert_eq!(p.inv.count_item("carbon"), before - 10);
        assert_eq!(p.credits, 150);
    }
}
