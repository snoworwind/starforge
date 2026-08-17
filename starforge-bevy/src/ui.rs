//! egui UI: HUD, inventory/crafting, tech tree, machine panels, ghost preview, menus.

use crate::audio;
use crate::data;
use crate::factory::{Machine, MachineKind, MachineState};
use crate::inventory::Slot;
use crate::player::Player;
use crate::save::Settings;
use crate::textures::{item_icon, Atlas, IconBuf};
use crate::world::World;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;

// ---------- In-game panel state ----------

#[derive(Resource, Default)]
pub struct UiState {
    pub panel: Panel,
    pub prompt: Option<String>,
    pub selected_inv: Option<usize>,
    /// 大字提示（标题, 副标题, 剩余秒数）
    pub big: Option<(String, String, f32)>,
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum Panel {
    #[default]
    None,
    Inventory,
    Tech,
    Machine(Entity),
    Pause,
    Trade,
    Garage,
    GalaxyMap,
}

impl UiState {
    pub fn locked(&self) -> bool {
        self.panel != Panel::None
    }
}

// ---------- Shared visual assets ----------

#[derive(Resource)]
pub struct IconMaterials {
    pub quad: Handle<Mesh>,
    pub fallback: Handle<StandardMaterial>,
    pub map: HashMap<String, Handle<StandardMaterial>>,
}

#[derive(Resource)]
pub struct IconImages {
    pub map: HashMap<String, Handle<Image>>,
}

/// egui texture ids for item icons (registered once at startup).
#[derive(Resource, Default)]
pub struct EguiIcons(pub HashMap<String, egui::TextureId>);

#[derive(Resource)]
pub struct GhostMat(pub Handle<StandardMaterial>);

#[derive(Component, Clone)]
pub struct Ghost {
    pub pos: Vec3,
    pub scale: Vec3,
    pub ok: bool,
    pub active: bool,
}

/// Build icon images + materials for every item (and the mining laser).
pub fn build_icons(
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> (IconMaterials, IconImages) {
    let atlas = Atlas::build();
    let mut im = IconMaterials {
        quad: meshes.add(Plane3d::default().mesh().size(0.46, 0.46)),
        fallback: materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.8, 0.8),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        map: HashMap::new(),
    };
    let mut ii = IconImages { map: HashMap::new() };
    let mut keys: Vec<String> = data::ITEMS.iter().map(|i| i.key.to_string()).collect();
    keys.push("laser".into());
    for key in keys {
        let icon = item_icon(&atlas, &key);
        let buf = icon_to_bytes(&icon, 32);
        let image = Image::new(
            bevy::render::render_resource::Extent3d {
                width: 32,
                height: 32,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            buf,
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        let img_handle = images.add(image);
        let mat = materials.add(StandardMaterial {
            base_color_texture: Some(img_handle.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        im.map.insert(key.clone(), mat);
        ii.map.insert(key, img_handle);
    }
    (im, ii)
}

fn icon_to_bytes(icon: &IconBuf, size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            out.extend_from_slice(&icon[y][x]);
        }
    }
    out
}

/// Look up a cached egui icon texture (registered at startup).
pub fn egui_icon(cache: &EguiIcons, key: &str) -> egui::TextureId {
    cache
        .0
        .get(key)
        .or_else(|| cache.0.get("fallback"))
        .copied()
        .unwrap_or(egui::TextureId::default())
}

// ---------- Ghost preview ----------

pub fn ghost_system(
    mut q: Query<(&Ghost, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    ghost_mat: Res<GhostMat>,
    time: Res<Time>,
) {
    let breath = 0.16 + (time.elapsed_secs() * 6.0).sin() * 0.07;
    let mut active = false;
    let mut ok = true;
    for (g, mut tf, mut vis) in &mut q {
        if !g.active {
            *vis = Visibility::Hidden;
            continue;
        }
        active = true;
        ok = g.ok;
        *vis = Visibility::Visible;
        tf.translation = g.pos;
        tf.scale = g.scale;
    }
    if let Some(mut m) = materials.get_mut(&ghost_mat.0) {
        m.base_color = if active {
            if ok {
                Color::srgba(0.21, 0.88, 0.91, breath)
            } else {
                Color::srgba(1.0, 0.27, 0.27, breath)
            }
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.0)
        };
    }
}

// ---------- Research state ----------

#[derive(Resource, Default)]
pub struct Research {
    pub techs: Vec<String>,
    pub active: Option<(String, f32)>, // (tech id, progress seconds)
}

pub fn research_system(time: Res<Time>, mut research: ResMut<Research>, mut player: Query<&mut Player>) {
    let mut completed: Option<String> = None;
    if let Some((id, prog)) = research.active.as_mut() {
        *prog += time.delta_secs();
        if let Some(tech) = data::TECHS.iter().find(|t| t.id == id) {
            if *prog >= tech.time {
                completed = Some(id.clone());
            }
        }
    }
    if let Some(id) = completed {
        research.active = None;
        research.techs.push(id.clone());
        if let Ok(mut p) = player.single_mut() {
            if let Some(tech) = data::TECHS.iter().find(|t| t.id == id) {
                p.toast(format!("科技解锁：{}", tech.name));
            }
        }
    }
}

// ---------- HUD ----------

pub fn hud_system(
    mut contexts: EguiContexts,
    cache: Res<EguiIcons>,
    player: Query<&Player>,
    world: Option<Res<World>>,
    ui: ResMut<UiState>,
    research: Res<Research>,
    time: Res<Time>,
    settings: Res<Settings>,
    space: Res<crate::daynight::SpaceFactor>,
    day: Res<crate::daynight::DayTime>,
    mode: Res<crate::space::FlightMode>,
    ship: Option<Res<crate::space::ShipState>>,
    game: Option<Res<crate::space::SpaceGame>>,
    quests: Option<Res<crate::quests::Quests>>,
    station: Option<Res<crate::station::StationState>>,
    power: Res<crate::factory::Power>,
) {
    let Ok(p) = player.single() else { return };
    if ui.panel != Panel::None {
        return;
    }
    let ctx = contexts.ctx_mut().expect("egui primary context");
    if !egui_fonts_ready(ctx) {
        return;
    }
    let screen = ctx.content_rect();
    let flying = *mode != crate::space::FlightMode::Planet && *mode != crate::space::FlightMode::Seated;

    // crosshair
    egui::Area::new(egui::Id::new("crosshair"))
        .fixed_pos(screen.center())
        .interactable(false)
        .show(ctx, |ui| {
            let c = screen.center();
            let painter = ui.painter();
            let col = egui::Color32::from_rgba_unmultiplied(0xEA, 0xFC, 0xFF, 0xCC);
            painter.line_segment(
                [c + egui::vec2(-7.0, 0.0), c + egui::vec2(-2.0, 0.0)],
                egui::Stroke::new(2.0, col),
            );
            painter.line_segment(
                [c + egui::vec2(2.0, 0.0), c + egui::vec2(7.0, 0.0)],
                egui::Stroke::new(2.0, col),
            );
            painter.line_segment(
                [c + egui::vec2(0.0, -7.0), c + egui::vec2(0.0, -2.0)],
                egui::Stroke::new(2.0, col),
            );
            painter.line_segment(
                [c + egui::vec2(0.0, 2.0), c + egui::vec2(0.0, 7.0)],
                egui::Stroke::new(2.0, col),
            );
        });

    // hotbar
    let slot_px = 48.0;
    let total = slot_px * 10.0 + 11.0 * 4.0;
    let origin = egui::pos2(screen.center().x - total / 2.0, screen.max.y - slot_px - 24.0);
    egui::Area::new(egui::Id::new("hotbar"))
        .fixed_pos(origin)
        .interactable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let selected = p.hot_idx == -1;
                draw_slot(ui, &cache, "laser", None, selected, slot_px);
                for i in 0..9 {
                    let s = p.inv.slots[i].clone();
                    let sel = p.hot_idx == i as i32;
                    let key = s
                        .as_ref()
                        .map(|s| s.item.as_str())
                        .unwrap_or("")
                        .to_string();
                    draw_slot(ui, &cache, &key, s, sel, slot_px);
                }
            });
        });

