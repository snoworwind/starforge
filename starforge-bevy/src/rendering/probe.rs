//! D01 minimal HDR post-process prototype: an exposure/clipping false-color
//! probe implemented with the locked `FullscreenMaterial` API.
//!
//! The probe runs in `Core3dSystems::PostProcess` before tonemapping, so it
//! reads and writes scene-linear HDR instead of a display-referred image. It
//! has three real jobs for this package: prove a custom shader compiles and
//! runs on the HDR view target, visualize whether sky/cloud/ground share one
//! exposure scale (02/2.1 step 3) and give later D/E work a diagnostic view.
//!
//! Failure handling (R011): if the shader fails to load the request is turned
//! off once and the component is removed, so the frame renders through the
//! normal path instead of a black screen or a per-frame recompile loop.

use bevy::asset::AssetLoadFailedEvent;
use bevy::camera::Exposure;
use bevy::core_pipeline::fullscreen_material::FullscreenMaterial;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;
use serde::Serialize;

use crate::visual::RenderCapabilities;

pub const PROBE_SHADER_PATH: &str = "shaders/visual/exposure_probe.wgsl";

/// Shader build exposed to the CLI and the QA variant matrix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum ProbeMode {
    #[default]
    Off,
    /// Seven exposure bands around middle gray (`cards_probe_stops`).
    FalseColor,
    /// Magenta/blue overlay above/below the clipping stops (`cards_probe_zebra`).
    Zebra,
    /// Grayscale log-luminance view (`cards_probe_luma`).
    Luminance,
}

impl ProbeMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::FalseColor => "false-color",
            Self::Zebra => "zebra",
            Self::Luminance => "luminance",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "stops" | "false-color" | "falsecolor" | "ev" => Some(Self::FalseColor),
            "zebra" | "clip" | "clipping" => Some(Self::Zebra),
            "luma" | "luminance" | "log" => Some(Self::Luminance),
            _ => None,
        }
    }

    pub fn shader_mode(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::FalseColor => 1,
            Self::Zebra => 2,
            Self::Luminance => 3,
        }
    }
}

/// Main-world request. Set by `--render-probe` or by the QA pass variants; it
/// never persists to `settings.json` because it is a diagnostic, not a
/// player-facing quality option.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ProbeRequest {
    pub mode: ProbeMode,
    pub strength: f32,
    pub middle_gray: f32,
    pub clip_stops: f32,
    pub shadow_stops: f32,
}

impl Default for ProbeRequest {
    fn default() -> Self {
        Self {
            mode: ProbeMode::Off,
            strength: 0.85,
            // 18% scene-linear middle gray, matching the gray card in the D01
            // scene; stops are relative to this value after exposure.
            middle_gray: 0.18,
            clip_stops: 4.5,
            shadow_stops: -4.5,
        }
    }
}

