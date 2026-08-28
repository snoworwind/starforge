//! egui UI: HUD, inventory/crafting, tech tree, machine panels, ghost preview, menus.

use crate::audio;
use crate::data;
use crate::factory::{Machine, MachineKind, MachineState};
use crate::inventory::Slot;
use crate::player::Player;
use crate::save::Settings;
use crate::schedule::{GameSet, GameState, ground_mode, in_planet_mode};
use crate::textures::{Atlas, IconBuf, item_icon};
use crate::world::World;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::collections::HashMap;

// ---------- In-game panel state ----------

#[derive(Resource, Default)]
pub struct UiState {
    pub panel: Panel,
    pub prompt: Option<String>,
    pub selected_inv: Option<usize>,
    /// 背包拖拽手持物品（JS cursorStack 移植）
    pub cursor: Option<Slot>,
    /// 大字提示（标题, 副标题, 剩余秒数）
    pub big: Option<(String, String, f32)>,
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct HudCamera<'w, 's> {
    query: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<Camera3d>>,
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct HudRuntime<'w> {
    power: Res<'w, crate::factory::Power>,
    lod: Res<'w, crate::lod::LodRuntime>,
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum Panel {
    #[default]
    None,
    Inventory,
    Tech,
    Machine(Entity),
    Pause,
    /// 实时太阳与环境光调节（F3）
    Lighting,
    Trade,
    Garage,
    GalaxyMap,
    /// Bevy 原生联机（O）
    Network,
    /// 创造物品库（P）
    Creative,
    /// 星球全息地图（M）
    Map,
    /// 空间站停泊服务菜单（泊入后 E 打开）
    Station,
    /// 空间站买船中心（停泊服务菜单进入）
    BuyShip,
}

impl UiState {
    pub fn locked(&self) -> bool {
        self.panel != Panel::None
    }

    /// 关闭当前面板（不处理手持物品）。
    pub fn close_panel(&mut self) {
        self.panel = Panel::None;
        self.selected_inv = None;
    }
}

/// 手持物品归还背包；背包满时剩余部分掉落在玩家身旁（JS dropCursor 移植）。
pub fn drop_cursor(
    ui: &mut UiState,
    player: &mut Player,
    commands: &mut Commands,
    world: &World,
    icons: &IconMaterials,
    sfx: &audio::Sfx,
) {
    if let Some(c) = ui.cursor.take() {
        let added = player.inv.add_item(&c.item, c.n);
        let left = c.n - added;
        if left > 0 {
            crate::creatures::spawn_drop(
                commands,
                world,
                icons,
                player.pos + Vec3::Y * 0.6,
                Vec3::ZERO,
                c.item,
                left,
                0.4,
            );
            audio::play(commands, sfx.pickup.clone(), 0.5, None);
        } else if added > 0 {
            audio::play(commands, sfx.click.clone(), 0.4, None);
        }
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
    let mut ii = IconImages {
        map: HashMap::new(),
    };
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
    for row in icon.iter().take(size) {
        for pixel in row.iter().take(size) {
            out.extend_from_slice(pixel);
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

pub fn research_system(
    time: Res<Time>,
    mut research: ResMut<Research>,
    mut player: Query<&mut Player>,
    mut big_ev: MessageWriter<crate::quests::BigMessageEvent>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    let mut completed: Option<String> = None;
    if let Some((id, prog)) = research.active.as_mut() {
        *prog += time.delta_secs();
        if let Some(tech) = data::TECHS.iter().find(|t| t.id == id)
            && *prog >= tech.time
        {
            completed = Some(id.clone());
        }
    }
    if let Some(id) = completed {
        research.active = None;
        research.techs.push(id.clone());
        if let Some(tech) = data::TECHS.iter().find(|t| t.id == id) {
            // JS：bigMessage('科技解锁', name—desc) + research 音
            big_ev.write(crate::quests::BigMessageEvent {
                title: format!("科技解锁：{}", tech.name),
                sub: tech.desc.to_string(),
                dur: 3.2,
            });
            audio::play(&mut commands, sfx.craft.clone(), 0.6, None);
            if let Ok(mut p) = player.single_mut() {
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
    runtime: HudRuntime,
    cam_q: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    let Ok(p) = player.single() else { return };
    if ui.panel != Panel::None {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let screen = ctx.content_rect();
    let flying =
        *mode != crate::space::FlightMode::Planet && *mode != crate::space::FlightMode::Seated;

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
            if let Some(mining) = &p.mining {
                painter.rect_stroke(
                    egui::Rect::from_center_size(c, egui::vec2(34.0, 34.0)),
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
                    egui::StrokeKind::Outside,
                );
                painter.text(
                    c + egui::vec2(0.0, 23.0),
                    egui::Align2::CENTER_TOP,
                    format!("采矿 {:.0}%", mining.prog.clamp(0.0, 1.0) * 100.0),
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(0xff, 0xd1, 0x66),
                );
            }
        });

    // hotbar
    let slot_px = 48.0;
    let total = slot_px * 10.0 + 11.0 * 4.0;
    let origin = egui::pos2(
        screen.center().x - total / 2.0,
        screen.max.y - slot_px - 24.0,
    );
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

    // vitals top-left（JS 段条/细条移植）
    egui::Area::new(egui::Id::new("vitals"))
        .fixed_pos(egui::pos2(12.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            {
                let painter = ui.painter();
                let bar_x = 44.0;
                // 护盾段条（6 段）
                painter.text(
                    egui::pos2(0.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    "护盾",
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_rgb(0x7f, 0x9d, 0xb0),
                );
                for i in 0..6 {
                    let r = egui::Rect::from_min_size(
                        egui::pos2(bar_x + i as f32 * 14.0, 0.0),
                        egui::vec2(12.0, 12.0),
                    );
                    let on = p.stats.shield as i32 > i;
                    painter.rect_filled(
                        r,
                        egui::CornerRadius::same(2),
                        if on {
                            egui::Color32::from_rgb(0x35, 0xe0, 0xe8)
                        } else {
                            egui::Color32::from_rgb(0x12, 0x32, 0x4a)
                        },
                    );
                }
                // 生命段条（8 段）
                painter.text(
                    egui::pos2(0.0, 18.0),
                    egui::Align2::LEFT_TOP,
                    "生命",
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_rgb(0x7f, 0x9d, 0xb0),
                );
                for i in 0..8 {
                    let r = egui::Rect::from_min_size(
                        egui::pos2(bar_x + i as f32 * 11.0, 16.0),
                        egui::vec2(9.0, 12.0),
                    );
                    let on = p.stats.hp as i32 > i;
                    painter.rect_filled(
                        r,
                        egui::CornerRadius::same(2),
                        if on {
                            egui::Color32::from_rgb(0xff, 0x6b, 0x6b)
                        } else {
                            egui::Color32::from_rgb(0x12, 0x32, 0x4a)
                        },
                    );
                }
                // 细条：氧气/防护/喷气/激光
                for (label, val, color, y) in [
                    (
                        "氧气",
                        p.stats.o2,
                        egui::Color32::from_rgb(0x5b, 0xc0, 0xff),
                        32.0,
                    ),
                    (
                        "防护",
                        p.stats.haz,
                        egui::Color32::from_rgb(0xff, 0xb3, 0x47),
                        44.0,
                    ),
                    (
                        "喷气",
                        p.stats.jet,
                        egui::Color32::from_rgb(0xff, 0xb3, 0x47),
                        56.0,
                    ),
                    (
                        "激光",
                        p.stats.laser,
                        egui::Color32::from_rgb(0xff, 0x6a, 0x4d),
                        68.0,
                    ),
                ] {
                    painter.text(
                        egui::pos2(0.0, y + 1.0),
                        egui::Align2::LEFT_TOP,
                        label,
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_rgb(0x7f, 0x9d, 0xb0),
                    );
                    let r = egui::Rect::from_min_size(egui::pos2(bar_x, y), egui::vec2(170.0, 6.0));
                    painter.rect_filled(
                        r,
                        egui::CornerRadius::same(2),
                        egui::Color32::from_rgb(0x12, 0x32, 0x4a),
                    );
                    let w = (170.0 * (val / 100.0).clamp(0.0, 1.0)).max(if val > 0.0 {
                        2.0
                    } else {
                        0.0
                    });
                    painter.rect_filled(
                        egui::Rect::from_min_size(r.min, egui::vec2(w, 6.0)),
                        egui::CornerRadius::same(2),
                        color,
                    );
                }
            }
            ui.add_space(80.0);
            if let Some(g) = game.as_ref() {
                ui.label(
                    egui::RichText::new(format!("₪ {}   ·   {}", p.credits, g.galaxy.name))
                        .size(14.0)
                        .color(egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
                );
                if !flying {
                    ui.label(
                        egui::RichText::new(format!(
                            "第 {} 颗星球 · 已到访 {}",
                            g.galaxy.planets.len(),
                            g.galaxy_count
                        ))
                        .size(13.0),
                    );
                }
            }
            if let Some(w) = world.as_ref() {
                let b = w.biome();
                let haz = if b.haz_name.is_empty() {
                    "宜居"
                } else {
                    b.haz_name
                };
                ui.label(
                    egui::RichText::new(format!(
                        "{} {}  ({:.0}, {:.0}, {:.0})",
                        b.name, haz, p.pos.x, p.pos.y, p.pos.z
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
                egui::RichText::new(format!(
                    "⚡ {} / {:.0} kW",
                    runtime.power.generation, runtime.power.used
                ))
                .size(13.0)
                .color(if runtime.power.sat < 0.99 {
                    egui::Color32::from_rgb(0xff, 0x55, 0x55)
                } else {
                    egui::Color32::from_rgb(0xff, 0xb3, 0x47)
                }),
            );
        });

    // 任务日志 top-right
    if let Some(qs) = quests.as_ref() {
        egui::Area::new(egui::Id::new("quests"))
            .fixed_pos(egui::pos2((screen.max.x - 262.0).max(8.0), 12.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(254.0_f32.min(screen.width().max(180.0)));
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
                        match qs.progress(p) {
                            Some(pr) => format!("▸ {} · {}", q.title, pr),
                            None => format!("▸ {}", q.title),
                        }
                    } else {
                        q.title.to_string()
                    };
                    let mut rt = egui::RichText::new(text).size(12.0).color(if done {
                        egui::Color32::from_rgb(0x7d, 0xff, 0x8a)
                    } else {
                        egui::Color32::WHITE
                    });
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
    if flying && let Some(s) = ship.as_ref() {
        // 速度表
        egui::Area::new(egui::Id::new("speedo"))
            .fixed_pos(egui::pos2(screen.center().x - 60.0, screen.max.y - 96.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0}", s.speed))
                        .size(38.0)
                        .strong()
                        .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                );
            });
        // 高度/大气层顶
        let alt_text = match *mode {
            crate::space::FlightMode::Atmo | crate::space::FlightMode::AtmoLand => {
                let gh = world
                    .as_ref()
                    .map(|w| w.top_at(s.pos.x.floor() as i32, s.pos.z.floor() as i32) as f32)
                    .unwrap_or(32.0);
                format!(
                    "高度 {:.0}m · 大气层顶 {:.0}m",
                    (s.pos.y - gh).max(0.0),
                    (crate::space::EXIT_Y - s.pos.y).max(0.0)
                )
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
            .fixed_pos(egui::pos2(
                (screen.max.x - 300.0).max(8.0),
                screen.max.y - 96.0,
            ))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_min_width(260.0_f32.min(screen.width().max(180.0)));
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
        if let Some(g) = game.as_ref()
            && let Some(lock) = &g.warp_lock
        {
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
            let target_dir = crate::space::galaxy_dir(lock.seed);
            let forward = crate::space::ship_forward(s.yaw, s.pitch);
            let align = forward.dot(target_dir);
            let side = forward.cross(target_dir).y;
            let guide = if align > 0.96 {
                "▲ 目标在准星前方"
            } else if side > 0.0 {
                "◀ 向左转向目标"
            } else {
                "▶ 向右转向目标"
            };
            egui::Area::new(egui::Id::new("warp_guidance"))
                .fixed_pos(egui::pos2(screen.center().x - 95.0, 104.0))
                .interactable(false)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} · 对准度 {:.0}%",
                            guide,
                            align.max(0.0) * 100.0
                        ))
                        .size(13.0)
                        .color(if align > 0.96 {
                            egui::Color32::from_rgb(0x7d, 0xff, 0x8a)
                        } else {
                            egui::Color32::from_rgb(0xff, 0xd1, 0x66)
                        }),
                    );
                });
            // 三维方向指引：目标在屏幕内画准星标记，在屏幕外画指向箭头
            if let Ok((_cam, cam_gt)) = cam_q.single() {
                let cam_pos = cam_gt.translation();
                let cam_fwd = cam_gt.rotation() * Vec3::NEG_Z;
                let cam_right = cam_gt.rotation() * Vec3::X;
                let cam_up = cam_gt.rotation() * Vec3::Y;
                let rel = (s.pos + target_dir * 5000.0) - cam_pos;
                let fd = rel.dot(cam_fwd);
                if fd > 1.0 {
                    let half_v = (75.0f32.to_radians() / 2.0).tan();
                    let aspect = (screen.width() / screen.height().max(1.0)).max(0.5);
                    let half_h = half_v * aspect;
                    let mut sx = screen.center().x
                        + (rel.dot(cam_right) / fd) / half_h * screen.width() * 0.5;
                    let mut sy =
                        screen.center().y - (rel.dot(cam_up) / fd) / half_v * screen.height() * 0.5;
                    let margin = 36.0;
                    if sx >= margin
                        && sx <= screen.width() - margin
                        && sy >= margin
                        && sy <= screen.max.y - margin
                    {
                        egui::Area::new(egui::Id::new("warp_marker"))
                            .fixed_pos(egui::pos2(sx - 14.0, sy - 14.0))
                            .interactable(false)
                            .show(ctx, |ui| {
                                ui.painter().text(
                                    ui.min_rect().center(),
                                    egui::Align2::CENTER_CENTER,
                                    "◎",
                                    egui::FontId::proportional(24.0),
                                    egui::Color32::from_rgb(0x7d, 0xff, 0x8a),
                                );
                            });
                    } else {
                        sx = sx.clamp(margin, screen.width() - margin);
                        sy = sy.clamp(margin, screen.max.y - margin);
                        let ang = (screen.center().y - sy).atan2(screen.center().x - sx);
                        let dir = egui::vec2(ang.cos(), ang.sin());
                        let perp = egui::vec2(-dir.y, dir.x);
                        let tip = dir * 14.0;
                        let base1 = -dir * 5.0 + perp * 7.0;
                        let base2 = -dir * 5.0 - perp * 7.0;
                        egui::Area::new(egui::Id::new("warp_arrow"))
                            .fixed_pos(egui::pos2(sx, sy))
                            .interactable(false)
                            .show(ctx, |ui| {
                                let c = ui.min_rect().center();
                                ui.painter().add(egui::Shape::convex_polygon(
                                    vec![c + tip, c + base1, c + base2],
                                    egui::Color32::from_rgb(0x7d, 0xff, 0x8a),
                                    egui::Stroke::NONE,
                                ));
                            });
                    }
                }
            }
        }
        // 操作提示
        egui::Area::new(egui::Id::new("flighthint"))
                .fixed_pos(egui::pos2(
                    (screen.center().x - 250.0).max(8.0),
                    screen.max.y - 60.0,
                ))
                .interactable(false)
                .show(ctx, |ui| {
                    ui.set_min_width(500.0_f32.min(screen.width().max(220.0)));
                    let hint = match *mode {
                        crate::space::FlightMode::Atmo => "W/S 油门 · Shift 加力 · A/D 滚转 · E 降落 · 拉升冲出大气层",
                        crate::space::FlightMode::Space => "W/S 油门 · Shift 加力 · J 脉冲 · C 扫描 · M 星系图 · 左键 开火 · 飞向空间站自动泊入 · 冲向星球再入",
                        _ => "",
                    };
                    ui.label(
                        egui::RichText::new(hint)
                            .size(12.0)
                            .color(egui::Color32::from_rgba_unmultiplied(0xc9, 0xe6, 0xee, 0xcc)),
                    );
                });
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
        let prompt_width = screen.width().clamp(280.0, 520.0);
        egui::Area::new(egui::Id::new("prompt"))
            .fixed_pos(egui::pos2(
                screen.center().x - prompt_width * 0.5,
                screen.max.y - slot_px - 52.0,
            ))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_min_width(prompt_width);
                ui.set_max_width(prompt_width);
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
            .fixed_pos(egui::pos2(screen.max.x - 260.0, 10.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(format!("{:.0} fps", 1.0 / time.delta_secs().max(1e-6)));
                ui.label(format!(
                    "高度 {:.0} · LOD 目标/驻留/可见 {}/{}/{}",
                    p.eye().y,
                    runtime.lod.stats.target_sections,
                    runtime.lod.stats.resident_sections,
                    runtime.lod.stats.visible_sections,
                ));
                ui.label(format!(
                    "队列 {} · 本帧 {} · {:.2} ms · 父级回退 {}",
                    runtime.lod.stats.queued_sections,
                    runtime.lod.stats.generated_this_frame,
                    runtime.lod.stats.build_ms,
                    runtime.lod.stats.parent_fallbacks,
                ));
            });
    }

    // research progress
    if let Some((id, prog)) = &research.active
        && let Some(tech) = data::TECHS.iter().find(|t| t.id == *id)
    {
        egui::Area::new(egui::Id::new("research"))
            .fixed_pos(egui::pos2(screen.center().x - 120.0, screen.max.y - 80.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "研究中：{} {:.0}%",
                    tech.name,
                    prog / tech.time * 100.0
                ));
            });
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
    if let Some((name, text, chars, total)) = dialog {
        let shown: String = text.chars().take(chars).collect();
        let full = chars >= total;
        egui::Area::new(egui::Id::new("dialogbox"))
            .fixed_pos(egui::pos2(screen.center().x - 320.0, screen.max.y - 190.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(
                        0x06, 0x0d, 0x16, 0xf0,
                    ))
                    .stroke(egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgb(0x0e, 0x6d, 0x78),
                    ))
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
        let message_width = screen.width().clamp(300.0, 600.0);
        egui::Area::new(egui::Id::new("bigmsg"))
            .fixed_pos(egui::pos2(
                screen.center().x - message_width * 0.5,
                screen.height() * 0.30,
            ))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_width(message_width);
                ui.set_max_width(message_width);
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

/// F3 光照调节面板。参数写入共享资源，daynight_system 会在下一帧把它们
/// 应用到地面/太空方向光、SunDisk、大气 IBL 和全局环境光。
pub fn lighting_panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut lighting: ResMut<crate::daynight::LightingTuning>,
    mut settings: ResMut<Settings>,
) {
    if ui_state.panel != Panel::Lighting {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }

    let mut close = false;
    let mut changed = false;
    egui::Window::new("☀ 光照调节")
        .id(egui::Id::new("lighting_panel"))
        .collapsible(false)
        .resizable(false)
        .default_pos(egui::pos2(24.0, 96.0))
        .show(ctx, |ui| {
            ui.label("实时调整，下一帧生效");
            ui.separator();

            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.sunlight_boost, 0.0..=150.0)
                        .text("太阳直射倍率"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.sun_disk_intensity, 0.0..=200.0)
                        .text("太阳盘亮度"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.atmosphere_fill, 0.0..=2.0)
                        .text("室外大气补光"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.ambient_multiplier, 0.0..=10.0)
                        .text("全局环境光倍率"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.space_atmosphere_fill, 0.0..=2.0)
                        .text("太空环境补光"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.bloom_intensity, 0.0..=8.0)
                        .text("Bloom 溢出强度"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.bloom_threshold, 0.0..=50.0).text("Bloom 阈值"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.bloom_threshold_softness, 0.0..=1.0)
                        .text("Bloom 阈值柔化"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut lighting.bloom_low_frequency_boost, 0.0..=5.0)
                        .text("Bloom 低频扩散"),
                )
                .changed();

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("恢复默认").clicked() {
                    *lighting = crate::daynight::LightingTuning::default();
                    changed = true;
                }
                if ui.button("关闭 (F3 / Esc)").clicked() {
                    close = true;
                }
            });
        });
    if close {
        ui_state.close_panel();
    }
    if changed {
        lighting.sanitize();
        lighting.save_to_settings(&mut settings);
        let _ = crate::save::save_settings(&settings);
    }
}

/// Draw the parked ship's marker in world-relative screen space.  Keeping it
/// as a separate system avoids inflating the main HUD system's parameter list
/// and lets the marker follow the real camera projection.
pub fn ship_label_system(
    mut contexts: EguiContexts,
    mode: Res<crate::space::FlightMode>,
    game: Option<Res<crate::space::SpaceGame>>,
    ship_asset: Res<crate::space::ShipAsset>,
    ui_state: Res<UiState>,
    camera: HudCamera,
    gt_q: Query<&GlobalTransform>,
    tf_q: Query<&Transform>,
) {
    if *mode != crate::space::FlightMode::Planet || ui_state.locked() {
        return;
    }
    let Some(game) = game else { return };
    if game.ship_pos == Vec3::ZERO {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let Ok((camera, camera_transform)) = camera.query.single() else {
        return;
    };
    // 投影飞船实体真实位置（game.ship_pos 可能是旧坐标/存档坐标），
    // 优先 GlobalTransform，首帧未生成时退回 Transform。
    let ship_pos = ship_asset
        .entity
        .and_then(|e| {
            gt_q.get(e)
                .map(|gt| gt.translation())
                .ok()
                .or_else(|| tf_q.get(e).ok().map(|t| t.translation))
        })
        .unwrap_or(game.ship_pos);
    let Ok(viewport) = camera.world_to_viewport(camera_transform, ship_pos + Vec3::Y * 7.0) else {
        return;
    };
    let screen = ctx.content_rect();
    let ppp = ctx.pixels_per_point().max(1.0);
    let pos = egui::pos2(viewport.x / ppp, viewport.y / ppp);
    if pos.x <= -180.0
        || pos.x >= screen.width() + 180.0
        || pos.y <= -60.0
        || pos.y >= screen.height() + 60.0
    {
        return;
    }
    let label_width = 300.0_f32.min(screen.width().max(220.0));
    let label_x =
        (pos.x - label_width * 0.5).clamp(4.0, (screen.width() - label_width - 4.0).max(4.0));
    let label_y = (pos.y - 16.0).clamp(4.0, (screen.height() - 30.0).max(4.0));
    egui::Area::new(egui::Id::new("ship_world_label"))
        .fixed_pos(egui::pos2(label_x, label_y))
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_width(label_width);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "▣ 飞船  ({:.0}, {:.0}, {:.0})",
                        ship_pos.x, ship_pos.y, ship_pos.z
                    ))
                    .size(13.0)
                    .color(egui::Color32::from_rgb(0x7d, 0xff, 0x8a)),
                );
            });
            // 从标签底部画一条细线指向飞船实际位置，锚点一目了然
            let r = ui.min_rect();
            let bottom = egui::pos2(r.center().x, r.bottom());
            ui.painter().line_segment(
                [bottom, egui::pos2(pos.x, pos.y)],
                egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_unmultiplied(0x7d, 0xff, 0x8a, 110),
                ),
            );
        });
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

