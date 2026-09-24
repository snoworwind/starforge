//! D01 rendering groundwork: the pass/format inventory, the lighting probe
//! used by the QA variant matrix and the minimal HDR post-process prototype.
//!
//! The module is deliberately small. It records what the locked renderer
//! actually draws (`inventory`), gives the visual QA runner a way to switch
//! individual lighting/post steps (`LightingProbe`), and ships one real
//! fullscreen-material shader (`probe`) that runs on the HDR view target
//! before tonemapping. When the probe is off the scene renders exactly as
//! before; when a probe is requested on an unsupported device it stays off
//! with a recorded reason instead of drawing a black screen.
//!
//! See `docs/art-overhaul/02-RENDERING-LIGHTING-CLOUDS.md` (2.1/2.2) and
//! `docs/art-overhaul/05-EXECUTION-BACKLOG.md` (D01).

mod inventory;
mod probe;

#[allow(unused_imports)]
pub use inventory::{
    AaMatrix, CameraRecord, ColorStage, PASS_INVENTORY_SCHEMA_VERSION, PassInventory, PassStage,
};
#[allow(unused_imports)]
pub use probe::{ExposureProbe, PROBE_SHADER_PATH, ProbeMode, ProbeRequest, ProbeStatus};

use bevy::prelude::*;
use serde::Serialize;

use crate::schedule::GameSet;

/// One-effect-at-a-time switch for the D01 capture matrix. `Full` reproduces
/// the shipping look; the other modes let the QA runner store paired images
/// (sun only / ambient only) without touching `Settings` or the save file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum LightingProbeMode {
    #[default]
    Full,
    SunOnly,
    AmbientOnly,
    /// Diffuse sky-dominant light with the sun dimmed, used by the B04
    /// material review to judge roughness without a hard key light.
    Overcast,
}

impl LightingProbeMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::SunOnly => "sun_only",
            Self::AmbientOnly => "ambient_only",
            Self::Overcast => "overcast",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "sun" | "sun_only" | "direct" => Some(Self::SunOnly),
            "ambient" | "ambient_only" | "env" => Some(Self::AmbientOnly),
            "overcast" | "cloudy" => Some(Self::Overcast),
            _ => None,
        }
    }
}

/// Resource consumed by `daynight_system`. Default is the shipping look.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct LightingProbe {
    pub mode: LightingProbeMode,
}

impl LightingProbe {
    pub fn set(&mut self, mode: LightingProbeMode) {
        self.mode = mode;
    }
}

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProbeRequest>()
            .init_resource::<ProbeStatus>()
            .init_resource::<LightingProbe>()
            .add_plugins(bevy::core_pipeline::fullscreen_material::FullscreenMaterialPlugin::<
                ExposureProbe,
            >::default())
            .add_systems(
                Startup,
                (probe::load_probe_shader, probe::log_probe_ready).chain(),
            )
            .add_systems(PostStartup, inventory::collect_pass_inventory)
            .add_systems(
                Update,
                (probe::watch_probe_shader, probe::sync_exposure_probe)
                    .chain()
                    .in_set(GameSet::CameraFx),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lighting_probe_modes_parse_and_roundtrip() {
        for mode in [
            LightingProbeMode::Full,
            LightingProbeMode::SunOnly,
            LightingProbeMode::AmbientOnly,
            LightingProbeMode::Overcast,
        ] {
            assert_eq!(LightingProbeMode::parse(mode.key()), Some(mode));
        }
        assert_eq!(
            LightingProbeMode::parse(" DIRECT "),
            Some(LightingProbeMode::SunOnly)
        );
        assert_eq!(
            LightingProbeMode::parse("ambient"),
            Some(LightingProbeMode::AmbientOnly)
        );
        assert_eq!(
            LightingProbeMode::parse("cloudy"),
            Some(LightingProbeMode::Overcast)
        );
        assert_eq!(LightingProbeMode::parse("nope"), None);
        let mut probe = LightingProbe::default();
        assert_eq!(probe.mode, LightingProbeMode::Full);
        probe.set(LightingProbeMode::SunOnly);
        assert_eq!(probe.mode, LightingProbeMode::SunOnly);
    }
}