    // vitals top-left
    egui::Area::new(egui::Id::new("vitals"))
        .fixed_pos(egui::pos2(12.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!("❤ {:.0}/8   🛡 {:.0}/6", p.stats.hp.ceil(), p.stats.shield.ceil()))
                    .size(18.0)
                    .strong(),
            );
            ui.label(egui::RichText::new(format!("⭕ O₂ {:.0}%", p.stats.o2)).size(14.0));
            ui.label(
                egui::RichText::new(format!(
                    "🛡 防护 {:.0}%   🚀 喷气 {:.0}%   ⚡ 激光 {:.0}%",
                    p.stats.haz, p.stats.jet, p.stats.laser
                ))
                .size(14.0),
            );
            if let Some(g) = game.as_ref() {
                ui.label(
                    egui::RichText::new(format!(
                        "₪ {}   ·   {}",
                        p.credits, g.galaxy.name
                    ))
                    .size(14.0)
                    .color(egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
                );
                if !flying {
                    ui.label(
                        egui::RichText::new(format!("第 {} 颗星球 · 已到访 {}", g.galaxy.planets.len(), g.galaxy_count))
                            .size(13.0),
                    );
                }
            }
            if let Some(w) = world.as_ref() {
                let b = w.biome();
                ui.label(
                    egui::RichText::new(format!(
                        "{} {}  ({:.0}, {:.0}, {:.0})",
                        b.name, b.haz_name, p.pos.x, p.pos.y, p.pos.z
                    ))
                    .size(13.0),
                );
            }
            let hh = (day.0 * 24.0) as i32;
            let mm = ((day.0 * 24.0 * 60.0) as i32) % 60;
            ui.label(egui::RichText::new(format!("⏰ {:02}:{:02}", hh, mm)).size(13.0));
            if space.0 > 0.01 && !flying {
                ui.label(
                    egui::RichText::new(format!("🛰 轨道高度 {:.0}%", space.0 * 100.0)).size(13.0),
                );
            }
            // 电力
            ui.label(
                egui::RichText::new(format!("⚡ {} / {:.0} kW", power.generation, power.used))
                    .size(13.0)
                    .color(if power.sat < 0.99 { egui::Color32::from_rgb(0xff, 0x55, 0x55) } else { egui::Color32::from_rgb(0xff, 0xb3, 0x47) }),
            );
        });

    // 任务日志 top-right
    if let Some(qs) = quests.as_ref() {
        egui::Area::new(egui::Id::new("quests"))
            .fixed_pos(egui::pos2(screen.max.x - 262.0, 12.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("◈ 任务日志")
                        .size(13.0)
                        .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                );
                let lo = qs.idx.saturating_sub(1);
                let hi = (qs.idx + 1).min(data::QUESTS.len());
                for i in lo..hi {
                    let q = &data::QUESTS[i];
                    let done = i < qs.idx;
                    let text = if done {
                        format!("✓ {}", q.title)
                    } else if i == qs.idx {
                        match qs.progress(&p) {
                            Some(pr) => format!("▸ {} · {}", q.title, pr),
                            None => format!("▸ {}", q.title),
                        }
                    } else {
                        q.title.to_string()
                    };
                    let mut rt = egui::RichText::new(text)
                        .size(12.0)
                        .color(if done { egui::Color32::from_rgb(0x7d, 0xff, 0x8a) } else { egui::Color32::WHITE });
                    if done {
                        rt = rt.strikethrough();
                    }
                    ui.label(rt);
                }
                if let Some(sq) = &qs.side {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "✦ 村庄委托：{} ×{}（奖励 ₪{}）{}/{}",
                            item_name(&sq.item),
                            sq.need,
                            sq.reward,
                            p.inv.count_item(&sq.item).min(sq.need),
                            sq.need
                        ))
                        .size(12.0)
                        .color(egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
                    );
                }
            });
    }

    // 太空 HUD
    if flying {
        if let Some(s) = ship.as_ref() {
            // 速度表
            egui::Area::new(egui::Id::new("speedo"))
                .fixed_pos(egui::pos2(screen.center().x - 60.0, screen.max.y - 96.0))
                .interactable(false)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new(format!("{:.0}", s.speed)).size(38.0).strong().color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)));
                });
            // 高度/大气层顶
            let alt_text = match *mode {
                crate::space::FlightMode::Atmo | crate::space::FlightMode::AtmoLand => {
                    let gh = world.as_ref().map(|w| w.top_at(s.pos.x.floor() as i32, s.pos.z.floor() as i32) as f32).unwrap_or(32.0);
                    format!("高度 {:.0}m · 大气层顶 {:.0}m", (s.pos.y - gh).max(0.0), (crate::space::EXIT_Y - s.pos.y).max(0.0))
                }
                _ => {
                    let mut nearest = "—".to_string();
                    if let Some(g) = game.as_ref() {
                        let pv = g.planet();
                        let d = s.pos.distance(Vec3::from(pv.pos)) - pv.radius;
                        nearest = format!("{} · {:.0}u", pv.name, d.max(0.0));
                    }
                    nearest
                }
            };
            egui::Area::new(egui::Id::new("pulsehint"))
                .fixed_pos(egui::pos2(screen.max.x - 280.0, screen.max.y - 96.0))
                .interactable(false)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(alt_text)
                            .size(13.0)
                            .color(egui::Color32::from_rgb(0xff, 0xb3, 0x47)),
                    );
                    if s.pulse_charge > 0.0 && s.pulse_charge < 1.0 {
                        ui.label(
                            egui::RichText::new(format!("脉冲充能 {:.0}%", s.pulse_charge * 100.0))
                                .size(13.0)
                                .color(egui::Color32::from_rgb(0xff, 0xb3, 0x47)),
                        );
                    }
                });
            // 氚
            egui::Area::new(egui::Id::new("tritium"))
                .fixed_pos(egui::pos2(60.0, screen.max.y - 96.0))
                .interactable(false)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(format!("◇ 氚 {}", p.inv.count_item("tritium")))
                            .size(14.0)
                            .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                    );
                    ui.label(
                        egui::RichText::new(format!("⟠ 曲率电池 {}", p.inv.count_item("warpcell")))
                            .size(14.0)
                            .color(egui::Color32::from_rgb(0xb4, 0x8c, 0xff)),
                    );
                });
            // 跃迁锁定
            if let Some(g) = game.as_ref() {
                if let Some(lock) = &g.warp_lock {
                    egui::Area::new(egui::Id::new("warplock"))
                        .fixed_pos(egui::pos2(screen.center().x - 120.0, 70.0))
                        .interactable(false)
                        .show(ctx, |ui| {
                            ui.label(
                                egui::RichText::new(format!("◎ 已锁定 {}", lock.name))
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                            );
                            if s.pulsing && s.speed >= crate::space::WARP_ENGAGE_SPEED {
                                ui.label(
                                    egui::RichText::new("跃迁条件满足 — 对准目标星系")
                                        .size(12.0)
                                        .color(egui::Color32::from_rgb(0x7d, 0xff, 0x8a)),
                                );
                            }
                        });
                }
            }
            // 操作提示
            egui::Area::new(egui::Id::new("flighthint"))
                .fixed_pos(egui::pos2(screen.center().x - 220.0, screen.max.y - 60.0))
                .interactable(false)
                .show(ctx, |ui| {
                    let hint = match *mode {
                        crate::space::FlightMode::Atmo => "W/S 油门 · Shift 加力 · A/D 滚转 · E 降落 · 拉升冲出大气层",
                        crate::space::FlightMode::Space => "W/S 油门 · Shift 加力 · J 脉冲 · C 扫描 · M 星系图 · E 泊入空间站 · 冲向星球再入",
                        _ => "",
                    };
                    ui.label(
                        egui::RichText::new(hint)
                            .size(12.0)
                            .color(egui::Color32::from_rgba_unmultiplied(0xc9, 0xe6, 0xee, 0xcc)),
                    );
                });
        }
    }

    // toasts top-center
    if !p.toasts.is_empty() {
        egui::Area::new(egui::Id::new("toasts"))
            .fixed_pos(egui::pos2(screen.center().x - 150.0, 90.0))
            .interactable(false)
            .show(ctx, |ui| {
                for (text, t) in &p.toasts {
                    let alpha = (t / 3.0).clamp(0.0, 1.0);
                    ui.label(
                        egui::RichText::new(text.clone())
                            .size(16.0)
                            .color(egui::Color32::from_white_alpha((alpha * 255.0) as u8)),
                    );
                }
            });
    }

    // prompt (interact hint) above hotbar
    if let Some(prompt) = &ui.prompt {
        egui::Area::new(egui::Id::new("prompt"))
            .fixed_pos(egui::pos2(screen.center().x - 120.0, screen.max.y - slot_px - 52.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(prompt.clone())
                        .size(15.0)
                        .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                );
            });
    }

    // fps
    if settings.show_fps {
        egui::Area::new(egui::Id::new("fps"))
            .fixed_pos(egui::pos2(screen.max.x - 90.0, 10.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(format!("{:.0} fps", 1.0 / time.delta_secs().max(1e-6)));
            });
    }

    // research progress
    if let Some((id, prog)) = &research.active {
        if let Some(tech) = data::TECHS.iter().find(|t| t.id == *id) {
            egui::Area::new(egui::Id::new("research"))
                .fixed_pos(egui::pos2(screen.center().x - 120.0, screen.max.y - 80.0))
                .interactable(false)
                .show(ctx, |ui| {
                    ui.label(format!("研究中：{} {:.0}%", tech.name, prog / tech.time * 100.0));
                });
        }
    }

    // 对话箱（主线/支线/站内）
    let mut dialog: Option<(String, String, usize, usize)> = None;
    if let Some(qs) = quests.as_ref() {
        if let Some(d) = &qs.dialog {
            let cur = &d.lines[d.idx];
            dialog = Some((d.name.clone(), cur.clone(), d.chars, cur.chars().count()));
        } else if let Some(d) = &qs.side_dialog {
            let cur = &d.lines[d.idx];
            dialog = Some((d.name.clone(), cur.clone(), d.chars, cur.chars().count()));
        }
    }
    if dialog.is_none() {
        if let Some(st) = station.as_ref() {
            if let Some(d) = &st.dlg {
                let cur = &d.lines[d.idx];
                dialog = Some((d.name.clone(), cur.clone(), d.chars, cur.chars().count()));
            }
        }
    }
    if let Some((name, text, chars, total)) = dialog {
        let shown: String = text.chars().take(chars).collect();
        let full = chars >= total;
        egui::Area::new(egui::Id::new("dialogbox"))
            .fixed_pos(egui::pos2(screen.center().x - 320.0, screen.max.y - 190.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(0x06, 0x0d, 0x16, 0xf0))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x0e, 0x6d, 0x78)))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_min_width(616.0);
                        ui.label(
                            egui::RichText::new(name.clone())
                                .size(14.0)
                                .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                        );
                        ui.label(
                            egui::RichText::new(shown)
                                .size(15.0)
                                .color(egui::Color32::from_rgb(0xc9, 0xe6, 0xee)),
                        );
                        if full {
                            ui.label(
                                egui::RichText::new("▼ 按 E 继续")
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(0xff, 0xb3, 0x47)),
                            );
                        }
                    });
            });
    }

    // 大字提示
    if let Some((title, sub, _t)) = &ui.big {
        egui::Area::new(egui::Id::new("bigmsg"))
            .fixed_pos(egui::pos2(screen.center().x - 300.0, screen.height() * 0.30))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_width(600.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(title.clone())
                            .size(30.0)
                            .strong()
                            .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                    );
                    ui.label(
                        egui::RichText::new(sub.clone())
                            .size(15.0)
                            .color(egui::Color32::from_rgb(0xff, 0xb3, 0x47)),
                    );
                });
            });
    }
}

