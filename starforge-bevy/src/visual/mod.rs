//! A02/A03 visual contracts: the single read-only frame snapshot, the
//! celestial-light snapshot and the capability/quality resolution shared by
//! rendering workstreams.
//!
//! `sample_visual_frame` runs once per frame in `Last`, after every camera
//! writer and after `planet_scale::update_visual_frame`, so the snapshot
//! contains the finalized camera and the current local planet frame. Consumers
//! in `Update` read the snapshot produced at the end of the previous frame;
//! all of them therefore see the same coherent frame (one-frame latency is the
//! documented contract, and `prev_view_proj` supports reprojection history).

mod capabilities;
mod diagnostics;
mod frame;
mod lifecycle;
mod quality;
mod settings;

#[allow(unused_imports)]
pub use capabilities::{CapabilitySimulation, FormatSupport, RenderCapabilities};
pub use diagnostics::{VISUAL_DIAGNOSTICS_SCHEMA_VERSION, VisualDiagnostics};
pub use frame::{CelestialLighting, VisualCameraId, VisualFrame, WorldEpoch};
#[allow(unused_imports)]
pub use lifecycle::{
    TaskSubmitError, VISUAL_OWNERSHIP, VisualLifecycle, VisualLifecycleDiagnostics,
    VisualLifecycleMut, VisualOwner, VisualRevision, VisualTaskRegistry, VisualTaskTicket,
    WorldTransition, advance_world_epoch,
};
pub use quality::{FeatureResolution, QualityProfile, ResolvedQuality, resolve_quality};
#[allow(unused_imports)]
pub use settings::{
    ApplyMode, CorrectionReason, LEGACY_SETTING_KEYS, SanitizeReport, SettingCorrection,
    SettingGroup, SettingSpec, SettingValueKind, SettingsLoadReport, VISUAL_SETTING_SPECS,
    VisualPreset, VisualSettings, apply_mode_for, apply_visual_preset, clamp_f32, clamp_u32,
    detect_visual_preset, reset_visual_settings, sanitize_cloud_tuning, sanitize_lighting_tuning,
    sanitize_settings, spec,
};

use bevy::camera::{Exposure, Hdr};
use bevy::light::{AtmosphereEnvironmentMapLight, DirectionalLightShadowMap};
use bevy::pbr::AtmosphereSettings;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;

use crate::planet_scale::PlanetVisualFrame;
use crate::save::Settings;
use crate::schedule::{GameSet, GameState};

/// Samples the finalized primary 3D camera into [`VisualFrame`].
///
/// This system is read-only with respect to the camera and must stay the only
/// writer of `VisualFrame`; camera pose owners keep writing `Transform`/
/// `Projection` exactly as before.
fn sample_visual_frame(
    time: Res<Time>,
    virtual_time: Res<Time<Virtual>>,
    state: Res<State<GameState>>,
    epoch: Res<WorldEpoch>,
    mut lifecycle: ResMut<VisualLifecycle>,
    planet: Res<PlanetVisualFrame>,
    mut frame: ResMut<VisualFrame>,
    camera: Query<(&Camera, &GlobalTransform, &Projection), With<Camera3d>>,
) {
    let Ok((camera, transform, projection)) = camera.single() else {
        return;
    };

    let camera_id = VisualCameraId::PRIMARY;
    let observation = lifecycle.observe_camera(
        camera_id,
        *epoch,
        camera
            .physical_viewport_size()
            .filter(|size| size.x > 0 && size.y > 0),
    );
    let view: Mat4 = transform.affine().inverse().into();
    let projection_matrix = projection.get_clip_from_view();
    let view_proj = projection_matrix * view;
    frame.frame_id = frame.frame_id.wrapping_add(1);
    if observation.reset_history {
        frame.prev_view = view;
        frame.prev_view_proj = view_proj;
    } else {
        frame.prev_view = frame.view;
        frame.prev_view_proj = frame.view_proj;
    }
    frame.view = view;
    frame.projection = projection_matrix;
    frame.view_proj = view_proj;
    frame.eye = transform.translation();
    frame.forward = transform.forward().as_vec3();
    frame.right = transform.right().as_vec3();
    frame.up = transform.up().as_vec3();
    let (fov_y_radians, near, far) = match projection {
        Projection::Perspective(perspective) => {
            (perspective.fov, perspective.near, perspective.far)
        }
        Projection::Orthographic(orthographic) => (0.0, orthographic.near, orthographic.far),
        Projection::Custom(_) => (0.0, 0.1, projection.far()),
    };
    frame.fov_y_radians = fov_y_radians;
    frame.near = near;
    frame.far = far;
    // Keep the last valid size while minimized; future render-target owners
    // use `VisualLifecycleDiagnostics::camera_suspended` to defer allocation.
    frame.viewport = observation.viewport;
    frame.world_epoch = epoch.0;
    let dt = time.delta_secs();
    frame.dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    frame.paused = virtual_time.is_paused() || !matches!(*state.get(), GameState::Playing);
    frame.planet = *planet;
    frame.camera_id = camera_id;
    // `origin_offset` is owned by the precision workstream (C05/D05); the
    // producer must not clobber a value they publish.
}

