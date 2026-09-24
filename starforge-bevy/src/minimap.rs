//! Corner minimap: a cached top-down terrain snapshot with live entity markers.
//!
//! Terrain is sampled from the generative height field plus the loaded chunk
//! voxels, refreshed four times a second into one egui texture. Markers are
//! drawn every frame on top. `N` toggles the map on the ground (in space the
//! same key opens the ship computer, so the two never conflict).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::player::Player;
use crate::schedule::GameState;
use crate::ui::UiState;

pub const MAP_PIXELS: usize = 132;
const REFRESH_SECONDS: f32 = 0.35;

#[derive(Resource)]
pub struct MinimapState {
    pub visible: bool,
    pub texture: Option<egui::TextureHandle>,
    pub refresh_timer: f32,
    pub zoom: f32,
    pub center: Vec2,
}

impl Default for MinimapState {
    fn default() -> Self {
        Self {
            visible: true,
            texture: None,
            refresh_timer: 0.0,
            zoom: 3.0,
            center: Vec2::ZERO,
        }
    }
}

/// Terrain colour for a surface block. Tuned to read at a glance rather than
/// to be literal.
pub fn surface_color(key: &str, height: i32) -> (u8, u8, u8) {
    let base = match key {
        "grass" => (94, 158, 72),
        "red_moss" => (156, 82, 74),
        "dirt" => (122, 90, 60),
        "stone" => (120, 122, 126),
        "sand" | "sandstone" => (206, 186, 130),
        "salt" => (222, 222, 226),
        "snow" => (238, 242, 248),
        "ice" => (188, 220, 240),
        "water" => (58, 106, 190),
        "lava" => (232, 108, 40),
        "log" | "planks" => (128, 92, 54),
        "leaves" => (58, 122, 58),
        "mush_stem" => (150, 118, 96),
        "mush_cap" => (150, 78, 122),
        "basalt" | "obsidian" => (54, 50, 58),
        "ash" => (92, 88, 86),
        "alien" => (118, 84, 148),
        "crystal" | "glow_shroom" => (110, 190, 200),
        "amber" => (196, 142, 58),
        "rust" | "ferrous" => (140, 92, 74),
        "hive" => (190, 156, 70),
        "murk" => (74, 104, 84),
        "metal" | "machine" => (150, 158, 168),
        "concrete" | "white_panel" => (176, 178, 180),
        "planks_b" => (130, 104, 66),
        _ => (104, 108, 112),
    };
    // Height shading: peaks lighter, valleys darker.
    let shade = (((height - 40) as f32) * 0.006).clamp(-0.22, 0.22);
    let adjust = |channel: u8| ((channel as f32 * (1.0 + shade)) as i32).clamp(0, 255) as u8;
    (adjust(base.0), adjust(base.1), adjust(base.2))
}

/// Samples the terrain into a fresh texture.
pub fn sample_minimap(
    world: &crate::world::World,
    center: Vec2,
    zoom: f32,
    size: usize,
) -> egui::ColorImage {
    let mut pixels = Vec::with_capacity(size * size);
    for py in 0..size {
        for px in 0..size {
            let wx = center.x + (px as f32 - size as f32 * 0.5) * zoom;
            let wz = center.y + (py as f32 - size as f32 * 0.5) * zoom;
            let x = wx.floor() as i32;
            let z = wz.floor() as i32;
            let (height, key) = if world.chunks.contains_key(&crate::world::ckey(
                x.div_euclid(crate::data::CHUNK),
                z.div_euclid(crate::data::CHUNK),
            )) {
                let h = world.top_at(x, z).clamp(1, crate::data::WORLD_H - 1);
                let id = world.get(x, h, z);
                if id == crate::data::ids::AIR {
                    (world.g.height_at(x as f32, z as f32), world.biome().deep)
                } else {
                    (h, crate::data::block_by_id(id).key)
                }
            } else {
                (world.g.height_at(x as f32, z as f32), world.biome().grass)
            };
            let (r, g, b) = surface_color(key, height);
            pixels.push(egui::Color32::from_rgb(r, g, b));
        }
    }
    egui::ColorImage {
        size: [size, size],
        pixels,
        source_size: egui::vec2(size as f32, size as f32),
    }
}