/// 大字提示消息接收 + 计时衰减。
pub fn big_message_system(
    time: Res<Time>,
    mut ui: ResMut<UiState>,
    mut ev: MessageReader<crate::quests::BigMessageEvent>,
) {
    let dt = time.delta_secs();
    if let Some((_, _, t)) = ui.big.as_mut() {
        *t -= dt;
        if *t <= 0.0 {
            ui.big = None;
        }
    }
    for e in ev.read() {
        ui.big = Some((e.title.clone(), e.sub.clone(), e.dur));
    }
}

fn draw_slot(
    ui: &mut egui::Ui,
    cache: &EguiIcons,
    key: &str,
    slot: Option<Slot>,
    selected: bool,
    size: f32,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(4),
        if selected {
            egui::Color32::from_rgb(0x23, 0x4a, 0x5e)
        } else {
            egui::Color32::from_rgba_unmultiplied(0x10, 0x14, 0x1a, 0xCC)
        },
    );
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x35, 0x46, 0x55)),
        egui::StrokeKind::Inside,
    );
    if !key.is_empty() {
        let tex = egui_icon(cache, key);
        let pad = size * 0.1;
        let img_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + pad, rect.min.y + pad),
            egui::pos2(rect.max.x - pad, rect.max.y - pad),
        );
        painter.image(
            tex,
            img_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    if let Some(s) = slot {
        painter.text(
            egui::pos2(rect.max.x - 4.0, rect.max.y - 14.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{}", s.n),
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        );
    }
}

fn slot_button(
    ui: &mut egui::Ui,
    cache: &EguiIcons,
    key: &str,
    slot: &Option<Slot>,
    selected: bool,
    size: f32,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(3),
        if selected {
            egui::Color32::from_rgb(0x23, 0x4a, 0x5e)
        } else {
            egui::Color32::from_rgba_unmultiplied(0x10, 0x14, 0x1a, 0xCC)
        },
    );
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(3),
        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x35, 0x46, 0x55)),
        egui::StrokeKind::Inside,
    );
    if !key.is_empty() {
        let tex = egui_icon(cache, key);
        let pad = size * 0.08;
        let img_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + pad, rect.min.y + pad),
            egui::pos2(rect.max.x - pad, rect.max.y - pad),
        );
        painter.image(
            tex,
            img_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    if let Some(s) = slot {
        painter.text(
            egui::pos2(rect.max.x - 3.0, rect.max.y - 12.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{}", s.n),
            egui::FontId::proportional(11.0),
            egui::Color32::WHITE,
        );
    }
    resp.clicked()
}

// ---------- Inventory panel ----------

pub fn inventory_panel_system(
    mut contexts: EguiContexts,
    cache: Res<EguiIcons>,
    mut player: Query<&mut Player>,
    mut ui_state: ResMut<UiState>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
    research: Res<Research>,
) {
    if ui_state.panel != Panel::Inventory {
        return;
    }
    let ctx = contexts.ctx_mut().expect("egui primary context");
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut craft_request: Option<usize> = None;
    let mut sort_request = false;
    let mut charge_request: Option<&'static str> = None;
    let mut tab: usize = 0;
    let mut click_slot: Option<usize> = None;

    // snapshot
    let credits = player.single().map(|p| p.credits).unwrap_or(0);
    let inv_snapshot = player.single().map(|p| p.inv.slots.clone()).unwrap_or_default();
    let sel = ui_state.selected_inv;

    egui::Window::new("背包与合成")
        .default_size([720.0, 470.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut !close)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (Tab)").clicked() {
                close = true;
            }
            ui.horizontal(|ui| {
                if ui.button("🧹 整理").clicked() {
                    sort_request = true;
                }
                ui.label(format!("₪ {credits}"));
            });
            ui.columns(2, |cols| {
                // left: inventory grid
                cols[0].label("物品栏 (前 9 格为快捷栏)");
                egui::Grid::new("inv_grid")
                    .num_columns(9)
                    .spacing([4.0, 4.0])
                    .show(&mut cols[0], |ui| {
                        for i in 0..36 {
                            let s = inv_snapshot[i].clone();
                            let is_sel = sel == Some(i);
                            let clicked = slot_button(
                                ui,
                                &cache,
                                s.as_ref().map(|s| s.item.as_str()).unwrap_or(""),
                                &s,
                                is_sel,
                                40.0,
                            );
                            if clicked {
                                click_slot = Some(i);
                            }
                            if (i + 1) % 9 == 0 {
                                ui.end_row();
                            }
                        }
                    });
                cols[0].horizontal(|ui| {
                    for (sys, item, cost, gain) in data::CHARGE_DEFS {
                        let label = match *sys {
                            "laser" => "⚡",
                            "shield" => "🛡",
                            "hp" => "❤",
                            "o2" => "⭕",
                            _ => "🧪",
                        };
                        let name = data::item_by_key(item).map(|i| i.name).unwrap_or(item);
                        if ui
                            .button(format!("{label} {name}×{cost} → +{gain:.0}"))
                            .clicked()
                        {
                            charge_request = Some(sys);
                        }
                    }
                });

                // right: crafting
                cols[1].label("合成 (T = 科技树)");
                cols[1].horizontal(|ui| {
                    if ui.selectable_label(tab == 0, "全部").clicked() {
                        tab = 0;
                    }
                    if ui.selectable_label(tab == 1, "材料").clicked() {
                        tab = 1;
                    }
                    if ui.selectable_label(tab == 2, "机器").clicked() {
                        tab = 2;
                    }
                    if ui.selectable_label(tab == 3, "方块").clicked() {
                        tab = 3;
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(330.0)
                    .show(&mut cols[1], |ui| {
                        for (idx, r) in data::RECIPES.iter().enumerate() {
                            if r.station != "hand" {
                                continue;
                            }
                            let Some(out_item) = data::item_by_key(r.output.0) else { continue };
                            let show = match tab {
                                1 => out_item.cat == "mat",
                                2 => out_item.cat == "mach",
                                3 => out_item.cat == "blk",
                                _ => true,
                            };
                            if !show {
                                continue;
                            }
                            if let Some(t) = r.tech {
                                if !research.techs.iter().any(|x| x == t) {
                                    continue;
                                }
                            }
                            let affordable = has_items(&inv_snapshot, r.inputs);
                            let inputs = r
                                .inputs
                                .iter()
                                .map(|(i, n)| format!("{}×{}", data::item_by_key(i).map(|i| i.name).unwrap_or(i), n))
                                .collect::<Vec<_>>()
                                .join(" + ");
                            ui.horizontal(|ui| {
                                draw_slot(ui, &cache, r.output.0, None, false, 34.0);
                                ui.vertical(|ui| {
                                    ui.label(format!("{} ×{}", out_item.name, r.output.1));
                                    ui.label(
                                        egui::RichText::new(format!("{inputs}  ({:.1}s)", r.time))
                                            .size(11.0)
                                            .color(egui::Color32::GRAY),
                                    );
                                });
                                let resp = ui.add_enabled(affordable, egui::Button::new("制作").small());
                                if resp.clicked() {
                                    craft_request = Some(idx);
                                }
                            });
                            ui.separator();
                        }
                    });
            });
        });

    if close {
        ui_state.panel = Panel::None;
        ui_state.selected_inv = None;
    }
    if let Some(i) = click_slot {
        match ui_state.selected_inv {
            None => ui_state.selected_inv = Some(i),
            Some(prev) if prev == i => ui_state.selected_inv = None,
            Some(prev) => {
                if let Ok(mut p) = player.single_mut() {
                    p.inv.slots.swap(prev, i);
                }
                ui_state.selected_inv = None;
            }
        }
    }
    if sort_request {
        if let Ok(mut p) = player.single_mut() {
            p.inv.sort_storage();
            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
        }
    }
    if let Some(idx) = craft_request {
        let r = &data::RECIPES[idx];
        if let Ok(mut p) = player.single_mut() {
            if p.inv.pay_items(r.inputs) {
                p.inv.add_item(r.output.0, r.output.1);
                audio::play(&mut commands, sfx.craft.clone(), 0.6, None);
                p.toast(format!(
                    "合成：{} ×{}",
                    data::item_by_key(r.output.0).map(|i| i.name).unwrap_or(r.output.0),
                    r.output.1
                ));
            } else {
                audio::play(&mut commands, sfx.error.clone(), 0.5, None);
                p.toast("材料不足");
            }
        }
    }
    if let Some(sys) = charge_request {
        if let Ok(mut p) = player.single_mut() {
            if p.charge(sys) {
                audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
            } else {
                audio::play(&mut commands, sfx.error.clone(), 0.5, None);
            }
        }
    }
}

fn has_items(slots: &[Option<Slot>], costs: &[(&str, i32)]) -> bool {
    costs.iter().all(|(item, n)| {
        let have: i32 = slots
            .iter()
            .flatten()
            .filter(|s| s.item == *item)
            .map(|s| s.n)
            .sum();
        have >= *n
    })
}

// ---------- Tech tree ----------

pub fn tech_panel_system(
    mut contexts: EguiContexts,
    cache: Res<EguiIcons>,
    mut player: Query<&mut Player>,
    mut ui_state: ResMut<UiState>,
    mut research: ResMut<Research>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    if ui_state.panel != Panel::Tech {
        return;
    }
    let ctx = contexts.ctx_mut().expect("egui primary context");
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut start: Option<&'static str> = None;
    let inv_snapshot = player.single().map(|p| p.inv.slots.clone()).unwrap_or_default();

    egui::Window::new("科技树")
        .default_size([1020.0, 520.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut !close)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (T)").clicked() {
                close = true;
            }
            let (resp, painter) = ui.allocate_painter(egui::vec2(990.0, 440.0), egui::Sense::hover());
            let origin = resp.rect.min + egui::vec2(30.0, 170.0);
            for t in data::TECHS {
                for req in t.req {
                    if let Some(rt) = data::TECHS.iter().find(|x| x.id == *req) {
                        let a = origin + egui::vec2(rt.pos.0, rt.pos.1);
                        let b = origin + egui::vec2(t.pos.0, t.pos.1);
                        painter.line_segment(
                            [a, b],
                            egui::Stroke::new(1.5, egui::Color32::from_rgb(0x3a, 0x4a, 0x5a)),
                        );
                    }
                }
            }
            for t in data::TECHS {
                let center = origin + egui::vec2(t.pos.0, t.pos.1);
                let rect = egui::Rect::from_center_size(center, egui::vec2(130.0, 66.0));
                let researched = research.techs.iter().any(|x| x == t.id) || t.unlocked;
                let affordable = has_items(&inv_snapshot, t.cost);
                let req_met = t.req.iter().all(|r| research.techs.iter().any(|x| x == r));
                let fill = if researched {
                    egui::Color32::from_rgb(0x1d, 0x4a, 0x2e)
                } else if req_met && affordable {
                    egui::Color32::from_rgb(0x1d, 0x3a, 0x4a)
                } else {
                    egui::Color32::from_rgb(0x22, 0x26, 0x2e)
                };
                painter.rect_filled(rect, egui::CornerRadius::same(6), fill);
                painter.rect_stroke(
                    rect,
                    egui::CornerRadius::same(6),
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(0x4a, 0x5a, 0x6a)),
                egui::StrokeKind::Inside,
            );
                let tex = egui_icon(&cache, t.icon);
                painter.image(
                    tex,
                    egui::Rect::from_min_size(rect.min + egui::vec2(5.0, 5.0), egui::vec2(28.0, 28.0)),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                painter.text(
                    rect.min + egui::vec2(38.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    t.name,
                    egui::FontId::proportional(13.0),
                    egui::Color32::WHITE,
                );
                let cost_txt = if t.cost.is_empty() {
                    "免费".to_string()
                } else {
                    t.cost
                        .iter()
                        .map(|(i, n)| format!("{}×{}", data::item_by_key(i).map(|i| i.name).unwrap_or(i), n))
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                painter.text(
                    rect.min + egui::vec2(38.0, 26.0),
                    egui::Align2::LEFT_TOP,
                    cost_txt,
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(0xca, 0xd2, 0xda),
                );
                let in_research = research.active.as_ref().map(|(id, _)| id == t.id).unwrap_or(false);
                if !researched && req_met && !in_research {
                    let btn_rect =
                        egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 46.0), egui::vec2(130.0, 18.0));
                    let resp = ui.interact(btn_rect, egui::Id::new(("tech", t.id)), egui::Sense::click());
                    painter.rect_filled(btn_rect, egui::CornerRadius::same(3), egui::Color32::from_rgb(0x2e, 0x55, 0x6e));
                    painter.text(
                        btn_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        if affordable { "研究" } else { "材料不足" },
                        egui::FontId::proportional(12.0),
                        egui::Color32::WHITE,
                    );
                    if resp.clicked() && affordable {
                        start = Some(t.id);
                    }
                } else if in_research {
                    let (_, prog) = research.active.as_ref().unwrap();
                    painter.text(
                        rect.min + egui::vec2(38.0, 48.0),
                        egui::Align2::LEFT_TOP,
                        format!("研究中 {:.0}%", prog / t.time * 100.0),
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_rgb(0x35, 0xe0, 0xe8),
                    );
                } else if researched {
                    painter.text(
                        rect.min + egui::vec2(38.0, 48.0),
                        egui::Align2::LEFT_TOP,
                        "已解锁",
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_rgb(0x7d, 0xff, 0x8a),
                    );
                }
            }
        });
    if close {
        ui_state.panel = Panel::None;
    }
    if let Some(id) = start {
        let tech = data::TECHS.iter().find(|t| t.id == id).unwrap();
        if let Ok(mut p) = player.single_mut() {
            if p.inv.pay_items(tech.cost) {
                research.active = Some((id.to_string(), 0.0));
                audio::play(&mut commands, sfx.craft.clone(), 0.6, None);
            } else {
                audio::play(&mut commands, sfx.error.clone(), 0.5, None);
            }
        }
    }
}

