//! B01/B02 art foundation: the asset manifest/audit, the surface-material
//! catalog and the global style/scale reference consumed by the later
//! B/C/F/G/H/I workstreams.
//!
//! Files:
//! - [`manifest`]: catalog built from code references + filesystem audit
//!   (`--art-audit`, JSON under `target/art-audit/`).
//! - [`catalog`]: stable surface-material IDs, face roles, variant rules and
//!   the frozen block→face→material fingerprint.
//! - [`style`]: base palette, emission tiers and real scale anchors.
//!
//! See `art/style-guide.md` and `docs/art-overhaul/01-ART-DIRECTION-AND-ASSETS.md`.

pub mod catalog;
mod manifest;
pub mod pbr;
mod style;
mod texture_pipeline;

#[allow(unused_imports)]
pub use catalog::{
    FACE_COUNT, FACE_ROLES, MATERIAL_CATALOG_VERSION, MATERIAL_COUNT, MaterialCatalogReport,
    MaterialCatalogSnapshot, MaterialFamily, MaterialSnapshot, MetallicPolicy, SurfaceMaterial,
    SurfaceMaterialId, SurfaceTag, UvStrategy, catalog_report, catalog_snapshot, cross_material,
    face_fingerprint, face_material, face_role, id_fingerprint, material_by_id, material_by_key,
    material_id, material_key, variant_slot,
};
#[allow(unused_imports)]
pub use manifest::{
    ArtAsset, ArtAuditReport, ArtCategory, ArtRole, ArtSource, MANIFEST_SCHEMA_VERSION, audit,
    builtin_manifest, run_audit,
};
#[allow(unused_imports)]
pub use style::{
    ART_STYLE_VERSION, BASE_PALETTE, EMISSION_TIERS, EmissionTier, PaletteSwatch, SCALE_REFERENCES,
    ScaleReference,
};
#[allow(unused_imports)]
pub use texture_pipeline::{
    ALPHA_CUTOFF, ArrayPlan, AtlasPlan, BASE_GUTTER, ChannelClass, CoverageStats, EdgeMode,
    MaterialMipInfo, PIPELINE_VERSION, PaddedAtlas, RouteDecision, RouteParams, SourceImage,
    TextureAuditReport, TextureRoute, build_catalog_chains, build_chain, build_padded_atlas,
    contact_sheet_rgb, coverage_stats, decide_route, plan_array, plan_atlas,
    reference_route_params, run_texture_audit, texture_audit_report, verify_bleed,
    verify_normal_filter, write_bmp,
};

use std::path::{Path, PathBuf};

/// Where `--art-audit` reads from: an explicit `STARFORGE_ASSET_DIR` override,
/// otherwise the checked-in source tree in a development checkout (licenses and
/// gitignored packs live there), otherwise the runtime asset root.
///
/// Returns `(root, is_source_tree)`.
pub fn audit_root(runtime_root: &Path, override_active: bool) -> (PathBuf, bool) {
    if override_active {
        return (runtime_root.to_path_buf(), false);
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    if source.is_dir() {
        return (source, true);
    }
    (runtime_root.to_path_buf(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_over_source_tree() {
        let root = PathBuf::from("D:/cold-install/assets");
        let (resolved, is_source) = audit_root(&root, true);
        assert_eq!(resolved, root);
        assert!(!is_source);
    }

    #[test]
    fn development_checkout_audits_the_source_tree() {
        let runtime = PathBuf::from("target/debug/assets");
        let (resolved, is_source) = audit_root(&runtime, false);
        assert!(is_source, "source tree must win in a dev checkout");
        assert!(resolved.ends_with("assets"));
        assert!(resolved.is_dir());
    }
}
