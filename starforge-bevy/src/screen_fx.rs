//! Full-screen feedback drawn through egui: damage vignette, low-health pulse,
//! underwater/heat/cold/toxic tints, hit-direction indicators and event
//! flashes. Everything is procedural (one cached radial-gradient texture) so it
//! costs no assets and matches the game's HUD style.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::player::Player;
use crate::schedule::GameSet;
use crate::space::ShipState;
use crate::tween::{clamp01, exp_approach, smoothstep};

/// A directional hit marker rendered around the crosshair.
#[derive(Clone, Copy, Debug)]
pub struct HitIndicator {
    /// World-space direction from the player toward the attacker.
    pub dir: Vec3,
    pub age: f32,
    pub strength: f32,
    /// True for ship hits so the marker can be dimmer and wider.
    pub ship: bool,
}

#[derive(Resource)]
pub struct ScreenFx {
    pub damage_flash: f32,
    pub heal_flash: f32,
    pub shield_flash: f32,
    pub discovery_flash: f32,
    pub level_flash: f32,
    pub death: f32,
    pub low_hp: f32,
    pub submerged: f32,
    pub heat: f32,
    pub cold: f32,
    pub toxic: f32,
    pub rad: f32,
    pub storm: f32,
    pub scan: f32,
    /// Blended night-vision overlay strength (written by the suit module).
    pub night_vision: f32,
    /// White lightning flash, decays fast.
    pub lightning: f32,
    pub hit_dirs: Vec<HitIndicator>,
    last_hp: f32,
    last_shield: f32,
    last_ship_hp: f32,
    last_o2: f32,
}

impl Default for ScreenFx {
    fn default() -> Self {
        Self {
            damage_flash: 0.0,
            heal_flash: 0.0,
            shield_flash: 0.0,
            discovery_flash: 0.0,
            level_flash: 0.0,
            death: 0.0,
            low_hp: 0.0,
            submerged: 0.0,
            heat: 0.0,
            cold: 0.0,
            toxic: 0.0,
            rad: 0.0,
            storm: 0.0,
            scan: 0.0,
            night_vision: 0.0,
            lightning: 0.0,
            hit_dirs: Vec::new(),
            last_hp: 8.0,
            last_shield: 6.0,
            last_ship_hp: 20.0,
            last_o2: 100.0,
        }
    }
}

impl ScreenFx {
    pub fn hit(&mut self, from: Vec3, to: Vec3, strength: f32) {
        let dir = (from - to).normalize_or_zero();
        self.hit_dirs.push(HitIndicator {
            dir,
            age: 0.0,
            strength: strength.clamp(0.2, 1.0),
            ship: false,
        });
        if self.hit_dirs.len() > 8 {
            self.hit_dirs.remove(0);
        }
    }

    pub fn ship_hit(&mut self, from: Vec3, to: Vec3, strength: f32) {
        let dir = (from - to).normalize_or_zero();
        self.hit_dirs.push(HitIndicator {
            dir,
            age: 0.0,
            strength: strength.clamp(0.2, 1.0),
            ship: true,
        });
        if self.hit_dirs.len() > 8 {
            self.hit_dirs.remove(0);
        }
    }

    pub fn discover(&mut self) {
        self.discovery_flash = 1.0;
    }

    pub fn level_up(&mut self) {
        self.level_flash = 1.0;
    }

    pub fn scanned(&mut self) {
        self.scan = 1.0;
    }

    pub fn heal(&mut self) {
        self.heal_flash = 1.0;
    }

    fn decay(&mut self, dt: f32) {
        let decay = |v: &mut f32, rate: f32| {
            *v = (*v - rate * dt).max(0.0);
        };
        decay(&mut self.damage_flash, 1.8);
        decay(&mut self.heal_flash, 1.4);
        decay(&mut self.shield_flash, 1.4);
        decay(&mut self.discovery_flash, 0.9);
        decay(&mut self.level_flash, 0.7);
        decay(&mut self.scan, 2.0);
        decay(&mut self.lightning, 3.2);
        for hit in &mut self.hit_dirs {
            hit.age += dt;
        }
        self.hit_dirs.retain(|hit| hit.age < 1.1);
    }
}