// ---------- Machine panels ----------

pub fn machine_panel_system(
    mut contexts: EguiContexts,
    cache: Res<EguiIcons>,
    mut player: Query<&mut Player>,
    mut ui_state: ResMut<UiState>,
    mut q: Query<(&Machine, &mut MachineState)>,
    power: Res<crate::factory::Power>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    let Panel::Machine(e) = ui_state.panel else { return };
    let Some((m, _)) = q.get(e).ok() else {
        ui_state.panel = Panel::None;
        return;
    };
    let kind = m.kind;
    let pos = m.pos;
    let ctx = contexts.ctx_mut().expect("egui primary context");
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut actions: Vec<MachinePanelAction> = Vec::new();
    let mut open = true;

    // 快照
    let inv_snapshot = player.single().map(|p| p.inv.slots.clone()).unwrap_or_default();
    let sel_info = ui_state
        .selected_inv
        .and_then(|i| inv_snapshot.get(i).cloned().flatten());
    let state_snap = q.get(e).ok().map(|(_, s)| s.clone());

    egui::Window::new(format!("◈ {}", kind.label()))
        .default_size([360.0, 360.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (E)").clicked() {
                close = true;
            }
            ui.label(format!(
                "位置 ({}, {}, {}) · 朝向 {}",
                pos[0], pos[1], pos[2], m.dir
            ));
            match &state_snap {
                Some(MachineState::Furnace(f)) => {
                    ui.horizontal(|ui| {
                        ui.label("燃料:");
                        draw_slot(ui, &cache, f.fuel.as_ref().map(|s| s.item.as_str()).unwrap_or(""), f.fuel.clone(), false, 40.0);
                        ui.label(format!("{:.1}s {}", f.burn, if f.on { "🔥" } else { "" }));
                    });
                    ui.horizontal(|ui| {
                        ui.label("输入:");
                        draw_slot(ui, &cache, f.input.as_ref().map(|s| s.item.as_str()).unwrap_or(""), f.input.clone(), false, 40.0);
                        ui.label("输出:");
                        draw_slot(ui, &cache, f.output.as_ref().map(|s| s.item.as_str()).unwrap_or(""), f.output.clone(), false, 40.0);
                    });
                    if let Some(rid) = f.recipe {
                        if let Some(r) = data::RECIPES.iter().find(|r| r.id == rid) {
                            ui.label(format!(
                                "烧炼中：{} {:.0}%",
                                data::item_by_key(r.output.0).map(|i| i.name).unwrap_or(r.output.0),
                                f.prog / r.time * 100.0
                            ));
                        }
                    }
                    if let Some(sel) = &sel_info {
                        ui.label(format!("选中：{} ×{}", item_name(&sel.item), sel.n));
                        ui.horizontal(|ui| {
                            if ui.button("⛽ 放入燃料").clicked() {
                                actions.push(MachinePanelAction::InsertFuel);
                            }
                            if ui.button("🧱 放入原料").clicked() {
                                actions.push(MachinePanelAction::InsertInput);
                            }
                        });
                    }
                    if ui.button("📤 取出成品").clicked() {
                        actions.push(MachinePanelAction::TakeOutput);
                    }
                }
                Some(MachineState::Chest(c)) => {
                    let slots = c.slots.clone();
                    chest_grid(ui, &cache, &slots, &mut actions);
                    if let Some(sel) = &sel_info {
                        ui.label(format!("选中：{} ×{}", item_name(&sel.item), sel.n));
                        if ui.button("📥 放入选中物品").clicked() {
                            actions.push(MachinePanelAction::ChestPut);
                        }
                    }
                }
                Some(MachineState::Collector(c)) => {
                    let slots = c.slots.clone();
                    chest_grid(ui, &cache, &slots, &mut actions);
                    if let Some(sel) = &sel_info {
                        ui.label(format!("选中：{} ×{}", item_name(&sel.item), sel.n));
                        if ui.button("📥 放入选中物品").clicked() {
                            actions.push(MachinePanelAction::ChestPut);
                        }
                    }
                }
                Some(MachineState::Miner(mn)) => {
                    ui.label(if m.active { "⛏ 开采中" } else { "⏸ 待机" });
                    if mn.output.is_some() {
                        ui.label(format!("产出：{} ×{}", item_name(mn.output.as_ref().unwrap().item.as_str()), mn.output.as_ref().unwrap().n));
                    }
                    ui.label(format!("矿脉消耗：{}/300", mn.deposit));
                    if ui.button("📤 取出产出").clicked() {
                        actions.push(MachinePanelAction::TakeOutput);
                    }
                }
                Some(MachineState::Crafter(cr)) => {
                    // 配方选择
                    let where_ = if kind == MachineKind::Refinery { "refinery" } else { "assembler" };
                    let mut current = cr.recipe.unwrap_or("");
                    let avail: Vec<&'static data::Recipe> = data::RECIPES
                        .iter()
                        .filter(|r| r.station == where_ || r.station == "both")
                        .collect();
                    egui::ComboBox::from_id_salt("recipe_pick")
                        .selected_text(if current.is_empty() { "选择配方".to_string() } else { current.to_string() })
                        .show_ui(ui, |ui| {
                            for r in &avail {
                                let out_name = data::item_by_key(r.output.0).map(|i| i.name).unwrap_or(r.output.0);
                                ui.selectable_value(&mut current, r.id, format!("{} → {} ×{}", recipe_in_str(r), out_name, r.output.1));
                            }
                        });
                    if current != cr.recipe.unwrap_or("") {
                        actions.push(MachinePanelAction::SetRecipe(current.to_string()));
                    }
                    if !cr.input.is_empty() {
                        let mut parts = Vec::new();
                        for (k, v) in &cr.input {
                            parts.push(format!("{}×{}", item_name(k), v));
                        }
                        ui.label(format!("原料：{}", parts.join(" ")));
                    }
                    ui.label(format!("进度 {:.0}% · {}", cr.prog * 100.0, if m.active { "⚙ 运行中" } else { "⏸ 待机" }));
                    if cr.output.is_some() {
                        ui.label(format!("产出：{} ×{}", item_name(cr.output.as_ref().unwrap().item.as_str()), cr.output.as_ref().unwrap().n));
                    }
                    if let Some(sel) = &sel_info {
                        if ui.button("📥 投入选中物品").clicked() {
                            actions.push(MachinePanelAction::InsertInput);
                        }
                    }
                    if ui.button("📤 取出产出").clicked() {
                        actions.push(MachinePanelAction::TakeOutput);
                    }
                }
                Some(MachineState::Reactor(r)) => {
                    ui.label(format!("铀燃料余量：{:.1}s {}", r.fuel, if m.active { "☢ 发电中" } else { "" }));
                    if let Some(sel) = &sel_info {
                        if sel.item == "uranium" && ui.button("☢ 投料铀-235（+60s）").clicked() {
                            actions.push(MachinePanelAction::InsertInput);
                        }
                    } else {
                        ui.label("在背包选中铀-235 后可投料");
                    }
                }
                Some(MachineState::Burner(b)) => {
                    ui.horizontal(|ui| {
                        ui.label("燃料:");
                        draw_slot(ui, &cache, b.fuel.as_ref().map(|s| s.item.as_str()).unwrap_or(""), b.fuel.clone(), false, 40.0);
                        ui.label(format!("{:.1}s {}", b.burn, if m.active { "⚡ 发电中" } else { "" }));
                    });
                    if let Some(sel) = &sel_info {
                        if ui.button("⛽ 放入燃料").clicked() {
                            actions.push(MachinePanelAction::InsertFuel);
                        }
                    }
                }
                Some(MachineState::Belt(b)) => {
                    ui.label(format!("{} 个物品在运输", b.items.len()));
                    ui.label("物品从朝向方向输出到下一台机器/传送带。");
                }
                Some(MachineState::Beacon(bc)) => {
                    let mut label = bc.label.clone();
                    ui.horizontal(|ui| {
                        ui.label("名称");
                        ui.text_edit_singleline(&mut label);
                    });
                    let mut gal = bc.gal;
                    ui.checkbox(&mut gal, "全星系显示");
                    if label != bc.label || gal != bc.gal {
                        actions.push(MachinePanelAction::BeaconLabel(label, gal));
                    }
                }
                Some(MachineState::Lumberbot(lb)) => {
                    ui.label(format!("碳载量：{}/40", lb.cargo));
                    if lb.deliver_t > 0.0 {
                        ui.label(format!("返航中… {:.1}s", lb.deliver_t));
                    } else {
                        ui.label(if m.active { "⛏ 巡林伐木中" } else { "⏸ 待机" });
                    }
                }
                Some(MachineState::Medbay(_)) => {
                    ui.label(if m.active { "✚ 治疗中" } else { "⏸ 待机" });
                    ui.label("站近自动治疗：每消耗 1 钠 + 1 氧气回复 3 生命");
                }
                _ => {
                    if matches!(kind, MachineKind::Solar | MachineKind::Wind | MachineKind::Launchpad) {
                        ui.label("该机器自动运行，无需操作。");
                    } else {
                        ui.label("该机器暂无面板。");
                    }
                }
            }
            ui.add_space(6.0);
            ui.separator();
            ui.label(
                egui::RichText::new(format!("⚡ 电网：发电 {} kW / 用电 {:.1} kW / 满足率 {:.0}%", power.generation, power.used, power.sat * 100.0))
                    .size(13.0)
                    .color(if power.sat < 0.99 { egui::Color32::from_rgb(0xff, 0x55, 0x55) } else { egui::Color32::from_rgb(0xff, 0xb3, 0x47) }),
            );
        });
    if !open {
        close = true;
    }
    if close {
        ui_state.panel = Panel::None;
    }
    // 应用动作
    if actions.is_empty() {
        return;
    }
    let Ok(mut p) = player.single_mut() else { return };
    let mclone = q.get(e).map(|(m, _)| m.clone()).unwrap();
    let Ok((_m, mut st)) = q.get_mut(e) else { return };
    for a in actions {
        match a {
            MachinePanelAction::InsertFuel => {
                if let Some(i) = ui_state.selected_inv {
                    if let Some(s) = p.inv.slots[i].clone() {
                        if data::fuel_value(&s.item) > 0.0 {
                            if crate::factory::machine_insert(&mclone, &mut st, &s.item) {
                                p.inv.remove_item(&s.item, 1);
                                audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                            }
                        } else {
                            p.toast("不是燃料");
                        }
                    }
                }
            }
            MachinePanelAction::InsertInput => {
                if let Some(i) = ui_state.selected_inv {
                    if let Some(s) = p.inv.slots[i].clone() {
                        if crate::factory::machine_insert(&mclone, &mut st, &s.item) {
                            p.inv.remove_item(&s.item, 1);
                            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                        } else {
                            p.toast("机器不接受该物品");
                        }
                    }
                }
            }
            MachinePanelAction::TakeOutput => {
                let out = match &mut *st {
                    MachineState::Furnace(f) => f.output.take(),
                    MachineState::Miner(mn) => mn.output.take(),
                    MachineState::Crafter(cr) => cr.output.take(),
                    _ => None,
                };
                if let Some(o) = out {
                    p.inv.add_item(&o.item, o.n);
                    audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
                }
            }
            MachinePanelAction::ChestTake(i) => {
                let taken = match &mut *st {
                    MachineState::Chest(c) => c.slots.get_mut(i).and_then(|s| s.take()),
                    MachineState::Collector(c) => c.slots.get_mut(i).and_then(|s| s.take()),
                    _ => None,
                };
                if let Some(s) = taken {
                    p.inv.add_item(&s.item, s.n);
                    audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
                }
            }
            MachinePanelAction::ChestPut => {
                if let Some(i) = ui_state.selected_inv {
                    if let Some(s) = p.inv.slots[i].clone() {
                        if crate::factory::machine_insert(&mclone, &mut st, &s.item) {
                            p.inv.remove_item(&s.item, 1);
                            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                        }
                    }
                }
            }
            MachinePanelAction::SetRecipe(id) => {
                let id_str: &'static str = Box::leak(id.clone().into_boxed_str());
                if let MachineState::Crafter(cr) = &mut *st {
                    // 切换配方：旧产出掉落
                    if let Some(o) = cr.output.take() {
                        p.inv.add_item(&o.item, o.n);
                    }
                    cr.input.clear();
                    cr.recipe = Some(id_str);
                    cr.prog = 0.0;
                    audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                }
            }
            MachinePanelAction::BeaconLabel(label, gal) => {
                if let MachineState::Beacon(bc) = &mut *st {
                    bc.label = label;
                    bc.gal = gal;
                }
            }
        }
    }
}

