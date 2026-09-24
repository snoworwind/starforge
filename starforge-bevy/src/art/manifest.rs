//! B01 asset manifest: the catalog is built from code references, then checked
//! against the filesystem (presence, exact path case, license files and
//! unreferenced runtime files). README/CREDITS text is provenance input, never
//! treated as the inventory of what the code actually loads.
//!
//! Run with `--art-audit`; the JSON report is written to
//! `target/art-audit/art-manifest.json`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::catalog::{MaterialCatalogReport, catalog_report, catalog_snapshot};
use super::style::ART_STYLE_VERSION;

/// Bumped when a change alters report fields. 2 adds the B02 `catalog` block
/// (additive; older consumers can ignore it).
pub const MANIFEST_SCHEMA_VERSION: u32 = 2;
const KENNEY_LICENSE: &str = "licenses/kenney_space-kit_LICENSE.txt";
const QUATERNIUS_LICENSE: &str = "licenses/quaternius_ultimate_animated_animals_LICENSE.txt";
/// KayKit, Poly Pizza and other CC0/CC-BY credits live in the repository
/// CREDITS.md; the special value resolves next to the asset root's parent.
const CREDITS_FILE: &str = "CREDITS.md";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtCategory {
    Character,
    Creature,
    Ship,
    Station,
    Planet,
    Asteroid,
    Shader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtSource {
    KayKit,
    Kenney,
    Quaternius,
    PolyPizza,
    Sketchfab,
    Project,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtRole {
    /// Referenced by a live code path and shipped with the repository.
    Live,
    /// Shipped and intentionally kept as a fallback/reserve, not instantiated
    /// by the current code (e.g. the CC0 Kenney ships while external packs are
    /// the live path). Tracked so B06/I03 can wire or delete them knowingly.
    Reserve,
    /// Not shipped in git; missing is expected and must not fail the audit.
    OptionalDownload,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArtAsset {
    pub key: String,
    pub category: ArtCategory,
    pub path: String,
    pub source: ArtSource,
    pub license: String,
    /// Relative to the asset root; `CREDITS.md` resolves one level up.
    pub license_file: String,
    pub role: ArtRole,
    pub style_version: u32,
    pub notes: String,
}

fn asset(
    category: ArtCategory,
    path: &str,
    source: ArtSource,
    license: &str,
    license_file: &str,
    role: ArtRole,
    notes: &str,
) -> ArtAsset {
    let file = path.rsplit('/').next().unwrap_or(path);
    let mut stem = file.split('.').next().unwrap_or(path);
    // Sketchfab exports use `scene.gltf`; identify those by their directory.
    if stem == "scene"
        && let Some(parent) = path.rsplit('/').nth(1)
    {
        stem = parent;
    }
    ArtAsset {
        key: format!("{:?}.{stem}", category).to_lowercase(),
        category,
        path: path.to_string(),
        source,
        license: license.to_string(),
        license_file: license_file.to_string(),
        role,
        style_version: ART_STYLE_VERSION,
        notes: notes.to_string(),
    }
}

/// Catalog built from the same constants the runtime loads. Paths are keyed,
/// then deduplicated: several creature kinds share one model file.
pub fn builtin_manifest() -> Vec<ArtAsset> {
    let mut out: Vec<ArtAsset> = Vec::new();
    let mut push = |candidate: ArtAsset| {
        if !out.iter().any(|existing| existing.path == candidate.path) {
            out.push(candidate);
        }
    };

    for path in crate::char::RESERVED_NPC_MODELS {
        let (source, license_file, notes) = if path.contains("adventurer_") {
            (
                ArtSource::KayKit,
                CREDITS_FILE,
                "KayKit Character Pack: Adventurers, CC0; idle clip 36",
            )
        } else {
            (
                ArtSource::Kenney,
                KENNEY_LICENSE,
                "Kenney Space Kit astronaut/alien, CC0",
            )
        };
        push(asset(
            ArtCategory::Character,
            path,
            source,
            "CC0-1.0",
            license_file,
            ArtRole::Reserve,
            &format!("{notes}; retired from runtime NPCs in favor of the original voxel rig"),
        ));
    }

    for kind in ["strider", "hopper", "crab", "beetle", "manta", "blob"] {
        let (path, _, _) = crate::creatures::creature_model(kind);
        push(asset(
            ArtCategory::Creature,
            path,
            ArtSource::Quaternius,
            "CC0-1.0",
            QUATERNIUS_LICENSE,
            ArtRole::Live,
            "Quaternius Ultimate Animated Animals; skinned idle/walk clips",
        ));
    }
    push(asset(
        ArtCategory::Creature,
        "models/creatures/sentinel.glb",
        ArtSource::KayKit,
        "CC0-1.0",
        CREDITS_FILE,
        ArtRole::Live,
        "KayKit Skeletons Skeleton_Warrior; scale 1.9/2.17",
    ));
    for path in [
        "models/creatures/crab.glb",
        "models/creatures/blob.glb",
        "models/creatures/strider.glb",
    ] {
        push(asset(
            ArtCategory::Creature,
            path,
            ArtSource::PolyPizza,
            "CC-BY-3.0",
            CREDITS_FILE,
            ArtRole::Reserve,
            "Legacy Poly Pizza creature kept in git; no longer the default passive model, requires attribution if ever shipped",
        ));
    }

    for path in [
        "models/asteroids/meteor.glb",
        "models/asteroids/meteor_detailed.glb",
    ] {
        push(asset(
            ArtCategory::Asteroid,
            path,
            ArtSource::Kenney,
            "CC0-1.0",
            KENNEY_LICENSE,
            ArtRole::Live,
            "Kenney Space Kit meteor; scaled by space.rs",
        ));
    }

    // External CC-BY downloads: the live ship/station path (see CREDITS.md).
    let external = [
        (
            ArtCategory::Ship,
            "models/external/ships/space_ship_b/scene.gltf",
            "yanix",
            "B class",
        ),
        (
            ArtCategory::Ship,
            "models/external/ships/space_ship_c/scene.gltf",
            "Comrade1280",
            "C class",
        ),
        (
            ArtCategory::Ship,
            "models/external/ships/space_ship_torb/scene.gltf",
            "tramkar",
            "ship_striker",
        ),
        (
            ArtCategory::Ship,
            "models/external/ships/supermatic_sky_cruiser/scene.gltf",
            "VertaScan",
            "A class",
        ),
        (
            ArtCategory::Ship,
            "models/external/ships/unsa_destroyer/scene.gltf",
            "xaxary",
            "S class",
        ),
        (
            ArtCategory::Station,
            "models/external/stations/space_station/scene.gltf",
            "re1monsen",
            "home galaxy station",
        ),
        (
            ArtCategory::Station,
            "models/external/stations/space_station_3/scene.gltf",
            "re1monsen",
            "galaxy station",
        ),
        (
            ArtCategory::Station,
            "models/external/stations/space_station_4/scene.gltf",
            "re1monsen",
            "galaxy station",
        ),
        (
            ArtCategory::Station,
            "models/external/stations/helveta/scene.gltf",
            "Inditrion Dradnon",
            "large galaxy station",
        ),
        (
            ArtCategory::Planet,
            "models/earth/scene.gltf",
            "SebastianSosnowski",
            "origin star Earth shell",
        ),
    ];
    for (category, path, author, note) in external {
        let license_file = format!("{}license.txt", &path[..path.rfind('/').unwrap_or(0) + 1]);
        push(asset(
            category,
            path,
            ArtSource::Sketchfab,
            "CC-BY-4.0",
            &license_file,
            ArtRole::OptionalDownload,
            &format!("{note}; author {author}; download per README 外部模型下载"),
        ));
    }

    // CC0 Kenney reserve: shipped, mapped by helper functions, not currently
    // instantiated (external packs are the live path).
    for class in ["C", "B", "A", "S"] {
        push(asset(
            ArtCategory::Ship,
            crate::space::ship_model_for(class),
            ArtSource::Kenney,
            "CC0-1.0",
            KENNEY_LICENSE,
            ArtRole::Reserve,
            "Kenney Space Kit reserve for the minimal-asset ship fallback",
        ));
    }
    for index in 0..4 {
        push(asset(
            ArtCategory::Ship,
            crate::space::visitor_model_for(index),
            ArtSource::Kenney,
            "CC0-1.0",
            KENNEY_LICENSE,
            ArtRole::Reserve,
            "Kenney Space Kit visitor reserve (external path is live)",
        ));
    }

    for (key, path, note) in [
        (
            "shader.planet_curvature",
            "shaders/planet_curvature.wgsl",
            "shared main-pass curvature",
        ),
        (
            "shader.planet_curvature_prepass",
            "shaders/planet_curvature_prepass.wgsl",
            "prepass counterpart",
        ),
        (
            "shader.cloud_shell",
            "shaders/cloud_shell.wgsl",
            "volumetric cloud shell",
        ),
        (
            "shader.exposure_probe",
            "shaders/visual/exposure_probe.wgsl",
            "D01 exposure/clipping fullscreen probe",
        ),
    ] {
        let mut candidate = asset(
            ArtCategory::Shader,
            path,
            ArtSource::Project,
            "MIT",
            "",
            ArtRole::Live,
            note,
        );
        candidate.key = key.to_string();
        push(candidate);
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArtAuditReport {
    pub schema_version: u32,
    pub style_version: u32,
    pub root: String,
    pub source_tree: bool,
    pub assets: Vec<ArtAsset>,
    pub live: usize,
    pub reserve: usize,
    pub optional: usize,
    pub optional_present: usize,
    /// Live/Reserve files that are absent even though git should ship them.
    pub missing_required: Vec<String>,
    /// OptionalDownload files that were not installed (expected).
    pub optional_missing: Vec<String>,
    /// Files that exist but do not match the manifest's exact path case.
    pub case_mismatches: Vec<String>,
    /// Manifest entries whose license file cannot be found.
    pub license_gaps: Vec<String>,
    /// `.glb`/`.gltf`/`.wgsl` files under `models/`+`shaders/` not in the
    /// manifest (dead or unregistered assets).
    pub unreferenced: Vec<String>,
    /// B02 surface-material coverage and the frozen block→face fingerprints.
    pub catalog: MaterialCatalogReport,
}

impl ArtAuditReport {
    /// Hard failures: shipped assets absent, path case that will break on a
    /// case-sensitive package, or a block/biome/painter that the material
    /// catalog cannot resolve. License gaps and unreferenced files are
    /// reported but do not fail the audit.
    pub fn failure_count(&self) -> usize {
        self.missing_required.len() + self.case_mismatches.len() + self.catalog.failure_count()
    }
}

fn license_path(root: &Path, license_file: &str) -> Option<PathBuf> {
    if license_file.is_empty() {
        return None;
    }
    if license_file == CREDITS_FILE {
        return root.parent().map(|parent| parent.join(CREDITS_FILE));
    }
    Some(root.join(license_file))
}

/// Exact-case lookup: `Path::is_file` is case-insensitive on Windows, so walk
/// each segment and compare the real directory entry names as strings.
fn exact_case_file(root: &Path, relative: &str) -> bool {
    let mut current = root.to_path_buf();
    for segment in relative.split('/') {
        let Ok(entries) = std::fs::read_dir(&current) else {
            return false;
        };
        let mut next = None;
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy() == segment {
                next = Some(entry.path());
                break;
            }
        }
        match next {
            Some(path) => current = path,
            None => return false,
        }
    }
    current.is_file()
}

fn runtime_source_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".glb") || lower.ends_with(".gltf") || lower.ends_with(".wgsl")
}

fn collect_unreferenced(
    root: &Path,
    directory: &Path,
    referenced: &HashSet<String>,
    out: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            collect_unreferenced(root, &path, referenced, out);
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !runtime_source_file(&name) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if !referenced.contains(&relative) {
            out.push(relative);
        }
    }
}

pub fn audit(root: &Path, source_tree: bool) -> ArtAuditReport {
    let assets = builtin_manifest();
    let referenced: HashSet<String> = assets.iter().map(|item| item.path.clone()).collect();
    let mut report = ArtAuditReport {
        schema_version: MANIFEST_SCHEMA_VERSION,
        style_version: ART_STYLE_VERSION,
        root: root.display().to_string(),
        source_tree,
        live: 0,
        reserve: 0,
        optional: 0,
        optional_present: 0,
        missing_required: Vec::new(),
        optional_missing: Vec::new(),
        case_mismatches: Vec::new(),
        license_gaps: Vec::new(),
        unreferenced: Vec::new(),
        catalog: catalog_report(),
        assets,
    };

    for item in &report.assets {
        let full = root.join(&item.path);
        let present = full.is_file();
        match item.role {
            ArtRole::Live => report.live += 1,
            ArtRole::Reserve => report.reserve += 1,
            ArtRole::OptionalDownload => {
                report.optional += 1;
                if present {
                    report.optional_present += 1;
                } else {
                    report.optional_missing.push(item.path.clone());
                }
            }
        }
        if present && !exact_case_file(root, &item.path) {
            report.case_mismatches.push(item.path.clone());
        }
        if !present && item.role != ArtRole::OptionalDownload {
            report.missing_required.push(item.path.clone());
        }
        // License files are only required when the asset itself is installed.
        let license_required = item.source != ArtSource::Project
            && (present || item.role != ArtRole::OptionalDownload);
        if license_required {
            let found = license_path(root, &item.license_file)
                .map(|path| path.is_file())
                .unwrap_or(false);
            if !found {
                report
                    .license_gaps
                    .push(format!("{} -> {}", item.key, item.license_file));
            }
        }
    }

    for directory in ["models", "shaders"] {
        collect_unreferenced(
            root,
            &root.join(directory),
            &referenced,
            &mut report.unreferenced,
        );
    }
    report.unreferenced.sort();

    report
}

/// CLI entry used by `--art-audit`. Writes the JSON report and returns the
/// process exit code (2 when shipped assets are missing or mis-cased).
pub fn run_audit(root: &Path, source_tree: bool) -> i32 {
    let report = audit(root, source_tree);
    let out_dir = PathBuf::from("target").join("art-audit");
    if let Err(err) = std::fs::create_dir_all(&out_dir) {
        eprintln!("art-audit: 无法创建 {}: {err}", out_dir.display());
    }
    let out_file = out_dir.join("art-manifest.json");
    match serde_json::to_string_pretty(&report) {
        Ok(json) => {
            if let Err(err) = std::fs::write(&out_file, json) {
                eprintln!("art-audit: 写入 {} 失败: {err}", out_file.display());
            }
        }
        Err(err) => eprintln!("art-audit: 序列化报告失败: {err}"),
    }
    let catalog_file = out_dir.join("material-catalog.json");
    match serde_json::to_string_pretty(&catalog_snapshot()) {
        Ok(json) => {
            if let Err(err) = std::fs::write(&catalog_file, json) {
                eprintln!("art-audit: 写入 {} 失败: {err}", catalog_file.display());
            }
        }
        Err(err) => eprintln!("art-audit: 序列化材质目录失败: {err}"),
    }
    println!(
        "ART_AUDIT root={} source_tree={} assets={} live={} reserve={} optional={}/{} missing_required={} case_mismatch={} license_gaps={} unreferenced={}",
        report.root,
        report.source_tree,
        report.assets.len(),
        report.live,
        report.reserve,
        report.optional_present,
        report.optional,
        report.missing_required.len(),
        report.case_mismatches.len(),
        report.license_gaps.len(),
        report.unreferenced.len(),
    );
    let catalog = &report.catalog;
    println!(
        "ART_AUDIT_CATALOG version={} materials={} blocks={} bindings={} unknown_tiles={} unknown_ground={} unregistered_painters={} unregistered_materials={} unused={} id_fp={:016x} face_fp={:016x}",
        catalog.catalog_version,
        catalog.materials,
        catalog.blocks,
        catalog.face_bindings,
        catalog.unknown_block_tiles.len(),
        catalog.unknown_ground_keys.len(),
        catalog.unregistered_painters.len(),
        catalog.unregistered_materials.len(),
        catalog.unused_materials.len(),
        catalog.id_fingerprint,
        catalog.face_fingerprint,
    );
    for file in &catalog.unknown_block_tiles {
        println!("ART_AUDIT_UNKNOWN_TILE {file}");
    }
    for key in &catalog.unknown_ground_keys {
        println!("ART_AUDIT_UNKNOWN_GROUND {key}");
    }
    for key in &catalog.unregistered_painters {
        println!("ART_AUDIT_UNREGISTERED_PAINTER {key}");
    }
    for key in &catalog.unregistered_materials {
        println!("ART_AUDIT_UNREGISTERED_MATERIAL {key}");
    }
    for key in &catalog.unused_materials {
        println!("ART_AUDIT_UNUSED_MATERIAL {key}");
    }
    for gap in &report.license_gaps {
        println!("ART_AUDIT_LICENSE_GAP {gap}");
    }
    for file in &report.unreferenced {
        println!("ART_AUDIT_UNREFERENCED {file}");
    }
    for file in &report.missing_required {
        println!("ART_AUDIT_MISSING {file}");
    }
    for file in &report.case_mismatches {
        println!("ART_AUDIT_CASE {file}");
    }
    println!("ART_AUDIT report={}", out_file.display());
    println!("ART_AUDIT_CATALOG report={}", catalog_file.display());
    if report.failure_count() == 0 {
        println!("ART_AUDIT_OK");
        0
    } else {
        println!("ART_AUDIT_FAIL failures={}", report.failure_count());
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_by_path(path: &str) -> Option<ArtAsset> {
        builtin_manifest()
            .into_iter()
            .find(|item| item.path == path)
    }

    #[test]
    fn manifest_keys_are_unique_and_paths_nonempty() {
        let mut keys = HashSet::new();
        for item in builtin_manifest() {
            assert!(keys.insert(item.key.clone()), "duplicate key {}", item.key);
            assert!(!item.path.is_empty());
            assert!(item.path.starts_with("models/") || item.path.starts_with("shaders/"));
        }
    }

    #[test]
    fn code_referenced_models_are_catalogued() {
        for path in crate::char::RESERVED_NPC_MODELS {
            assert!(
                manifest_by_path(path).is_some(),
                "NPC model not in manifest: {path}"
            );
        }
        for kind in ["strider", "hopper", "crab", "beetle", "manta", "blob"] {
            let (path, _, _) = crate::creatures::creature_model(kind);
            assert!(
                manifest_by_path(path).is_some(),
                "creature model not in manifest: {path}"
            );
        }
        assert!(manifest_by_path("models/creatures/sentinel.glb").is_some());
        assert!(manifest_by_path("models/asteroids/meteor.glb").is_some());
    }

    #[test]
    fn third_party_assets_declare_license_and_provenance() {
        for item in builtin_manifest() {
            if item.source == ArtSource::Project {
                continue;
            }
            assert!(!item.license.is_empty(), "{}", item.key);
            assert!(!item.license_file.is_empty(), "{}", item.key);
            assert!(!item.notes.is_empty(), "{}", item.key);
        }
    }

    #[test]
    fn source_tree_catalog_is_present_on_disk() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let report = audit(&root, true);
        assert!(
            report.missing_required.is_empty(),
            "shipped assets missing from source tree: {:?}",
            report.missing_required
        );
        assert!(
            report.case_mismatches.is_empty(),
            "path case mismatches: {:?}",
            report.case_mismatches
        );
        assert!(
            report.license_gaps.is_empty(),
            "license paths drifted: {:?}",
            report.license_gaps
        );
    }

    #[test]
    fn audit_embeds_b02_material_catalog() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let report = audit(&root, true);
        assert_eq!(report.schema_version, MANIFEST_SCHEMA_VERSION);
        assert_eq!(
            report.catalog.materials,
            crate::art::catalog::MATERIAL_COUNT
        );
        assert_eq!(report.catalog.failure_count(), 0, "{:?}", report.catalog);
        // Catalog failures must surface through the audit's single failure
        // counter, or ART_AUDIT_FAIL would hide a broken block→tile mapping.
        let mut broken = report.clone();
        broken
            .catalog
            .unknown_block_tiles
            .push("dead_tile".to_string());
        assert!(broken.failure_count() > report.failure_count());
    }

    fn write_fixture_file(root: &Path, relative: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, b"x").expect("write");
    }

    #[test]
    fn audit_classifies_missing_optional_and_unreferenced() {
        let root = std::env::temp_dir().join(format!("starforge-b01-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mkdir");
        write_fixture_file(root.parent().expect("parent"), "CREDITS.md");
        for item in builtin_manifest() {
            if item.role != ArtRole::OptionalDownload {
                write_fixture_file(&root, &item.path);
            }
            if !item.license_file.is_empty() && item.license_file != CREDITS_FILE {
                write_fixture_file(&root, &item.license_file);
            }
        }
        write_fixture_file(&root, "models/dead_stray.glb");

        let report = audit(&root, false);
        assert_eq!(report.failure_count(), 0, "{report:?}");
        assert_eq!(report.optional_present, 0);
        assert_eq!(report.optional_missing.len(), report.optional);
        assert!(report.license_gaps.is_empty(), "{:?}", report.license_gaps);
        assert!(
            report
                .unreferenced
                .contains(&"models/dead_stray.glb".to_string())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn audit_flags_wrong_case_on_case_insensitive_filesystems() {
        let root = std::env::temp_dir().join(format!("starforge-b01-case-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mkdir");
        let wrong = root.join("models").join("Creatures");
        std::fs::create_dir_all(&wrong).expect("mkdir");
        std::fs::write(wrong.join("sentinel.glb"), b"x").expect("write");
        let report = audit(&root, false);
        assert!(
            report
                .case_mismatches
                .iter()
                .any(|path| path == "models/creatures/sentinel.glb"),
            "{:?}",
            report.case_mismatches
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
