//! A01 visual QA harness: deterministic scenes, manifests and frame-time
//! metrics for the art overhaul.
//!
//! Enable with `--visual-qa`. The mode boots a fixed world, builds the S01
//! courtyard, S02 room and S06 route scenes (plus the opt-in B01 calibration
//! rig and D01 exposure/pass matrix) from [`scene`], locks camera and day
//! clock, saves raw frames under `target/visual-qa/<commit>/<scene>/<run>/`
//! and writes `manifest.json`/`metrics.json` next to them. It never writes user
//! saves or `settings.json`.
//!
//! See `docs/art-overhaul/05-EXECUTION-BACKLOG.md` (A01) and
//! `docs/art-overhaul/06-VALIDATION-AND-RISK-REGISTER.md` (6.1/6.8).

mod baseline;
mod scene;

use std::path::{Path, PathBuf};

use bevy::camera::Exposure;
use bevy::diagnostic::SystemInfo;
use bevy::pbr::{ContactShadows, ScreenSpaceAmbientOcclusion};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{PrimaryWindow, Window};
use serde::Serialize;

use crate::daynight::DayTime;
use crate::player::Player;
use crate::rendering::{
    LightingProbe, LightingProbeMode, PassInventory, ProbeMode, ProbeRequest, ProbeStatus,
};
use crate::save::Settings;
use crate::schedule::{GameSet, GameState};
use crate::terrain::TerrainDiagnostics;
use crate::textures::AtlasRes;
use crate::visual::{
    RenderCapabilities, ResolvedQuality, VisualDiagnostics, VisualLifecycleDiagnostics,
};
use crate::weather::{CloudDebug, CloudDebugMode, CloudDiagnostics};
use crate::world::{ChunkMesh, World};

pub use scene::{BuiltScene, SCENE_VERSION, SceneId, ScenePose};

const MANIFEST_SCHEMA_VERSION: u32 = 5;
/// Frames between two capture poses of a staged scene.
const POSE_GAP_FRAMES: u32 = 15;
const ROUTE_CAPTURE_FRACTIONS: [f32; 3] = [0.1, 0.5, 0.9];
const ROUTE_FOV: f32 = 70.0;
/// Frames kept alive after the last capture so the async screenshot writer can
/// flush before the process exits.
const DRAIN_FRAMES: u32 = 15;
const MAX_MEASURED_FRAMES: usize = 20_000;

/// D01 pass-matrix variant. Each value maps to a real camera/lighting state;
/// `full` restores the shipping look and is also the restore state when a D01
/// scene finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PassVariant {
    Full,
    NoPost,
    SunOnly,
    AmbientOnly,
    ProbeStops,
    ProbeZebra,
    /// B04 overcast review light (dim sun, lifted sky fill).
    Overcast,
}

impl PassVariant {
    fn key(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::NoPost => "no_post",
            Self::SunOnly => "sun_only",
            Self::AmbientOnly => "ambient_only",
            Self::ProbeStops => "probe_stops",
            Self::ProbeZebra => "probe_zebra",
            Self::Overcast => "overcast",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "no_post" | "nopost" => Some(Self::NoPost),
            "sun_only" | "sun" | "direct" => Some(Self::SunOnly),
            "ambient_only" | "ambient" => Some(Self::AmbientOnly),
            "probe_stops" | "probe" => Some(Self::ProbeStops),
            "probe_zebra" => Some(Self::ProbeZebra),
            "overcast" | "cloudy" => Some(Self::Overcast),
            _ => None,
        }
    }
}

/// Camera state saved before a D01 variant removes components, so `full` can
/// restore exactly what the app spawned instead of a second default copy.
#[derive(Clone)]
struct SavedPostFx {
    bloom: Option<Bloom>,
    ssao: Option<ScreenSpaceAmbientOcclusion>,
    contact: Option<ContactShadows>,
}

/// Precise scene-linear PBR cards for the D01 gray-card row. Built once per
/// `Playing` entry through the real `Assets<StandardMaterial>` pipeline.
#[derive(Resource)]
struct ProbeCards {
    mesh: Handle<Mesh>,
    materials: Vec<Handle<StandardMaterial>>,
}

impl ProbeCards {
    fn material(&self, index: u8) -> Handle<StandardMaterial> {
        self.materials
            .get(index as usize)
            .or_else(|| self.materials.first())
            .cloned()
            .unwrap_or_default()
    }
}