enum MachinePanelAction {
    InsertFuel,
    InsertInput,
    TakeOutput,
    ChestTake(usize),
    ChestPut,
    SetRecipe(String),
    BeaconLabel(String, bool),
}

fn chest_grid(
    ui: &mut egui::Ui,
    cache: &EguiIcons,
    slots: &[Option<Slot>],
    actions: &mut Vec<MachinePanelAction>,
) {
    egui::Grid::new("chest_grid")
        .num_columns(6)
        .spacing([4.0, 4.0])
        .show(ui, |ui| {
            for i in 0..slots.len() {
                let s = slots[i].clone();
                if slot_button(ui, cache, s.as_ref().map(|s| s.item.as_str()).unwrap_or(""), &s, false, 42.0) {
                    actions.push(MachinePanelAction::ChestTake(i));
                }
                if (i + 1) % 6 == 0 {
                    ui.end_row();
                }
            }
        });
}

fn item_name(key: &str) -> &str {
    data::item_by_key(key).map(|i| i.name).unwrap_or(key)
}

fn recipe_in_str(r: &data::Recipe) -> String {
    r.inputs
        .iter()
        .map(|(i, n)| format!("{}×{}", item_name(i), n))
        .collect::<Vec<_>>()
        .join("+")
}

// ---------- Fonts ----------

