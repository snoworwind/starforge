//! Guild terminal presentation. Mutations are dispatched after drawing so every
//! button sees the same inventory and progression snapshot for the frame.

use crate::{
    frontier::*,
    player::Player,
    quests::Quests,
    ui::{Panel, Research, UiState},
};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

enum Action {
    Accept(usize),
    Deliver(usize),
    Cancel(usize),
    Route(String),
    Advance(String),
    Survey(String),
    Milestone(String),
}

fn cargo(ui: &mut egui::Ui, p: &Player, costs: &[(&str, i32)]) {
    for (id, n) in costs {
        let have = p.inv.count_item(id);
        ui.colored_label(
            if have >= *n {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::LIGHT_YELLOW
            },
            format!("{}  {have}/{n}", item_name(id)),
        );
    }
}

pub fn panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut quests: ResMut<Quests>,
    mut player: Query<&mut Player>,
    research: Res<Research>,
    game: Res<crate::space::SpaceGame>,
) {
    if ui_state.panel != Panel::Frontier {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let Ok(mut p) = player.single_mut() else {
        return;
    };
    let mut action = None;
    let mut tab = ui_state.frontier_tab;
    let mut open = true;
    let f = &quests.frontier;
    let rank = f.rank();
    let screen = ctx.content_rect();
    egui::Window::new(egui::RichText::new("边疆公会 · 探索终端").color(egui::Color32::from_rgb(116, 230, 224)))
        .open(&mut open)
        .frame(egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
            .fill(egui::Color32::from_rgb(14, 24, 36))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(54, 113, 129)))
            .inner_margin(14))
        .default_width(740.0)
        .max_width((screen.width() - 32.0).max(260.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(225, 235, 244));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 7.0);
            ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            ui.style_mut().text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
            ui.horizontal_wrapped(|ui| {
                ui.heading(RANKS[rank].0);
                ui.label(format!("声望 {}  ·  信用点 Cr {}", f.reputation, p.credits));
                ui.label(format!("订单 {}  /  调查 {}/16  /  远征 {}/6", f.completed_orders, f.surveyed.len(), f.completed_routes()));
            });
            if let Some((name, need)) = RANKS.get(rank + 1) {
                let start = RANKS[rank].1;
                ui.add(egui::ProgressBar::new((f.reputation - start) as f32 / (need - start) as f32)
                    .text(format!("下一级：{name} · {}/{need}", f.reputation)));
            } else { ui.label("已获得公会最高头衔 · 继续探索与承接补给订单"); }
            ui.horizontal_wrapped(|ui| {
                for (i, name) in ["补给委托", "远征故事", "生态图鉴", "里程碑"].iter().enumerate() {
                    ui.selectable_value(&mut tab, i, *name);
                }
            });
            ui.separator();
            if p.creative() { ui.colored_label(egui::Color32::LIGHT_YELLOW, "创造模式可浏览内容；生存模式中可接受任务与领取奖励。"); }
            ui.add_enabled_ui(!p.creative() && !p.dead, |ui| {
                egui::ScrollArea::vertical().id_salt("frontier_scroll").max_height((screen.height() - 250.0).max(120.0)).show(ui, |ui| {
                    match tab {
                        0 => {
                            ui.label("最多同时接受 3 单。交付会消耗角色背包中的全部所需物资；结算后该槽位刷新。无时间限制，可跨星系交付。取消不扣物品，也不发奖励。");
                            let available = CONTRACTS.iter().filter(|c| c.rank <= rank && c.unlocked(&research.techs)).count();
                            ui.small(format!("当前已解锁 {available}/{} 种订单；提高公会等级并研究科技可解锁高级供应链。", CONTRACTS.len()));
                            for (slot, offer) in f.offers(game.galaxy.seed, &research.techs).into_iter().enumerate() {
                                let Some(c) = offer else { continue; };
                                ui.push_id(slot, |ui| {
                                    ui.group(|ui| {
                                        ui.set_min_width(ui.available_width());
                                        let accepted = f.active[slot].is_some();
                                        ui.heading(format!("{}{}", if accepted { "进行中 · " } else { "可接取 · " }, c.title));
                                        ui.small(format!("{} · {}级委托", c.client, c.rank + 1));
                                        ui.label(c.desc);
                                        cargo(ui, &p, c.cargo);
                                        ui.label(format!("奖励：Cr {} · 研究数据 ×{} · 声望 +{}", c.credits(), c.rank + 1, 8 + c.rank * 4));
                                        ui.horizontal(|ui| {
                                            if accepted {
                                                if ui.add_enabled(p.inv.has_items(c.cargo), egui::Button::new("交付物资并结算")).clicked() { action = Some(Action::Deliver(slot)); }
                                                if ui.small_button("取消委托").clicked() { action = Some(Action::Cancel(slot)); }
                                            } else if ui.button("接受委托").clicked() { action = Some(Action::Accept(slot)); }
                                        });
                                    });
                                });
                                ui.add_space(8.0);
                            }
                        }
                        1 => {
                            ui.label("每条远征分四阶段。交付阶段消耗物资；建设和事件从该阶段开始计数，调查和研究认可已有成果。切换追踪保留所有路线进度。");
                            for route in EXPEDITIONS {
                                ui.push_id(route.id, |ui| { ui.group(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    let stage = f.routes.get(route.id).map(|p| p.stage);
                                    let done = stage.is_some_and(|s| s >= route.steps.len());
                                    ui.heading(format!("{}{}", if done { "已完成 · " } else { "" }, route.name));
                                    ui.label(route.desc);
                                    ui.small(format!("要求：{} · 终章奖励 Cr {} / 研究数据 ×8 / 声望 +30", RANKS[route.rank].0, 1200 + route.rank * 400));
                                    if let Some(stage) = stage && let Some(step) = route.steps.get(stage) {
                                        ui.label(egui::RichText::new(format!("阶段 {}/4 · {}", stage + 1, step.title)).strong());
                                        ui.label(step.story);
                                        if let Goal::Deliver(costs) = step.goal { cargo(ui, &p, costs); }
                                        let (have, need) = f.route_progress(route, &p, &quests.placed, &research.techs);
                                        ui.add(egui::ProgressBar::new(have as f32 / need.max(1) as f32).text(format!("{have}/{need}")));
                                        if stage + 1 < route.steps.len() { ui.small("本阶段奖励：Cr 200 · 研究数据 ×2 · 声望 +5"); }
                                        if ui.add_enabled(have >= need, egui::Button::new("完成阶段并领取奖励")).clicked() { action = Some(Action::Advance(route.id.into())); }
                                    }
                                    if !done && ui.add_enabled(rank >= route.rank, egui::Button::new(if stage.is_none() { "开始远征" } else if f.tracked.as_deref() == Some(route.id) { "正在追踪" } else { "追踪这条远征" })).clicked() {
                                        action = Some(Action::Route(route.id.into()));
                                    }
                                    ui.collapsing("查看完整路线", |ui| { for (i, step) in route.steps.iter().enumerate() {
                                        ui.label(format!("{}. {} — {}", i + 1, step.title, step.story));
                                        if let Goal::Deliver(costs) = step.goal { ui.small(cargo_label(costs)); }
                                    }});
                                }); });
                                ui.add_space(8.0);
                            }
                        }
                        2 => {
                            ui.label("在地面按 C 扫描。同一生态需要 3 个调查点：同一星球上两两相距至少 64m，也可在不同星球采样。集齐后在此交付样本，每种生态奖励一次。");
                            for biome in crate::data::BIOMES {
                                let Some((item, n, lore)) = survey_spec(biome.key) else { continue; };
                                let count = f.samples.get(biome.key).map_or(0, Vec::len);
                                let done = f.surveyed.contains(biome.key);
                                ui.push_id(biome.key, |ui| { ui.group(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.heading(format!("{}{} · 扫描 {count}/3", if done { "已归档 · " } else { "" }, biome.name));
                                    ui.label(lore);
                                    if !done {
                                        cargo(ui, &p, &[(item, n)]);
                                        let hazardous = biome.haz.is_some();
                                        ui.small(format!("奖励：Cr {} · 研究数据 ×{} · 声望 +15", if hazardous {450} else {250}, if hazardous {6} else {4}));
                                        if ui.add_enabled(count >= 3 && p.inv.count_item(item) >= n, egui::Button::new("提交样本与调查报告")).clicked() { action = Some(Action::Survey(biome.key.into())); }
                                    }
                                }); });
                            }
                        }
                        _ => {
                            ui.label("成就奖励需要手动领取，每项仅可领取一次。事件从本次版本的游戏记录开始累计。");
                            for m in MILESTONES {
                                let count = f.milestone_progress(m);
                                let claimed = f.milestones.contains(m.id);
                                ui.push_id(m.id, |ui| { ui.group(|ui| {
                                    ui.heading(m.name);
                                    ui.label(m.desc);
                                    ui.add(egui::ProgressBar::new(count as f32 / m.need as f32).text(format!("{count}/{}", m.need)));
                                    ui.small(format!("奖励：Cr {} · 研究数据 ×{}", m.credits, m.data));
                                    if ui.add_enabled(count >= m.need && !claimed, egui::Button::new(if claimed { "已领取" } else { "领取奖励" })).clicked() { action = Some(Action::Milestone(m.id.into())); }
                                }); });
                            }
                        }
                    }
                });
            });
            ui.separator();
            ui.small("L / Esc 关闭 · F5 保存 · 地面 C 调查 · M 查看星图与生态 · 奖励空间不足时保留物资和进度");
        });
    ui_state.frontier_tab = tab;
    if !open {
        ui_state.close_panel();
    }
    if let Some(action) = action {
        let credits = p.credits;
        let data = p.inv.count_item("data");
        // Disjoint fields: expedition counters read placement totals while the
        // persisted frontier record is updated.
        let Quests {
            frontier, placed, ..
        } = &mut *quests;
        let result = match action {
            Action::Accept(slot) => frontier.accept(slot, game.galaxy.seed, &research.techs),
            Action::Deliver(slot) => frontier.finish_order(slot, &mut p),
            Action::Cancel(slot) => {
                frontier.cancel(slot);
                Ok(())
            }
            Action::Route(id) => frontier.start_route(&id, placed),
            Action::Advance(id) => frontier.advance_route(&id, &mut p, placed, &research.techs),
            Action::Survey(id) => frontier.submit_survey(&id, &mut p),
            Action::Milestone(id) => frontier.claim_milestone(&id, &mut p),
        };
        match result {
            Ok(()) => {
                if frontier.rank() > rank {
                    p.toast(format!(
                        "公会晋升：{}！新委托与远征已解锁",
                        RANKS[frontier.rank()].0
                    ));
                } else if p.credits > credits {
                    let reward = p.credits - credits;
                    let research_reward = p.inv.count_item("data") - data;
                    p.toast(format!(
                        "公会结算：+Cr {reward} · 研究数据 +{research_reward}"
                    ));
                } else {
                    p.toast("公会记录已更新");
                }
            }
            Err(message) => p.toast(message),
        }
    }
}