/// Updates the effect state from the live player/ship every frame, before the
/// overlay is painted.
#[allow(clippy::too_many_arguments)]
pub fn screen_fx_track_system(
    time: Res<Time>,
    world: Option<Res<crate::world::World>>,
    mut fx: ResMut<ScreenFx>,
    player: Query<&Player>,
    ship: Option<Res<ShipState>>,
    creatures: Query<(&crate::creatures::Creature, &Transform)>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    fx.decay(dt);
    let Ok(p) = player.single() else { return };

    // Damage detection: any hp/shield drop produces a flash; if a nearby
    // creature is the likely source, add a directional marker.
    let max_shield = p.stat_max("shield");
    let took_damage = p.stats.hp < fx.last_hp - 0.01 || p.stats.shield < fx.last_shield - 0.01;
    if took_damage {
        let drop = (fx.last_hp - p.stats.hp).max(fx.last_shield - p.stats.shield);
        fx.damage_flash = (0.55 + drop * 0.22).min(1.0);
        if let Some((_, tf)) = creatures
            .iter()
            .filter(|(c, tf)| {
                c.hp > 0.0 && c.aggro_t > 0.0 && tf.translation.distance(p.pos) < 24.0
            })
            .min_by(|a, b| {
                a.1.translation
                    .distance_squared(p.pos)
                    .partial_cmp(&b.1.translation.distance_squared(p.pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            fx.hit(tf.translation, p.pos, 0.6);
        }
    }
    if p.stats.hp > fx.last_hp + 0.01 {
        fx.heal();
    }
    if p.stats.shield > fx.last_shield + 0.01 && p.stats.hp <= fx.last_hp + 0.01 {
        fx.shield_flash = 1.0;
    }
    fx.last_hp = p.stats.hp;
    fx.last_shield = p.stats.shield.min(max_shield);
    fx.last_o2 = p.stats.o2;

    if let Some(ship) = ship.as_ref() {
        if ship.hp < fx.last_ship_hp - 0.01 {
            let drop = fx.last_ship_hp - ship.hp;
            fx.damage_flash = (0.5 + drop * 0.2).min(1.0);
        }
        fx.last_ship_hp = ship.hp;
    }

    // Low-health pulse: the lower the hp, the faster and stronger the beat.
    let hp_ratio = clamp01(p.stats.hp / 8.0);
    let beat =
        (time.elapsed_secs() * (5.0 - hp_ratio * 2.0) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    fx.low_hp = exp_approach(
        fx.low_hp,
        if hp_ratio < 0.45 && !p.creative() {
            (1.0 - hp_ratio / 0.45).min(1.0) * (0.45 + beat * 0.55)
        } else {
            0.0
        },
        6.0,
        dt,
    );
    fx.death = if p.dead {
        (fx.death + dt * 2.4).min(1.0)
    } else {
        (fx.death - dt * 1.6).max(0.0)
    };

    // Environmental tints follow biome hazards and where the camera is.
    let biome = world.as_deref().map(|w| w.biome());
    let hazard = biome.and_then(|b| b.haz);
    let exposed = 1.0 - p.stats.haz / 100.0;
    let target = |active: bool, strength: f32| if active { strength } else { 0.0 };
    fx.heat = exp_approach(
        fx.heat,
        target(hazard == Some("heat"), (0.35 + exposed) * 0.5).min(0.85),
        2.4,
        dt,
    );
    fx.cold = exp_approach(
        fx.cold,
        target(hazard == Some("cold"), (0.3 + exposed) * 0.5).min(0.8),
        2.4,
        dt,
    );
    fx.toxic = exp_approach(
        fx.toxic,
        target(hazard == Some("toxic"), (0.35 + exposed) * 0.55).min(0.85),
        2.4,
        dt,
    );
    fx.rad = exp_approach(
        fx.rad,
        target(hazard == Some("rad"), (0.35 + exposed) * 0.55).min(0.85),
        2.4,
        dt,
    );
    fx.storm = exp_approach(
        fx.storm,
        target(hazard == Some("storm"), (0.25 + exposed) * 0.5).min(0.75),
        2.4,
        dt,
    );
    fx.submerged = exp_approach(fx.submerged, if p.in_liquid { 1.0 } else { 0.0 }, 4.0, dt);
}

/// Paints the overlay. Registered inside the HUD set so panels naturally
/// suppress it along with the rest of the HUD.
pub fn screen_fx_draw_system(
    mut contexts: EguiContexts,
    fx: Res<ScreenFx>,
    ui_state: Res<crate::ui::UiState>,
    player: Query<&Player>,
    mut vignette: Local<Option<egui::TextureHandle>>,
) {
    if ui_state.locked() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let Ok(p) = player.single() else { return };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("screen_fx"),
    ));
    let rect = ctx.viewport_rect();
    let full = rect.expand2(egui::vec2(2.0, 2.0));

    let texture = vignette.get_or_insert_with(|| {
        let image = vignette_image();
        ctx.load_texture("sf_vignette", image, egui::TextureOptions::LINEAR)
    });

    // Base environmental tints.
    let paint_tint = |color: egui::Color32, alpha: f32| {
        if alpha > 0.002 {
            painter.rect_filled(full, egui::CornerRadius::ZERO, color.gamma_multiply(alpha));
        }
    };
    paint_tint(egui::Color32::from_rgb(30, 90, 160), fx.submerged * 0.32);
    paint_tint(egui::Color32::from_rgb(255, 120, 40), fx.heat * 0.14);
    paint_tint(egui::Color32::from_rgb(150, 210, 255), fx.cold * 0.16);
    paint_tint(egui::Color32::from_rgb(80, 220, 110), fx.toxic * 0.13);
    paint_tint(egui::Color32::from_rgb(150, 90, 220), fx.rad * 0.12);
    paint_tint(egui::Color32::from_rgb(120, 150, 255), fx.storm * 0.1);
    // Night vision: green light amplification wash plus a soft edge glow.
    if fx.night_vision > 0.002 {
        paint_tint(
            egui::Color32::from_rgb(60, 255, 120),
            fx.night_vision * 0.14,
        );
        paint_tint(
            egui::Color32::from_rgb(120, 255, 170),
            fx.night_vision * 0.05,
        );
    }

    // Vignette layers, from ambient to damage to death. The texture already
    // carries the center-transparent/edge-opaque gradient, so the tint color
    // and alpha do all the layering.
    let draw_vignette = |tint: egui::Color32, alpha: f32| {
        if alpha <= 0.002 {
            return;
        }
        painter.image(
            texture.id(),
            full,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint.gamma_multiply(alpha),
        );
    };
    let ambient = 0.22 + fx.low_hp * 0.25;
    draw_vignette(egui::Color32::BLACK, ambient);
    let damage = fx.damage_flash;
    draw_vignette(egui::Color32::from_rgb(200, 20, 20), damage * 0.85);
    if fx.hurt_flash() > 0.0 {
        draw_vignette(egui::Color32::from_rgb(255, 60, 40), fx.hurt_flash() * 0.5);
    }
    draw_vignette(egui::Color32::from_rgb(120, 0, 0), fx.low_hp * 0.75);
    draw_vignette(egui::Color32::from_rgb(255, 30, 10), fx.death * 0.95);
    draw_vignette(
        egui::Color32::from_rgb(240, 255, 255),
        (fx.scan * 0.35).min(0.35),
    );

    // Event flashes.
    let flash = |color: egui::Color32, amount: f32| {
        if amount > 0.002 {
            painter.rect_filled(
                full,
                egui::CornerRadius::ZERO,
                color.gamma_multiply(amount.min(1.0) * 0.6),
            );
        }
    };
    flash(egui::Color32::from_rgb(120, 255, 180), fx.heal_flash);
    flash(egui::Color32::from_rgb(90, 210, 255), fx.shield_flash);
    flash(egui::Color32::from_rgb(255, 235, 150), fx.discovery_flash);
    flash(egui::Color32::from_rgb(180, 150, 255), fx.level_flash);
    flash(egui::Color32::from_rgb(235, 240, 255), fx.lightning * 0.9);

    // Directional hit markers around the crosshair.
    let center = rect.center();
    let camera_yaw = p.yaw;
    for hit in &fx.hit_dirs {
        let alpha = (1.0 - clamp01(hit.age / 1.1)).powi(2) * hit.strength;
        if alpha < 0.01 || hit.dir.length_squared() < 1e-6 {
            continue;
        }
        // Project the world direction onto the camera's yaw plane.
        let rotated = Quat::from_rotation_y(-camera_yaw) * hit.dir;
        let angle = rotated.x.atan2(-rotated.z);
        let radius = rect.height().min(rect.width()) * if hit.ship { 0.42 } else { 0.3 };
        let dir2 = egui::vec2(angle.sin(), -angle.cos());
        let pos = center + dir2 * radius;
        // A small chevron pointing toward the source.
        let side = egui::vec2(-dir2.y, dir2.x);
        let size = if hit.ship { 13.0 } else { 10.0 };
        let color = if hit.ship {
            egui::Color32::from_rgba_unmultiplied(120, 200, 255, (alpha * 220.0) as u8)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 90, 70, (alpha * 230.0) as u8)
        };
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos + dir2 * size,
                pos - dir2 * size * 0.4 + side * size * 0.7,
                pos - dir2 * size * 0.4 - side * size * 0.7,
            ],
            color,
            egui::Stroke::NONE,
        ));
    }
}

