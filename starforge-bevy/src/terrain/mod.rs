//! C01 terrain foundation: coverage diagnostics, deterministic input
//! signatures and the opt-in LOD/curvature/dirty debug overlay.
//!
//! Scope decisions:
//! - Near chunks, hierarchical LOD and the legacy far mesh are three separate
//!   owners. This module only *reports* their radii/state; it never changes the
//!   streaming or LOD selection logic (that is C03/C04/C06).
//! - The hierarchical LOD is a surface approximation: caves, bridges,
//!   overhangs and player towers are not covered by it. The report states this
//!   explicitly so later packages do not read coverage as volumetric.
//!
//! See `docs/art-overhaul/03-WORLD-AND-PROCEDURAL-BUILDINGS.md` (3.1/3.2) and
//! `docs/art-overhaul/05-EXECUTION-BACKLOG.md` (C01).

mod diag;
mod overlay;

#[allow(unused_imports)]
pub use diag::{
    CoverageBand, CurvatureCoverage, FarCoverage, LodCoverage, NearCoverage,
    TERRAIN_DIAGNOSTICS_SCHEMA_VERSION, TerrainDiagnostics, input_signature,
};
#[allow(unused_imports)]
pub use overlay::{TerrainOverlay, terrain_overlay_system};

use bevy::prelude::*;

use crate::schedule::{GameSet, GameState};

/// C01 plugin: diagnostics resource + opt-in overlay. Normal runs only pay for
/// the per-frame snapshot counters, not for any mesh/visibility change.
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainDiagnostics>()
            .init_resource::<TerrainOverlay>()
            .add_systems(
                Update,
                (
                    diag::collect_terrain_diagnostics.in_set(GameSet::LateScan),
                    overlay::terrain_overlay_system.in_set(GameSet::LateScan),
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