fn build_probe_cards(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> ProbeCards {
    let mesh = meshes.add(Cuboid::new(1.6, 2.2, 0.18));
    let mut card = |base: LinearRgba, roughness: f32, metallic: f32, emissive: LinearRgba| {
        materials.add(StandardMaterial {
            base_color: Color::LinearRgba(base),
            perceptual_roughness: roughness,
            metallic,
            emissive,
            ..default()
        })
    };
    let cards = vec![
        // 0 black, 1 18% gray, 2 50% gray, 3 90% white, 4 overbright emissive,
        // 5 metal (needs the environment reflection to read), 6 saturated color.
        card(
            LinearRgba::rgb(0.03, 0.03, 0.03),
            0.90,
            0.0,
            LinearRgba::BLACK,
        ),
        card(
            LinearRgba::rgb(0.18, 0.18, 0.18),
            0.90,
            0.0,
            LinearRgba::BLACK,
        ),
        card(
            LinearRgba::rgb(0.50, 0.50, 0.50),
            0.85,
            0.0,
            LinearRgba::BLACK,
        ),
        card(
            LinearRgba::rgb(0.90, 0.90, 0.90),
            0.85,
            0.0,
            LinearRgba::BLACK,
        ),
        card(
            LinearRgba::rgb(0.02, 0.02, 0.02),
            0.50,
            0.0,
            LinearRgba::rgb(5.0, 4.5, 3.5),
        ),
        card(
            LinearRgba::rgb(0.85, 0.85, 0.85),
            0.22,
            1.0,
            LinearRgba::BLACK,
        ),
        card(
            LinearRgba::rgb(0.08, 0.50, 0.65),
            0.60,
            0.0,
            LinearRgba::BLACK,
        ),
    ];
    ProbeCards {
        mesh,
        materials: cards,
    }
}

/// B04: one `StandardMaterial` per `MaterialFamily`, built from the family
/// representative tile plus generated normal/ORM/emission maps. All shape
/// meshes carry vertex tangents so the normal maps actually apply.
#[derive(Resource)]
struct FamilyMaterials {
    shapes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
}

impl FamilyMaterials {
    fn shape(&self, index: u8) -> Option<Handle<Mesh>> {
        self.shapes.get(index as usize).cloned()
    }

    fn material(&self, family: u8) -> Option<Handle<StandardMaterial>> {
        self.materials.get(family as usize).cloned()
    }
}

/// Per-shape pivot offset and a pose rotation that keeps the silhouette
/// readable from the courtyard camera.
fn family_shape_transform(shape: u8, pos: Vec3, yaw: f32) -> Transform {
    let (offset, rotation) = match shape {
        0 => (Vec3::Y * 0.75, Quat::IDENTITY), // sphere r=0.75
        1 => (Vec3::Y * 0.60, Quat::IDENTITY), // cube 1.2
        2 => (Vec3::Y * 0.45, Quat::from_rotation_z(0.6)), // ramp
        3 => (Vec3::Y * 0.65, Quat::from_rotation_y(0.15)), // thin sheet
        4 => (
            Vec3::Y * 0.28,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ), // interior prop (torus lying flat)
        _ => (Vec3::ZERO, Quat::IDENTITY),
    };
    Transform::from_translation(pos + offset).with_rotation(Quat::from_rotation_y(yaw) * rotation)
}

fn tagged_mesh(meshes: &mut Assets<Mesh>, mut mesh: Mesh) -> Handle<Mesh> {
    let _ = mesh.generate_tangents();
    meshes.add(mesh)
}

fn build_family_materials(
    atlas: &crate::textures::Atlas,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> FamilyMaterials {
    use crate::art::SourceImage;
    use crate::art::catalog::MaterialFamily;
    use crate::art::pbr;
    let shapes = vec![
        tagged_mesh(meshes, Mesh::from(Sphere::new(0.75))),
        tagged_mesh(meshes, Mesh::from(Cuboid::new(1.2, 1.2, 1.2))),
        tagged_mesh(meshes, Mesh::from(Cuboid::new(1.6, 0.22, 1.1))),
        tagged_mesh(meshes, Mesh::from(Cuboid::new(1.3, 1.3, 0.07))),
        tagged_mesh(meshes, Mesh::from(Torus::new(0.26, 0.58))),
    ];
    let mut prototypes = Vec::new();
    for family in MaterialFamily::ALL {
        let id = pbr::family_representative(family);
        let Some(material) = crate::art::material_by_id(id) else {
            continue;
        };
        let Some(tile) = atlas.tile_material(id) else {
            continue;
        };
        let pbr = pbr::surface_pbr(material);
        let albedo = images.add(pbr::image_from_rgba(
            16,
            SourceImage::from_tile(tile).pixels,
            true,
        ));
        let normal = images.add(pbr::image_from_rgba(
            16,
            pbr::build_normal_map(tile, pbr.normal_strength),
            false,
        ));
        let orm = images.add(pbr::image_from_rgba(
            16,
            pbr::build_orm_map(tile, &pbr),
            false,
        ));
        let mut prototype = StandardMaterial {
            base_color_texture: Some(albedo),
            normal_map_texture: Some(normal),
            metallic_roughness_texture: Some(orm),
            metallic: 1.0,
            perceptual_roughness: 1.0,
            ..default()
        };
        if let Some(tint) = pbr::emission_tint(&pbr) {
            prototype.emissive_texture = Some(images.add(pbr::image_from_rgba(
                16,
                pbr::build_emission_map(tile, &pbr),
                false,
            )));
            prototype.emissive = tint;
        }
        match pbr.alpha {
            pbr::PbrAlpha::Opaque => {}
            pbr::PbrAlpha::Cutout => {
                prototype.alpha_mode = AlphaMode::Mask(0.4);
                prototype.double_sided = true;
                prototype.cull_mode = None;
            }
            pbr::PbrAlpha::Blend => {
                prototype.alpha_mode = AlphaMode::Blend;
                prototype.double_sided = true;
                prototype.cull_mode = None;
            }
        }
        prototypes.push(materials.add(prototype));
    }
    FamilyMaterials {
        shapes,
        materials: prototypes,
    }
}

/// D01: apply one matrix variant through the live components/resources. Only
/// the QA runner calls this, and every mode has a matching restore path.
fn apply_pass_variant(
    commands: &mut Commands,
    entity: Entity,
    variant: PassVariant,
    saved: &SavedPostFx,
    probe: &mut ProbeRequest,
    lighting: &mut LightingProbe,
) {
    if variant == PassVariant::NoPost {
        commands
            .entity(entity)
            .remove::<Bloom>()
            .remove::<ScreenSpaceAmbientOcclusion>()
            .remove::<ContactShadows>();
    } else {
        if let Some(bloom) = &saved.bloom {
            commands.entity(entity).insert(bloom.clone());
        }
        if let Some(ssao) = &saved.ssao {
            commands.entity(entity).insert(ssao.clone());
        }
        if let Some(contact) = &saved.contact {
            commands.entity(entity).insert(*contact);
        }
    }
    match variant {
        PassVariant::SunOnly => {
            lighting.set(LightingProbeMode::SunOnly);
            probe.disable();
        }
        PassVariant::AmbientOnly => {
            lighting.set(LightingProbeMode::AmbientOnly);
            probe.disable();
        }
        PassVariant::ProbeStops => {
            lighting.set(LightingProbeMode::Full);
            probe.activate(ProbeMode::FalseColor);
        }
        PassVariant::ProbeZebra => {
            lighting.set(LightingProbeMode::Full);
            probe.activate(ProbeMode::Zebra);
        }
        PassVariant::Overcast => {
            lighting.set(LightingProbeMode::Overcast);
            probe.disable();
        }
        PassVariant::Full | PassVariant::NoPost => {
            lighting.set(LightingProbeMode::Full);
            probe.disable();
        }
    }
}

type QaCameraQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Transform,
        &'static mut Projection,
        Option<&'static Exposure>,
        Option<&'static Msaa>,
        Option<&'static Bloom>,
        Option<&'static ScreenSpaceAmbientOcclusion>,
        Option<&'static ContactShadows>,
    ),
    With<Camera3d>,
>;

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

// ---------- Configuration ----------

#[derive(Clone, Debug, Resource)]
pub struct VisualQaConfig {
    pub seed: u32,
    pub biome: String,
    pub day_time: f32,
    pub output_root: PathBuf,
    pub run_label: String,
    pub scenes: Vec<SceneId>,
    pub route_frames: u32,
    pub capture: bool,
    pub keep_open: bool,
}

impl Default for VisualQaConfig {
    fn default() -> Self {
        Self {
            seed: 11,
            biome: "lush".to_string(),
            // `DayTime`: 0.25 = 06:00 and 0.5 = noon (see `daynight::day_factor`).
            day_time: 0.5,
            output_root: PathBuf::from("target").join("visual-qa"),
            run_label: format!("run-{}", unix_seconds()),
            scenes: SceneId::DEFAULT_RUN.to_vec(),
            route_frames: 900,
            capture: true,
            keep_open: false,
        }
    }
}

impl VisualQaConfig {
    pub fn from_args() -> Option<Self> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        Self::parse(&args)
    }

    /// Pure parser so the flag surface can be unit tested without an `App`.
    /// Unknown arguments are ignored, which lets `--visual-qa` combine with
    /// the existing `--clouds-off`/`--legacy-lod` probes.
    pub fn parse(args: &[String]) -> Option<Self> {
        let mut config = Self::default();
        let mut enabled = false;
        let mut scene_list: Vec<SceneId> = Vec::new();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--visual-qa" => enabled = true,
                "--visual-qa-no-capture" => config.capture = false,
                "--visual-qa-keep-open" => config.keep_open = true,
                flag @ ("--visual-qa-seed"
                | "--visual-qa-biome"
                | "--visual-qa-day"
                | "--visual-qa-out"
                | "--visual-qa-run"
                | "--visual-qa-scene"
                | "--visual-qa-route-frames") => {
                    index += 1;
                    let value = args.get(index).cloned().unwrap_or_default();
                    match flag {
                        "--visual-qa-seed" => match value.parse() {
                            Ok(seed) => config.seed = seed,
                            Err(_) => warn_arg(flag, &value),
                        },
                        "--visual-qa-biome" => {
                            if crate::data::BIOMES.iter().any(|b| b.key == value) {
                                config.biome = value;
                            } else {
                                warn_arg(flag, &value);
                            }
                        }
                        "--visual-qa-day" => match value.parse::<f32>() {
                            Ok(day) if day.is_finite() => config.day_time = day.rem_euclid(1.0),
                            _ => warn_arg(flag, &value),
                        },
                        "--visual-qa-out" => config.output_root = PathBuf::from(value),
                        "--visual-qa-run" => {
                            if !value.is_empty() {
                                config.run_label = value;
                            }
                        }
                        "--visual-qa-route-frames" => match value.parse() {
                            Ok(frames) => config.route_frames = frames,
                            Err(_) => warn_arg(flag, &value),
                        },
                        "--visual-qa-scene" => {
                            for key in value.split([',', ';', ' ']) {
                                if let Some(id) = SceneId::from_key(key) {
                                    if !scene_list.contains(&id) {
                                        scene_list.push(id);
                                    }
                                } else if !key.trim().is_empty() {
                                    warn_arg(flag, key);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            index += 1;
        }
        if !enabled {
            return None;
        }
        if !scene_list.is_empty() {
            config.scenes = scene_list;
        }
        if config.route_frames == 0 {
            config.route_frames = 1;
        }
        Some(config)
    }
}

fn warn_arg(flag: &str, value: &str) {
    eprintln!("visual-qa: {flag} 的值 `{value}` 无效，已保留默认值");
}

// ---------- Run state ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Build,
    Wait,
    Poses,
    Route,
    Drain,
    Finished,
}

#[derive(Clone, Serialize)]
struct CaptureRecord {
    pose: String,
    frame: u32,
    cloud_age_s: f32,
    file: String,
    /// E01: the CPU shell-interval model for the capture pose, so each frame
    /// carries the region/interval evidence next to the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_first: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_far: Option<[f32; 2]>,
}

#[derive(Clone, Serialize, Default)]
struct FrameStats {
    measured_frames: usize,
    p50_ms: f32,
    p95_ms: f32,
    p99_ms: f32,
    max_ms: f32,
    mean_ms: f32,
}

#[derive(Clone, Serialize)]
struct SceneResult {
    scene_id: String,
    status: String,
    manifest: String,
    metrics: Option<String>,
    captures: usize,
    measured_frames: usize,
    p50_ms: f32,
    p95_ms: f32,
    p99_ms: f32,
    max_ms: f32,
}

#[derive(Resource)]
struct VisualQaRun {
    config: VisualQaConfig,
    baseline: baseline::Baseline,
    scene_index: usize,
    stage: Stage,
    /// Frames since this scene's build started.
    scene_frame: u32,
    /// Frames since the current stage started.
    stage_frame: u32,
    /// Absolute scene frame at which the first pose is shown.
    poses_base: u32,
    /// Wall clock accumulated while Playing (also drives the route).
    run_time: f32,
    route_time: f32,
    /// Estimated cloud age since the world became playable. Not an engine
    /// value: `weather::ClimateRuntime` owns the real clock until E03 makes it
    /// explicit, but both accumulate the same frame deltas.
    cloud_age: f32,
    measure_from: u32,
    captured: usize,
    metrics_ms: Vec<f32>,
    captures: Vec<CaptureRecord>,
    results: Vec<SceneResult>,
    built: Option<BuiltScene>,
    last_pose: Option<ScenePose>,
    /// D01: original post components captured before the first variant and the
    /// variant currently applied; cleared at scene end.
    saved_post: Option<SavedPostFx>,
    applied_variant: Option<String>,
    failure: Option<String>,
    exit_code: i32,
    summary_written: bool,
    diagnostics_written: bool,
}

impl VisualQaRun {
    fn new(config: VisualQaConfig) -> Self {
        Self {
            config,
            baseline: baseline::collect(),
            scene_index: 0,
            stage: Stage::Build,
            scene_frame: 0,
            stage_frame: 0,
            poses_base: 0,
            run_time: 0.0,
            route_time: 0.0,
            cloud_age: 0.0,
            measure_from: 0,
            captured: 0,
            metrics_ms: Vec::new(),
            captures: Vec::new(),
            results: Vec::new(),
            built: None,
            last_pose: None,
            saved_post: None,
            applied_variant: None,
            failure: None,
            exit_code: 0,
            summary_written: false,
            diagnostics_written: false,
        }
    }

    fn reset_for_play(&mut self) {
        self.scene_index = 0;
        self.stage = Stage::Build;
        self.scene_frame = 0;
        self.stage_frame = 0;
        self.poses_base = 0;
        self.run_time = 0.0;
        self.route_time = 0.0;
        self.cloud_age = 0.0;
        self.measure_from = 0;
        self.captured = 0;
        self.metrics_ms.clear();
        self.captures.clear();
        self.results.clear();
        self.built = None;
        self.last_pose = None;
        self.saved_post = None;
        self.applied_variant = None;
        self.failure = None;
        self.exit_code = 0;
        self.summary_written = false;
        self.diagnostics_written = false;
    }

    fn active_scene(&self) -> Option<SceneId> {
        self.config.scenes.get(self.scene_index).copied()
    }

    fn commit_label(&self) -> String {
        self.baseline
            .commit
            .as_deref()
            .map(|commit| commit.chars().take(7).collect())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn run_root(&self) -> PathBuf {
        self.config.output_root.join(self.commit_label())
    }

    fn scene_dir(&self, scene: SceneId) -> PathBuf {
        self.run_root()
            .join(scene.key())
            .join(&self.config.run_label)
    }

    fn summary_path(&self) -> PathBuf {
        self.run_root()
            .join(format!("{}-summary.json", self.config.run_label))
    }

    fn record_failure(&mut self, reason: &str) {
        eprintln!("visual-qa: {reason}");
        if self.failure.is_none() {
            self.failure = Some(reason.to_string());
        }
        self.exit_code = 2;
        self.write_summary();
    }

    fn write_summary(&mut self) {
        if self.summary_written {
            return;
        }
        self.summary_written = true;
        let summary = RunSummary {
            schema_version: MANIFEST_SCHEMA_VERSION,
            commit: self.baseline.commit.clone(),
            crate_root: self.baseline.crate_root.clone(),
            run_label: self.config.run_label.clone(),
            seed: self.config.seed,
            biome: self.config.biome.clone(),
            day_time: self.config.day_time,
            output_root: self.run_root().display().to_string(),
            status: if self.exit_code == 0 {
                "ok".to_string()
            } else {
                "failed".to_string()
            },
            failure: self.failure.clone(),
            source: self.baseline.src.clone(),
            catalog_version: crate::art::MATERIAL_CATALOG_VERSION,
            catalog_id_fingerprint: crate::art::id_fingerprint(),
            catalog_face_fingerprint: crate::art::face_fingerprint(),
            scenes: self.results.clone(),
        };
        let path = self.summary_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        write_json(&path, &summary);
    }

    fn finish_scene(
        &mut self,
        chunk_meshes: usize,
        meshes: usize,
        images: usize,
        lifecycle: &VisualLifecycleDiagnostics,
        terrain: &TerrainDiagnostics,
        clouds: &CloudDiagnostics,
        plan_cache: crate::structures::PlanCacheStats,
    ) {
        let Some(scene) = self.active_scene() else {
            return;
        };
        let stats = frame_stats(&self.metrics_ms);
        let scene_dir = self.scene_dir(scene);
        let metrics = MetricsReport {
            schema_version: MANIFEST_SCHEMA_VERSION,
            scene_id: scene.key().to_string(),
            status: "ok".to_string(),
            measured_frames: stats.measured_frames,
            p50_ms: stats.p50_ms,
            p95_ms: stats.p95_ms,
            p99_ms: stats.p99_ms,
            max_ms: stats.max_ms,
            mean_ms: stats.mean_ms,
            cloud_age_s: self.cloud_age,
            chunk_meshes,
            meshes,
            images,
            visual_lifecycle: lifecycle.clone(),
            terrain: terrain.clone(),
            clouds: clouds.clone(),
            plan_cache,
            captures: self.captures.clone(),
            notes: vec![
                "CPU frame deltas are sampled on the main schedule; per-pass GPU timings are not part of A01".to_string(),
                "deltas are capped at 500ms so a loading stall cannot masquerade as steady state".to_string(),
                "E01 cloud diagnostics read the live material uniform, not the settings request".to_string(),
                "G01 plan_cache is the bounded structure-plan cache; eviction only regenerates the same plan".to_string(),
            ],
        };
        let metrics_path = scene_dir.join("metrics.json");
        write_json(&metrics_path, &metrics);
        self.results.push(SceneResult {
            scene_id: scene.key().to_string(),
            status: "ok".to_string(),
            manifest: "manifest.json".to_string(),
            metrics: Some("metrics.json".to_string()),
            captures: self.captures.len(),
            measured_frames: stats.measured_frames,
            p50_ms: stats.p50_ms,
            p95_ms: stats.p95_ms,
            p99_ms: stats.p99_ms,
            max_ms: stats.max_ms,
        });
        self.scene_index += 1;
        self.built = None;
        self.metrics_ms.clear();
        self.captures.clear();
        self.captured = 0;
        self.scene_frame = 0;
        self.stage_frame = 0;
        self.route_time = 0.0;
        if self.scene_index >= self.config.scenes.len() {
            self.stage = Stage::Finished;
            self.write_summary();
        } else {
            self.stage = Stage::Build;
        }
    }
}

fn frame_stats(samples: &[f32]) -> FrameStats {
    if samples.is_empty() {
        return FrameStats::default();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |p: f32| -> f32 {
        let index = ((sorted.len() - 1) as f32 * p).round() as usize;
        sorted[index.min(sorted.len() - 1)]
    };
    let sum: f32 = sorted.iter().sum();
    FrameStats {
        measured_frames: sorted.len(),
        p50_ms: percentile(0.50),
        p95_ms: percentile(0.95),
        p99_ms: percentile(0.99),
        max_ms: *sorted.last().unwrap_or(&0.0),
        mean_ms: sum / sorted.len() as f32,
    }
}

// ---------- JSON reports ----------

#[derive(Serialize)]
struct QualitySnapshot {
    view_dist: i32,
    lod_mode: String,
    pixelated: bool,
    clouds: bool,
    weather: bool,
    cloud_coverage: f32,
    cloud_density: f32,
    cloud_raymarch_steps: u32,
    cloud_render_width: u32,
    cloud_render_height: u32,
    exposure_ev100: Option<f32>,
    msaa: String,
}

#[derive(Serialize)]
struct ClockSnapshot {
    day_time: f32,
    day_time_locked: bool,
}

#[derive(Serialize)]
struct SceneSnapshot {
    anchor: [f32; 3],
    poses: Vec<ScenePose>,
    samples: Vec<scene::SampleRef>,
    modified_voxels: u32,
    route_points: usize,
    /// Non-voxel props spawned by the runner (B01 humanoids/machine visuals).
    props: usize,
}

#[derive(Serialize)]
struct SceneManifest {
    schema_version: u32,
    scene_id: String,
    scene_version: u32,
    scene_title: String,
    commit: Option<String>,
    crate_root: Option<String>,
    world_seed: u32,
    biome: String,
    generator_version: String,
    visual_style_version: u32,
    quality: QualitySnapshot,
    clock: ClockSnapshot,
    hardware: baseline::HardwareSnapshot,
    assets: baseline::AssetsSnapshot,
    source: baseline::SourceStats,
    scene: SceneSnapshot,
    capture_plan: Vec<String>,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct MetricsReport {
    schema_version: u32,
    scene_id: String,
    status: String,
    measured_frames: usize,
    p50_ms: f32,
    p95_ms: f32,
    p99_ms: f32,
    max_ms: f32,
    mean_ms: f32,
    cloud_age_s: f32,
    chunk_meshes: usize,
    meshes: usize,
    images: usize,
    visual_lifecycle: VisualLifecycleDiagnostics,
    terrain: TerrainDiagnostics,
    clouds: CloudDiagnostics,
    /// G01: bounded plan cache state for the measured scene.
    plan_cache: crate::structures::PlanCacheStats,
    captures: Vec<CaptureRecord>,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct RunSummary {
    schema_version: u32,
    commit: Option<String>,
    crate_root: Option<String>,
    run_label: String,
    seed: u32,
    biome: String,
    day_time: f32,
    output_root: String,
    status: String,
    failure: Option<String>,
    source: baseline::SourceStats,
    /// B02: stable material catalog version and frozen mapping fingerprints,
    /// so K05 can refuse to pair frames captured under different block→face
    /// mappings.
    catalog_version: u32,
    catalog_id_fingerprint: u64,
    catalog_face_fingerprint: u64,
    scenes: Vec<SceneResult>,
}

fn write_json<T: Serialize>(path: &Path, value: &T) {
    let json = match serde_json::to_string_pretty(value) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("visual-qa: 序列化 {} 失败: {err}", path.display());
            return;
        }
    };
    if let Err(err) = std::fs::write(path, json) {
        eprintln!("visual-qa: 写入 {} 失败: {err}", path.display());
    }
}

// ---------- Systems ----------

fn visual_qa_clock(run: Res<VisualQaRun>, mut day: ResMut<DayTime>) {
    day.0 = run
        .last_pose
        .as_ref()
        .and_then(|pose| pose.day_time)
        .unwrap_or(run.config.day_time);
}

/// A03 evidence: record the probed renderer capabilities and the resolved
/// quality (including simulation overrides and downgrade reasons) next to the
/// run summary. Written once per `Playing` entry; hardware is constant during
/// a run.
fn write_capabilities_report(
    run: Res<VisualQaRun>,
    capabilities: Res<RenderCapabilities>,
    quality: Res<ResolvedQuality>,
) {
    #[derive(Serialize)]
    struct CapabilitiesReport<'a> {
        schema_version: u32,
        run_label: String,
        capabilities: &'a RenderCapabilities,
        resolved_quality: &'a ResolvedQuality,
        /// B03: the surface texture route the probed device actually selects
        /// (array / padded atlas / classic) plus the compression choice.
        texture_route: crate::art::RouteDecision,
        surface_compression: &'static str,
    }
    let report = CapabilitiesReport {
        schema_version: MANIFEST_SCHEMA_VERSION,
        run_label: run.config.run_label.clone(),
        capabilities: &capabilities,
        resolved_quality: &quality,
        texture_route: capabilities.texture_route(crate::art::MATERIAL_COUNT as u32, 64),
        surface_compression: capabilities.surface_compression(),
    };
    let path = run
        .run_root()
        .join(format!("{}-capabilities.json", run.config.run_label));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_json(&path, &report);
}

/// A05 evidence: one file that combines the effective validated settings, the
/// resolved quality, the lifecycle counters and the budget warnings, so K03/K05
/// do not have to stitch together several reports.
fn write_diagnostics_report(run: &VisualQaRun, diagnostics: &VisualDiagnostics) {
    #[derive(Serialize)]
    struct DiagnosticsReport<'a> {
        schema_version: u32,
        run_label: String,
        commit: Option<String>,
        diagnostics: &'a VisualDiagnostics,
    }
    let report = DiagnosticsReport {
        schema_version: crate::visual::VISUAL_DIAGNOSTICS_SCHEMA_VERSION,
        run_label: run.config.run_label.clone(),
        commit: run.baseline.commit.clone(),
        diagnostics,
    };
    let path = run
        .run_root()
        .join(format!("{}-diagnostics.json", run.config.run_label));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_json(&path, &report);
}

/// D01 evidence: the pass/format inventory plus the live probe status, so the
/// rendering report states what ran on the GPU instead of implying it.
fn write_rendering_report(
    run: &VisualQaRun,
    inventory: Option<&PassInventory>,
    capabilities: &RenderCapabilities,
    status: &ProbeStatus,
) {
    #[derive(Serialize)]
    struct RenderingReport<'a> {
        schema_version: u32,
        run_label: String,
        commit: Option<String>,
        inventory: Option<&'a PassInventory>,
        requested_probe: String,
        probe_status: &'a ProbeStatus,
        gpu_timestamps: bool,
        notes: Vec<String>,
    }
    let report = RenderingReport {
        schema_version: crate::rendering::PASS_INVENTORY_SCHEMA_VERSION,
        run_label: run.config.run_label.clone(),
        commit: run.baseline.commit.clone(),
        inventory,
        requested_probe: status.initial_request.clone(),
        probe_status: status,
        gpu_timestamps: capabilities.features.timestamp_query,
        notes: vec![
            "D01: per-pass GPU timing is deferred to K03; the timestamp feature is reported above"
                .to_string(),
            "the probe runs in Core3dSystems::PostProcess before tonemapping; probe_status.activations proves the pass ran"
                .to_string(),
            "D01 scene variants record full/no_post/sun_only/ambient_only and probe stops/zebra as paired frames"
                .to_string(),
        ],
    };
    let path = run
        .run_root()
        .join(format!("{}-rendering.json", run.config.run_label));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_json(&path, &report);
}

fn visual_qa_on_play(
    mut run: ResMut<VisualQaRun>,
    mut day: ResMut<DayTime>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    atlas: Res<AtlasRes>,
) {
    run.reset_for_play();
    day.0 = run.config.day_time;
    commands.insert_resource(build_probe_cards(&mut meshes, &mut materials));
    commands.insert_resource(build_family_materials(
        &atlas.atlas,
        &mut meshes,
        &mut materials,
        &mut images,
    ));
    println!(
        "VISUAL_QA_START seed={} biome={} scenes={:?} out={}",
        run.config.seed,
        run.config.biome,
        run.config
            .scenes
            .iter()
            .map(|scene| scene.key())
            .collect::<Vec<_>>(),
        run.run_root().display()
    );
}

#[allow(clippy::too_many_arguments)]
fn visual_qa_driver(
    time: Res<Time>,
    settings: Res<Settings>,
    mut run: ResMut<VisualQaRun>,
    mut commands: Commands,
    mut world: Option<ResMut<World>>,
    mut camera: QaCameraQuery,
    mut player: Query<&mut Player>,
    mut day: ResMut<DayTime>,
    meshes: Res<Assets<Mesh>>,
    images: Res<Assets<Image>>,
    chunks: Query<Entity, With<ChunkMesh>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    system_info: Option<Res<SystemInfo>>,
    window: Query<&Window, With<PrimaryWindow>>,
    (
        lifecycle,
        diagnostics,
        terrain,
        cards,
        families,
        mut probe_request,
        mut lighting_probe,
        pass_inventory,
        probe_status,
        capabilities,
        clouds,
        mut cloud_debug,
    ): (
        Res<VisualLifecycleDiagnostics>,
        Res<VisualDiagnostics>,
        Res<TerrainDiagnostics>,
        Option<Res<ProbeCards>>,
        Option<Res<FamilyMaterials>>,
        ResMut<ProbeRequest>,
        ResMut<LightingProbe>,
        Option<Res<PassInventory>>,
        Res<ProbeStatus>,
        Res<RenderCapabilities>,
        Res<CloudDiagnostics>,
        ResMut<CloudDebug>,
    ),
    asset_server: Res<AssetServer>,
) {
    // B04 poses can override the day clock; other scenes keep the run-level
    // `--visual-qa-day`.
    day.0 = run
        .last_pose
        .as_ref()
        .and_then(|pose| pose.day_time)
        .unwrap_or(run.config.day_time);
    let raw_dt = time.delta_secs();
    let dt = if raw_dt.is_finite() {
        raw_dt.clamp(0.0, 0.5)
    } else {
        0.0
    };
    run.run_time += dt;
    run.cloud_age += dt;
    run.scene_frame = run.scene_frame.saturating_add(1);

    match run.stage {
        Stage::Build => {
            let Some(scene) = run.active_scene() else {
                run.record_failure("未配置任何场景");
                run.stage = Stage::Finished;
                return;
            };
            let Some(world) = world.as_deref_mut() else {
                run.record_failure("Playing 状态下缺少 World 资源");
                run.stage = Stage::Finished;
                return;
            };
            let built = scene::build(scene, world, run.config.seed);
            // B01 props use the real asset pipeline (GLB load, machine visual)
            // instead of a private runner copy.
            for prop in &built.props {
                match prop {
                    scene::SceneProp::Humanoid { model, pos, yaw } => {
                        crate::char::spawn_humanoid_model(
                            &mut commands,
                            &asset_server,
                            model,
                            Vec3::from_array(*pos),
                            *yaw,
                        );
                    }
                    scene::SceneProp::Machine { key, pos, dir } => {
                        crate::factory::spawn_machine(&mut commands, *pos, key, *dir);
                    }
                    scene::SceneProp::Card { material, pos, yaw } => {
                        if let Some(cards) = cards.as_deref() {
                            commands.spawn((
                                Mesh3d(cards.mesh.clone()),
                                MeshMaterial3d(cards.material(*material)),
                                Transform::from_translation(Vec3::from_array(*pos))
                                    .with_rotation(Quat::from_rotation_y(*yaw)),
                            ));
                        }
                    }
                    scene::SceneProp::FamilySample {
                        family,
                        shape,
                        pos,
                        yaw,
                    } => {
                        if let Some(families) = families.as_deref()
                            && let Some(mesh) = families.shape(*shape)
                            && let Some(material) = families.material(*family)
                        {
                            commands.spawn((
                                Mesh3d(mesh),
                                MeshMaterial3d(material),
                                family_shape_transform(*shape, Vec3::from_array(*pos), *yaw),
                            ));
                        }
                    }
                }
            }
            let scene_dir = run.scene_dir(scene);
            if let Err(err) = std::fs::create_dir_all(scene_dir.join("raw-frames")) {
                run.record_failure(&format!("无法创建 {}: {err}", scene_dir.display()));
                run.stage = Stage::Finished;
                return;
            }
            // D01/B04: remember the shipping post components before the first
            // variant removes or restores anything.
            if matches!(scene, SceneId::D01 | SceneId::B04) {
                run.saved_post =
                    camera
                        .single()
                        .ok()
                        .map(|(_, _, _, _, _, bloom, ssao, contact)| SavedPostFx {
                            bloom: bloom.cloned(),
                            ssao: ssao.cloned(),
                            contact: contact.cloned(),
                        });
                run.applied_variant = None;
            }
            let hardware = baseline::collect_hardware(
                adapter.as_deref(),
                window.single().ok(),
                system_info.as_deref(),
            );
            let (exposure, msaa) = match camera.single() {
                Ok((_, _, _, exposure, msaa, _, _, _)) => (
                    exposure.map(|exposure| exposure.ev100),
                    msaa.map(|msaa| format!("{msaa:?}")),
                ),
                Err(_) => (None, None),
            };
            let manifest = build_manifest(
                &run,
                scene,
                &built,
                &settings,
                hardware,
                exposure,
                msaa.unwrap_or_else(|| "unknown".to_string()),
            );
            write_json(&scene_dir.join("manifest.json"), &manifest);
            run.measure_from = scene.settle_frames() + scene.measure_skip_frames();
            run.last_pose = built.poses.first().cloned();
            run.built = Some(built);
            run.stage = Stage::Wait;
            run.stage_frame = 0;
            run.captured = 0;
        }
        Stage::Wait => {
            let settle = run
                .active_scene()
                .map(|scene| scene.settle_frames())
                .unwrap_or(0);
            if run.stage_frame >= settle {
                run.stage = Stage::Poses;
                run.stage_frame = 0;
                run.poses_base = run.scene_frame;
                run.captured = 0;
            }
        }
        Stage::Poses => {
            let pose_count = run
                .built
                .as_ref()
                .map(|built| built.poses.len())
                .unwrap_or(0);
            if pose_count == 0 {
                run.stage = next_after_poses(&run);
                run.stage_frame = 0;
                run.captured = 0;
            } else {
                let elapsed = run.scene_frame.saturating_sub(run.poses_base);
                let index = ((elapsed / POSE_GAP_FRAMES) as usize).min(pose_count - 1);
                if let Some(pose) = run
                    .built
                    .as_ref()
                    .and_then(|built| built.poses.get(index))
                    .cloned()
                {
                    run.last_pose = Some(pose);
                }
                // D01/B04: switch the pass/lighting variant once per pose
                // change. Commands and resources apply before the capture
                // frame at this pose.
                if matches!(run.active_scene(), Some(SceneId::D01) | Some(SceneId::B04))
                    && let Some(variant) = run
                        .last_pose
                        .as_ref()
                        .and_then(|pose| pose.variant.as_deref())
                        .and_then(PassVariant::parse)
                    && run.applied_variant.as_deref() != Some(variant.key())
                {
                    if let Some(saved) = run.saved_post.clone()
                        && let Ok((entity, _, _, _, _, _, _, _)) = camera.single()
                    {
                        apply_pass_variant(
                            &mut commands,
                            entity,
                            variant,
                            &saved,
                            &mut probe_request,
                            &mut lighting_probe,
                        );
                    }
                    run.applied_variant = Some(variant.key().to_string());
                }
                // E01: the cloud debug view is a resource, so switching poses
                // is enough; the uniform is written by `climate_system`.
                if run.active_scene() == Some(SceneId::E01)
                    && let Some(variant) =
                        run.last_pose.as_ref().and_then(|pose| pose.variant.clone())
                    && run.applied_variant.as_deref() != Some(variant.as_str())
                    && let Some(mode) = CloudDebugMode::parse(&variant)
                {
                    cloud_debug.mode = mode;
                    run.applied_variant = Some(variant);
                }
                let last_capture = (pose_count as u32 - 1) * POSE_GAP_FRAMES;
                if elapsed >= last_capture + 8 {
                    run.stage = next_after_poses(&run);
                    run.stage_frame = 0;
                    run.captured = 0;
                }
            }
        }
        Stage::Route => {
            run.route_time += dt;
            let duration = run.config.route_frames.max(1) as f32 / 60.0;
            let progress = (run.route_time / duration.max(f32::EPSILON)).clamp(0.0, 1.0);
            if let Some(route) = run.built.as_ref().map(|built| built.route.as_slice())
                && let Some(pose) = scene::route_pose(route, progress, ROUTE_FOV)
            {
                run.last_pose = Some(pose);
            }
            if run.route_time >= duration {
                run.stage = Stage::Drain;
                run.stage_frame = 0;
            }
        }
        Stage::Drain => {
            // D01/B04: restore the shipping post/lighting state before leaving
            // the scene so later scenes in the same run are not affected.
            if run.stage_frame == 0
                && matches!(run.active_scene(), Some(SceneId::D01) | Some(SceneId::B04))
            {
                if let Some(saved) = run.saved_post.clone()
                    && let Ok((entity, _, _, _, _, _, _, _)) = camera.single()
                {
                    apply_pass_variant(
                        &mut commands,
                        entity,
                        PassVariant::Full,
                        &saved,
                        &mut probe_request,
                        &mut lighting_probe,
                    );
                }
                run.saved_post = None;
                run.applied_variant = None;
            }
            // E01: debug views are per-pose only; never leak one into the next
            // scene of the same run.
            if run.stage_frame == 0 && run.active_scene() == Some(SceneId::E01) {
                cloud_debug.mode = CloudDebugMode::Off;
                run.applied_variant = None;
            }
            if run.stage_frame >= DRAIN_FRAMES {
                let plan_cache = world
                    .as_deref()
                    .map(|world| world.g.plan_cache_stats())
                    .unwrap_or_default();
                run.finish_scene(
                    chunks.iter().count(),
                    meshes.len(),
                    images.len(),
                    &lifecycle,
                    &terrain,
                    &clouds,
                    plan_cache,
                );
            }
        }
        Stage::Finished => {
            if !run.diagnostics_written {
                run.diagnostics_written = true;
                write_diagnostics_report(&run, &diagnostics);
                write_rendering_report(
                    &run,
                    pass_inventory.as_deref(),
                    &capabilities,
                    &probe_status,
                );
            }
            run.stage_frame = run.stage_frame.saturating_add(1);
            if run.stage_frame == DRAIN_FRAMES + 30 && !run.config.keep_open {
                let label = if run.exit_code == 0 { "OK" } else { "FAIL" };
                println!(
                    "VISUAL_QA_{label} scenes={} out={}",
                    run.results.len(),
                    run.run_root().display()
                );
                std::process::exit(run.exit_code);
            }
            return;
        }
    }

    if let Some(pose) = run.last_pose.clone() {
        apply_pose(&mut camera, &pose);
        pin_player(&mut player, &pose);
    }

    if run.config.capture
        && let Some(scene) = run.active_scene()
    {
        match run.stage {
            Stage::Poses => {
                let elapsed = run.scene_frame.saturating_sub(run.poses_base);
                // Capture one frame after the pose switch: `clouds`/`VisualFrame`
                // are sampled in `Last`, so frame N carries the camera of N-1.
                // Waiting one frame pairs each image with its own pose data.
                if elapsed % POSE_GAP_FRAMES == 1
                    && let Some(pose) = run.last_pose.clone()
                {
                    spawn_capture(&mut commands, &mut run, scene, &pose.name, Some(&clouds));
                }
            }
            Stage::Route => {
                let duration = run.config.route_frames.max(1) as f32 / 60.0;
                let progress = (run.route_time / duration.max(f32::EPSILON)).clamp(0.0, 1.0);
                while run.captured < ROUTE_CAPTURE_FRACTIONS.len()
                    && progress >= ROUTE_CAPTURE_FRACTIONS[run.captured]
                {
                    let name = format!("route_{:03}", (progress * 100.0).round() as u32);
                    spawn_capture(&mut commands, &mut run, scene, &name, Some(&clouds));
                    run.captured += 1;
                }
            }
            _ => {}
        }
    }

    if matches!(run.stage, Stage::Poses | Stage::Route)
        && run.scene_frame >= run.measure_from
        && run.metrics_ms.len() < MAX_MEASURED_FRAMES
    {
        run.metrics_ms.push(dt * 1000.0);
    }
    run.stage_frame = run.stage_frame.saturating_add(1);
}

fn next_after_poses(run: &VisualQaRun) -> Stage {
    let has_route = run
        .built
        .as_ref()
        .is_some_and(|built| built.route.len() >= 2);
    if has_route {
        Stage::Route
    } else {
        Stage::Drain
    }
}

fn apply_pose(camera: &mut QaCameraQuery, pose: &ScenePose) {
    let Ok((_, mut transform, mut projection, _, _, _, _, _)) = camera.single_mut() else {
        return;
    };
    transform.translation = Vec3::from_array(pose.eye);
    transform.rotation = Quat::from_rotation_y(pose.yaw) * Quat::from_rotation_x(pose.pitch);
    *projection = Projection::Perspective(PerspectiveProjection {
        fov: pose.fov.to_radians(),
        far: crate::space::CAM_FAR,
        ..default()
    });
}

/// Keep the streaming center under the QA camera so chunks are generated
/// around the framed area even though the player is not being simulated.
fn pin_player(player: &mut Query<&mut Player>, pose: &ScenePose) {
    let Ok(mut player) = player.single_mut() else {
        return;
    };
    player.pos = Vec3::from_array(pose.eye) - Vec3::Y;
    player.vel = Vec3::ZERO;
    player.yaw = pose.yaw;
    player.pitch = pose.pitch;
    player.mining = None;
    player.on_ground = false;
    player.dead = false;
    // Spawn toasts otherwise sit in the middle of every capture.
    if !player.toasts.is_empty() {
        player.toasts.clear();
    }
}

fn spawn_capture(
    commands: &mut Commands,
    run: &mut VisualQaRun,
    scene: SceneId,
    pose: &str,
    clouds: Option<&CloudDiagnostics>,
) {
    let sanitized: String = pose
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let file = format!("{sanitized}_f{:04}.png", run.scene_frame);
    let path = run.scene_dir(scene).join("raw-frames").join(&file);
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    run.captures.push(CaptureRecord {
        pose: pose.to_string(),
        frame: run.scene_frame,
        cloud_age_s: run.cloud_age,
        file: format!("raw-frames/{file}"),
        cloud_region: clouds.map(|clouds| clouds.center_ray.camera_region.key().to_string()),
        cloud_first: clouds.and_then(|clouds| {
            clouds
                .center_ray
                .first
                .map(|segment| [segment.enter, segment.exit])
        }),
        cloud_far: clouds.and_then(|clouds| {
            clouds
                .center_ray
                .far
                .map(|segment| [segment.enter, segment.exit])
        }),
    });
}

#[allow(clippy::too_many_arguments)]
fn build_manifest(
    run: &VisualQaRun,
    scene: SceneId,
    built: &BuiltScene,
    settings: &Settings,
    hardware: baseline::HardwareSnapshot,
    exposure_ev100: Option<f32>,
    msaa: String,
) -> SceneManifest {
    let capture_plan = if built.route.len() >= 2 {
        ROUTE_CAPTURE_FRACTIONS
            .iter()
            .map(|fraction| {
                format!(
                    "route_{:03} @ {:.0}%",
                    (fraction * 100.0) as u32,
                    fraction * 100.0
                )
            })
            .collect()
    } else {
        built.poses.iter().map(|pose| pose.name.clone()).collect()
    };
    SceneManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        scene_id: scene.key().to_string(),
        scene_version: SCENE_VERSION,
        scene_title: scene.title().to_string(),
        commit: run.baseline.commit.clone(),
        crate_root: run.baseline.crate_root.clone(),
        world_seed: run.config.seed,
        biome: run.config.biome.clone(),
        generator_version: run
            .baseline
            .commit
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        visual_style_version: crate::art::ART_STYLE_VERSION,
        quality: QualitySnapshot {
            view_dist: settings.view_dist,
            lod_mode: format!("{:?}", settings.lod_mode),
            pixelated: settings.pixelated,
            clouds: settings.clouds,
            weather: settings.weather,
            cloud_coverage: settings.cloud_coverage,
            cloud_density: settings.cloud_density,
            cloud_raymarch_steps: settings.cloud_raymarch_steps,
            cloud_render_width: settings.cloud_render_width,
            cloud_render_height: settings.cloud_render_height,
            exposure_ev100,
            msaa,
        },
        clock: ClockSnapshot {
            day_time: run.config.day_time,
            day_time_locked: true,
        },
        hardware,
        assets: run.baseline.assets.clone(),
        source: run.baseline.src.clone(),
        scene: SceneSnapshot {
            anchor: built.anchor,
            poses: built.poses.clone(),
            samples: built.samples.clone(),
            modified_voxels: built.modified_voxels,
            route_points: built.route.len(),
            props: built.props.len(),
        },
        capture_plan,
        notes: vec![
            "manifest is run-order independent; mesh/image counts and timings live in metrics.json"
                .to_string(),
            "HUD is visible in raw frames; K05 owns the paired human review".to_string(),
            "cloud age is recorded per capture from harness time until E03 exposes the cloud clock"
                .to_string(),
            "player state is pinned for streaming; gameplay is not paused".to_string(),
        ],
    }
}

// ---------- Plugin ----------

pub struct VisualQaPlugin;

impl Plugin for VisualQaPlugin {
    fn build(&self, app: &mut App) {
        let Some(config) = app.world().get_resource::<VisualQaConfig>().cloned() else {
            return;
        };
        app.insert_resource(VisualQaRun::new(config))
            .add_systems(
                OnEnter(GameState::Playing),
                (visual_qa_on_play, write_capabilities_report),
            )
            .add_systems(
                Update,
                visual_qa_clock
                    .in_set(GameSet::CommonLamp)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                visual_qa_driver
                    .in_set(GameSet::CameraFx)
                    .after(crate::photo::photo_camera_system)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn parse_requires_enable_flag() {
        assert!(VisualQaConfig::parse(&args(&["--smoke"])).is_none());
    }

    #[test]
    fn parse_reads_scenes_and_options() {
        let config = VisualQaConfig::parse(&args(&[
            "--visual-qa",
            "--visual-qa-scene",
            "S06,S01",
            "--visual-qa-seed",
            "1337",
            "--visual-qa-day",
            "0.75",
            "--visual-qa-route-frames",
            "120",
            "--visual-qa-no-capture",
            "--visual-qa-run",
            "test-run",
        ]))
        .expect("enabled");
        assert_eq!(config.scenes, vec![SceneId::S06, SceneId::S01]);
        assert_eq!(config.seed, 1337);
        assert!((config.day_time - 0.75).abs() < 1e-6);
        assert_eq!(config.route_frames, 120);
        assert!(!config.capture);
        assert_eq!(config.run_label, "test-run");
    }

    #[test]
    fn parse_ignores_bad_values_and_duplicates() {
        let config = VisualQaConfig::parse(&args(&[
            "--visual-qa",
            "--visual-qa-seed",
            "not-a-number",
            "--visual-qa-biome",
            "nope",
            "--visual-qa-scene",
            "S01,S01,nope",
        ]))
        .expect("enabled");
        assert_eq!(config.seed, 11);
        assert_eq!(config.biome, "lush");
        assert_eq!(config.scenes, vec![SceneId::S01]);
    }

    #[test]
    fn pass_variants_parse_and_roundtrip() {
        for variant in [
            PassVariant::Full,
            PassVariant::NoPost,
            PassVariant::SunOnly,
            PassVariant::AmbientOnly,
            PassVariant::ProbeStops,
            PassVariant::ProbeZebra,
            PassVariant::Overcast,
        ] {
            assert_eq!(PassVariant::parse(variant.key()), Some(variant));
        }
        assert_eq!(
            PassVariant::parse("probe_zebra"),
            Some(PassVariant::ProbeZebra)
        );
        assert_eq!(PassVariant::parse("cloudy"), Some(PassVariant::Overcast));
        assert_eq!(PassVariant::parse("nope"), None);
    }

    #[test]
    fn frame_stats_are_ordered() {
        let samples: Vec<f32> = (1..=100).map(|value| value as f32).collect();
        let stats = frame_stats(&samples);
        assert_eq!(stats.measured_frames, 100);
        assert!(stats.p50_ms <= stats.p95_ms);
        assert!(stats.p95_ms <= stats.p99_ms);
        assert_eq!(stats.p99_ms, 99.0);
        assert_eq!(stats.max_ms, 100.0);
        assert!(stats.mean_ms > 50.0 && stats.mean_ms < 51.0);
    }

    #[test]
    fn empty_stats_are_zero() {
        let stats = frame_stats(&[]);
        assert_eq!(stats.measured_frames, 0);
        assert_eq!(stats.p99_ms, 0.0);
    }
}
