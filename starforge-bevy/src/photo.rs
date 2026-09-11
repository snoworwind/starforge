//! Photo mode (`F2`): pauses the HUD, lets the camera orbit the player with
//! sliders for distance/FOV, applies a light film filter and saves PNG
//! screenshots to `screenshots/` next to the executable.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::player::Player;
use crate::schedule::GameState;
use crate::tween::{Spring, exp_approach};
use crate::ui::{Panel, UiState};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhotoFilter {
    None,
    Warm,
    Cold,
    Noir,
    Vivid,
}

impl PhotoFilter {
    pub const ALL: [PhotoFilter; 5] = [
        PhotoFilter::None,
        PhotoFilter::Warm,
        PhotoFilter::Cold,
        PhotoFilter::Noir,
        PhotoFilter::Vivid,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PhotoFilter::None => "原色",
            PhotoFilter::Warm => "暖阳",
            PhotoFilter::Cold => "寒夜",
            PhotoFilter::Noir => "黑白",
            PhotoFilter::Vivid => "鲜艳",
        }
    }

    /// (r, g, b, alpha) overlay used to approximate the filter.
    pub fn overlay(self) -> (u8, u8, u8, u8) {
        match self {
            PhotoFilter::None => (0, 0, 0, 0),
            PhotoFilter::Warm => (255, 150, 60, 26),
            PhotoFilter::Cold => (80, 140, 255, 30),
            PhotoFilter::Noir => (20, 20, 20, 60),
            PhotoFilter::Vivid => (255, 60, 160, 18),
        }
    }

    /// Second pass overlay (vignette tint / extra contrast wash).
    pub fn accent(self) -> (u8, u8, u8, u8) {
        match self {
            PhotoFilter::None => (0, 0, 0, 0),
            PhotoFilter::Warm => (255, 220, 160, 14),
            PhotoFilter::Cold => (190, 220, 255, 14),
            PhotoFilter::Noir => (0, 0, 0, 80),
            PhotoFilter::Vivid => (90, 255, 220, 12),
        }
    }
}

#[derive(Resource)]
pub struct PhotoState {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov: f32,
    pub filter: PhotoFilter,
    pub letterbox: bool,
    pub hide_hud: bool,
    pub filter_blend: f32,
    pub distance_spring: Spring,
    pub last_screenshot: Option<String>,
}

impl Default for PhotoState {
    fn default() -> Self {
        Self {
            yaw: 0.6,
            pitch: 0.35,
            distance: 9.0,
            fov: 55.0,
            filter: PhotoFilter::None,
            letterbox: true,
            hide_hud: true,
            filter_blend: 0.0,
            distance_spring: Spring::new(9.0, 40.0, 0.9),
            last_screenshot: None,
        }
    }
}

/// F2 toggles the photo panel.
pub fn photo_hotkey_system(keys: Res<ButtonInput<KeyCode>>, mut ui_state: ResMut<UiState>) {
    if keys.just_pressed(KeyCode::F2) {
        if ui_state.panel == Panel::Photo {
            ui_state.close_panel();
        } else if !ui_state.locked() {
            ui_state.panel = Panel::Photo;
        }
    }
}

/// Orbiting camera. Runs after every other camera writer in the frame so the
/// player-controlled first person camera cannot fight it.
pub fn photo_camera_system(
    time: Res<Time>,
    mut photo: ResMut<PhotoState>,
    ui_state: Res<UiState>,
    player: Query<&Player>,
    mut cam: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if ui_state.panel != Panel::Photo {
        photo.filter_blend = exp_approach(photo.filter_blend, 0.0, 6.0, time.delta_secs());
        return;
    }
    let Ok(player) = player.single() else { return };
    photo.filter_blend = exp_approach(photo.filter_blend, 1.0, 5.0, time.delta_secs());
    let focus = player.pos + Vec3::Y * 1.1;
    let target_distance = photo.distance;
    let distance = photo
        .distance_spring
        .update(target_distance, time.delta_secs());
    let offset = Vec3::new(
        photo.yaw.sin() * photo.pitch.cos(),
        photo.pitch.sin().clamp(-0.5, 1.3),
        photo.yaw.cos() * photo.pitch.cos(),
    ) * distance;
    let position = focus + offset;
    for (mut transform, mut projection) in &mut cam {
        transform.translation = position;
        transform.look_at(focus, Vec3::Y);
        *projection = Projection::Perspective(PerspectiveProjection {
            fov: photo.fov.clamp(20.0, 100.0).to_radians(),
            far: crate::space::CAM_FAR,
            ..default()
        });
    }
}

/// Enables camera/mouse drag + wheel zoom while the panel is open. Mouse look
/// is disabled because the cursor is unlocked with a panel open.
pub fn photo_input_system(
    mut contexts: EguiContexts,
    mut photo: ResMut<PhotoState>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let delta = ctx.input(|i| i.pointer.delta());
    if mouse.pressed(MouseButton::Left) {
        photo.yaw -= delta.x * 0.008;
        photo.pitch = (photo.pitch + delta.y * 0.006).clamp(-0.35, 1.25);
    }
    for event in wheel.read() {
        photo.distance = (photo.distance - event.y * 0.6).clamp(3.0, 40.0);
    }
}