/// One-time egui setup (fonts + icon texture registration). The primary egui
/// context only exists after the first camera spawns, so we retry until it is
/// available, then run once.
#[derive(Resource, Default)]
pub struct EguiReady;

/// True once egui's font system is usable. bevy_egui 0.41 + egui 0.35 can start
/// a frame without fonts ready on the very first pass; UI systems use this to
/// skip drawing until fonts exist. The result is cached: once fonts are ready
/// they stay ready for the lifetime of the process.
pub fn egui_fonts_ready(ctx: &egui::Context) -> bool {
    static FONTS_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if FONTS_READY.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ctx.fonts(|f| {
            let _ = f.definitions().font_data.len();
        });
    }))
    .is_ok();
    if ok {
        FONTS_READY.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    ok
}

pub fn setup_egui(
    mut contexts: EguiContexts,
    images: Res<IconImages>,
    mut cache: ResMut<EguiIcons>,
    ready: Option<Res<EguiReady>>,
    mut commands: Commands,
) {
    if ready.is_some() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return; // no primary context yet (no camera) — retry next frame
    };
    if !egui_fonts_ready(ctx) {
        return; // fonts not initialized yet — retry next frame
    }
    // CJK font
    let mut fonts = egui::FontDefinitions::default();
    let font_bytes: &[u8] = include_bytes!("../assets/fonts/NotoSansSC.ttf");
    let data = egui::FontData::from_static(font_bytes);
    fonts
        .font_data
        .insert("noto_sc".to_string(), std::sync::Arc::new(data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("noto_sc".to_string());
    }
    // NOTE: egui 0.35 reloads all fonts on the pass after set_fonts; defer by one
    // pass to avoid clashing with the very first frame's font creation.
    let mut first_apply = true;
    let _ = &mut first_apply;
    ctx.set_fonts(fonts);
    // register all item icons as egui textures
    for key in images.map.keys() {
        if let Some(h) = images.map.get(key) {
            let id = contexts.add_image(bevy_egui::EguiTextureHandle::Strong(h.clone()));
            cache.0.insert(key.clone(), id);
        }
    }
    commands.insert_resource(EguiReady);
}