/// N toggles the minimap on the ground.
pub fn minimap_hotkey_system(keys: Res<ButtonInput<KeyCode>>, mut minimap: ResMut<MinimapState>) {
    if keys.just_pressed(KeyCode::KeyN) {
        minimap.visible = !minimap.visible;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn minimap_system(
    mut contexts: EguiContexts,
    time: Res<Time>,
    ui_state: Res<UiState>,
    mode: Res<crate::space::FlightMode>,
    mut minimap: ResMut<MinimapState>,
    world: Option<Res<crate::world::World>>,
    player: Query<&Player>,
    creatures: Query<(&crate::creatures::Creature, &Transform)>,
    machines: Query<&crate::factory::Machine>,
    game: Option<Res<crate::space::SpaceGame>>,
) {
    if ui_state.locked()
        || !minimap.visible
        || !matches!(
            *mode,
            crate::space::FlightMode::Planet | crate::space::FlightMode::Seated
        )
    {
        return;
    }
    let Some(world) = world else { return };
    let Ok(player) = player.single() else { return };
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    // Refresh the cached terrain texture on a timer / when the player moved
    // far enough for the old snapshot to be useless.
    let center = Vec2::new(player.pos.x, player.pos.z);
    minimap.refresh_timer -= time.delta_secs();
    let drifted = minimap.center.distance(center) > minimap.zoom * 6.0;
    if minimap.texture.is_none() || minimap.refresh_timer <= 0.0 || drifted {
        minimap.refresh_timer = REFRESH_SECONDS;
        minimap.center = center;
        let image = sample_minimap(&world, center, minimap.zoom, MAP_PIXELS);
        match &mut minimap.texture {
            Some(handle) => handle.set(image, egui::TextureOptions::NEAREST),
            None => {
                minimap.texture =
                    Some(ctx.load_texture("minimap", image, egui::TextureOptions::NEAREST));
            }
        }
    }
    let Some(texture) = &minimap.texture else {
        return;
    };
    let viewport = ctx.viewport_rect();
    let map_size = 176.0;
    let origin = egui::pos2(viewport.left() + 16.0, viewport.bottom() - map_size - 16.0);
    let rect = egui::Rect::from_min_size(origin, egui::vec2(map_size, map_size));
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("minimap"),
    ));
    painter.rect_filled(
        rect.expand(3.0),
        egui::CornerRadius::same(8),
        egui::Color32::from_rgba_unmultiplied(8, 12, 18, 210),
    );
    painter.image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    // World -> map transform.
    let world_to_map = |position: Vec3| -> egui::Pos2 {
        let dx = (position.x - center.x) / minimap.zoom;
        let dz = (position.z - center.y) / minimap.zoom;
        egui::pos2(rect.center().x + dx, rect.center().y + dz)
    };
    // Machines.
    for machine in &machines {
        let position = Vec3::new(
            machine.pos[0] as f32 + 0.5,
            0.0,
            machine.pos[2] as f32 + 0.5,
        );
        let p = world_to_map(position);
        if rect.contains(p) {
            painter.rect_filled(
                egui::Rect::from_center_size(p, egui::vec2(3.0, 3.0)),
                egui::CornerRadius::ZERO,
                egui::Color32::from_rgb(0x35, 0xe0, 0xe8),
            );
        }
    }
    // Wildlife (aggressive bright red, passive amber).
    for (creature, transform) in &creatures {
        if creature.hp <= 0.0 {
            continue;
        }
        let p = world_to_map(transform.translation);
        if !rect.contains(p) {
            continue;
        }
        let aggressive = matches!(creature.kind, "crab" | "beetle" | "hopper" | "sentinel")
            && creature.aggro_t > 0.0;
        let color = if creature.kind == "sentinel" {
            egui::Color32::from_rgb(0xff, 0x55, 0x55)
        } else if aggressive {
            egui::Color32::from_rgb(0xff, 0x8a, 0x40)
        } else {
            egui::Color32::from_rgb(0xff, 0xd9, 0x66)
        };
        painter.circle_filled(p, 2.2, color);
    }
    // Parked ship.
    if let Some(game) = game.as_deref() {
        let p = world_to_map(game.ship_pos);
        if rect.contains(p) {
            painter.circle_filled(p, 3.2, egui::Color32::from_rgb(0xf0, 0xf0, 0xf0));
            painter.circle_stroke(
                p,
                5.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
            );
        }
    }
    // Player arrow (map is north-up, arrow follows yaw).
    let center_p = rect.center();
    let forward = Vec2::new(-player.yaw.sin(), -player.yaw.cos());
    let side = Vec2::new(forward.y, -forward.x);
    let tip = center_p + egui::vec2(forward.x, forward.y) * 7.0;
    let left = center_p + egui::vec2(side.x, side.y) * 4.0 - egui::vec2(forward.x, forward.y) * 3.0;
    let right =
        center_p - egui::vec2(side.x, side.y) * 4.0 - egui::vec2(forward.x, forward.y) * 3.0;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        egui::Color32::from_rgb(0xff, 0xff, 0xff),
        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x10, 0x14, 0x1a)),
    ));
    // Border + compass.
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(6),
        egui::Stroke::new(1.5, egui::Color32::from_rgb(0x35, 0x46, 0x55)),
        egui::StrokeKind::Middle,
    );
    let label = |text: &str, pos: egui::Pos2| {
        painter.text(
            pos,
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(10.0),
            egui::Color32::from_gray(190),
        );
    };
    label("N", egui::pos2(rect.center().x, rect.top() + 8.0));
    label("S", egui::pos2(rect.center().x, rect.bottom() - 8.0));
    label("W", egui::pos2(rect.left() + 9.0, rect.center().y));
    label("E", egui::pos2(rect.right() - 9.0, rect.center().y));
    // Biome caption.
    painter.text(
        egui::pos2(rect.left(), rect.bottom() + 6.0),
        egui::Align2::LEFT_TOP,
        format!("{} · N 隐藏", world.biome().name),
        egui::FontId::proportional(11.0),
        egui::Color32::from_gray(160),
    );
}

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MinimapState>().add_systems(
            Update,
            (minimap_hotkey_system, minimap_system)
                .chain()
                .in_set(crate::schedule::GameSet::HudMain)
                .run_if(in_state(GameState::Playing)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_distinct_for_key_families() {
        let grass = surface_color("grass", 60);
        let sand = surface_color("sand", 60);
        let water = surface_color("water", 60);
        assert_ne!(grass, sand);
        assert_ne!(sand, water);
        assert_ne!(grass, water);
        // Channels are valid and never fully black.
        for key in ["grass", "sand", "water", "snow", "lava", "alien"] {
            let (r, g, b) = surface_color(key, 60);
            assert!((r as u32 + g as u32 + b as u32) > 30, "{key}");
        }
    }

    #[test]
    fn height_shading_brightens_peaks() {
        let low = surface_color("stone", 10);
        let high = surface_color("stone", 90);
        assert!(high.0 > low.0 && high.1 > low.1);
    }

    #[test]
    fn sampled_image_has_requested_size() {
        let world = crate::world::World::new(42, "lush", 3);
        let image = sample_minimap(&world, Vec2::ZERO, 3.0, 16);
        assert_eq!(image.size, [16, 16]);
        assert_eq!(image.pixels.len(), 16 * 16);
    }
}