pub fn photo_panel_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut photo: ResMut<PhotoState>,
    mut commands: Commands,
) {
    if ui_state.panel != Panel::Photo {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("photo_overlay"),
    ));
    let viewport = ctx.viewport_rect();
    // Filter wash.
    let blend = photo.filter_blend;
    let (r, g, b, a) = photo.filter.overlay();
    if a > 0 {
        painter.rect_filled(
            viewport,
            egui::CornerRadius::ZERO,
            egui::Color32::from_rgba_unmultiplied(r, g, b, (a as f32 * blend) as u8),
        );
    }
    let (r2, g2, b2, a2) = photo.filter.accent();
    if a2 > 0 {
        painter.rect_filled(
            viewport,
            egui::CornerRadius::ZERO,
            egui::Color32::from_rgba_unmultiplied(r2, g2, b2, (a2 as f32 * blend) as u8),
        );
    }
    // Letterbox bars.
    if photo.letterbox {
        let bar = viewport.height() * 0.085;
        let color = egui::Color32::from_black_alpha((230.0 * blend) as u8);
        painter.rect_filled(
            egui::Rect::from_min_max(
                viewport.min,
                egui::pos2(viewport.max.x, viewport.min.y + bar),
            ),
            egui::CornerRadius::ZERO,
            color,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(viewport.min.x, viewport.max.y - bar),
                viewport.max,
            ),
            egui::CornerRadius::ZERO,
            color,
        );
    }
    // Control panel.
    egui::Window::new("摄影模式")
        .default_pos(egui::pos2(viewport.right() - 300.0, 80.0))
        .default_size(egui::vec2(270.0, 300.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("按住左键拖动镜头 · 滚轮缩放")
                    .size(12.0)
                    .color(egui::Color32::from_gray(170)),
            );
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("距离");
                ui.add(egui::Slider::new(&mut photo.distance, 3.0..=40.0).suffix(" m"));
            });
            ui.horizontal(|ui| {
                ui.label("视野");
                ui.add(egui::Slider::new(&mut photo.fov, 20.0..=100.0).suffix("°"));
            });
            ui.horizontal(|ui| {
                ui.label("仰角");
                ui.add(egui::Slider::new(&mut photo.pitch, -0.35..=1.25).fixed_decimals(2));
            });
            ui.horizontal(|ui| {
                ui.label("旋转");
                ui.add(egui::Slider::new(&mut photo.yaw, -3.15..=3.15).fixed_decimals(2));
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for filter in PhotoFilter::ALL {
                    if ui
                        .selectable_label(photo.filter == filter, filter.name())
                        .clicked()
                    {
                        photo.filter = filter;
                    }
                }
            });
            ui.checkbox(&mut photo.letterbox, "电影黑边");
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("📷 拍摄 (F2 退出)").clicked() {
                    photo.last_screenshot = Some(save_screenshot(&mut commands));
                }
                if ui.button("退出").clicked() {
                    ui_state.close_panel();
                }
            });
            if let Some(path) = &photo.last_screenshot {
                ui.label(
                    egui::RichText::new(format!("已保存：{path}"))
                        .size(11.0)
                        .color(egui::Color32::from_rgb(0x7d, 0xff, 0x8a)),
                );
            }
        });
}

/// Queues a screenshot into `screenshots/photo_<unix>.png`.
pub fn save_screenshot(commands: &mut Commands) -> String {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = crate::save::saves_dir()
        .parent()
        .map(|parent| parent.join("screenshots"))
        .unwrap_or_else(|| std::path::PathBuf::from("screenshots"));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("photo_{stamp}.png"));
    let path_text = path.to_string_lossy().into_owned();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    path_text
}

pub struct PhotoPlugin;

impl Plugin for PhotoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PhotoState>()
            .add_systems(
                Update,
                (photo_hotkey_system, photo_input_system).run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                photo_camera_system
                    .in_set(crate::schedule::GameSet::CameraFx)
                    .after(crate::camera_fx::camera_shake_system)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                photo_panel_system.run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_have_distinct_overlays() {
        let none = PhotoFilter::None.overlay();
        assert_eq!(none.3, 0);
        for filter in PhotoFilter::ALL {
            let overlay = filter.overlay();
            let accent = filter.accent();
            assert!(overlay.3 <= 80 && accent.3 <= 90);
        }
        assert_ne!(PhotoFilter::Warm.overlay(), PhotoFilter::Cold.overlay());
        assert_ne!(PhotoFilter::Noir.overlay(), PhotoFilter::Vivid.overlay());
    }

    #[test]
    fn default_state_is_sane() {
        let state = PhotoState::default();
        assert!(state.distance > 0.0);
        assert!((20.0..=100.0).contains(&state.fov));
        assert!(state.pitch.abs() < 1.3);
    }
}