// ---------- Pause menu ----------

pub fn pause_panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut settings: ResMut<Settings>,
    world: Option<ResMut<World>>,
    player: Query<&Player>,
    research: Res<Research>,
    mut save_ev: MessageWriter<SaveEvent>,
    mut quit_ev: MessageWriter<QuitToMenuEvent>,
    day: Res<crate::daynight::DayTime>,
) {
    if ui_state.panel != Panel::Pause {
        return;
    }
    let ctx = contexts.ctx_mut().expect("egui primary context");
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "pause_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let mut close = false;
    let mut do_save = false;
    let mut do_quit = false;
    egui::CentralPanel::default().show(&mut root, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.label(egui::RichText::new("STARFORGE · 星穹熔炉").size(32.0).strong());
            ui.label(egui::RichText::new("Bevy 移植版").size(16.0));
            ui.add_space(24.0);
            if ui.button("▶ 继续游戏 (Esc)").clicked() {
                close = true;
            }
            if ui.button("💾 保存并继续 (F5)").clicked() {
                do_save = true;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("渲染距离 (区块)");
                if ui.add(egui::Slider::new(&mut settings.view_dist, 3..=16)).changed() {
                    if let Some(mut w) = world {
                        w.view_dist = settings.view_dist;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("鼠标灵敏度");
                ui.add(egui::Slider::new(&mut settings.mouse_sens, 0.3..=2.5));
            });
            ui.horizontal(|ui| {
                ui.label("音量");
                ui.add(egui::Slider::new(&mut settings.volume, 0.0..=1.0));
            });
            ui.checkbox(&mut settings.show_fps, "显示 FPS");
            if ui.checkbox(&mut settings.pixelated, "像素风渲染（重启生效）").changed() {
                let _ = crate::save::save_settings(&settings);
            }
            ui.add_space(8.0);
            let pos = player.single().map(|p| p.pos).unwrap_or_default();
            ui.label(format!(
                "位置 ({:.1}, {:.1}, {:.1}) · 第 {} 天",
                pos.x,
                pos.y,
                pos.z,
                (day.0).floor() as i32 + 1
            ));
            ui.label(format!("已解锁科技：{} / {}", research.techs.len(), data::TECHS.len()));
            ui.add_space(16.0);
            if ui.button("🏠 保存并返回主菜单").clicked() {
                do_save = true;
                do_quit = true;
            }
            if ui.button("🚪 退出游戏").clicked() {
                std::process::exit(0);
            }
        });
    });
    if close {
        ui_state.panel = Panel::None;
    }
    if do_save {
        save_ev.write(SaveEvent);
    }
    if do_quit {
        quit_ev.write(QuitToMenuEvent);
    }
}

#[derive(Message)]
pub struct SaveEvent;

#[derive(Message)]
pub struct QuitToMenuEvent;

/// Handle F5 quicksave.
pub fn quicksave_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut save_ev: MessageWriter<SaveEvent>,
    ui: Res<UiState>,
) {
    if keys.just_pressed(KeyCode::F5) && ui.panel == Panel::None {
        save_ev.write(SaveEvent);
    }
}

/// Panel toggle hotkeys (Tab/T/E/Esc) + E interaction.
pub fn panel_hotkeys_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut player: Query<&mut Player>,
    world: Res<World>,
    machines: Query<(Entity, &Machine)>,
    mode: Res<crate::space::FlightMode>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        match ui_state.panel {
            Panel::None => {
                ui_state.panel = Panel::Pause;
                ui_state.selected_inv = None;
            }
            Panel::Pause => ui_state.panel = Panel::None,
            _ => ui_state.panel = Panel::None,
        }
    }
    if keys.just_pressed(KeyCode::Tab) && !ui_state.locked() {
        ui_state.panel = Panel::Inventory;
        ui_state.selected_inv = None;
    }
    if keys.just_pressed(KeyCode::KeyT) && !ui_state.locked() && *mode == crate::space::FlightMode::Planet {
        ui_state.panel = Panel::Tech;
    }
    if keys.just_pressed(KeyCode::KeyE) && !ui_state.locked() && *mode == crate::space::FlightMode::Planet {
        let Ok(mut p) = player.single_mut() else { return };
        let origin = p.eye();
        let dir = p.look_dir();
        if p.stats.haz < 95.0 && p.inv.count_item("sodium") > 0 {
            if p.charge("haz") {
                p.toast("防护已充能");
                audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
                return;
            }
        }
        if p.stats.o2 < 95.0 && p.inv.count_item("oxygen") > 0 {
            if p.charge("o2") {
                p.toast("氧气已充能");
                audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
                return;
            }
        }
        if let Some((cell, _n, dist)) = world.raycast(origin, dir, 5.0) {
            if dist <= 5.0 {
                if let Some((e, _)) = machines.iter().find(|(_, m)| m.pos == cell) {
                    ui_state.panel = Panel::Machine(e);
                    audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                    return;
                }
            }
        }
        p.toast("附近没有可交互的机器");
    }
}

/// C: scan pulse — expanding ring from the player.
#[derive(Resource, Default)]
pub struct ScanPulse {
    pub t: f32,
    pub active: bool,
}

pub fn scan_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut pulse: ResMut<ScanPulse>,
    time: Res<Time>,
    mut materials: ResMut<Assets<crate::materials::TerrainMat>>,
    mats: Res<crate::materials::TerrainMaterials>,
    player: Query<&Player>,
    ui: Res<UiState>,
) {
    if keys.just_pressed(KeyCode::KeyC) && !ui.locked() {
        pulse.active = true;
        pulse.t = 0.0;
    }
    let Ok(p) = player.single() else { return };
    if pulse.active {
        pulse.t += time.delta_secs();
        let r = pulse.t * 480.0;
        let a = (pulse.t * 0.9).clamp(0.0, 0.9)
            * (1.0 - (pulse.t - 0.9).max(0.0) / 0.5).clamp(0.0, 1.0);
        if let Some(mut m) = materials.get_mut(&mats.solid) {
            let c = &mut m.extension.curve;
            c.scan_r = r;
            c.scan_cx = p.pos.x;
            c.scan_cz = p.pos.z;
            c.scan_a = a;
        }
        if pulse.t > 1.4 {
            pulse.active = false;
            if let Some(mut m) = materials.get_mut(&mats.solid) {
                m.extension.curve.scan_a = 0.0;
            }
        }
    }
}

// ---------- 空间站贸易终端 ----------