/// 背包槽位操作（JS bindSlotEvents 移植）。
enum InvAction {
    Left(usize),
    Right(usize),
    Shift(usize),
    Trash,
    Charge(&'static str),
    EquipHot(usize),
    Unequip(&'static str),
    ConsumeHot(usize),
}

/// 带左右键与 Shift 检测的槽位按钮响应。
struct SlotResp {
    clicked: bool,
    secondary: bool,
    shift: bool,
}

fn slot_button_ex(
    ui: &mut egui::Ui,
    cache: &EguiIcons,
    key: &str,
    slot: &Option<Slot>,
    selected: bool,
    size: f32,
    tooltip: bool,
) -> SlotResp {
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
    if tooltip && resp.hovered() {
        let item_key = if key.is_empty() {
            slot.as_ref().map(|s| s.item.clone())
        } else {
            Some(key.to_string())
        };
        if let Some(ik) = &item_key
            && let Some(it) = data::item_by_key(ik)
        {
            let cat = match it.cat {
                "res" => "资源",
                "mat" => "材料",
                "blk" => "方块",
                "mach" => "机器",
                "equip" => "装备",
                _ => "物品",
            };
            resp.clone().on_hover_text(format!(
                "{}\n{} · 基准价 ₪{}\n{}",
                it.name, cat, it.price, it.desc
            ));
        }
    }
    let shift = ui.input(|i| i.modifiers.shift);
    SlotResp {
        clicked: resp.clicked(),
        secondary: resp.secondary_clicked(),
        shift,
    }
}

pub fn inventory_panel_system(
    mut contexts: EguiContexts,
    cache: Res<EguiIcons>,
    mut player: Query<&mut Player>,
    mut ui_state: ResMut<UiState>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
    research: Res<Research>,
    world: Res<World>,
    icons: Res<IconMaterials>,
    mut tab: Local<usize>,
) {
    if ui_state.panel != Panel::Inventory {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut actions: Vec<InvAction> = Vec::new();
    let mut craft_request: Option<(usize, i32)> = None;
    let mut sort_request = false;

    // snapshot
    let credits = player.single().map(|p| p.credits).unwrap_or(0);
    let inv_snapshot = player
        .single()
        .map(|p| p.inv.slots.clone())
        .unwrap_or_default();
    let stats_snap = player
        .single()
        .map(|p| p.stats.clone())
        .unwrap_or_else(|_| crate::player::Stats::full());
    let equipment_snap = player
        .single()
        .map(|p| p.equipment.clone())
        .unwrap_or_default();
    let hot_selection = player.single().ok().and_then(|p| {
        p.hot_slot()
            .and_then(|index| p.inv.slots[index].clone().map(|slot| (index, slot)))
    });
    let cursor_snap = ui_state.cursor.clone();
    let drop_mult = player
        .single()
        .map(|p| p.difficulty.drop_mult())
        .unwrap_or(1.0);

    egui::Window::new("◈ 外骨骼背包")
        .default_size([820.0, 640.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut !close)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("🧹 整理").clicked() {
                    sort_request = true;
                }
                ui.label(format!("₪ {credits}"));
                if let Some(c) = &cursor_snap {
                    ui.label(
                        egui::RichText::new(format!("手持：{} ×{}", item_name(&c.item), c.n))
                            .color(egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕ 关闭 (Tab)").clicked() {
                        close = true;
                    }
                });
            });
            ui.columns(2, |cols| {
                // left: hotbar row + storage grid
                cols[0].label("快捷栏 (1-9 · 0=激光)");
                egui::Grid::new("inv_hotbar")
                    .num_columns(9)
                    .spacing([4.0, 4.0])
                    .show(&mut cols[0], |ui| {
                        for (i, s) in inv_snapshot
                            .iter()
                            .enumerate()
                            .take(crate::inventory::HOTBAR)
                        {
                            let s = s.clone();
                            let key = s.as_ref().map(|s| s.item.as_str()).unwrap_or("");
                            let r = slot_button_ex(ui, &cache, key, &s, false, 40.0, true);
                            if r.clicked && !r.shift {
                                actions.push(InvAction::Left(i));
                            } else if r.secondary {
                                actions.push(InvAction::Right(i));
                            } else if r.clicked && r.shift {
                                actions.push(InvAction::Shift(i));
                            }
                        }
                    });
                cols[0].add_space(6.0);
                cols[0].label("储物 (27 格)");
                egui::Grid::new("inv_grid")
                    .num_columns(9)
                    .spacing([4.0, 4.0])
                    .show(&mut cols[0], |ui| {
                        for (i, s) in inv_snapshot
                            .iter()
                            .enumerate()
                            .take(36)
                            .skip(crate::inventory::HOTBAR)
                        {
                            let s = s.clone();
                            let key = s.as_ref().map(|s| s.item.as_str()).unwrap_or("");
                            let r = slot_button_ex(ui, &cache, key, &s, false, 40.0, true);
                            if r.clicked && !r.shift {
                                actions.push(InvAction::Left(i));
                            } else if r.secondary {
                                actions.push(InvAction::Right(i));
                            } else if r.clicked && r.shift {
                                actions.push(InvAction::Shift(i));
                            }
                        }
                    });
                cols[0].add_space(6.0);
                // 充能面板
                cols[0].label("充能");
                for (sys, item, cost, gain) in data::CHARGE_DEFS {
                    let label = match *sys {
                        "laser" => "⚡",
                        "shield" => "🛡",
                        "hp" => "❤",
                        "o2" => "⭕",
                        _ => "🧪",
                    };
                    let name = data::item_by_key(item).map(|i| i.name).unwrap_or(item);
                    let cur = stats_snap.get(sys);
                    let max = match *sys {
                        "hp" => 8.0,
                        "shield" => 6.0,
                        _ => 100.0,
                    };
                    let can = cur < max - 0.01 && has_items(&inv_snapshot, &[(item, *cost)]);
                    if cols[0]
                        .add_enabled(
                            can,
                            egui::Button::new(format!(
                                "{label} {cur:.0}/{max:.0} 充能 {name}×{cost} → +{gain:.0}"
                            ))
                            .small(),
                        )
                        .clicked()
                    {
                        actions.push(InvAction::Charge(sys));
                    }
                }
                cols[0].add_space(6.0);
                cols[0].label("外骨骼装备");
                for (slot, label, equipped) in [
                    ("suit", "防护", equipment_snap.suit.as_deref()),
                    (
                        "life_support",
                        "生命维持",
                        equipment_snap.life_support.as_deref(),
                    ),
                    ("tool", "工具", equipment_snap.tool.as_deref()),
                    ("defense", "防御", equipment_snap.defense.as_deref()),
                ] {
                    cols[0].horizontal(|ui| {
                        ui.label(format!(
                            "{label}：{}",
                            equipped.map(item_name).unwrap_or("未装备")
                        ));
                        if equipped.is_some() && ui.small_button("卸下").clicked() {
                            actions.push(InvAction::Unequip(slot));
                        }
                    });
                }
                if let Some((index, selected)) = &hot_selection
                    && let Some(item) = data::item_by_key(&selected.item)
                {
                    if item.equipment.is_some()
                        && cols[0].button(format!("装备：{}", item.name)).clicked()
                    {
                        actions.push(InvAction::EquipHot(*index));
                    }
                    if matches!(selected.item.as_str(), "medkit" | "oxygen_cell" | "hazard_cell")
                        && cols[0].button(format!("使用：{}", item.name)).clicked()
                    {
                        actions.push(InvAction::ConsumeHot(*index));
                    }
                }
                cols[0].add_space(6.0);
                // 垃圾桶 + 操作提示
                cols[0].horizontal(|ui| {
                    let has_cursor = cursor_snap.is_some();
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::same(3),
                        egui::Color32::from_rgba_unmultiplied(0x2a, 0x14, 0x14, 0xCC),
                    );
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::same(3),
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x8a, 0x35, 0x35)),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "🗑",
                        egui::FontId::proportional(20.0),
                        egui::Color32::WHITE,
                    );
                    if resp.clicked() && has_cursor {
                        actions.push(InvAction::Trash);
                    }
                    ui.label(
                        egui::RichText::new("左键：选取/放下 · 右键：拆半/放1 · Shift+左键：快速移动 · 🗑：销毁手持 · 🧹：整理")
                            .size(11.0)
                            .color(egui::Color32::from_rgb(0x7f, 0x9d, 0xb0)),
                    );
                });
                // right: crafting
                cols[1].label("⚒ 便携合成 (Shift+点击 合成 5 个)");
                cols[1].horizontal(|ui| {
                    let labels = ["全部", "材料", "机器", "方块"];
                    for (i, l) in labels.iter().enumerate() {
                        if ui.selectable_label(*tab == i, *l).clicked() {
                            *tab = i;
                        }
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .show(&mut cols[1], |ui| {
                        for (idx, r) in data::RECIPES.iter().enumerate() {
                            if r.station != "hand" {
                                continue;
                            }
                            let Some(out_item) = data::item_by_key(r.output.0) else { continue };
                            let show = match *tab {
                                1 => out_item.cat == "mat",
                                2 => out_item.cat == "mach",
                                3 => out_item.cat == "blk",
                                _ => true,
                            };
                            if !show {
                                continue;
                            }
                            let tech_locked = !data::recipe_unlocked(&research.techs, r);
                            if tech_locked {
                                continue;
                            }
                            let affordable = has_items(&inv_snapshot, r.inputs);
                            let out_n = (r.output.1 as f32 * drop_mult).round() as i32;
                            let mut cost_parts = Vec::new();
                            for (i, n) in r.inputs {
                                let have: i32 = inv_snapshot
                                    .iter()
                                    .flatten()
                                    .filter(|s| s.item == *i)
                                    .map(|s| s.n)
                                    .sum();
                                let name = data::item_by_key(i).map(|i| i.name).unwrap_or(i);
                                cost_parts.push((
                                    name,
                                    *n,
                                    have >= *n,
                                ));
                            }
                            ui.horizontal(|ui| {
                                draw_slot(ui, &cache, r.output.0, None, false, 34.0);
                                ui.vertical(|ui| {
                                    ui.label(format!("{} ×{}", out_item.name, out_n));
                                    ui.horizontal(|ui| {
                                        for (name, n, ok) in &cost_parts {
                                            ui.label(
                                                egui::RichText::new(format!("{name}×{n}"))
                                                    .size(11.0)
                                                    .color(if *ok {
                                                        egui::Color32::from_rgb(0x7d, 0xff, 0x8a)
                                                    } else {
                                                        egui::Color32::from_rgb(0xff, 0x55, 0x55)
                                                    }),
                                            );
                                        }
                                        ui.label(
                                            egui::RichText::new(format!("({:.1}s)", r.time))
                                                .size(11.0)
                                                .color(egui::Color32::GRAY),
                                        );
                                    });
                                });
                                let resp = ui.add_enabled(affordable, egui::Button::new("合成").small());
                                if resp.clicked() {
                                    let count = if resp.ctx.input(|i| i.modifiers.shift) { 5 } else { 1 };
                                    craft_request = Some((idx, count));
                                }
                            });
                            ui.separator();
                        }
                    });
            });
        });

    if close {
        ui_state.close_panel();
        if let Ok(mut p) = player.single_mut() {
            drop_cursor(&mut ui_state, &mut p, &mut commands, &world, &icons, &sfx);
        }
    }
    // 应用槽位操作
    let Ok(mut p) = player.single_mut() else {
        return;
    };
    for a in actions {
        match a {
            InvAction::Left(i) => {
                let slot = p.inv.slots[i].clone();
                match ui_state.cursor.take() {
                    None => {
                        if let Some(s) = slot {
                            ui_state.cursor = Some(s);
                            p.inv.slots[i] = None;
                            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                        }
                    }
                    Some(c) => match slot {
                        None => {
                            p.inv.slots[i] = Some(c);
                            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                        }
                        Some(s) if s.item == c.item => {
                            let stack = data::item_by_key(&c.item).map(|i| i.stack).unwrap_or(250);
                            let add = (stack - s.n).min(c.n);
                            if add > 0 {
                                p.inv.slots[i] = Some(Slot {
                                    item: s.item,
                                    n: s.n + add,
                                });
                                let left = c.n - add;
                                if left > 0 {
                                    ui_state.cursor = Some(Slot {
                                        item: c.item,
                                        n: left,
                                    });
                                }
                                audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                            } else {
                                ui_state.cursor = Some(c);
                            }
                        }
                        Some(s) => {
                            // 不同物品：交换
                            p.inv.slots[i] = Some(c);
                            ui_state.cursor = Some(s);
                            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                        }
                    },
                }
            }
            InvAction::Right(i) => {
                let slot = p.inv.slots[i].clone();
                match ui_state.cursor.take() {
                    None => {
                        if let Some(s) = slot {
                            if s.n <= 1 {
                                ui_state.cursor = Some(s);
                                p.inv.slots[i] = None;
                            } else {
                                let half = (s.n as f32 / 2.0).ceil() as i32;
                                ui_state.cursor = Some(Slot {
                                    item: s.item.clone(),
                                    n: half,
                                });
                                p.inv.slots[i] = Some(Slot {
                                    item: s.item,
                                    n: s.n - half,
                                });
                            }
                            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                        }
                    }
                    Some(c) => match slot {
                        None => {
                            // 放 1 个
                            let n = c.n - 1;
                            p.inv.slots[i] = Some(Slot {
                                item: c.item.clone(),
                                n: 1,
                            });
                            if n > 0 {
                                ui_state.cursor = Some(Slot { item: c.item, n });
                            }
                            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                        }
                        Some(s) if s.item == c.item => {
                            let stack = data::item_by_key(&c.item).map(|i| i.stack).unwrap_or(250);
                            if s.n < stack {
                                p.inv.slots[i] = Some(Slot {
                                    item: s.item,
                                    n: s.n + 1,
                                });
                                let left = c.n - 1;
                                if left > 0 {
                                    ui_state.cursor = Some(Slot {
                                        item: c.item,
                                        n: left,
                                    });
                                }
                                audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                            } else {
                                ui_state.cursor = Some(c);
                            }
                        }
                        Some(_) => {
                            ui_state.cursor = Some(c);
                        }
                    },
                }
            }
            InvAction::Shift(i) => {
                // 快速移动：热栏 ↔ 储物
                let target_range: Vec<usize> = if i < crate::inventory::HOTBAR {
                    (crate::inventory::HOTBAR..36).collect()
                } else {
                    (0..crate::inventory::HOTBAR).collect()
                };
                let Some(s) = p.inv.slots[i].clone() else {
                    continue;
                };
                let stack = data::item_by_key(&s.item).map(|i| i.stack).unwrap_or(250);
                // 先合并到部分堆
                let mut moved = 0;
                for t in &target_range {
                    if moved >= s.n {
                        break;
                    }
                    if let Some(ts) = &mut p.inv.slots[*t]
                        && ts.item == s.item
                        && ts.n < stack
                    {
                        let add = (stack - ts.n).min(s.n - moved);
                        ts.n += add;
                        moved += add;
                    }
                }
                // 再放入空格
                for t in &target_range {
                    if moved >= s.n {
                        break;
                    }
                    if p.inv.slots[*t].is_none() {
                        let add = (s.n - moved).min(stack);
                        p.inv.slots[*t] = Some(Slot {
                            item: s.item.clone(),
                            n: add,
                        });
                        moved += add;
                    }
                }
                if moved > 0 {
                    let left = s.n - moved;
                    if left > 0 {
                        p.inv.slots[i] = Some(Slot {
                            item: s.item,
                            n: left,
                        });
                    } else {
                        p.inv.slots[i] = None;
                    }
                    audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                }
            }
            InvAction::Trash => {
                if ui_state.cursor.take().is_some() {
                    audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                }
            }
            InvAction::Charge(sys) => {
                if p.charge(sys) {
                    audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
                } else {
                    audio::play(&mut commands, sfx.error.clone(), 0.5, None);
                }
            }
            InvAction::EquipHot(index) => {
                let Some(slot) = p.inv.slots.get(index).cloned().flatten() else {
                    continue;
                };
                match p.equipment.equip(&slot.item) {
                    Ok(previous) => {
                        p.inv.take_from_slot(index, 1);
                        if let Some(previous) = previous {
                            let added = p.inv.add_item(&previous, 1);
                            if added == 0 {
                                crate::creatures::spawn_drop(
                                    &mut commands,
                                    &world,
                                    &icons,
                                    p.pos + Vec3::Y * 0.5,
                                    Vec3::ZERO,
                                    previous,
                                    1,
                                    0.4,
                                );
                            }
                        }
                        let max_o2 = p.stat_max("o2");
                        let max_shield = p.stat_max("shield");
                        p.stats.o2 = p.stats.o2.min(max_o2);
                        p.stats.shield = p.stats.shield.min(max_shield);
                        audio::play(&mut commands, sfx.click.clone(), 0.5, None);
                    }
                    Err(message) => p.toast(message),
                }
            }
            InvAction::Unequip(slot_name) => {
                if let Some(item) = p.equipment.take_slot(slot_name) {
                    let added = p.inv.add_item(&item, 1);
                    if added == 0 {
                        crate::creatures::spawn_drop(
                            &mut commands,
                            &world,
                            &icons,
                            p.pos + Vec3::Y * 0.5,
                            Vec3::ZERO,
                            item,
                            1,
                            0.4,
                        );
                    }
                    let max_o2 = p.stat_max("o2");
                    let max_shield = p.stat_max("shield");
                    p.stats.o2 = p.stats.o2.min(max_o2);
                    p.stats.shield = p.stats.shield.min(max_shield);
                }
            }
            InvAction::ConsumeHot(index) => {
                let Some(slot) = p.inv.slots.get(index).cloned().flatten() else {
                    continue;
                };
                let used = match slot.item.as_str() {
                    "medkit" if p.stats.hp < p.stat_max("hp") => {
                        let max_hp = p.stat_max("hp");
                        p.stats.hp = (p.stats.hp + 6.0).min(max_hp);
                        true
                    }
                    "oxygen_cell" if p.stats.o2 < p.stat_max("o2") => {
                        let max_o2 = p.stat_max("o2");
                        p.stats.o2 = (p.stats.o2 + 80.0).min(max_o2);
                        true
                    }
                    "hazard_cell" if p.stats.haz < 100.0 => {
                        p.stats.haz = (p.stats.haz + 70.0).min(100.0);
                        true
                    }
                    _ => false,
                };
                if used {
                    p.inv.take_from_slot(index, 1);
                    audio::play(&mut commands, sfx.pickup.clone(), 0.6, None);
                } else {
                    p.toast("当前无需使用该消耗品");
                }
            }
        }
    }
    if sort_request {
        p.inv.sort_storage();
        audio::play(&mut commands, sfx.click.clone(), 0.4, None);
    }
    if let Some((idx, count)) = craft_request {
        let r = &data::RECIPES[idx];
        let mut crafted = 0;
        for _ in 0..count {
            if !p.inv.pay_items(r.inputs) {
                break;
            }
            let out_n = (r.output.1 as f32 * drop_mult).round() as i32;
            let added = p.inv.add_item(r.output.0, out_n);
            let left = out_n - added;
            if left > 0 {
                crate::creatures::spawn_drop(
                    &mut commands,
                    &world,
                    &icons,
                    p.pos + Vec3::Y * 0.6,
                    Vec3::ZERO,
                    r.output.0.to_string(),
                    left,
                    0.4,
                );
            }
            crafted += 1;
        }
        if crafted > 0 {
            audio::play(&mut commands, sfx.craft.clone(), 0.6, None);
            p.toast(format!(
                "合成：{} ×{}",
                data::item_by_key(r.output.0)
                    .map(|i| i.name)
                    .unwrap_or(r.output.0),
                crafted
            ));
        } else {
            audio::play(&mut commands, sfx.error.clone(), 0.5, None);
            p.toast("材料不足");
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
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut start: Option<&'static str> = None;
    let inv_snapshot = player
        .single()
        .map(|p| p.inv.slots.clone())
        .unwrap_or_default();

    egui::Window::new("科技树")
        .default_size([1020.0, 520.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut !close)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (T)").clicked() {
                close = true;
            }
            let data_n = player
                .single()
                .map(|p| p.inv.count_item("data"))
                .unwrap_or(0);
            ui.label(
                egui::RichText::new(format!("⬡ 研究数据 ×{data_n}"))
                    .size(14.0)
                    .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
            );
            let (resp, painter) =
                ui.allocate_painter(egui::vec2(990.0, 440.0), egui::Sense::hover());
            // Project the full technology coordinate space into the visible
            // canvas. The old unscaled positions placed late-game nodes well
            // outside the fixed-size window.
            let project = |pos: (f32, f32)| {
                resp.rect.min + egui::vec2(55.0 + pos.0 * 0.62, 20.0 + pos.1 * 0.58)
            };
            // 连线三态配色（JS: done #7dff8a66 / req 完成 #ffb34766 / 锁定 #24405a，非 done 虚线 6 4）
            for t in data::TECHS {
                for req in t.req {
                    if let Some(rt) = data::TECHS.iter().find(|x| x.id == *req) {
                        let a = project(rt.pos);
                        let b = project(t.pos);
                        let t_done = research.techs.iter().any(|x| x == t.id) || t.unlocked;
                        let req_done = data::tech_unlocked(&research.techs, req);
                        let (color, dash) = if t_done {
                            (
                                egui::Color32::from_rgba_unmultiplied(0x7d, 0xff, 0x8a, 0x40),
                                0.0,
                            )
                        } else if req_done {
                            (
                                egui::Color32::from_rgba_unmultiplied(0xff, 0xb3, 0x47, 0x40),
                                6.0,
                            )
                        } else {
                            (egui::Color32::from_rgb(0x24, 0x40, 0x5a), 6.0)
                        };
                        if dash > 0.0 {
                            painter.add(egui::Shape::dashed_line(
                                &[a, b],
                                egui::Stroke::new(2.0, color),
                                dash,
                                4.0,
                            ));
                        } else {
                            painter.line_segment([a, b], egui::Stroke::new(2.0, color));
                        }
                    }
                }
            }
            for t in data::TECHS {
                let center = project(t.pos);
                let rect = egui::Rect::from_center_size(center, egui::vec2(130.0, 66.0));
                let researched = research.techs.iter().any(|x| x == t.id) || t.unlocked;
                let affordable = has_items(&inv_snapshot, t.cost);
                let req_met = data::tech_requirements_met(&research.techs, t);
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
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(5.0, 5.0),
                        egui::vec2(28.0, 28.0),
                    ),
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
                        .map(|(i, n)| {
                            format!(
                                "{}×{}",
                                data::item_by_key(i).map(|i| i.name).unwrap_or(i),
                                n
                            )
                        })
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
                let in_research = research
                    .active
                    .as_ref()
                    .map(|(id, _)| id == t.id)
                    .unwrap_or(false);
                if !researched && req_met && !in_research {
                    let btn_rect = egui::Rect::from_min_size(
                        rect.min + egui::vec2(0.0, 46.0),
                        egui::vec2(130.0, 18.0),
                    );
                    let resp = ui.interact(
                        btn_rect,
                        egui::Id::new(("tech", t.id)),
                        egui::Sense::click(),
                    );
                    painter.rect_filled(
                        btn_rect,
                        egui::CornerRadius::same(3),
                        egui::Color32::from_rgb(0x2e, 0x55, 0x6e),
                    );
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
                    let Some((_, prog)) = research.active.as_ref() else {
                        return;
                    };
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
        let Some(tech) = data::TECHS.iter().find(|t| t.id == id) else {
            return;
        };
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
    research: Res<Research>,
    mut ui_state: ResMut<UiState>,
    mut q: Query<(&Machine, &mut MachineState)>,
    power: Res<crate::factory::Power>,
    world: Res<World>,
    icons: Res<IconMaterials>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    let Panel::Machine(e) = ui_state.panel else {
        return;
    };
    let Some((m, _)) = q.get(e).ok() else {
        ui_state.panel = Panel::None;
        return;
    };
    let kind = m.kind;
    let pos = m.pos;
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut actions: Vec<MachinePanelAction> = Vec::new();
    let mut open = true;

    // 快照
    let inv_snapshot = player
        .single()
        .map(|p| p.inv.slots.clone())
        .unwrap_or_default();
    let sel_info = ui_state
        .selected_inv
        .and_then(|i| inv_snapshot.get(i).cloned().flatten());
    let state_snap = q.get(e).ok().map(|(_, s)| s.clone());

    egui::Window::new(format!("◈ {}", kind.label()))
        .default_size([420.0, 620.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (Esc)").clicked() {
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
                        draw_slot(
                            ui,
                            &cache,
                            f.fuel.as_ref().map(|s| s.item.as_str()).unwrap_or(""),
                            f.fuel.clone(),
                            false,
                            40.0,
                        );
                        ui.label(format!("{:.1}s {}", f.burn, if f.on { "🔥" } else { "" }));
                    });
                    ui.horizontal(|ui| {
                        ui.label("输入:");
                        draw_slot(
                            ui,
                            &cache,
                            f.input.as_ref().map(|s| s.item.as_str()).unwrap_or(""),
                            f.input.clone(),
                            false,
                            40.0,
                        );
                        ui.label("输出:");
                        draw_slot(
                            ui,
                            &cache,
                            f.output.as_ref().map(|s| s.item.as_str()).unwrap_or(""),
                            f.output.clone(),
                            false,
                            40.0,
                        );
                    });
                    if let Some(rid) = f.recipe
                        && let Some(r) = data::RECIPES.iter().find(|r| r.id == rid)
                    {
                        ui.label(format!(
                            "烧炼中：{} {:.0}%",
                            data::item_by_key(r.output.0)
                                .map(|i| i.name)
                                .unwrap_or(r.output.0),
                            f.prog / r.time * 100.0
                        ));
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
                Some(MachineState::Tank(c)) => {
                    ui.label("仅接受酸液、冷却剂、氧气电池和环境净化电池。");
                    let slots = c.slots.clone();
                    chest_grid(ui, &cache, &slots, &mut actions);
                    if let Some(sel) = &sel_info {
                        ui.label(format!("选中：{} ×{}", item_name(&sel.item), sel.n));
                        if ui.button("📥 注入选中流体").clicked() {
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
                    ui.label(if m.active {
                        "⛏ 开采中"
                    } else {
                        "⏸ 待机"
                    });
                    if let Some(output) = &mn.output {
                        ui.label(format!(
                            "产出：{} ×{}",
                            item_name(output.item.as_str()),
                            output.n
                        ));
                    }
                    ui.label(format!("矿脉消耗：{}/300", mn.deposit));
                    if ui.button("📤 取出产出").clicked() {
                        actions.push(MachinePanelAction::TakeOutput);
                    }
                }
                Some(MachineState::Crafter(cr)) => {
                    // 配方选择（装配机含便携配方，JS where:'both' 语义）
                    let where_ = if kind == MachineKind::Refinery {
                        "refinery"
                    } else {
                        "assembler"
                    };
                    let mut current = cr.recipe.unwrap_or("");
                    let avail: Vec<&'static data::Recipe> = data::RECIPES
                        .iter()
                        .filter(|r| {
                            data::recipe_unlocked(&research.techs, r)
                                && (r.station == where_
                                    || r.station == "both"
                                    || (where_ == "assembler" && r.station == "hand"))
                        })
                        .collect();
                    egui::ComboBox::from_id_salt("recipe_pick")
                        .selected_text(if current.is_empty() {
                            "选择配方".to_string()
                        } else {
                            current.to_string()
                        })
                        .show_ui(ui, |ui| {
                            for r in &avail {
                                let out_name = data::item_by_key(r.output.0)
                                    .map(|i| i.name)
                                    .unwrap_or(r.output.0);
                                ui.selectable_value(
                                    &mut current,
                                    r.id,
                                    format!("{} → {} ×{}", recipe_in_str(r), out_name, r.output.1),
                                );
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
                    ui.label(format!(
                        "进度 {:.0}% · {}",
                        cr.prog * 100.0,
                        if m.active {
                            "⚙ 运行中"
                        } else {
                            "⏸ 待机"
                        }
                    ));
                    if let Some(output) = &cr.output {
                        ui.label(format!(
                            "产出：{} ×{}",
                            item_name(output.item.as_str()),
                            output.n
                        ));
                    }
                    if let Some(_sel) = &sel_info
                        && ui.button("📥 投入选中物品").clicked()
                    {
                        actions.push(MachinePanelAction::InsertInput);
                    }
                    if ui.button("📤 取出产出").clicked() {
                        actions.push(MachinePanelAction::TakeOutput);
                    }
                }
                Some(MachineState::Reactor(r)) => {
                    ui.label(format!(
                        "铀燃料余量：{:.1}s {}",
                        r.fuel,
                        if m.active { "☢ 发电中" } else { "" }
                    ));
                    if let Some(sel) = &sel_info {
                        if sel.item == "uranium" && ui.button("☢ 投料铀-235（+60s）").clicked()
                        {
                            actions.push(MachinePanelAction::InsertInput);
                        }
                    } else {
                        ui.label("在背包选中铀-235 后可投料");
                    }
                }
                Some(MachineState::Burner(b)) => {
                    ui.horizontal(|ui| {
                        ui.label("燃料:");
                        draw_slot(
                            ui,
                            &cache,
                            b.fuel.as_ref().map(|s| s.item.as_str()).unwrap_or(""),
                            b.fuel.clone(),
                            false,
                            40.0,
                        );
                        ui.label(format!(
                            "{:.1}s {}",
                            b.burn,
                            if m.active { "⚡ 发电中" } else { "" }
                        ));
                    });
                    if let Some(_sel) = &sel_info
                        && ui.button("⛽ 放入燃料").clicked()
                    {
                        actions.push(MachinePanelAction::InsertFuel);
                    }
                }
                Some(MachineState::Belt(b)) => {
                    ui.label(format!("{} 个物品在运输", b.items.len()));
                    if matches!(kind, MachineKind::Pipe | MachineKind::Pump) {
                        ui.label("流体从朝向方向输出，只接受密封流体物品。");
                    } else {
                        ui.label("物品从朝向方向输出到下一台机器/传送带。");
                    }
                }
                Some(MachineState::Router(router)) => {
                    ui.label(format!("{} 个物品在分流", router.items.len()));
                    if kind == MachineKind::Filter {
                        ui.label(format!(
                            "当前筛选：{}",
                            router.filter.as_deref().map(item_name).unwrap_or("未设置")
                        ));
                        if let Some(sel) = &sel_info
                            && ui
                                .button(format!("设为 {}", item_name(&sel.item)))
                                .clicked()
                        {
                            actions.push(MachinePanelAction::SetFilter(Some(sel.item.clone())));
                        }
                        if ui.button("清除筛选").clicked() {
                            actions.push(MachinePanelAction::SetFilter(None));
                        }
                        ui.label("匹配物走正面，其他物走右侧。配置不消耗物品。");
                    } else {
                        ui.label("物品按左、前、右顺序轮流分配；堵塞时尝试其他出口。");
                    }
                }
                Some(MachineState::Battery(battery)) => {
                    ui.label(format!(
                        "储能：{:.1} / {:.0} kWs ({:.0}%)",
                        battery.charge,
                        crate::factory::BATTERY_CAPACITY,
                        battery.charge / crate::factory::BATTERY_CAPACITY * 100.0
                    ));
                    ui.label("与电缆相邻后，只为所在局部电网充放电。");
                }
                Some(MachineState::Colony(colony)) => {
                    let oxygen = colony.input.get("oxygen_cell").copied().unwrap_or(0);
                    let medkits = colony.input.get("medkit").copied().unwrap_or(0);
                    let biofiber = colony.input.get("biofiber").copied().unwrap_or(0);
                    ui.label(format!(
                        "舱室规模：{} / 12 · 居民：{} / 8",
                        colony.habitat, colony.residents
                    ));
                    ui.label(format!(
                        "补给：压缩氧气瓶 {oxygen} · 医疗包 {medkits} · 生物纤维 {biofiber}"
                    ));
                    ui.add(
                        egui::ProgressBar::new(colony.prog.clamp(0.0, 1.0))
                            .show_percentage()
                            .text(if m.active {
                                "殖民周期运行中"
                            } else if colony.residents == 0 {
                                "需要至少 12 块舱室材料"
                            } else {
                                "等待补给、电力或输出空间"
                            }),
                    );
                    ui.label(format!(
                        "已完成 {} 个周期 · 每周期产出研究数据 ×2 与 ₪{}",
                        colony.cycles,
                        200 + colony.residents * 25
                    ));
                    if let Some(output) = &colony.output {
                        ui.label(format!("待取：{} ×{}", item_name(&output.item), output.n));
                    }
                    if let Some(sel) = &sel_info {
                        if crate::factory::colony_supply(&sel.item) {
                            if ui.button("📥 投入殖民补给").clicked() {
                                actions.push(MachinePanelAction::InsertInput);
                            }
                        } else {
                            ui.label("殖民核心仅接受压缩氧气瓶、医疗包和生物纤维。");
                        }
                    }
                    if ui.button("📤 取出研究数据").clicked() {
                        actions.push(MachinePanelAction::TakeOutput);
                    }
                }
                Some(MachineState::Turret(turret)) => {
                    ui.label(if !m.active {
                        "⚠ 电力不足"
                    } else if turret.engaged {
                        "⌖ 锁定敌对目标"
                    } else {
                        "✓ 自动警戒中"
                    });
                    ui.label("射程 24 格 · 单发伤害 3.5 · 射击间隔 0.65 秒");
                    ui.label("待机耗电 1 kW · 交战耗电 10 kW");
                    ui.label(format!("累计击杀：{}", turret.kills));
                    ui.label("遗迹守卫会被主动攻击；本地生物只在进入敌对状态后成为目标。");
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
                        ui.label(if m.active {
                            "⛏ 巡林伐木中"
                        } else {
                            "⏸ 待机"
                        });
                    }
                }
                Some(MachineState::Medbay(_)) => {
                    ui.label(if m.active {
                        "✚ 治疗中"
                    } else {
                        "⏸ 待机"
                    });
                    ui.label("站近自动治疗：每消耗 1 钠 + 1 氧气回复 3 生命");
                }
                _ => {
                    if matches!(
                        kind,
                        MachineKind::Solar
                            | MachineKind::Wind
                            | MachineKind::Launchpad
                            | MachineKind::Cable
                            | MachineKind::Geothermal
                    ) {
                        ui.label("该机器自动运行，无需操作。");
                    } else {
                        ui.label("该机器暂无面板。");
                    }
                }
            }
            ui.add_space(6.0);
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "⚡ 电网：发电 {} kW / 用电 {:.1} kW / 满足率 {:.0}%",
                    power.generation,
                    power.used,
                    power.sat * 100.0
                ))
                .size(13.0)
                .color(if power.sat < 0.99 {
                    egui::Color32::from_rgb(0xff, 0x55, 0x55)
                } else {
                    egui::Color32::from_rgb(0xff, 0xb3, 0x47)
                }),
            );
            // 外骨骼背包（JS：机器面板内嵌 36 格背包；Shift+点击 直接放入机器）
            ui.separator();
            ui.label(
                egui::RichText::new("◈ 外骨骼背包（点击选中 · Shift+点击 放入机器）")
                    .size(13.0)
                    .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
            );
            egui::Grid::new("mach_inv_grid")
                .num_columns(9)
                .spacing([4.0, 4.0])
                .show(ui, |ui| {
                    for (i, s) in inv_snapshot.iter().enumerate().take(36) {
                        let s = s.clone();
                        let key = s.as_ref().map(|s| s.item.as_str()).unwrap_or("");
                        let is_sel = ui_state.selected_inv == Some(i);
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(3),
                            if is_sel {
                                egui::Color32::from_rgb(0x23, 0x4a, 0x5e)
                            } else {
                                egui::Color32::from_rgba_unmultiplied(0x10, 0x14, 0x1a, 0xCC)
                            },
                        );
                        ui.painter().rect_stroke(
                            rect,
                            egui::CornerRadius::same(3),
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(0x35, 0x46, 0x55)),
                            egui::StrokeKind::Inside,
                        );
                        if !key.is_empty() {
                            let tex = egui_icon(&cache, key);
                            ui.painter().image(
                                tex,
                                egui::Rect::from_min_max(
                                    rect.min + egui::vec2(3.0, 3.0),
                                    rect.max - egui::vec2(3.0, 3.0),
                                ),
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                        if let Some(s) = &s {
                            ui.painter().text(
                                egui::pos2(rect.max.x - 3.0, rect.max.y - 11.0),
                                egui::Align2::RIGHT_BOTTOM,
                                format!("{}", s.n),
                                egui::FontId::proportional(10.0),
                                egui::Color32::WHITE,
                            );
                        }
                        if resp.clicked() {
                            let shift = ui.input(|i| i.modifiers.shift);
                            if shift {
                                actions.push(MachinePanelAction::InsertItem(i));
                            } else {
                                ui_state.selected_inv = if is_sel { None } else { Some(i) };
                            }
                        }
                        if (i + 1) % 9 == 0 {
                            ui.end_row();
                        }
                    }
                });
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
    let Ok(mut p) = player.single_mut() else {
        return;
    };
    let Some(mclone) = q.get(e).ok().map(|(m, _)| m.clone()) else {
        ui_state.panel = Panel::None;
        return;
    };
    let Ok((_m, mut st)) = q.get_mut(e) else {
        return;
    };
    for a in actions {
        match a {
            MachinePanelAction::InsertFuel => {
                if let Some(i) = ui_state.selected_inv
                    && let Some(s) = p.inv.slots.get(i).cloned().flatten()
                {
                    if data::fuel_value(&s.item) > 0.0 {
                        if crate::factory::machine_insert(&mclone, &mut st, &s.item) {
                            let removed = p.inv.take_from_slot(i, 1);
                            debug_assert_eq!(removed.as_ref().map(|taken| taken.n), Some(1));
                            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                        }
                    } else {
                        p.toast("不是燃料");
                    }
                }
            }
            MachinePanelAction::InsertInput => {
                if let Some(i) = ui_state.selected_inv
                    && let Some(s) = p.inv.slots.get(i).cloned().flatten()
                {
                    if crate::factory::machine_insert(&mclone, &mut st, &s.item) {
                        let removed = p.inv.take_from_slot(i, 1);
                        debug_assert_eq!(removed.as_ref().map(|taken| taken.n), Some(1));
                        audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                    } else {
                        p.toast("机器不接受该物品");
                    }
                }
            }
            MachinePanelAction::TakeOutput => {
                let out = match &mut *st {
                    MachineState::Furnace(f) => f.output.take(),
                    MachineState::Miner(mn) => mn.output.take(),
                    MachineState::Crafter(cr) => cr.output.take(),
                    MachineState::Colony(colony) => colony.output.take(),
                    _ => None,
                };
                if let Some(o) = out {
                    let added = p.inv.add_item(&o.item, o.n);
                    let left = o.n - added;
                    if left > 0 {
                        crate::creatures::spawn_drop(
                            &mut commands,
                            &world,
                            &icons,
                            Vec3::new(
                                mclone.pos[0] as f32 + 0.5,
                                mclone.pos[1] as f32 + 1.2,
                                mclone.pos[2] as f32 + 0.5,
                            ),
                            Vec3::ZERO,
                            o.item,
                            left,
                            0.4,
                        );
                    }
                    audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
                }
            }
            MachinePanelAction::ChestTake(i) => {
                let taken = match &mut *st {
                    MachineState::Chest(c) => c.slots.get_mut(i).and_then(|s| s.take()),
                    MachineState::Tank(c) => c.slots.get_mut(i).and_then(|s| s.take()),
                    MachineState::Collector(c) => c.slots.get_mut(i).and_then(|s| s.take()),
                    _ => None,
                };
                if let Some(s) = taken {
                    let added = p.inv.add_item(&s.item, s.n);
                    let left = s.n - added;
                    if left > 0 {
                        crate::creatures::spawn_drop(
                            &mut commands,
                            &world,
                            &icons,
                            Vec3::new(
                                mclone.pos[0] as f32 + 0.5,
                                mclone.pos[1] as f32 + 1.2,
                                mclone.pos[2] as f32 + 0.5,
                            ),
                            Vec3::ZERO,
                            s.item,
                            left,
                            0.4,
                        );
                    }
                    audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
                }
            }
            MachinePanelAction::ChestPut => {
                if let Some(i) = ui_state.selected_inv
                    && let Some(s) = p.inv.slots.get(i).cloned().flatten()
                    && crate::factory::machine_insert(&mclone, &mut st, &s.item)
                {
                    let removed = p.inv.take_from_slot(i, 1);
                    debug_assert_eq!(removed.as_ref().map(|taken| taken.n), Some(1));
                    audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                }
            }
            MachinePanelAction::SetRecipe(id) => {
                if let MachineState::Crafter(cr) = &mut *st {
                    let where_ = if mclone.kind == MachineKind::Refinery {
                        "refinery"
                    } else {
                        "assembler"
                    };
                    let Some(recipe) = data::RECIPES.iter().find(|recipe| {
                        recipe.id == id
                            && data::recipe_unlocked(&research.techs, recipe)
                            && (recipe.station == where_
                                || recipe.station == "both"
                                || (where_ == "assembler" && recipe.station == "hand"))
                    }) else {
                        p.toast("该配方尚未解锁或不适用于此机器");
                        continue;
                    };
                    // 切换配方：退还全部原料 + 进行中配方一组 + 旧产出（JS refund，溢出掉落机旁）
                    let drop_pos = Vec3::new(
                        mclone.pos[0] as f32 + 0.5,
                        mclone.pos[1] as f32 + 1.2,
                        mclone.pos[2] as f32 + 0.5,
                    );
                    let refund_one =
                        |p: &mut Player, commands: &mut Commands, item: String, n: i32| {
                            let added = p.inv.add_item(&item, n);
                            let left = n - added;
                            if left > 0 {
                                crate::creatures::spawn_drop(
                                    commands,
                                    &world,
                                    &icons,
                                    drop_pos,
                                    Vec3::ZERO,
                                    item,
                                    left,
                                    0.4,
                                );
                            }
                        };
                    for (k, v) in cr.input.drain() {
                        refund_one(&mut p, &mut commands, k, v);
                    }
                    if cr.prog > 0.0
                        && let Some(rid) = cr.recipe
                        && let Some(r) = data::RECIPES.iter().find(|r| r.id == rid)
                    {
                        for (i, n) in r.inputs {
                            refund_one(&mut p, &mut commands, i.to_string(), *n);
                        }
                    }
                    if let Some(o) = cr.output.take() {
                        refund_one(&mut p, &mut commands, o.item, o.n);
                    }
                    cr.recipe = Some(recipe.id);
                    cr.prog = 0.0;
                    audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                }
            }
            MachinePanelAction::InsertItem(i) => {
                if let Some(s) = p.inv.slots.get(i).cloned().flatten() {
                    if crate::factory::machine_insert(&mclone, &mut st, &s.item) {
                        let removed = p.inv.take_from_slot(i, 1);
                        debug_assert_eq!(removed.as_ref().map(|taken| taken.n), Some(1));
                        audio::play(&mut commands, sfx.click.clone(), 0.4, None);
                    } else {
                        p.toast("机器不接受该物品");
                        audio::play(&mut commands, sfx.error.clone(), 0.4, None);
                    }
                }
            }
            MachinePanelAction::BeaconLabel(label, gal) => {
                if let MachineState::Beacon(bc) = &mut *st {
                    bc.label = label;
                    bc.gal = gal;
                }
            }
            MachinePanelAction::SetFilter(filter) => {
                if let MachineState::Router(router) = &mut *st
                    && mclone.kind == MachineKind::Filter
                {
                    router.filter = filter;
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
    SetFilter(Option<String>),
    InsertItem(usize),
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
            for (i, s) in slots.iter().enumerate() {
                let s = s.clone();
                if slot_button(
                    ui,
                    cache,
                    s.as_ref().map(|s| s.item.as_str()).unwrap_or(""),
                    &s,
                    false,
                    42.0,
                ) {
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
    mut cloud_tuning: ResMut<crate::weather::CloudTuning>,
    world: Option<ResMut<World>>,
    player: Query<&Player>,
    research: Res<Research>,
    mut save_ev: MessageWriter<SaveEvent>,
    day: Res<crate::daynight::DayTime>,
) {
    if ui_state.panel != Panel::Pause {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
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
            ui.label(
                egui::RichText::new("STARFORGE · 星穹熔炉")
                    .size(32.0)
                    .strong(),
            );
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
                if ui
                    .add(egui::Slider::new(&mut settings.view_dist, 3..=16))
                    .changed()
                    && let Some(mut w) = world
                {
                    w.view_dist = settings.view_dist;
                }
            });
            let mut hierarchical = settings.lod_mode == crate::save::LodMode::Hierarchical;
            if ui
                .checkbox(&mut hierarchical, "层级体素远景（Voxy 模式）")
                .changed()
            {
                settings.lod_mode = if hierarchical {
                    crate::save::LodMode::Hierarchical
                } else {
                    crate::save::LodMode::Legacy
                };
                let _ = crate::save::save_settings(&settings);
            }
            ui.horizontal(|ui| {
                ui.label("鼠标灵敏度");
                ui.add(egui::Slider::new(&mut settings.mouse_sens, 0.3..=2.5));
            });
            ui.horizontal(|ui| {
                ui.label("音量");
                if ui
                    .add(egui::Slider::new(&mut settings.volume, 0.0..=1.0))
                    .changed()
                {
                    crate::audio::set_master_volume(settings.volume);
                    let _ = crate::save::save_settings(&settings);
                }
            });
            ui.checkbox(&mut settings.show_fps, "显示 FPS");
            if ui
                .checkbox(&mut settings.pixelated, "像素风渲染（重启生效）")
                .changed()
            {
                let _ = crate::save::save_settings(&settings);
            }
            let mut climate_changed = false;
            climate_changed |= ui.checkbox(&mut settings.clouds, "体积云层").changed();
            climate_changed |= ui.checkbox(&mut settings.weather, "生态天气粒子").changed();
            if climate_changed {
                let _ = crate::save::save_settings(&settings);
            }
            ui.separator();
            let mut cloud_changed = false;
            ui.collapsing("☁ 体积云实时参数", |ui| {
                ui.horizontal(|ui| {
                    let value = cloud_tuning.coverage;
                    ui.label("覆盖率");
                    cloud_changed |= ui
                        .add(
                            egui::Slider::new(&mut cloud_tuning.coverage, 0.0..=1.0)
                                .text(format!("{value:.2}")),
                        )
                        .changed();
                });
                ui.horizontal(|ui| {
                    let value = cloud_tuning.density;
                    ui.label("体积密度");
                    cloud_changed |= ui
                        .add(
                            egui::Slider::new(&mut cloud_tuning.density, 0.0..=1.0)
                                .text(format!("{value:.2}")),
                        )
                        .changed();
                });
                ui.horizontal(|ui| {
                    let value = cloud_tuning.raymarch_steps;
                    ui.label("主 Raymarch");
                    cloud_changed |= ui
                        .add(
                            egui::Slider::new(&mut cloud_tuning.raymarch_steps, 4..=64)
                                .text(format!("{value} 步")),
                        )
                        .changed();
                });
                // 球壳云随主 HDR 目标分辨率渲染，无独立低分辨率目标。
                ui.small("步数越高，体积雾 GPU 开销越大");
            });
            if cloud_changed {
                cloud_tuning.sanitize();
                cloud_tuning.save_to_settings(&mut settings);
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
            ui.label(format!(
                "已解锁科技：{} / {}",
                research.techs.len(),
                data::TECHS.len()
            ));
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
        save_ev.write(SaveEvent {
            quit_after: do_quit,
        });
    }
}

#[derive(Message)]
pub struct SaveEvent {
    /// Return to the menu only after both character and world files succeed.
    pub quit_after: bool,
}

#[derive(Message)]
pub struct QuitToMenuEvent;

/// Handle F5 quicksave（JS：任意时刻可存档）。
pub fn quicksave_system(keys: Res<ButtonInput<KeyCode>>, mut save_ev: MessageWriter<SaveEvent>) {
    if keys.just_pressed(KeyCode::F5) {
        save_ev.write(SaveEvent { quit_after: false });
    }
}

/// 失焦（Alt-Tab/切窗）清空按键与飞行输入，防卡键（JS window.blur 移植）。
pub fn clear_input_on_focus_lost(
    mut focus_ev: MessageReader<bevy::window::WindowFocused>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut space_input: ResMut<crate::space::SpaceInput>,
    mut player: Query<&mut Player>,
) {
    for ev in focus_ev.read() {
        if !ev.focused {
            keys.clear();
            mouse.clear();
            space_input.clear();
            for mut p in &mut player {
                p.mining = None;
            }
        }
    }
}

/// Panel toggle hotkeys (Tab/T/E/Esc) + E interaction.
#[allow(clippy::too_many_arguments)]
pub fn panel_hotkeys_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut quests: Option<ResMut<crate::quests::Quests>>,
    mut player: Query<&mut Player>,
    world: Res<World>,
    machines: Query<(Entity, &Machine)>,
    mode: Res<crate::space::FlightMode>,
    icons: Res<IconMaterials>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        // 对话优先关闭（JS: dialogActive → closeDialog，Esc 不绕过对话开暂停面板）
        if let Some(q) = quests.as_deref_mut() {
            if q.side_dialog.is_some() {
                q.side_dialog = None;
                return;
            }
            if q.dialog.is_some() {
                q.dialog = None;
                return;
            }
        }
        let was_locked = ui_state.locked();
        match ui_state.panel {
            Panel::None => {
                ui_state.panel = Panel::Pause;
                ui_state.selected_inv = None;
                audio::play(&mut commands, sfx.click.clone(), 0.5, None);
            }
            _ => {
                ui_state.close_panel();
                audio::play(&mut commands, sfx.click.clone(), 0.4, None);
            }
        }
        if was_locked {
            // 关闭面板时手持物品归还背包
            if let Ok(mut p) = player.single_mut() {
                drop_cursor(&mut ui_state, &mut p, &mut commands, &world, &icons, &sfx);
            }
        }
    }
    // F3：打开/关闭实时光照调节面板。
    if keys.just_pressed(KeyCode::F3) {
        if ui_state.panel == Panel::Lighting {
            ui_state.close_panel();
        } else if !ui_state.locked() {
            ui_state.panel = Panel::Lighting;
            ui_state.selected_inv = None;
        }
    }
    // Tab：开关背包/合成面板（JS: UI.toggle('invPanel')）
    if keys.just_pressed(KeyCode::Tab) {
        if ui_state.panel == Panel::Inventory {
            ui_state.close_panel();
            if let Ok(mut p) = player.single_mut() {
                drop_cursor(&mut ui_state, &mut p, &mut commands, &world, &icons, &sfx);
            }
            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
        } else if !ui_state.locked() {
            ui_state.panel = Panel::Inventory;
            ui_state.selected_inv = None;
            audio::play(&mut commands, sfx.click.clone(), 0.5, None);
        }
    }
    if keys.just_pressed(KeyCode::KeyT)
        && !ui_state.locked()
        && *mode == crate::space::FlightMode::Planet
    {
        ui_state.panel = Panel::Tech;
    }
    // O：Bevy 原生联机面板
    if keys.just_pressed(KeyCode::KeyO) && !ui_state.locked() {
        ui_state.panel = Panel::Network;
        ui_state.selected_inv = None;
        audio::play(&mut commands, sfx.click.clone(), 0.5, None);
    }
    // N：换船电脑（空间站停泊 / 太空中均可）
    if keys.just_pressed(KeyCode::KeyN)
        && !ui_state.locked()
        && matches!(
            *mode,
            crate::space::FlightMode::Station | crate::space::FlightMode::Space
        )
    {
        ui_state.panel = Panel::Garage;
        ui_state.selected_inv = None;
        audio::play(&mut commands, sfx.click.clone(), 0.5, None);
    }
    // P：创造物品库（JS: creative && (planet|space)）
    if keys.just_pressed(KeyCode::KeyP)
        && !ui_state.locked()
        && matches!(
            *mode,
            crate::space::FlightMode::Planet | crate::space::FlightMode::Space
        )
        && let Ok(p) = player.single()
        && p.creative()
    {
        ui_state.panel = Panel::Creative;
        ui_state.selected_inv = None;
        audio::play(&mut commands, sfx.click.clone(), 0.5, None);
    }
    // M：星球地图（planet/seated/atmo）
    if keys.just_pressed(KeyCode::KeyM)
        && !ui_state.locked()
        && matches!(
            *mode,
            crate::space::FlightMode::Planet
                | crate::space::FlightMode::Seated
                | crate::space::FlightMode::Atmo
        )
    {
        ui_state.panel = Panel::Map;
        ui_state.selected_inv = None;
        audio::play(&mut commands, sfx.click.clone(), 0.5, None);
    }
    if keys.just_pressed(KeyCode::KeyE)
        && !ui_state.locked()
        && *mode == crate::space::FlightMode::Planet
    {
        let Ok(mut p) = player.single_mut() else {
            return;
        };
        let origin = p.eye();
        let dir = p.look_dir();
        if p.stats.haz < 95.0 && p.inv.count_item("sodium") > 0 && p.charge("haz") {
            p.toast("防护已充能");
            audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
            return;
        }
        if p.stats.o2 < 95.0 && p.inv.count_item("oxygen") > 0 && p.charge("o2") {
            p.toast("氧气已充能");
            audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
            return;
        }
        if let Some((cell, _n, dist)) = world.raycast(origin, dir, 5.0)
            && dist <= 5.0
            && let Some((e, _)) = machines.iter().find(|(_, m)| m.pos == cell)
        {
            ui_state.panel = Panel::Machine(e);
            audio::play(&mut commands, sfx.click.clone(), 0.4, None);
            return;
        }
        p.toast("附近没有可交互的机器");
    }
}

/// 扫描冷却/标记（JS doScan：cd 6s、范围 24/48/80、标记 25s 过期）。
#[derive(Resource, Default)]
pub struct ScanState {
    pub cd: f32,
    pub marker_mat: Option<Handle<StandardMaterial>>,
    pub marker_mesh: Option<Handle<Mesh>>,
    /// 扫描环网格（Annulus，外半径 1.0，动画时整体缩放）。
    pub ring_mesh: Option<Handle<Mesh>>,
}

/// 扫描标记（发光小立方，靠近/被挖/过期即散）。
#[derive(Component)]
pub struct ScanMarker {
    pub cell: [i32; 3],
    pub block_id: u8,
    pub expire: f32,
}

/// 原生扫描环：地面上的扁平圆环，随 `t` 扩张并淡出（替代旧着色器扫描脉冲）。
#[derive(Component)]
pub struct ScanRing {
    pub t: f32,
    pub mat: Handle<StandardMaterial>,
}

pub fn scan_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ScanState>,
    time: Res<Time>,
    player: Query<&Player>,
    ui: Res<UiState>,
    world: Res<World>,
    research: Res<Research>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut stdmats: ResMut<Assets<StandardMaterial>>,
    mut big_ev: MessageWriter<crate::quests::BigMessageEvent>,
    mut markers: Query<(Entity, &mut ScanMarker, &Transform), Without<ScanRing>>,
    mut rings: Query<(Entity, &mut ScanRing, &mut Transform), Without<ScanMarker>>,
    sfx: Res<audio::Sfx>,
) {
    let dt = time.delta_secs();
    state.cd = (state.cd - dt).max(0.0);
    let Ok(p) = player.single() else { return };
    // 扫描环动画：扩张 + 淡出（JS 同口径：r = t*480，1.4s 生命周期）
    for (e, mut ring, mut tf) in &mut rings {
        ring.t += dt;
        let r = ring.t * 480.0;
        let a =
            (ring.t * 0.9).clamp(0.0, 0.9) * (1.0 - (ring.t - 0.9).max(0.0) / 0.5).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(r);
        if let Some(mut m) = stdmats.get_mut(&ring.mat) {
            m.base_color = Color::srgba(0.13, 0.86, 0.9, a);
        }
        if ring.t > 1.4 {
            commands.entity(e).despawn();
        }
    }
    // 标记过期 / 靠近 3.5m / 方块被挖 → 消散（JS doScan 标记生命周期）
    for (e, mut m, tf) in &mut markers {
        m.expire -= dt;
        let dist = tf.translation.distance(p.pos);
        let mined = world.get(m.cell[0], m.cell[1], m.cell[2]) != m.block_id;
        if m.expire <= 0.0 || dist < 3.5 || mined {
            commands.entity(e).despawn();
        }
    }
    if keys.just_pressed(KeyCode::KeyC) && !ui.locked() {
        if state.cd > 0.0 {
            return;
        }
        state.cd = 6.0;
        audio::play(&mut commands, sfx.pickup.clone(), 0.6, None);
        let range: i32 = if research.techs.iter().any(|t| t == "scan2") {
            80
        } else if research.techs.iter().any(|t| t == "scan1") {
            48
        } else {
            24
        };
        let pcx = p.pos.x.floor() as i32;
        let pcz = p.pos.z.floor() as i32;
        let py = p.pos.y.floor() as i32;
        let y_lo = (py - 24).max(1);
        let y_hi = (py + 16).min(crate::data::WORLD_H - 1);
        let r2 = range * range;
        let mut cands: Vec<([i32; 3], u8, f32)> = Vec::new();
        for chunk in world.chunks.values() {
            let bx = chunk.cx * crate::data::CHUNK;
            let bz = chunk.cz * crate::data::CHUNK;
            let cdx = bx - pcx;
            let cdz = bz - pcz;
            if cdx.abs() > range + 16 || cdz.abs() > range + 16 {
                continue;
            }
            for y in y_lo..=y_hi {
                for lz in 0..crate::data::CHUNK {
                    for lx in 0..crate::data::CHUNK {
                        let x = bx + lx;
                        let z = bz + lz;
                        let dx = x - pcx;
                        let dz = z - pcz;
                        if dx * dx + dz * dz > r2 {
                            continue;
                        }
                        let id = chunk.data[crate::world::lidx(lx, y, lz)];
                        if id == 0 {
                            continue;
                        }
                        let def = crate::data::block_by_id(id);
                        let target = def.ore
                            || matches!(
                                def.key,
                                "sodium_plant"
                                    | "oxygen_plant"
                                    | "glow_shroom"
                                    | "crystal"
                                    | "amber"
                            );
                        if !target {
                            continue;
                        }
                        let d2 = (dx * dx + dz * dz) as f32 + ((y - py) * (y - py)) as f32;
                        cands.push(([x, y, z], id, d2));
                    }
                }
            }
        }
        // 按距离升序 → 同类（同 block id）曼哈顿 <6 去重只留最近 → 上限 24
        cands.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        let mut seen: Vec<(u8, i32, i32)> = Vec::new();
        let mut out: Vec<([i32; 3], u8)> = Vec::new();
        for (cell, id, _) in cands {
            let dup = seen
                .iter()
                .any(|(k, sx, sz)| *k == id && (cell[0] - sx).abs() + (cell[2] - sz).abs() < 6);
            if dup {
                continue;
            }
            seen.push((id, cell[0], cell[2]));
            out.push((cell, id));
            if out.len() >= 24 {
                break;
            }
        }
        if state.marker_mesh.is_none() {
            state.marker_mesh = Some(meshes.add(Cuboid::new(0.24, 0.24, 0.24)));
        }
        if state.marker_mat.is_none() {
            state.marker_mat = Some(stdmats.add(StandardMaterial {
                base_color: Color::srgb(0.35, 0.9, 0.95),
                emissive: LinearRgba::new(0.2, 0.8, 0.9, 1.0) * 1.6,
                unlit: true,
                ..default()
            }));
        }
        let (Some(mesh), Some(mat)) = (state.marker_mesh.clone(), state.marker_mat.clone()) else {
            return;
        };
        for (cell, id) in &out {
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(Vec3::new(
                    cell[0] as f32 + 0.5,
                    cell[1] as f32 + 1.35,
                    cell[2] as f32 + 0.5,
                )),
                Visibility::default(),
                ScanMarker {
                    cell: *cell,
                    block_id: *id,
                    expire: 25.0,
                },
                crate::InGame,
            ));
        }
        // 原生扫描环：地面扁平圆环，随 t 扩张淡出（替代旧着色器扫描脉冲）
        if state.ring_mesh.is_none() {
            state.ring_mesh = Some(meshes.add(Annulus::new(0.97, 1.0).mesh().build()));
        }
        let gy = world.top_at(p.pos.x.floor() as i32, p.pos.z.floor() as i32) as f32 + 0.5;
        if let Some(ring_mesh) = state.ring_mesh.clone() {
            let ring_mat = stdmats.add(StandardMaterial {
                base_color: Color::srgba(0.13, 0.86, 0.9, 0.9),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            });
            commands.spawn((
                Mesh3d(ring_mesh),
                MeshMaterial3d(ring_mat.clone()),
                Transform::from_translation(Vec3::new(p.pos.x, gy, p.pos.z))
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
                ScanRing {
                    t: 0.0,
                    mat: ring_mat,
                },
                crate::InGame,
            ));
        }
        big_ev.write(crate::quests::BigMessageEvent {
            title: format!("扫描完成：发现 {} 处矿物信号", out.len()),
            sub: format!("范围 {}m", range),
            dur: 2.5,
        });
    }
}

// ---------- 空间站贸易终端 ----------

pub fn economy_system(time: Res<Time>, mut game: ResMut<crate::space::SpaceGame>) {
    game.economy_t += time.delta_secs();
    if game.economy_t < 180.0 {
        return;
    }
    game.economy_t -= 180.0;
    for item in data::TRADE_GOODS {
        let rare = matches!(
            *item,
            "warpcell" | "antimatter" | "advanced_circuit" | "ship_alloy" | "cobalt"
        );
        let restock = if rare { 1 } else { 4 };
        let stock = game.galaxy.stock.entry((*item).to_string()).or_default();
        *stock = (*stock + restock).min(if rare { 12 } else { 120 });
        let price = game.galaxy.market.entry((*item).to_string()).or_insert(1.0);
        *price += (1.0 - *price) * 0.08;
    }
}

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
    let Ok(ctx) = contexts.ctx_mut() else { return };
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
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    egui::Grid::new("trade_grid")
                        .num_columns(4)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            for item in data::TRADE_GOODS {
                                let buy = data::trade_buy_price(item, game.market(), has_trade_ai);
                                let sell = data::trade_sell_price(item, game.market());
                                let stock = game.galaxy.stock.get(*item).copied().unwrap_or(0);
                                let have =
                                    player.single().map(|p| p.inv.count_item(item)).unwrap_or(0);
                                ui.label(item_name(item));
                                ui.label(format!("买 ₪{buy} / 卖 ₪{sell}"));
                                ui.label(format!("库存 {stock} · 持有 {have}"));
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(stock > 0, egui::Button::new("买")).clicked()
                                    {
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
    let Ok(mut p) = player.single_mut() else {
        return;
    };
    let mut traded = false;
    for (item, price, n) in buy_req {
        let stock = game.galaxy.stock.get(&item).copied().unwrap_or(0);
        if stock >= n && p.credits >= price * n && p.inv.room_for(&item) >= n {
            p.credits -= price * n;
            let added = p.inv.add_item(&item, n);
            debug_assert_eq!(added, n);
            // 市场漂移（JS: mod = min(1.6, mod + 0.01*n)）
            let m = game.galaxy.market.entry(item.clone()).or_insert(1.0);
            *m = (*m + 0.01 * n as f32).min(1.6);
            *game.galaxy.stock.entry(item.clone()).or_default() -= n;
            traded = true;
            audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
        } else if stock < n {
            p.toast("空间站库存不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        } else if p.credits < price * n {
            p.toast("信用点不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        } else {
            p.toast("背包空间不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        }
    }
    for (item, price, n) in sell_req {
        if p.inv.count_item(&item) >= n {
            p.inv.remove_item(&item, n);
            p.credits = p.credits.saturating_add(price.saturating_mul(n));
            // 市场漂移（JS: mod = max(0.5, mod - 0.012*n)）
            let m = game.galaxy.market.entry(item.clone()).or_insert(1.0);
            *m = (*m - 0.012 * n as f32).max(0.5);
            let stock = game.galaxy.stock.entry(item.clone()).or_default();
            *stock = (*stock + n).min(100_000);
            traded = true;
            audio::play(&mut commands, sfx.click.clone(), 0.5, None);
        } else {
            p.toast("物品不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        }
    }
    if let Some(tech) = blueprint_req {
        if p.credits
            >= data::STATION_BLUEPRINTS
                .iter()
                .find(|b| b.tech == tech)
                .map(|b| b.price)
                .unwrap_or(i32::MAX)
        {
            p.credits -= data::STATION_BLUEPRINTS
                .iter()
                .find(|b| b.tech == tech)
                .map(|b| b.price)
                .unwrap_or(0);
            if !research.techs.iter().any(|t| t == tech) {
                research.techs.push(tech.to_string());
            }
            p.toast(format!(
                "蓝图已获取：{}",
                data::TECHS
                    .iter()
                    .find(|t| t.id == tech)
                    .map(|t| t.name)
                    .unwrap_or(tech)
            ));
            traded = true;
            audio::play(&mut commands, sfx.craft.clone(), 0.6, None);
        } else {
            p.toast("信用点不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        }
    }
    if traded {
        flag_ev.write(crate::quests::FlagEvent {
            flag: "traded".into(),
        });
    }
}

// ---------- 换船电脑 ----------

pub fn garage_panel_system(
    mut contexts: EguiContexts,
    cache: Res<EguiIcons>,
    mut ui_state: ResMut<UiState>,
    mut player: Query<&mut Player>,
    game: Res<crate::space::SpaceGame>,
    mut ship_asset: ResMut<crate::space::ShipAsset>,
    mut switch_ev: MessageWriter<crate::station::ShipSwitchEvent>,
) {
    if ui_state.panel != Panel::Garage {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut switch_req: Option<usize> = None;
    let mut cargo_take: Option<usize> = None;
    let mut cargo_put = false;
    let selected_cargo = player.single().ok().and_then(|p| {
        let index = p.hot_slot()?;
        Some((index, p.inv.slots.get(index)?.as_ref()?.clone()))
    });
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
            egui::Grid::new("cargo_grid")
                .num_columns(6)
                .spacing([4.0, 4.0])
                .show(ui, |ui| {
                    for i in 0..n {
                        let s = ship_asset.data.inv.get(i).cloned().flatten();
                        if slot_button(
                            ui,
                            &cache,
                            s.as_ref().map(|s| s.item.as_str()).unwrap_or(""),
                            &s,
                            false,
                            40.0,
                        ) {
                            cargo_take = Some(i);
                        }
                        if (i + 1) % 6 == 0 {
                            ui.end_row();
                        }
                    }
                });
            ui.label("（点击舱内物品取出到背包）");
            let put_label = selected_cargo
                .as_ref()
                .map(|(_, slot)| format!("存入当前快捷栏：{} ×{}", item_name(&slot.item), slot.n))
                .unwrap_or_else(|| "当前快捷栏没有可存入物品".to_string());
            if ui
                .add_enabled(selected_cargo.is_some(), egui::Button::new(put_label))
                .clicked()
            {
                cargo_put = true;
            }
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
        let Some(s) = game.garage.get(i).cloned() else {
            return;
        };
        switch_ev.write(crate::station::ShipSwitchEvent {
            cls: s.cls.clone(),
            model: s.model.clone(),
            garage_idx: Some(i),
        });
    }
    if let Some(i) = cargo_take {
        let Ok(mut p) = player.single_mut() else {
            return;
        };
        if let Some(slot) = ship_asset.data.inv.get_mut(i)
            && let Some(s) = slot.as_mut()
        {
            let take = s.n.min(p.inv.room_for(&s.item));
            if take > 0 {
                let added = p.inv.add_item(&s.item, take);
                s.n -= added;
                if s.n <= 0 {
                    *slot = None;
                }
            } else {
                p.toast("背包空间不足");
            }
        }
    }
    if cargo_put {
        let Ok(mut p) = player.single_mut() else {
            return;
        };
        if let Some((selected_index, selected)) = selected_cargo {
            let capacity = data::ship_class_by_key(&ship_asset.data.cls).slots;
            let mut cargo = crate::inventory::Inventory::from_slots_with_capacity(
                std::mem::take(&mut ship_asset.data.inv),
                capacity,
            );
            let added = cargo.add_item(&selected.item, selected.n);
            ship_asset.data.inv = cargo.slots;
            if added > 0 {
                // The action is explicitly tied to the selected hotbar slot.
                // Removing by item key could consume a different stack and
                // leave the clicked stack untouched.
                let removed = p.inv.take_from_slot(selected_index, added);
                debug_assert_eq!(removed.as_ref().map(|slot| slot.n), Some(added));
            }
            if added < selected.n {
                p.toast("飞船货仓空间不足");
            }
        }
    }
}

// ---------- 空间站停泊服务 ----------

/// 停泊后按 E 打开的空间站服务菜单：贸易 / 买船 / 换船。
pub fn station_services_panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mode: Res<crate::space::FlightMode>,
    station: Option<Res<crate::station::StationState>>,
) {
    if ui_state.panel != Panel::Station {
        return;
    }
    // 离开站态/未停泊时自动关闭
    let docked = *mode == crate::space::FlightMode::Station
        && station
            .as_ref()
            .is_some_and(|st| st.phase == crate::station::StationPhase::Parked);
    if !docked {
        ui_state.panel = Panel::None;
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    egui::Window::new("◈ 空间站服务")
        .default_size([320.0, 230.0])
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("已停泊于空间站顶部 · 选择服务")
                    .size(13.0)
                    .color(egui::Color32::from_rgb(0x9a, 0xa6, 0xb2)),
            );
            ui.separator();
            if ui
                .button(egui::RichText::new("💰 银河交易终端").size(16.0))
                .clicked()
            {
                ui_state.panel = Panel::Trade;
            }
            if ui
                .button(egui::RichText::new("🚀 买船中心").size(16.0))
                .clicked()
            {
                ui_state.panel = Panel::BuyShip;
            }
            if ui
                .button(egui::RichText::new("🔁 换船电脑").size(16.0))
                .clicked()
            {
                ui_state.panel = Panel::Garage;
            }
            ui.separator();
            ui.label(egui::RichText::new("Esc 关闭 · W 离站").size(12.0));
        });
}

/// 买船中心：游商不再对话卖船，统一在本中心出售。
pub fn buy_ship_panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut player: Query<&mut Player>,
    mut switch_ev: MessageWriter<crate::station::ShipSwitchEvent>,
    mut big_ev: MessageWriter<crate::quests::BigMessageEvent>,
    station: Option<Res<crate::station::StationState>>,
    mut commands: Commands,
    sfx: Res<crate::audio::Sfx>,
) {
    if ui_state.panel != Panel::BuyShip {
        return;
    }
    let Some(st) = station.as_deref() else {
        ui_state.panel = Panel::None;
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut buy_req: Option<usize> = None;
    let credits = player.single().map(|p| p.credits).unwrap_or(0);
    egui::Window::new("◈ 舰船交易中心")
        .default_size([460.0, 320.0])
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!("₪ {credits}"))
                    .size(20.0)
                    .strong()
                    .color(egui::Color32::from_rgb(0xff, 0xd1, 0x66)),
            );
            ui.separator();
            for (i, o) in st.offers.iter().enumerate() {
                let cls = crate::data::ship_class_by_key(&o.cls);
                let model_name = crate::data::SHIP_MODEL_NAMES
                    .iter()
                    .find(|(k, _)| *k == o.model)
                    .map(|(_, n)| *n)
                    .unwrap_or("飞船");
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "「{model_name}」 {} 级 · {} · 货仓 {} 格",
                        cls.key, cls.weapon_name, cls.slots
                    ));
                    if ui.button(format!("购买 ₪{}", o.price)).clicked() {
                        buy_req = Some(i);
                    }
                });
                ui.end_row();
            }
        });
    if let Some(i) = buy_req {
        let Some(offer) = st.offers.get(i).cloned() else {
            return;
        };
        let Ok(mut p) = player.single_mut() else {
            return;
        };
        if p.credits >= offer.price {
            p.credits -= offer.price;
            switch_ev.write(crate::station::ShipSwitchEvent {
                cls: offer.cls.clone(),
                model: offer.model.clone(),
                garage_idx: None,
            });
            big_ev.write(crate::quests::BigMessageEvent {
                title: "成交！".into(),
                sub: format!("已购入 {} 级飞船", offer.cls),
                dur: 2.4,
            });
            audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
        } else {
            p.toast("信用点不足");
            audio::play(&mut commands, sfx.error.clone(), 0.4, None);
        }
    }
}

// ---------- 创造物品库（P） ----------

pub fn creative_panel_system(
    mut contexts: EguiContexts,
    cache: Res<EguiIcons>,
    mut player: Query<&mut Player>,
    mut ui_state: ResMut<UiState>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    if ui_state.panel != Panel::Creative {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut give: Vec<(String, i32)> = Vec::new();
    egui::Window::new("✦ 创造物品库")
        .default_size([520.0, 560.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut !close)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("左键 +64 · 右键 +1")
                        .size(12.0)
                        .color(egui::Color32::from_rgb(0x7f, 0x9d, 0xb0)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕ 关闭 (P)").clicked() {
                        close = true;
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(480.0)
                .show(ui, |ui| {
                    egui::Grid::new("creative_grid")
                        .num_columns(8)
                        .spacing([4.0, 4.0])
                        .show(ui, |ui| {
                            for (i, item) in data::ITEMS.iter().enumerate() {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(48.0, 48.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(3),
                                    egui::Color32::from_rgba_unmultiplied(0x10, 0x14, 0x1a, 0xCC),
                                );
                                ui.painter().rect_stroke(
                                    rect,
                                    egui::CornerRadius::same(3),
                                    egui::Stroke::new(
                                        1.0,
                                        egui::Color32::from_rgb(0x35, 0x46, 0x55),
                                    ),
                                    egui::StrokeKind::Inside,
                                );
                                let tex = egui_icon(&cache, item.key);
                                ui.painter().image(
                                    tex,
                                    egui::Rect::from_min_max(
                                        rect.min + egui::vec2(4.0, 4.0),
                                        rect.max - egui::vec2(4.0, 4.0),
                                    ),
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    egui::Color32::WHITE,
                                );
                                resp.clone()
                                    .on_hover_text(format!("{}\n{}", item.name, item.desc));
                                if resp.clicked() {
                                    give.push((item.key.to_string(), 64));
                                } else if resp.secondary_clicked() {
                                    give.push((item.key.to_string(), 1));
                                }
                                if (i + 1) % 8 == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                });
        });
    if close {
        ui_state.close_panel();
    }
    if !give.is_empty()
        && let Ok(mut p) = player.single_mut()
    {
        for (item, n) in give {
            p.inv.add_item(&item, n);
        }
        audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
    }
}

// ---------- 星球全息地图（M） ----------

/// 地图面板状态（名字输入、范围、待添加点、选中项）。
#[derive(Resource, Default)]
pub struct MapState {
    pub name: String,
    pub gal: bool,
    pub pending: Option<(i32, i32)>,
    pub sel: Option<usize>,
    /// 星系地图当前悬停/点击选择。必须跨帧保存，否则点击星点后下一帧
    /// `galaxy_map_system` 会用旧的跃迁锁定值覆盖它，导致锁定按钮失效。
    pub galaxy_sel: Option<u32>,
}

/// 地图可见区域（世界生成范围，JS genStructures x∈[-650,650] z∈[-220,220]）。
const MAP_X0: i32 = -650;
const MAP_X1: i32 = 650;
const MAP_Z0: i32 = -220;
const MAP_Z1: i32 = 220;

pub fn planet_map_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut game: ResMut<crate::space::SpaceGame>,
    mut map: ResMut<MapState>,
    player: Query<&Player>,
    world: Res<World>,
    machines: Query<(&crate::factory::Machine, &crate::factory::MachineState)>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    if ui_state.panel != Panel::Map {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let Ok(p) = player.single() else { return };
    let mut close = false;
    let mut add_req = false;
    let mut del_req: Option<usize> = None;
    let mut toggle_req: Option<usize> = None;
    let (cw, ch) = (560.0f32, 340.0f32);
    let sx = cw / (MAP_X1 - MAP_X0) as f32;
    let sz = ch / (MAP_Z1 - MAP_Z0) as f32;
    let to_canvas = |wx: f32, wz: f32| -> egui::Pos2 {
        egui::pos2((wx - MAP_X0 as f32) * sx, ch - (wz - MAP_Z0 as f32) * sz)
    };
    let to_world = |px: f32, py: f32| -> (i32, i32) {
        (
            ((px / sx) as i32 + MAP_X0).clamp(MAP_X0, MAP_X1),
            (((ch - py) / sz) as i32 + MAP_Z0).clamp(MAP_Z0, MAP_Z1),
        )
    };
    egui::Window::new("◈ 星球全息地图")
        .default_size([600.0, 520.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut !close)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "X {:.0} · Z {:.0} · 标记 {} 个",
                        p.pos.x,
                        p.pos.z,
                        game.marks.len()
                    ))
                    .size(12.0)
                    .color(egui::Color32::from_rgb(0x7f, 0x9d, 0xb0)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕ 关闭 (M)").clicked() {
                        close = true;
                    }
                });
            });
            let (resp, painter) =
                ui.allocate_painter(egui::vec2(cw, ch), egui::Sense::click_and_drag());
            let rect = resp.rect;
            // 底图
            painter.rect_filled(
                rect,
                egui::CornerRadius::same(4),
                egui::Color32::from_rgb(0x07, 0x10, 0x1a),
            );
            // 网格
            for gx in (MAP_X0..=MAP_X1).step_by(130) {
                let a = to_canvas(gx as f32, MAP_Z0 as f32);
                let b = to_canvas(gx as f32, MAP_Z1 as f32);
                painter.line_segment(
                    [
                        rect.min + egui::vec2(a.x, a.y),
                        rect.min + egui::vec2(b.x, b.y),
                    ],
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(0x1d, 0x3a, 0x52, 0x80),
                    ),
                );
            }
            for gz in (MAP_Z0..=MAP_Z1).step_by(110) {
                let a = to_canvas(MAP_X0 as f32, gz as f32);
                let b = to_canvas(MAP_X1 as f32, gz as f32);
                painter.line_segment(
                    [
                        rect.min + egui::vec2(a.x, a.y),
                        rect.min + egui::vec2(b.x, b.y),
                    ],
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(0x1d, 0x3a, 0x52, 0x80),
                    ),
                );
            }
            let pin = |painter: &egui::Painter,
                       pos: egui::Pos2,
                       color: egui::Color32,
                       label: Option<&str>| {
                let r = egui::Rect::from_center_size(pos, egui::vec2(7.0, 7.0));
                painter.circle_filled(pos, 4.0, color);
                if let Some(l) = label {
                    painter.text(
                        pos + egui::vec2(6.0, -6.0),
                        egui::Align2::LEFT_TOP,
                        l,
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_rgb(0xc9, 0xe6, 0xee),
                    );
                }
                let _ = r;
            };
            // 村庄 / 遗迹（无限地表：只画地图窗口内的结构）
            for s in world
                .g
                .structures_in_rect(MAP_X0 - 24, MAP_Z0 - 24, MAP_X1 + 24, MAP_Z1 + 24)
            {
                match s {
                    crate::world::Structure::Village { x, z, .. } => {
                        pin(
                            &painter,
                            rect.min + to_canvas(x as f32, z as f32).to_vec2(),
                            egui::Color32::from_rgb(0x4d, 0xc8, 0x6a),
                            None,
                        );
                    }
                    crate::world::Structure::Ruin { x, z, .. } => {
                        pin(
                            &painter,
                            rect.min + to_canvas(x as f32, z as f32).to_vec2(),
                            egui::Color32::from_rgb(0xd8, 0xb0, 0x38),
                            None,
                        );
                    }
                }
            }
            // 信标方块（金色菱形）
            for (m, bs) in &machines {
                if let crate::factory::MachineState::Beacon(bc) = bs {
                    pin(
                        &painter,
                        rect.min
                            + to_canvas(m.pos[0] as f32 + 0.5, m.pos[2] as f32 + 0.5).to_vec2(),
                        egui::Color32::from_rgb(0xff, 0xa0, 0x30),
                        Some(&bc.label),
                    );
                }
            }
            // 飞船
            pin(
                &painter,
                rect.min + to_canvas(game.ship_pos.x, game.ship_pos.z).to_vec2(),
                egui::Color32::from_rgb(0x35, 0xe0, 0xe8),
                Some("🚀"),
            );
            // 玩家箭头（按朝向）
            {
                let pp = rect.min + to_canvas(p.pos.x, p.pos.z).to_vec2();
                let ang = -p.yaw;
                let dir = egui::vec2(ang.sin(), -ang.cos()) * 10.0;
                let mut pts = vec![
                    pp + dir,
                    pp + egui::vec2(-dir.y, dir.x) * 0.6,
                    pp + egui::vec2(dir.y, -dir.x) * 0.6,
                ];
                pts.push(pp + dir);
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(0x7d, 0xff, 0x8a)),
                ));
            }
            // 用户标记
            for (i, m) in game.marks.iter().enumerate() {
                let c = if m.gal {
                    egui::Color32::from_rgb(0xc0, 0x7d, 0xff)
                } else {
                    egui::Color32::from_rgb(0xff, 0xd1, 0x66)
                };
                let pp = rect.min + to_canvas(m.x as f32, m.z as f32).to_vec2();
                pin(&painter, pp, c, Some(&m.label));
                if map.sel == Some(i) {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(pp, egui::vec2(14.0, 14.0)),
                        egui::CornerRadius::same(7),
                        egui::Stroke::new(1.5, egui::Color32::WHITE),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            // 待添加点
            if let Some((mx, mz)) = map.pending {
                pin(
                    &painter,
                    rect.min + to_canvas(mx as f32, mz as f32).to_vec2(),
                    egui::Color32::from_rgb(0xff, 0x44, 0x44),
                    None,
                );
            }
            // 点击拾取
            if resp.clicked()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let (wx, wz) = to_world(pos.x - rect.min.x, pos.y - rect.min.y);
                map.pending = Some((wx, wz));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("标记名");
                ui.add(egui::TextEdit::singleline(&mut map.name).desired_width(160.0));
                ui.checkbox(&mut map.gal, "✦ 全星系显示");
                let can_add = map.pending.is_some() && !map.name.trim().is_empty();
                if ui
                    .add_enabled(can_add, egui::Button::new("⚑ 添加标记"))
                    .clicked()
                {
                    add_req = true;
                }
                if let Some((mx, mz)) = map.pending {
                    ui.label(
                        egui::RichText::new(format!("坐标 ({mx}, {mz})"))
                            .size(12.0)
                            .color(egui::Color32::from_rgb(0xff, 0xb3, 0x47)),
                    );
                }
            });
            ui.separator();
            ui.label(
                egui::RichText::new("标记列表（⚑ 切换全星系 · 🗑 删除）")
                    .size(12.0)
                    .color(egui::Color32::from_rgb(0x7f, 0x9d, 0xb0)),
            );
            if game.marks.is_empty() {
                ui.label("（暂无标记）");
            }
            for (i, m) in game.marks.iter().enumerate() {
                ui.horizontal(|ui| {
                    let sel = map.sel == Some(i);
                    if ui
                        .selectable_label(sel, format!("{} ({}, {})", m.label, m.x, m.z))
                        .clicked()
                    {
                        map.sel = if sel { None } else { Some(i) };
                        map.pending = None;
                    }
                    if ui
                        .button(if m.gal {
                            "✦ 全星系"
                        } else {
                            "⚑ 本星球"
                        })
                        .clicked()
                    {
                        toggle_req = Some(i);
                    }
                    if ui.button("🗑").clicked() {
                        del_req = Some(i);
                    }
                });
            }
        });
    if close {
        ui_state.close_panel();
        map.pending = None;
        map.sel = None;
    }
    if add_req && let Some((mx, mz)) = map.pending.take() {
        game.marks.push(crate::space::Mark {
            x: mx,
            z: mz,
            y: world.top_at(mx, mz) + 1,
            label: map.name.trim().to_string(),
            gal: map.gal,
        });
        map.name.clear();
        audio::play(&mut commands, sfx.click.clone(), 0.5, None);
    }
    if let Some(i) = del_req
        && i < game.marks.len()
    {
        game.marks.remove(i);
        map.sel = None;
        audio::play(&mut commands, sfx.click.clone(), 0.4, None);
    }
    if let Some(i) = toggle_req
        && let Some(m) = game.marks.get_mut(i)
    {
        m.gal = !m.gal;
        audio::play(&mut commands, sfx.click.clone(), 0.4, None);
    }
}

