//! Contextual onboarding: one-shot tips the first time a situation appears,
//! plus a low-key persistent hint above the hotbar that always points at the
//! most useful next action. Seen tips are persisted in the world flags.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::collections::HashSet;

use crate::achievements::PlayerStats;
use crate::player::Player;
use crate::schedule::GameState;
use crate::ui::{Panel, Research, UiState};

#[derive(Resource, Default)]
pub struct Tutorial {
    pub seen: HashSet<String>,
    pub check_timer: f32,
    pub hint: Option<String>,
    pub hint_fade: f32,
}

impl Tutorial {
    fn mark(&mut self, id: &str) {
        self.seen.insert(id.to_string());
    }
}

fn tip_unseen(tutorial: &Tutorial, quests: &crate::quests::Quests, id: &str) -> bool {
    !tutorial.seen.contains(id)
        && !quests
            .flags
            .get(&format!("tip:{id}"))
            .copied()
            .unwrap_or(false)
}

/// Restores seen tips from the world flags on load.
pub fn tutorial_restore_system(mut tutorial: ResMut<Tutorial>, quests: Res<crate::quests::Quests>) {
    tutorial.seen.clear();
    tutorial.hint = None;
    for (key, value) in &quests.flags {
        if *value && let Some(id) = key.strip_prefix("tip:") {
            tutorial.seen.insert(id.to_string());
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn tutorial_system(
    time: Res<Time>,
    mode: Res<crate::space::FlightMode>,
    day: Option<Res<crate::daynight::DayTime>>,
    stats: Res<PlayerStats>,
    research: Res<Research>,
    mut tutorial: ResMut<Tutorial>,
    mut quests: ResMut<crate::quests::Quests>,
    mut player: Query<&mut Player>,
) {
    tutorial.check_timer -= time.delta_secs();
    let Ok(mut player) = player.single_mut() else {
        return;
    };
    if tutorial.check_timer > 0.0 {
        return;
    }
    tutorial.check_timer = 0.6;

    // ---------- One-shot tips ----------
    let mut tip: Option<&'static str> = None;
    if tip_unseen(&tutorial, &quests, "move") && player.play_time > 1.0 {
        tip = Some("WASD 移动 · 空格跳跃（长按喷气）· Shift 疾跑");
    } else if tip_unseen(&tutorial, &quests, "laser")
        && player.hot_idx != -1
        && stats.get(crate::achievements::Metric::BlocksMined) < 1.0
    {
        tip = Some("按 0 或滚轮切换到采矿激光，对准方块按住左键采集");
    } else if tip_unseen(&tutorial, &quests, "inventory")
        && player.inv.slots.iter().flatten().next().is_some()
    {
        tip = Some("按 Tab 打开背包与合成，先把材料做成工具和熔炉");
    } else if tip_unseen(&tutorial, &quests, "craft")
        && player.inv.count_item("carbon") >= 4
        && player.inv.count_item("stone") >= 8
    {
        tip = Some("材料够了：Tab → 合成 → 熔炉，然后放置并投入矿石");
    } else if tip_unseen(&tutorial, &quests, "machine")
        && stats.get(crate::achievements::Metric::MachinesPlaced) >= 1.0
    {
        tip = Some("对着机器按 E 打开面板，右键可放置传送带与箱子");
    } else if tip_unseen(&tutorial, &quests, "scan")
        && research.techs.iter().any(|tech| tech == "scan1")
    {
        tip = Some("研究完成：按 C 脉冲击扫描矿物，标记会持续 25 秒");
    } else if tip_unseen(&tutorial, &quests, "night")
        && day
            .as_deref()
            .is_some_and(|day| crate::daynight::day_factor(day.0) < 0.25)
    {
        tip = Some("夜幕降临：放置光源方块，注意氧气与温度");
    } else if tip_unseen(&tutorial, &quests, "ship")
        && quests.flags.get("shipRepaired").copied().unwrap_or(false)
    {
        tip = Some("飞船已修复：靠近按 E 登船，舱内按 W 点火起飞");
    } else if tip_unseen(&tutorial, &quests, "space") && *mode == crate::space::FlightMode::Space {
        tip = Some("太空飞行：鼠标转向 · W 加速 · J 脉冲引擎 · M 星系图");
    } else if tip_unseen(&tutorial, &quests, "codex")
        && stats.get(crate::achievements::Metric::CodexEntries) >= 10.0
    {
        tip = Some("按 K 查看星际图鉴，按 J 查看成就进度");
    } else if tip_unseen(&tutorial, &quests, "abilities")
        && research.techs.iter().any(|tech| tech == "exo_mobility")
    {
        tip = Some("机动外骨骼解锁：按 Q 冲刺（消耗喷气能量），B/F 夜视与头灯需战术目镜");
    }
    if let Some(text) = tip {
        let id = match text {
            t if t.starts_with("WASD") => "move",
            t if t.starts_with("按 0") => "laser",
            t if t.starts_with("按 Tab") => "inventory",
            t if t.starts_with("材料够了") => "craft",
            t if t.starts_with("对着机器") => "machine",
            t if t.starts_with("研究完成") => "scan",
            t if t.starts_with("夜幕") => "night",
            t if t.starts_with("飞船已修复") => "ship",
            t if t.starts_with("太空飞行") => "space",
            t if t.starts_with("按 K") => "codex",
            _ => "abilities",
        };
        tutorial.mark(id);
        quests.flags.insert(format!("tip:{id}"), true);
        player.toast(text);
    }

    // ---------- Persistent contextual hint ----------
    let mut hint: Option<String> = None;
    if !player.dead && *mode == crate::space::FlightMode::Planet {
        if player.stats.o2 < 30.0 && player.inv.count_item("oxygen") > 0 {
            hint = Some("氧气偏低 — Tab 打开背包点充能，或使用氧气瓶".into());
        } else if player.stats.haz < 30.0 && player.inv.count_item("sodium") > 0 {
            hint = Some("危险防护偏低 — 用钠充能防护系统".into());
        } else if player.stats.hp <= 3.0 && player.inv.count_item("medkit") > 0 {
            hint = Some("生命值危急 — 按 H 使用医疗包".into());
        }
    }
    if hint.is_some() {
        tutorial.hint_fade = (tutorial.hint_fade + time.delta_secs() * 3.0).min(1.0);
    } else {
        tutorial.hint_fade = (tutorial.hint_fade - time.delta_secs() * 2.0).max(0.0);
        if tutorial.hint_fade <= 0.0 {
            tutorial.hint = None;
        }
    }
    if let Some(text) = hint {
        tutorial.hint = Some(text);
    }
}

/// Draws the persistent hint above the hotbar.
pub fn tutorial_hint_system(
    mut contexts: EguiContexts,
    ui_state: Res<UiState>,
    tutorial: Res<Tutorial>,
) {
    if ui_state.locked() || ui_state.panel != Panel::None {
        return;
    }
    let Some(hint) = &tutorial.hint else { return };
    if tutorial.hint_fade <= 0.01 {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("tutorial_hint"),
    ));
    let viewport = ctx.viewport_rect();
    let alpha = (tutorial.hint_fade * 200.0) as u8;
    let pos = egui::pos2(viewport.center().x, viewport.bottom() - 148.0);
    // Soft pill background.
    let galley = painter.layout_no_wrap(
        hint.clone(),
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgba_unmultiplied(220, 235, 245, alpha),
    );
    let rect = egui::Rect::from_center_size(
        pos,
        egui::vec2(galley.size().x + 22.0, galley.size().y + 10.0),
    );
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(8),
        egui::Color32::from_rgba_unmultiplied(10, 16, 24, (alpha as f32 * 0.8) as u8),
    );
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        egui::Color32::WHITE,
    );
}

pub struct TutorialPlugin;

impl Plugin for TutorialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tutorial>()
            .add_systems(OnEnter(GameState::Playing), tutorial_restore_system)
            .add_systems(
                Update,
                tutorial_system
                    .run_if(in_state(GameState::Playing))
                    .run_if(crate::schedule::ground_mode),
            )
            .add_systems(
                Update,
                tutorial_hint_system
                    .in_set(crate::schedule::GameSet::HudMain)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tips_are_only_shown_once() {
        let mut tutorial = Tutorial::default();
        assert!(tip_unseen(
            &tutorial,
            &crate::quests::Quests::default(),
            "move"
        ));
        tutorial.mark("move");
        assert!(!tip_unseen(
            &tutorial,
            &crate::quests::Quests::default(),
            "move"
        ));
    }

    #[test]
    fn world_flags_suppress_tips() {
        let tutorial = Tutorial::default();
        let mut quests = crate::quests::Quests::default();
        quests.flags.insert("tip:move".into(), true);
        assert!(!tip_unseen(&tutorial, &quests, "move"));
    }
}