pub fn trade_panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut player: Query<&mut Player>,
    mut game: ResMut<crate::space::SpaceGame>,
    mut research: ResMut<Research>,
    mut flag_ev: MessageWriter<crate::quests::FlagEvent>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    if ui_state.panel != Panel::Trade {
        return;
    }
    let ctx = contexts.ctx_mut().expect("egui primary context");
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut buy_req: Vec<(String, i32, i32)> = Vec::new(); // (item, price, n)
    let mut sell_req: Vec<(String, i32, i32)> = Vec::new();
    let mut blueprint_req: Option<&'static str> = None;
    let has_trade_ai = research.techs.iter().any(|t| t == "trade_ai");
    egui::Window::new("◈ 银河交易终端")
        .default_size([420.0, 520.0])
        .resizable(false)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (Esc)").clicked() {
                close = true;
            }
            let credits = player.single().map(|p| p.credits).unwrap_or(0);
            ui.label(
                egui::RichText::new(format!("₪ {credits}"))
                    .size(20.0)
                    .strong()
                    .color(egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
            );
            ui.separator();
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                egui::Grid::new("trade_grid").num_columns(4).spacing([10.0, 4.0]).show(ui, |ui| {
                    for item in data::TRADE_GOODS {
                        let buy = data::trade_buy_price(item, game.market(), has_trade_ai);
                        let sell = data::trade_sell_price(item, game.market());
                        let have = player.single().map(|p| p.inv.count_item(item)).unwrap_or(0);
                        ui.label(item_name(item));
                        ui.label(format!("买 ₪{buy} / 卖 ₪{sell}"));
                        ui.label(format!("持有 {have}"));
                        ui.horizontal(|ui| {
                            if ui.button("买").clicked() {
                                buy_req.push((item.to_string(), buy, 1));
                            }
                            if ui.button("卖").clicked() {
                                sell_req.push((item.to_string(), sell, 1));
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.label(
                egui::RichText::new("◈ 科技蓝图")
                    .size(14.0)
                    .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
            );
            for bp in data::STATION_BLUEPRINTS {
                let owned = research.techs.iter().any(|t| t == bp.tech);
                if owned {
                    ui.label(format!("{} — 已拥有", bp.name));
                } else {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} — ₪{}", bp.name, bp.price));
                        if ui.button("购买").clicked() {
                            blueprint_req = Some(bp.tech);
                        }
                    });
                }
            }
        });
    if close {
        ui_state.panel = Panel::None;
        return;
    }
    let Ok(mut p) = player.single_mut() else { return };
    let mut traded = false;
    for (item, price, n) in buy_req {
        if p.credits >= price * n {
            p.credits -= price * n;
            p.inv.add_item(&item, n);
            traded = true;
            audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
        } else {
            p.toast("信用点不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        }
    }
    for (item, price, n) in sell_req {
        if p.inv.count_item(&item) >= n {
            p.inv.remove_item(&item, n);
            p.credits += price * n;
            traded = true;
            audio::play(&mut commands, sfx.click.clone(), 0.5, None);
        } else {
            p.toast("物品不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        }
    }
    if let Some(tech) = blueprint_req {
        if p.credits >= data::STATION_BLUEPRINTS.iter().find(|b| b.tech == tech).map(|b| b.price).unwrap_or(i32::MAX) {
            p.credits -= data::STATION_BLUEPRINTS.iter().find(|b| b.tech == tech).map(|b| b.price).unwrap_or(0);
            if !research.techs.iter().any(|t| t == tech) {
                research.techs.push(tech.to_string());
            }
            p.toast(format!("蓝图已获取：{}", data::TECHS.iter().find(|t| t.id == tech).map(|t| t.name).unwrap_or(tech)));
            traded = true;
            audio::play(&mut commands, sfx.craft.clone(), 0.6, None);
        } else {
            p.toast("信用点不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        }
    }
    if traded {
        flag_ev.write(crate::quests::FlagEvent { flag: "traded".into() });
    }
}

// ---------- 换船电脑 ----------

pub fn garage_panel_system(
    mut contexts: EguiContexts,
    cache: Res<EguiIcons>,
    mut ui_state: ResMut<UiState>,
    mut player: Query<&mut Player>,
    mut game: ResMut<crate::space::SpaceGame>,
    ship_asset: Res<crate::space::ShipAsset>,
    mut switch_ev: MessageWriter<crate::station::ShipSwitchEvent>,
) {
    if ui_state.panel != Panel::Garage {
        return;
    }
    let ctx = contexts.ctx_mut().expect("egui primary context");
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut switch_req: Option<usize> = None;
    let mut cargo_take: Option<usize> = None;
    egui::Window::new("◈ 舰船调度终端")
        .default_size([420.0, 420.0])
        .resizable(false)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (Esc)").clicked() {
                close = true;
            }
            let cls = data::ship_class_by_key(&ship_asset.data.cls);
            ui.label(
                egui::RichText::new(format!(
                    "当前座驾：{}（{} 级 · {}）",
                    ship_asset.data.name, cls.key, cls.weapon_name
                ))
                .size(15.0)
                .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
            );
            ui.label(format!("货仓（{} 格）：", cls.slots));
            let n = cls.slots;
            egui::Grid::new("cargo_grid").num_columns(6).spacing([4.0, 4.0]).show(ui, |ui| {
                for i in 0..n {
                    let s = game.ship_inv.get(i).cloned().flatten();
                    if slot_button(ui, &cache, s.as_ref().map(|s| s.item.as_str()).unwrap_or(""), &s, false, 40.0) {
                        cargo_take = Some(i);
                    }
                    if (i + 1) % 6 == 0 {
                        ui.end_row();
                    }
                }
            });
            ui.label("（点击舱内物品取出到背包）");
            ui.separator();
            ui.label(
                egui::RichText::new("◈ 机库飞船")
                    .size(14.0)
                    .color(egui::Color32::from_rgb(0xff, 0xb3, 0x47)),
            );
            if game.garage.is_empty() {
                ui.label("（机库为空）");
            }
            for (i, s) in game.garage.iter().enumerate() {
                let cls = data::ship_class_by_key(&s.cls);
                ui.horizontal(|ui| {
                    ui.label(format!("{}（{} 级）", s.name, cls.key));
                    if ui.button("换乘").clicked() {
                        switch_req = Some(i);
                    }
                });
            }
        });
    if close {
        ui_state.panel = Panel::None;
        return;
    }
    if let Some(i) = switch_req {
        let Some(s) = game.garage.get(i).cloned() else { return };
        switch_ev.write(crate::station::ShipSwitchEvent {
            cls: s.cls.clone(),
            model: s.model.clone(),
            garage_idx: Some(i),
        });
    }
    if let Some(i) = cargo_take {
        let Ok(mut p) = player.single_mut() else { return };
        if let Some(s) = game.ship_inv.get_mut(i).and_then(|s| s.take()) {
            p.inv.add_item(&s.item, s.n);
        }
    }
}

// ---------- 星系地图 ----------

pub fn galaxy_map_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut game: ResMut<crate::space::SpaceGame>,
    mut ship: ResMut<crate::space::ShipState>,
    mode: Res<crate::space::FlightMode>,
) {
    if ui_state.panel != Panel::GalaxyMap {
        return;
    }
    if *mode != crate::space::FlightMode::Space {
        ui_state.panel = Panel::None;
        return;
    }
    let ctx = contexts.ctx_mut().expect("egui primary context");
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut lock_req: Option<u32> = None;
    egui::Window::new("◈ 星系地图")
        .default_size([560.0, 480.0])
        .resizable(false)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (M / Esc)").clicked() {
                close = true;
            }
            ui.label(
                egui::RichText::new(format!("当前星系：{}（种子 {}）", game.galaxy.name, game.galaxy.seed))
                    .size(16.0)
                    .strong()
                    .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
            );
            ui.label(
                egui::RichText::new(format!("星球 {} 颗 · 已访问星系 {}", game.galaxy.planets.len(), game.galaxy_count))
                    .size(13.0),
            );
            if let Some(lock) = &game.warp_lock {
                ui.label(
                    egui::RichText::new(format!("◎ 已锁定：{}", lock.name))
                        .size(14.0)
                        .color(egui::Color32::from_rgb(0x7d, 0xff, 0x8a)),
                );
            }
            ui.label("对准锁定星系方向 + 按住 J 脉冲冲刺至 700 u/s 即自动跃迁（需曲率电池×1）");
            ui.separator();
            egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                let cur = game.galaxy.seed;
                let mut seeds = crate::space::neighbor_seeds(cur);
                seeds.sort();
                for seed in seeds {
                    let name = data::galaxy_name(seed);
                    let visited = game.archives.contains_key(&seed);
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{}{}",
                            name,
                            if visited { " · 已到访" } else { "" }
                        ));
                        if ui.button("锁定").clicked() {
                            lock_req = Some(seed);
                        }
                    });
                }
            });
        });
    if close {
        ui_state.panel = Panel::None;
        return;
    }
    if let Some(seed) = lock_req {
        game.warp_lock = Some(crate::space::WarpLock {
            seed,
            name: data::galaxy_name(seed),
        });
        let _ = &mut ship;
    }
}