pub fn galaxy_map_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut game: ResMut<crate::space::SpaceGame>,
    mut map: ResMut<MapState>,
    mode: Res<crate::space::FlightMode>,
    time: Res<Time>,
) {
    if ui_state.panel != Panel::GalaxyMap {
        return;
    }
    if *mode != crate::space::FlightMode::Space {
        ui_state.panel = Panel::None;
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !egui_fonts_ready(ctx) {
        return;
    }
    let mut close = false;
    let mut lock_req: Option<u32> = None;
    // 选择状态不能是局部变量：egui 每帧重绘，按钮点击通常发生在选中星点的
    // 下一帧。优先恢复已有锁定目标，否则保留用户刚选中的星点。
    if map.galaxy_sel.is_none() {
        map.galaxy_sel = game.warp_lock.as_ref().map(|lock| lock.seed);
    }
    egui::Window::new("◈ 星系地图")
        .default_size([600.0, 540.0])
        .resizable(false)
        .show(ctx, |ui| {
            if ui.button("✕ 关闭 (M / Esc)").clicked() {
                close = true;
            }
            ui.label(
                egui::RichText::new(format!(
                    "当前星系：{}（种子 {}）",
                    game.galaxy.name, game.galaxy.seed
                ))
                .size(16.0)
                .strong()
                .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
            );
            ui.label(
                egui::RichText::new(format!(
                    "星球 {} 颗 · 已访问星系 {}",
                    game.galaxy.planets.len(),
                    game.galaxy_count
                ))
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
            let (response, painter) =
                ui.allocate_painter(egui::vec2(565.0, 340.0), egui::Sense::click_and_drag());
            let rect = response.rect;
            painter.rect_filled(rect, 5.0, egui::Color32::from_rgb(3, 8, 20));
            let center = rect.center();
            for radius in [55.0, 110.0, 160.0] {
                painter.circle_stroke(
                    center,
                    radius,
                    egui::Stroke::new(0.7, egui::Color32::from_rgba_unmultiplied(53, 224, 232, 35)),
                );
            }
            painter.line_segment(
                [
                    egui::pos2(rect.left(), center.y),
                    egui::pos2(rect.right(), center.y),
                ],
                egui::Stroke::new(0.6, egui::Color32::from_rgba_unmultiplied(53, 224, 232, 28)),
            );
            painter.line_segment(
                [
                    egui::pos2(center.x, rect.top()),
                    egui::pos2(center.x, rect.bottom()),
                ],
                egui::Stroke::new(0.6, egui::Color32::from_rgba_unmultiplied(53, 224, 232, 28)),
            );

            let rot =
                Quat::from_rotation_y(time.elapsed_secs() * 0.075) * Quat::from_rotation_x(-0.38);
            let mut stars: Vec<(u32, egui::Pos2, f32, bool)> =
                crate::space::neighbor_seeds(game.galaxy.seed)
                    .into_iter()
                    .map(|seed| {
                        let h = seed.wrapping_mul(0x9E37_79B9);
                        let radius = 0.42 + (h & 0xff) as f32 / 255.0 * 0.56;
                        let p = rot * (crate::space::galaxy_dir(seed) * radius);
                        let perspective = 1.0 / (1.35 - p.z * 0.42);
                        let screen = center + egui::vec2(p.x, -p.y) * (192.0 * perspective);
                        (seed, screen, p.z, game.archives.contains_key(&seed))
                    })
                    .collect();
            stars.sort_by(|a, b| a.2.total_cmp(&b.2));
            let pointer = response.hover_pos();
            let hovered = pointer.and_then(|pos| {
                stars
                    .iter()
                    .filter_map(|star| {
                        let d = star.1.distance(pos);
                        (d < 11.0).then_some((star.0, d))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|entry| entry.0)
            });
            for (seed, pos, depth, visited) in &stars {
                let is_selected = map.galaxy_sel == Some(*seed);
                let is_hovered = hovered == Some(*seed);
                painter.line_segment(
                    [center, *pos],
                    egui::Stroke::new(
                        if is_selected { 1.2 } else { 0.35 },
                        if is_selected {
                            egui::Color32::from_rgba_unmultiplied(125, 255, 138, 125)
                        } else {
                            egui::Color32::from_rgba_unmultiplied(53, 224, 232, 18)
                        },
                    ),
                );
                let size = (2.2 + (*depth + 1.0) * 1.8)
                    + if is_selected || is_hovered { 2.2 } else { 0.0 };
                let color = if is_selected {
                    egui::Color32::from_rgb(125, 255, 138)
                } else if *visited {
                    egui::Color32::from_rgb(255, 209, 102)
                } else {
                    egui::Color32::from_rgb(103, 207, 255)
                };
                painter.circle_filled(*pos, size, color);
                if is_selected || is_hovered {
                    painter.text(
                        *pos + egui::vec2(8.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        data::galaxy_name(*seed),
                        egui::FontId::proportional(12.0),
                        color,
                    );
                }
            }
            painter.circle_filled(center, 7.0, egui::Color32::WHITE);
            painter.circle_stroke(
                center,
                12.0,
                egui::Stroke::new(1.2, egui::Color32::from_rgb(53, 224, 232)),
            );
            painter.text(
                center + egui::vec2(14.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "当前位置",
                egui::FontId::proportional(12.0),
                egui::Color32::WHITE,
            );
            if response.clicked()
                && let Some(seed) = hovered
            {
                map.galaxy_sel = Some(seed);
            }
            ui.small("星域会缓慢自转；金色为已到访星系，绿色为当前锁定目标。");
            if let Some(seed) = map.galaxy_sel {
                ui.horizontal(|ui| {
                    ui.label(format!("目标：{} · 种子 {}", data::galaxy_name(seed), seed));
                    if ui.button("◎ 锁定跃迁目标").clicked() {
                        lock_req = Some(seed);
                    }
                });
            }
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
        map.galaxy_sel = Some(seed);
    }
}

// ---------- Plugin ----------

/// egui 0.35 requires one contiguous begin_pass → draw → end_pass lifecycle per
/// frame (as egui's own `run_ui` does). bevy_egui 0.41.1's split calls
/// (begin_pass in PreUpdate, end_pass in PostUpdate) break egui 0.35's hit-test
/// data chain (`prev_pass.widgets` stays empty, so no widget is ever hovered or
/// clicked). Fix: switch the context to manual mode and run the whole pass
/// inside Update, with all UI systems chained between the two.
fn egui_manual_pass(mut q: Query<&mut bevy_egui::EguiContextSettings>) {
    for mut s in &mut q {
        if !s.run_manually {
            s.run_manually = true;
        }
    }
}

fn egui_begin_pass(mut contexts: EguiContexts, mut input: Query<&mut bevy_egui::EguiInput>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if let Ok(mut inp) = input.single_mut() {
        let raw = std::mem::take(&mut inp.0);
        ctx.begin_pass(raw);
    }
}

fn egui_end_pass(mut contexts: EguiContexts, mut full: Query<&mut bevy_egui::EguiFullOutput>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let out = ctx.end_pass();
    if let Ok(mut f) = full.single_mut() {
        f.0 = Some(out);
    }
}

/// Startup: build item icon meshes/materials plus the textured egui icon registry.
fn build_icon_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut stdmats: ResMut<Assets<StandardMaterial>>,
) {
    let (icon_mats, mut icon_imgs) = build_icons(&mut meshes, &mut images, &mut stdmats);
    // white fallback icon (1×1) — egui texture registration happens lazily in setup_egui
    let white = images.add(Image::new(
        bevy::render::render_resource::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        vec![255u8, 255, 255, 255],
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    ));
    icon_imgs.map.insert("fallback".to_string(), white);
    commands.insert_resource(icon_mats);
    commands.insert_resource(icon_imgs);
}

/// egui/UI plugin: panel state machine, HUD, icons and the manual egui pass loop.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveEvent>()
            .add_message::<QuitToMenuEvent>()
            .init_resource::<UiState>()
            .init_resource::<Research>()
            .init_resource::<EguiIcons>()
            .init_resource::<ScanState>()
            .init_resource::<MapState>()
            .add_systems(Startup, build_icon_assets)
            .add_systems(
                PreUpdate,
                egui_manual_pass
                    .after(bevy_egui::EguiPreUpdateSet::InitContexts)
                    .before(bevy_egui::EguiPreUpdateSet::BeginPass),
            )
            .add_systems(PostUpdate, setup_egui)
            .add_systems(Update, egui_begin_pass.in_set(GameSet::UiBeginPass))
            .add_systems(Update, egui_end_pass.in_set(GameSet::UiEndPass))
            // playing 通用：面板热键 → 存档触发 → 失焦清键 → 大字消息 → 科技进度
            .add_systems(
                Update,
                (
                    (
                        panel_hotkeys_system,
                        quicksave_system,
                        clear_input_on_focus_lost,
                        big_message_system,
                        economy_system,
                    )
                        .chain()
                        .in_set(GameSet::CommonUi),
                    research_system.in_set(GameSet::CommonResearch),
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                scan_system
                    .in_set(GameSet::LateScan)
                    .run_if(in_state(GameState::Playing))
                    .run_if(ground_mode),
            )
            .add_systems(
                Update,
                (
                    ghost_system
                        .in_set(GameSet::HudGhostUi)
                        .run_if(in_planet_mode),
                    (hud_system, ship_label_system)
                        .chain()
                        .in_set(GameSet::HudMain),
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                (
                    lighting_panel_system.in_set(GameSet::PanelLighting),
                    inventory_panel_system.in_set(GameSet::PanelInventory),
                    tech_panel_system.in_set(GameSet::PanelTech),
                    machine_panel_system.in_set(GameSet::PanelMachine),
                    pause_panel_system.in_set(GameSet::PanelPause),
                    trade_panel_system.in_set(GameSet::PanelTrade),
                    garage_panel_system.in_set(GameSet::PanelGarage),
                    station_services_panel_system.in_set(GameSet::PanelStationServices),
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                (
                    (buy_ship_panel_system, galaxy_map_system)
                        .chain()
                        .in_set(GameSet::SaveBuy),
                    creative_panel_system.in_set(GameSet::SaveCreative),
                    planet_map_system.in_set(GameSet::SaveMap),
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