/// Recompute the effective quality whenever the request or capabilities
/// change. Runs before the climate/lighting chain so weather and daynight read
/// the resolved flags for the current frame.
fn refresh_resolved_quality(
    settings: Res<Settings>,
    capabilities: Res<RenderCapabilities>,
    mut quality: ResMut<ResolvedQuality>,
) {
    let profile = QualityProfile::from_settings(&settings);
    let resolved = resolve_quality(&profile, &capabilities);
    if *quality != resolved {
        if resolved.has_downgrades() {
            for reason in &resolved.downgrades {
                warn!("quality downgrade: {reason}");
            }
        }
        *quality = resolved;
    }
}

/// Apply the one-time fallbacks that must happen before rendering starts.
/// Runs in `PostStartup` so the camera entity spawned by the flow plugin
/// already exists.
fn apply_capability_fallbacks(
    capabilities: Res<RenderCapabilities>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    cameras: Query<Entity, With<Camera3d>>,
    mut commands: Commands,
) {
    let size = capabilities.recommended_shadow_map_size(shadow_map.size as u32) as usize;
    if size != shadow_map.size {
        warn!(
            "shadow map downgraded {}→{} (max_texture_dimension_2d={})",
            shadow_map.size, size, capabilities.limits.max_texture_dimension_2d
        );
        shadow_map.size = size;
    }
    if !capabilities.hdr_supported() {
        for entity in &cameras {
            commands
                .entity(entity)
                .remove::<Hdr>()
                .remove::<Bloom>()
                .remove::<AtmosphereSettings>()
                .remove::<AtmosphereEnvironmentMapLight>();
        }
        warn!("HDR target unsupported; camera falls back to LDR without Bloom/atmosphere");
    } else if capabilities.simulated.is_some() && !capabilities.atmosphere_supported() {
        // On a real device Bevy's AtmospherePlugin refuses to load when the
        // storage-image format is missing; during simulation the adapter still
        // supports it, so remove the components explicitly to exercise the
        // same fallback path (ClearColor + fog sky).
        for entity in &cameras {
            commands
                .entity(entity)
                .remove::<AtmosphereSettings>()
                .remove::<AtmosphereEnvironmentMapLight>();
        }
        warn!("simulated atmosphere downgrade: camera falls back to ClearColor/fog sky");
    }
}

fn log_first_frame(frame: Res<VisualFrame>, exposure: Query<&Exposure, With<Camera3d>>) {
    if frame.frame_id == 1 {
        let ev100 = exposure
            .single()
            .map(|exposure| exposure.ev100)
            .unwrap_or(0.0);
        info!(
            "visual frame contract ready: camera={:?} viewport={}x{} fov={:.1}deg ev100={:.1} epoch={}",
            frame.camera_id,
            frame.viewport.x,
            frame.viewport.y,
            frame.fov_y_radians.to_degrees(),
            ev100,
            frame.world_epoch,
        );
    }
}

pub struct VisualPlugin;

impl Plugin for VisualPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldEpoch>()
            .init_resource::<VisualLifecycle>()
            .init_resource::<VisualLifecycleDiagnostics>()
            .init_resource::<VisualFrame>()
            .init_resource::<CelestialLighting>()
            .init_resource::<RenderCapabilities>()
            .init_resource::<ResolvedQuality>()
            .init_resource::<VisualDiagnostics>()
            .add_systems(OnEnter(GameState::Loading), lifecycle::begin_loading_epoch)
            .add_systems(OnExit(GameState::Playing), lifecycle::finish_playing_epoch)
            .add_systems(
                Startup,
                (
                    capabilities::collect_render_capabilities,
                    settings::configure_visual_settings,
                ),
            )
            .add_systems(PostStartup, apply_capability_fallbacks)
            .add_systems(Update, refresh_resolved_quality.in_set(GameSet::CommonUi))
            .add_systems(
                Last,
                (
                    sample_visual_frame,
                    lifecycle::collect_lifecycle_diagnostics,
                    diagnostics::collect_visual_diagnostics,
                    log_first_frame,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_frame_defaults_are_safe_for_consumers() {
        let frame = VisualFrame::default();
        assert!(frame.paused, "default frame must not pretend to simulate");
        assert_eq!(frame.viewport, UVec2::ONE);
        assert_eq!(
            frame.planet.radius,
            crate::planet_scale::PLANET_SCALE.local_planet_radius
        );
    }

    #[test]
    fn celestial_default_is_a_valid_noonside_fallback() {
        let light = CelestialLighting::default();
        assert!(light.daylight > 0.9);
        assert!((light.to_sun_direction + light.travel_direction).length() < 1e-5);
        assert!(light.environment_fill > 0.0);
    }

    #[test]
    fn default_capabilities_do_not_claim_unsupported_features() {
        let capabilities = RenderCapabilities::default();
        assert!(!capabilities.hdr_supported());
        assert!(!capabilities.clouds_supported());
        assert_eq!(capabilities.recommended_shadow_map_size(4096), 256);
    }
}