impl ScreenFx {
    fn hurt_flash(&self) -> f32 {
        self.damage_flash * 0.5
    }
}

/// Radial darkening mask: white RGB with an alpha ramp from transparent in the
/// centre to opaque at the edges.
fn vignette_image() -> egui::ColorImage {
    const SIZE: usize = 128;
    let mut pixels = Vec::with_capacity(SIZE * SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let nx = (x as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
            let ny = (y as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
            let d = (nx * nx + ny * ny).sqrt() / std::f32::consts::SQRT_2;
            let alpha = smoothstep(0.42, 1.0, d);
            let value = (alpha * 255.0) as u8;
            pixels.push(egui::Color32::from_rgba_unmultiplied(255, 255, 255, value));
        }
    }
    egui::ColorImage {
        size: [SIZE, SIZE],
        pixels,
        source_size: egui::vec2(SIZE as f32, SIZE as f32),
    }
}

pub struct ScreenFxPlugin;

impl Plugin for ScreenFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenFx>().add_systems(
            Update,
            (screen_fx_track_system, screen_fx_draw_system)
                .chain()
                .in_set(GameSet::HudMain)
                .run_if(in_state(crate::schedule::GameState::Playing)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_markers_record_direction() {
        let mut fx = ScreenFx::default();
        fx.hit(Vec3::new(3.0, 1.0, 0.0), Vec3::ZERO, 0.8);
        assert_eq!(fx.hit_dirs.len(), 1);
        let dir = fx.hit_dirs[0].dir;
        let expected = 3.0 / 10.0f32.sqrt();
        assert!((dir.x - expected).abs() < 1e-4);
        assert!(dir.length() - 1.0 < 1e-4);
    }

    #[test]
    fn hit_markers_are_bounded() {
        let mut fx = ScreenFx::default();
        for i in 0..20 {
            fx.hit(Vec3::new(i as f32, 0.0, 0.0), Vec3::ZERO, 1.0);
        }
        assert!(fx.hit_dirs.len() <= 8);
    }

    #[test]
    fn decay_clears_transient_flashes() {
        let mut fx = ScreenFx {
            damage_flash: 1.0,
            discovery_flash: 1.0,
            ..default()
        };
        for _ in 0..120 {
            fx.decay(1.0 / 60.0);
        }
        assert_eq!(fx.damage_flash, 0.0);
        assert_eq!(fx.discovery_flash, 0.0);
    }

    #[test]
    fn vignette_center_is_transparent() {
        let image = vignette_image();
        let center = (64 * 128 + 64) as usize;
        assert!(image.pixels[center].a() < 4);
        assert!(image.pixels[0].a() > 100);
    }
}