impl ProbeRequest {
    pub fn enabled(mode: ProbeMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    pub fn activate(&mut self, mode: ProbeMode) {
        self.mode = mode;
    }

    pub fn disable(&mut self) {
        self.mode = ProbeMode::Off;
    }

    pub fn is_active(&self) -> bool {
        self.mode != ProbeMode::Off
    }
}

/// Uniform + activation component for the fullscreen pass. Presence means the
/// pass runs on that camera; `mode == 0` still returns the unmodified source,
/// which is how the pipeline can be validated without changing the image.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, ExtractComponent, ShaderType)]
pub struct ExposureProbe {
    pub mode: u32,
    pub middle_gray: f32,
    pub clip_stops: f32,
    pub shadow_stops: f32,
    pub exposure_ev100: f32,
    pub strength: f32,
}

impl ExposureProbe {
    pub fn from_request(request: &ProbeRequest, exposure_ev100: f32) -> Self {
        fn finite(value: f32, fallback: f32) -> f32 {
            if value.is_finite() { value } else { fallback }
        }
        Self {
            mode: request.mode.shader_mode(),
            middle_gray: finite(request.middle_gray, 0.18).clamp(1e-4, 1.0),
            clip_stops: finite(request.clip_stops, 4.5).clamp(0.0, 12.0),
            shadow_stops: finite(request.shadow_stops, -4.5).clamp(-12.0, 0.0),
            exposure_ev100: finite(exposure_ev100, 13.0).clamp(0.0, 30.0),
            strength: finite(request.strength, 0.85).clamp(0.0, 1.0),
        }
    }
}

impl FullscreenMaterial for ExposureProbe {
    fn fragment_shader() -> ShaderRef {
        PROBE_SHADER_PATH.into()
    }
}

/// Observable state for the rendering report; explains why the probe is or is
/// not on the GPU instead of a silent no-op (R117).
#[derive(Resource, Clone, Debug, Serialize)]
pub struct ProbeStatus {
    /// Probe mode requested by `--render-probe` at startup; D01 scene variants
    /// change `requested` later, so this preserves the launch intent.
    pub initial_request: String,
    pub requested: String,
    pub shader_loaded: bool,
    pub shader_failed: bool,
    pub active: bool,
    pub cameras: usize,
    /// How often the pass turned on during this run and on how many frames it
    /// rendered; 0/0 proves the pass did not run, not merely that it vanished.
    pub activations: u64,
    pub active_frames: u64,
    pub reason: String,
}

impl Default for ProbeStatus {
    fn default() -> Self {
        Self {
            initial_request: ProbeMode::Off.key().to_string(),
            requested: ProbeMode::Off.key().to_string(),
            shader_loaded: false,
            shader_failed: false,
            active: false,
            cameras: 0,
            activations: 0,
            active_frames: 0,
            reason: "idle".to_string(),
        }
    }
}

/// Owns the shader handle so load failures can be matched by id.
#[derive(Resource, Clone, Debug)]
pub struct ProbeShader(pub Handle<Shader>);

pub fn load_probe_shader(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    request: Res<ProbeRequest>,
    mut status: ResMut<ProbeStatus>,
) {
    status.initial_request = request.mode.key().to_string();
    commands.insert_resource(ProbeShader(asset_server.load(PROBE_SHADER_PATH)));
}

pub fn log_probe_ready(request: Res<ProbeRequest>) {
    if request.is_active() {
        info!(
            "render probe requested: mode={} strength={:.2} shader={PROBE_SHADER_PATH}",
            request.mode.key(),
            request.strength,
        );
    }
}

/// Record shader load success/failure. There is no public access to the
/// pipeline compilation result from the main world; the asset load state is
/// the earliest stable signal, and a failed pipeline would simply skip the
/// pass (the fullscreen system early-returns on a missing pipeline).
pub fn watch_probe_shader(
    mut loaded: MessageReader<bevy::asset::AssetEvent<Shader>>,
    mut failures: MessageReader<AssetLoadFailedEvent<Shader>>,
    shader: Option<Res<ProbeShader>>,
    mut request: ResMut<ProbeRequest>,
    mut status: ResMut<ProbeStatus>,
) {
    let Some(shader) = shader else {
        return;
    };
    status.requested = request.mode.key().to_string();
    for event in loaded.read() {
        if event.is_loaded_with_dependencies(shader.0.id()) {
            status.shader_loaded = true;
        }
    }
    for failure in failures.read() {
        if failure.id != shader.0.id() {
            continue;
        }
        error!(
            "render probe shader failed to load ({:?}); probe disabled, scene continues on the normal path",
            failure.error
        );
        status.shader_failed = true;
        status.shader_loaded = false;
        status.reason = format!("shader load failed: {:?}", failure.error);
        request.disable();
    }
}

/// Keep exactly the primary 3D cameras in sync with the request and the live
/// exposure. Runs before render extraction so toggles apply on the same frame.
pub fn sync_exposure_probe(
    mut commands: Commands,
    request: Res<ProbeRequest>,
    capabilities: Option<Res<RenderCapabilities>>,
    mut status: ResMut<ProbeStatus>,
    mut warned_unsupported: Local<bool>,
    cameras: Query<(Entity, Option<&Exposure>, Option<&ExposureProbe>), With<Camera3d>>,
) {
    status.requested = request.mode.key().to_string();
    let wants = request.is_active() && !status.shader_failed;
    if wants
        && let Some(capabilities) = capabilities.as_deref()
        && !capabilities.hdr_supported()
    {
        if !*warned_unsupported {
            *warned_unsupported = true;
            warn!(
                "render probe disabled: HDR/filterable view target unsupported (R012/R045); scene renders without the pass"
            );
        }
        status.active = false;
        status.cameras = 0;
        status.reason = "HDR/filterable target unsupported (R012/R045)".to_string();
        for (entity, _, current) in &cameras {
            if current.is_some() {
                commands.entity(entity).remove::<ExposureProbe>();
            }
        }
        return;
    }

    let mut active = 0usize;
    let was_active = status.active;
    for (entity, exposure, current) in &cameras {
        if wants {
            let probe = ExposureProbe::from_request(
                &request,
                exposure.map(|exposure| exposure.ev100).unwrap_or(13.0),
            );
            if current != Some(&probe) {
                commands.entity(entity).insert(probe);
            }
            active += 1;
        } else if current.is_some() {
            commands.entity(entity).remove::<ExposureProbe>();
        }
    }
    status.active = wants && active > 0;
    status.cameras = if wants { active } else { 0 };
    if status.active {
        status.active_frames = status.active_frames.saturating_add(1);
        if !was_active {
            status.activations = status.activations.saturating_add(1);
        }
    }
    if wants {
        status.reason = if status.shader_loaded {
            "active".to_string()
        } else {
            "active (shader still loading)".to_string()
        };
    } else if !status.shader_failed {
        status.reason = "disabled".to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_modes_parse_and_map_to_shader_modes() {
        assert_eq!(ProbeMode::parse("off"), Some(ProbeMode::Off));
        assert_eq!(ProbeMode::parse("ZEBRA"), Some(ProbeMode::Zebra));
        assert_eq!(ProbeMode::parse("falsecolor"), Some(ProbeMode::FalseColor));
        assert_eq!(ProbeMode::parse("luma"), Some(ProbeMode::Luminance));
        assert_eq!(ProbeMode::parse("nope"), None);
        assert_eq!(ProbeMode::Off.shader_mode(), 0);
        assert_eq!(ProbeMode::FalseColor.shader_mode(), 1);
        assert_eq!(ProbeMode::Zebra.shader_mode(), 2);
        assert_eq!(ProbeMode::Luminance.shader_mode(), 3);
    }

    #[test]
    fn probe_request_activation_roundtrip() {
        let mut request = ProbeRequest::default();
        assert!(!request.is_active());
        request.activate(ProbeMode::Zebra);
        assert!(request.is_active());
        assert_eq!(request.mode, ProbeMode::Zebra);
        request.disable();
        assert!(!request.is_active());
    }

    #[test]
    fn probe_uniform_clamps_non_finite_and_out_of_range_inputs() {
        let request = ProbeRequest {
            mode: ProbeMode::FalseColor,
            strength: f32::NAN,
            middle_gray: 0.0,
            clip_stops: 99.0,
            shadow_stops: -99.0,
        };
        let probe = ExposureProbe::from_request(&request, f32::INFINITY);
        assert_eq!(probe.mode, 1);
        assert_eq!(probe.strength, 0.85, "non-finite strength falls back");
        assert!(probe.middle_gray >= 1e-4);
        assert_eq!(probe.clip_stops, 12.0);
        assert_eq!(probe.shadow_stops, -12.0);
        assert_eq!(probe.exposure_ev100, 13.0, "non-finite exposure falls back");
    }
}
