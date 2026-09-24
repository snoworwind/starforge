//! A06 application assembly: the single entry point that parses launch
//! options, bootstraps the asset roots and registers every plugin exactly once.
//!
//! `main.rs` is reduced to `fn main() { app::run() }`. `GameSet` ordering stays
//! owned by [`crate::schedule::configure`] (called from [`GameFlowPlugin`]);
//! this module only decides *which* plugins are added and in what order, so a
//! future tool or test can reuse the same pipeline instead of re-reading
//! `std::env::args()` or duplicating registration.
//!
//! See `docs/art-overhaul/05-EXECUTION-BACKLOG.md` (A06) and
//! `docs/art-overhaul/00-BASELINE-AND-ARCHITECTURE.md` (0.3/0.7).

use std::path::{Path, PathBuf};

use bevy::camera::{Exposure, Hdr, ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{AtmosphereEnvironmentMapLight, DirectionalLightShadowMap};
use bevy::pbr::{
    AtmosphereMode, AtmosphereSettings, ContactShadows, DistanceFog, FogFalloff,
    ScreenSpaceAmbientOcclusion,
};
use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::EguiPlugin;

use crate::schedule::{GameSet, GameState};
use crate::visual_qa::{VisualQaConfig, VisualQaPlugin};

// ---------- Launch options ----------

/// Every CLI switch the application understands, parsed once.
///
/// Before A06 `main`, `lod.rs` and `visual_qa` each scanned
/// `std::env::args()` independently. A single parse keeps the flag surface
/// auditable and lets `--lod-log` become a normal resource instead of a lazy
/// process-wide env read inside a hot system.
#[derive(Clone, Debug, Default)]
pub struct LaunchOptions {
    pub smoke: bool,
    pub play: bool,
    pub clouds_off: bool,
    pub climate_on: bool,
    pub legacy_lod: bool,
    pub simulate_low_device: bool,
    pub lod_log: bool,
    pub art_audit: bool,
    /// B03 `--texture-audit`: headless mip/atlas/array audit, no window/GPU.
    pub texture_audit: bool,
    pub terrain_overlay: bool,
    /// D01 `--render-probe <off|false-color|zebra|luminance>`; diagnostic only.
    pub render_probe: Option<String>,
    /// E01 `--cloud-debug <off|density|interval|...>`; diagnostic only.
    pub cloud_debug: Option<String>,
    pub visual_qa: Option<VisualQaConfig>,
}

impl LaunchOptions {
    pub fn parse(args: &[String]) -> Self {
        let has = |flag: &str| args.iter().any(|arg| arg == flag);
        let value_of = |flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .and_then(|index| args.get(index + 1))
                .cloned()
        };
        Self {
            smoke: has("--smoke"),
            play: has("--play"),
            clouds_off: has("--clouds-off"),
            climate_on: has("--climate-on"),
            legacy_lod: has("--legacy-lod"),
            simulate_low_device: has("--simulate-low-device"),
            lod_log: has("--lod-log"),
            art_audit: has("--art-audit"),
            texture_audit: has("--texture-audit"),
            terrain_overlay: has("--terrain-overlay"),
            render_probe: value_of("--render-probe"),
            cloud_debug: value_of("--cloud-debug"),
            visual_qa: VisualQaConfig::parse(args),
        }
    }
}

// ---------- Asset bootstrap ----------

/// Files that must exist for the shipped (minimal) package to run. They are all
/// versioned in git; nothing here belongs to the downloadable packs. Audio and
/// fonts are compiled into the executable with `include_bytes!`, so only the
/// AssetServer-loaded models/shaders are listed.
pub const REQUIRED_ASSET_FILES: &[&str] = &[
    "models/creatures/quaternius_alpaca.gltf",
    "models/creatures/quaternius_deer.gltf",
    "models/creatures/quaternius_fox.gltf",
    "models/creatures/quaternius_wolf.gltf",
    "models/creatures/sentinel.glb",
    "models/asteroids/meteor.glb",
    "models/asteroids/meteor_detailed.glb",
    "shaders/planet_curvature.wgsl",
    "shaders/planet_curvature_prepass.wgsl",
    "shaders/cloud_shell.wgsl",
    "shaders/visual/exposure_probe.wgsl",
];

/// Large models the README asks the player to download. Missing files must not
/// abort the run: the game degrades to the minimal asset path and the loader
/// falls back per call site.
pub const OPTIONAL_ASSET_FILES: &[&str] = &[
    "models/external/ships/space_ship_b/scene.gltf",
    "models/external/ships/space_ship_c/scene.gltf",
    "models/external/ships/space_ship_torb/scene.gltf",
    "models/external/ships/supermatic_sky_cruiser/scene.gltf",
    "models/external/ships/unsa_destroyer/scene.gltf",
    "models/external/stations/space_station/scene.gltf",
    "models/external/stations/space_station_3/scene.gltf",
    "models/external/stations/space_station_4/scene.gltf",
    "models/external/stations/helveta/scene.gltf",
    "models/earth/scene.gltf",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetRootSource {
    /// `assets/` beside the executable, as before A06.
    Executable,
    /// `STARFORGE_ASSET_DIR` test hook (isolated cold-install/minimal runs).
    Override,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetRoot {
    pub path: PathBuf,
    pub source: AssetRootSource,
}

/// Resolve where `AssetPlugin` should read from. A non-empty
/// `STARFORGE_ASSET_DIR` override wins so tests and K04 cold-install checks can
/// point at an isolated tree without touching the real package.
pub fn resolve_asset_root(exe_dir: Option<&Path>, override_value: Option<&str>) -> AssetRoot {
    if let Some(value) = override_value
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return AssetRoot {
            path: PathBuf::from(value),
            source: AssetRootSource::Override,
        };
    }
    AssetRoot {
        path: exe_dir
            .map(|dir| dir.join("assets"))
            .unwrap_or_else(|| PathBuf::from("assets")),
        source: AssetRootSource::Executable,
    }
}

/// Result of one bootstrap pass over an asset root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetBootstrapReport {
    pub asset_root: PathBuf,
    pub copied_from_source: bool,
    pub missing_required: Vec<String>,
    pub missing_optional: Vec<String>,
    pub present_optional: usize,
    pub optional_total: usize,
}

impl AssetBootstrapReport {
    /// True when every downloadable pack file was found; the run may still be
    /// minimal when only required files were checked.
    pub fn is_full(&self) -> bool {
        self.optional_total > 0 && self.missing_optional.is_empty()
    }

    pub fn mode(&self) -> &'static str {
        if self.is_full() { "full" } else { "minimal" }
    }

    pub fn has_required(&self) -> bool {
        self.missing_required.is_empty()
    }
}

fn missing_files(root: &Path, files: &[&str]) -> Vec<String> {
    files
        .iter()
        .filter(|file| !root.join(file).is_file())
        .map(|file| (*file).to_string())
        .collect()
}

/// Classify an asset root against the shipped/optional manifest.
pub fn classify_assets(root: &Path) -> AssetBootstrapReport {
    let missing_required = missing_files(root, REQUIRED_ASSET_FILES);
    let missing_optional = missing_files(root, OPTIONAL_ASSET_FILES);
    AssetBootstrapReport {
        asset_root: root.to_path_buf(),
        copied_from_source: false,
        present_optional: OPTIONAL_ASSET_FILES.len() - missing_optional.len(),
        optional_total: OPTIONAL_ASSET_FILES.len(),
        missing_required,
        missing_optional,
    }
}

/// Ensure the shipped assets are usable in a development build by copying
/// `models/` and `shaders/` from the source tree next to the executable when a
/// required file is absent. Packaged or minimal trees already containing the
/// required files are left untouched, except that changed shaders are refreshed
/// (they are tiny, and a stale `target/.../assets` copy otherwise hides shader
/// edits from every QA run).
pub fn bootstrap_assets(asset_root: &Path, source_root: Option<&Path>) -> AssetBootstrapReport {
    let mut report = classify_assets(asset_root);
    let mut copied = false;
    if let Some(source) = source_root.filter(|path| path.is_dir()) {
        // Copy only the top-level group that actually needs it, so a missing
        // shader does not re-copy the multi-hundred-megabyte models.
        let missing = |prefix: &str| {
            report
                .missing_required
                .iter()
                .any(|file| file.starts_with(prefix))
        };
        let shader_differs = REQUIRED_ASSET_FILES
            .iter()
            .filter(|file| file.starts_with("shaders/"))
            .any(|file| {
                match (
                    std::fs::read(source.join(file)),
                    std::fs::read(asset_root.join(file)),
                ) {
                    (Ok(source_bytes), Ok(target_bytes)) => source_bytes != target_bytes,
                    _ => false,
                }
            });
        if missing("models/") {
            let _ = copy_dir_all(&source.join("models"), &asset_root.join("models"));
            copied = true;
        }
        if missing("shaders/") || shader_differs {
            let _ = copy_dir_all(&source.join("shaders"), &asset_root.join("shaders"));
            copied = true;
        }
    }
    if copied {
        report = classify_assets(asset_root);
        report.copied_from_source = true;
    }
    report
}

fn log_asset_bootstrap(report: &AssetBootstrapReport) {
    if !report.has_required() {
        warn!(
            "asset bootstrap: {} required asset(s) missing under {}: {:?}",
            report.missing_required.len(),
            report.asset_root.display(),
            report.missing_required
        );
    }
    if report.copied_from_source {
        info!(
            "asset bootstrap: copied shipped models/shaders into {}",
            report.asset_root.display()
        );
    }
    if report.is_full() {
        info!(
            "asset bootstrap: full asset mode root={} optional={}/{}",
            report.asset_root.display(),
            report.present_optional,
            report.optional_total
        );
    } else {
        warn!(
            "asset bootstrap: minimal asset mode root={} optional={}/{} missing={:?} (see README 外部模型下载)",
            report.asset_root.display(),
            report.present_optional,
            report.optional_total,
            report.missing_optional
        );
    }
}

// ----------------- fail-safe plugin registration ----------

/// Add a plugin unless it is already registered. Bevy panics on duplicate
/// unique plugins, which makes a double registration a crash instead of a
/// visible mistake; this helper turns the A06 "重复注册系统" risk into one
/// warning and a no-op.
pub fn add_plugin_once<P: Plugin>(app: &mut App, plugin: P) {
    if app.is_plugin_added::<P>() {
        warn!(
            "plugin {} already registered; ignoring duplicate add",
            std::any::type_name::<P>()
        );
        return;
    }
    app.add_plugins(plugin);
}

// ---------- Plugin composition ----------

/// 全部游戏模块插件的装配顺序（依赖序）。
pub struct StarForgePlugins {
    settings: crate::save::Settings,
    cloud_tuning: crate::weather::CloudTuning,
    lighting_tuning: crate::daynight::LightingTuning,
}

impl StarForgePlugins {
    pub fn new(
        settings: crate::save::Settings,
        cloud_tuning: crate::weather::CloudTuning,
        lighting_tuning: crate::daynight::LightingTuning,
    ) -> Self {
        Self {
            settings,
            cloud_tuning,
            lighting_tuning,
        }
    }
}

impl PluginGroup for StarForgePlugins {
    fn build(self) -> bevy::app::PluginGroupBuilder {
        use bevy::app::PluginGroupBuilder;
        PluginGroupBuilder::start::<Self>()
            .add(crate::rng::RngPlugin)
            .add(crate::data::DataPlugin)
            .add(crate::planet_scale::PlanetScalePlugin)
            .add(crate::visual::VisualPlugin)
            .add(crate::rendering::RenderingPlugin)
            .add(crate::inventory::InventoryPlugin)
            .add(crate::save::SaveSettingsPlugin(self.settings))
            .add(crate::audio::GameAudioPlugin)
            .add(crate::music::MusicPlugin)
            .add(crate::textures::TexturePlugin)
            .add(crate::feedback::FeedbackPlugin)
            .add(crate::particles::ParticlePlugin)
            .add(crate::camera_fx::CameraFxPlugin)
            .add(crate::ui::UiPlugin)
            .add(crate::codex::CodexPlugin)
            .add(crate::achievements::AchievementsPlugin)
            .add(crate::blueprint::BlueprintPlugin)
            .add(crate::tutorial::TutorialPlugin)
            .add(crate::minimap::MinimapPlugin)
            .add(crate::photo::PhotoPlugin)
            .add(crate::screen_fx::ScreenFxPlugin)
            .add(crate::quests::QuestsPlugin)
            .add(crate::char::CharPlugin)
            .add(crate::suit::SuitPlugin)
            .add(crate::materials::MaterialsPlugin)
            .add(crate::daynight::DayNightPlugin {
                lighting: self.lighting_tuning,
            })
            .add(crate::weather::WeatherPlugin {
                cloud: self.cloud_tuning,
            })
            .add(crate::player::PlayerPlugin)
            .add(crate::creatures::CreaturesPlugin)
            .add(crate::wildlife::WildlifePlugin)
            .add(crate::factory::FactoryPlugin)
            .add(crate::machine_fx::MachineFxPlugin)
            .add(crate::station::StationPlugin)
            .add(crate::storms::StormPlugin)
            .add(crate::space::SpacePlugin)
            .add(crate::network::NetworkPlugin)
            .add(crate::lod::LodPlugin)
            .add(crate::terrain::TerrainPlugin)
            .add(crate::world::WorldPlugin)
    }
}

/// 游戏流程插件：状态机、菜单/加载、进驻清理与调度契约配置。
pub struct GameFlowPlugin;

impl Plugin for GameFlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .add_systems(Startup, startup)
            .add_systems(OnEnter(GameState::Loading), crate::on_enter_loading)
            .add_systems(OnExit(GameState::Loading), crate::on_exit_loading)
            .add_systems(
                Update,
                (
                    (crate::smoke_boot, crate::menu_system)
                        .chain()
                        .in_set(GameSet::Menu)
                        .run_if(in_state(GameState::Menu)),
                    crate::loading_system
                        .in_set(GameSet::Loading)
                        .run_if(in_state(GameState::Loading)),
                ),
            )
            // playing 后段：星球切换/可见性（天空同步归 daynight 插件）
            .add_systems(
                Update,
                ((
                    crate::planet_switch_system,
                    crate::ground_scene_visibility_system,
                )
                    .chain()
                    .in_set(GameSet::LateSwitchFlow)
                    .run_if(in_state(GameState::Playing)),),
            )
            // playing 保存尾链（map → save → quit → smoke）
            .add_systems(
                Update,
                (
                    (crate::save_settings_system, crate::save_system)
                        .chain()
                        .in_set(GameSet::SaveWrite),
                    (crate::quit_to_menu_system, crate::smoke_exit)
                        .chain()
                        .in_set(GameSet::SaveQuit),
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
        crate::schedule::configure(app);
    }
}

// ---------- Application entry ----------

pub fn run() {
    install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = LaunchOptions::parse(&args);

    let exe_dir = executable_dir();
    let asset_override = std::env::var("STARFORGE_ASSET_DIR").ok();
    let asset_root = resolve_asset_root(exe_dir.as_deref(), asset_override.as_deref());
    // B01 asset audit is filesystem-only and must not open a window or touch
    // saves/GPU state, so it runs before the app is built.
    if options.art_audit {
        let (root, source_tree) = crate::art::audit_root(
            &asset_root.path,
            asset_root.source == AssetRootSource::Override,
        );
        std::process::exit(crate::art::run_audit(&root, source_tree));
    }
    // B03 texture audit is CPU-only (atlas tiles + mips); it also runs before
    // the app so it can report on machines without a working GPU. Low-device
    // simulation lowers the reference array limits to exercise the fallback.
    if options.texture_audit {
        std::process::exit(crate::art::run_texture_audit(options.simulate_low_device));
    }
    let source_assets = exe_dir
        .as_deref()
        .map(|dir| dir.join("..").join("..").join("assets"));
    let bootstrap = bootstrap_assets(&asset_root.path, source_assets.as_deref());

    let (mut settings, settings_load_report) = crate::save::load_settings_reported();
    // Read-only visual probe overrides. They never persist to settings.json.
    if options.visual_qa.is_some() {
        // Visual QA runs the modern render path; pixel mode is a separate
        // art preset and is recorded by its own future scene set.
        settings.pixelated = false;
    }
    if options.clouds_off {
        settings.clouds = false;
    }
    if options.climate_on {
        // Read-only probe: force the climate request on even when the saved
        // settings disabled it, so capability fallbacks can be verified.
        settings.clouds = true;
        settings.weather = true;
    }
    if options.legacy_lod {
        settings.lod_mode = crate::save::LodMode::Legacy;
    }
    let cloud_tuning = crate::weather::CloudTuning::from_settings(&settings);
    let lighting_tuning = crate::daynight::LightingTuning::from_settings(&settings);

    let mut app = App::new();
    app.insert_resource(settings_load_report)
        .insert_resource(crate::lod::LodLogging(options.lod_log))
        .insert_resource(ClearColor(Color::srgb(0.05, 0.07, 0.1)))
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .add_plugins(
            DefaultPlugins
                // The game world spans hundreds of units, while Bevy's
                // spatial mixer attenuates by inverse squared distance.
                // Compress world coordinates before they reach the mixer so
                // nearby combat and wildlife sounds remain audible.
                .set(bevy::audio::AudioPlugin {
                    default_spatial_scale: bevy::audio::SpatialScale::new(0.05),
                    ..default()
                })
                .set(bevy::asset::AssetPlugin {
                    file_path: asset_root.path.to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "STARFORGE 星穹熔炉 · Bevy 移植版".into(),
                        resolution: (1280, 720).into(),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                }),
        );
    // AssetPlugin's root is read during plugin build, but the summary must be
    // logged after DefaultPlugins installed the tracing subscriber.
    if asset_root.source == AssetRootSource::Override {
        warn!(
            "STARFORGE_ASSET_DIR override active: {}",
            asset_root.path.display()
        );
    }
    log_asset_bootstrap(&bootstrap);
    // D01: a diagnostic probe, never persisted to settings.json.
    if let Some(value) = options.render_probe.as_deref() {
        match crate::rendering::ProbeMode::parse(value) {
            Some(mode) => {
                app.insert_resource(crate::rendering::ProbeRequest::enabled(mode));
            }
            None => warn!(
                "--render-probe 的值 `{value}` 无效，已忽略（可选 off/false-color/zebra/luminance）"
            ),
        }
    }
    // E01: cloud-shell debug view, also diagnostic-only.
    if let Some(value) = options.cloud_debug.as_deref() {
        match crate::weather::CloudDebugMode::parse(value) {
            Some(mode) => {
                app.insert_resource(crate::weather::CloudDebug { mode });
            }
            None => warn!(
                "--cloud-debug 的值 `{value}` 无效，已忽略（可选 off/density/interval/transmittance/scattering/steps/depth/light/faces）"
            ),
        }
    }
    add_plugin_once(&mut app, EguiPlugin::default());
    app.add_plugins(StarForgePlugins::new(
        settings,
        cloud_tuning,
        lighting_tuning,
    ));
    add_plugin_once(&mut app, GameFlowPlugin);
    // A03 fallback probe: lower probed limits without touching real settings.
    if options.simulate_low_device {
        app.insert_resource(crate::visual::CapabilitySimulation::low_device());
    }
    // C01 debug overlay: read-only coverage visualization, off by default.
    if options.terrain_overlay {
        app.insert_resource(crate::terrain::TerrainOverlay(true));
    }
    // Visual QA needs its config resource before the plugin is built so the
    // plugin can stay a no-op in normal runs.
    if let Some(config) = options.visual_qa.clone() {
        app.insert_resource(crate::WorldRequest {
            world_name: "visual_qa".into(),
            char_name: "visual_qa".into(),
            seed: config.seed,
            biome: config.biome.clone(),
            difficulty: crate::data::Difficulty::Creative,
            load: false,
            appearance: crate::save::Appearance::random(config.seed),
        });
        app.insert_resource(config);
    }
    add_plugin_once(&mut app, VisualQaPlugin);
    if options.smoke || options.play {
        // 自动建世界进入游戏（--play 不退出，供交互验证；--smoke 额外自测退出）
        app.insert_resource(crate::WorldRequest {
            world_name: "smoke".into(),
            char_name: "smoker".into(),
            seed: 4242,
            biome: "lush".into(),
            difficulty: crate::data::Difficulty::Normal,
            load: false,
            appearance: crate::save::Appearance::random(4242),
        });
    }
    if options.smoke {
        app.insert_resource(crate::SmokeFlag { frames: 0 });
    }
    if options.play || options.smoke || options.visual_qa.is_some() {
        app.insert_resource(crate::player::CursorCaptureDisabled);
    }
    app.run();
}

fn install_panic_hook() {
    // Silence the expected egui first-frame font bootstrap panic (caught & retried
    // by ui::egui_fonts_ready); keep printing any other panic.
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        if msg.contains("No fonts available until first call to Context::run()") {
            return;
        }
        eprintln!("panic: {msg}");
        if let Some(loc) = info.location() {
            eprintln!("  at {loc}");
        }
    }));
}

/// 发布版资源根目录固定在可执行文件旁，避免受启动时当前工作目录或
/// `CARGO_MANIFEST_DIR` 环境变量影响。
fn executable_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
}

// ---------- Startup ----------

fn startup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    settings: Res<crate::save::Settings>,
) {
    // persistent camera: created now so bevy_egui's primary context exists in menus too.
    // The player camera system drives it during Playing.
    let cam = commands
        .spawn((
            Camera3d::default(),
            Hdr,
            AtmosphereSettings {
                rendering_method: AtmosphereMode::Raymarched,
                ..default()
            },
            // Feed the raymarched sky back into PBR as diffuse environment
            // lighting; without this the small voxel ground can remain nearly
            // black even while the atmospheric sky is bright. Intensity is
            // overwritten each frame by daynight_system from the F3 tuning
            // (default 1.0 = Bevy's atmosphere example value).
            AtmosphereEnvironmentMapLight {
                intensity: 1.0,
                ..default()
            },
            // ContactShadows requires a depth prepass. Add it explicitly so
            // the requirement remains clear even if camera composition
            // changes later.
            DepthPrepass,
            ContactShadows {
                // The default 0.3 world-unit ray is too short for the
                // voxel creatures and machinery; extend it to cover their
                // feet-to-ground contact without turning it into a second
                // long-range shadow system.
                linear_steps: 24,
                thickness: 0.15,
                length: 2.5,
            },
            // Ambient fill alone cannot know that a voxel ceiling is above
            // the camera. SSAO restores local occlusion around terrain and
            // the contact areas that should remain dark.
            ScreenSpaceAmbientOcclusion::default(),
            Bloom {
                // Keep normal materials out of the glow and reserve Bloom
                // for the over-bright sun and genuinely emissive pixels.
                intensity: 0.12,
                low_frequency_boost: 0.35,
                prefilter: BloomPrefilter {
                    threshold: 1.5,
                    threshold_softness: 0.2,
                },
                ..Bloom::NATURAL
            },
            Tonemapping::AcesFitted,
            // Fixed physical daylight exposure (matches Bevy's atmosphere
            // example: RAW_SUNLIGHT at EV100 13). Auto exposure was removed:
            // it normalized the metered region back to middle gray every
            // frame, so the F3 lighting sliders (ambient/sun) had no visible
            // effect — exactly the "ambient max still dark" report. The JS
            // original has no auto exposure either; lighting is manual.
            Exposure { ev100: 13.0 },
            Msaa::Off,
            Projection::Perspective(PerspectiveProjection {
                fov: 75f32.to_radians(),
                far: crate::space::CAM_FAR,
                ..default()
            }),
            Transform::from_xyz(96.0, 90.0, 96.0),
            // Bevy's spatial audio uses the camera as the listener.  The ear
            // gap is expressed in game units and is scaled by AudioPlugin.
            bevy::audio::SpatialListener::new(2.0),
            // 高度雾（JS planetScene.fog 移植）：远景融入天穹，隐藏流式区块边缘；
            // daynight_system 会在爬升时动态收拢雾距（替代原曲率/淡出着色器）
            DistanceFog {
                color: Color::srgb(0.7, 0.85, 1.0),
                directional_light_color: Color::WHITE,
                directional_light_exponent: 1.0,
                falloff: FogFalloff::Linear {
                    start: 90.0,
                    end: 1050.0,
                },
            },
        ))
        .id();
    // 像素风低分辨率渲染：3D 相机渲染到 640×360 目标，UI 相机全屏最近邻放大
    if settings.pixelated {
        let mut lowres = Image::new(
            bevy::render::render_resource::Extent3d {
                width: 640,
                height: 360,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            // The camera is HDR because Atmosphere, SunDisk, Bloom and the
            // sunlight exposure all operate before tonemapping. Keep the
            // pixelated render target HDR as well, otherwise the post-process
            // chain would be clipped at 1.0 before it reaches the screen.
            vec![0u8; 640 * 360 * 8],
            bevy::render::render_resource::TextureFormat::Rgba16Float,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        // 作为渲染目标必须带 RENDER_ATTACHMENT（Image::new 默认仅绑定/拷贝）
        lowres.texture_descriptor.usage |=
            bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT;
        lowres.sampler = bevy::image::ImageSampler::nearest();
        let lowres = images.add(lowres);
        commands
            .entity(cam)
            .insert(RenderTarget::Image(ImageRenderTarget {
                handle: lowres.clone(),
                scale_factor: 1.0,
            }));
        commands.insert_resource(PixelTarget(lowres.clone()));
        // UI 相机 + 全屏放大节点
        commands.spawn((
            Camera2d,
            Camera {
                order: 10,
                ..default()
            },
        ));
        commands.spawn((
            Node {
                width: bevy::ui::Val::Percent(100.0),
                height: bevy::ui::Val::Percent(100.0),
                ..default()
            },
            bevy::ui::widget::ImageNode::new(lowres),
            PixelUpscale,
        ));
    }
}

/// 像素风渲染目标（设置开启时在 startup 创建）。
#[derive(Resource)]
struct PixelTarget(pub Handle<Image>);

#[derive(Component)]
struct PixelUpscale;

/// 递归复制目录（自解压素材用）。
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            let _ = std::fs::copy(entry.path(), &target);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct DummyPlugin;

    impl Plugin for DummyPlugin {
        fn build(&self, _app: &mut App) {}
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn launch_options_parse_every_flag() {
        let options = LaunchOptions::parse(&args(&[
            "--smoke",
            "--play",
            "--clouds-off",
            "--climate-on",
            "--legacy-lod",
            "--simulate-low-device",
            "--lod-log",
            "--art-audit",
            "--texture-audit",
            "--terrain-overlay",
            "--render-probe",
            "zebra",
            "--cloud-debug",
            "density",
        ]));
        assert!(options.smoke);
        assert!(options.play);
        assert!(options.clouds_off);
        assert!(options.climate_on);
        assert!(options.legacy_lod);
        assert!(options.simulate_low_device);
        assert!(options.lod_log);
        assert!(options.art_audit);
        assert!(options.texture_audit);
        assert!(options.terrain_overlay);
        assert_eq!(options.render_probe.as_deref(), Some("zebra"));
        assert_eq!(options.cloud_debug.as_deref(), Some("density"));
        assert!(options.visual_qa.is_none());
    }

    #[test]
    fn launch_options_delegate_visual_qa_parsing() {
        let options = LaunchOptions::parse(&args(&[
            "--visual-qa",
            "--visual-qa-seed",
            "1337",
            "--visual-qa-no-capture",
        ]));
        let config = options.visual_qa.expect("visual qa enabled");
        assert_eq!(config.seed, 1337);
        assert!(!config.capture);
        assert!(!options.smoke);
    }

    #[test]
    fn duplicate_plugin_registration_is_ignored() {
        let mut app = App::new();
        add_plugin_once(&mut app, DummyPlugin);
        add_plugin_once(&mut app, DummyPlugin);
        assert!(app.is_plugin_added::<DummyPlugin>());
    }

    #[test]
    fn asset_root_override_wins() {
        let exe = Path::new("C:/game");
        let root = resolve_asset_root(Some(exe), Some("  D:/cold-install/assets "));
        assert_eq!(root.path, PathBuf::from("D:/cold-install/assets"));
        assert_eq!(root.source, AssetRootSource::Override);

        let empty = resolve_asset_root(Some(exe), Some("   "));
        assert_eq!(empty.path, exe.join("assets"));
        assert_eq!(empty.source, AssetRootSource::Executable);

        let fallback = resolve_asset_root(None, None);
        assert_eq!(fallback.path, PathBuf::from("assets"));
    }

    #[test]
    fn asset_inventory_separates_required_and_optional() {
        let root = std::env::temp_dir().join(format!("starforge-a06-inv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for file in REQUIRED_ASSET_FILES {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"x").expect("write");
        }
        let first_optional = OPTIONAL_ASSET_FILES[0];
        let optional_path = root.join(first_optional);
        std::fs::create_dir_all(optional_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&optional_path, b"x").expect("write");

        let report = classify_assets(&root);
        assert!(report.has_required());
        assert!(!report.is_full(), "only one optional pack file is present");
        assert_eq!(report.present_optional, 1);
        assert_eq!(
            report.missing_optional.len(),
            OPTIONAL_ASSET_FILES.len() - 1
        );
        assert!(
            !report
                .missing_optional
                .contains(&first_optional.to_string())
        );
        assert_eq!(report.mode(), "minimal");

        for file in OPTIONAL_ASSET_FILES {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"x").expect("write");
        }
        let full = classify_assets(&root);
        assert!(full.is_full());
        assert_eq!(full.mode(), "full");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_requires_source_only_when_files_are_missing() {
        let root = std::env::temp_dir().join(format!("starforge-a06-boot-{}", std::process::id()));
        let source = root.join("source");
        let asset_root = root.join("assets");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&asset_root).expect("mkdir");
        for file in REQUIRED_ASSET_FILES {
            let path = asset_root.join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"x").expect("write");
        }

        let untouched = bootstrap_assets(&asset_root, Some(&source));
        assert!(!untouched.copied_from_source);
        assert!(untouched.has_required());

        let source_file = source.join(REQUIRED_ASSET_FILES[0]);
        std::fs::create_dir_all(source_file.parent().expect("parent")).expect("mkdir");
        std::fs::write(&source_file, b"x").expect("write");
        std::fs::remove_file(asset_root.join(REQUIRED_ASSET_FILES[0])).expect("remove");
        let copied = bootstrap_assets(&asset_root, Some(&source));
        assert!(copied.copied_from_source);
        assert!(copied.has_required());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_refreshes_changed_shaders_without_recopying_models() {
        let root = std::env::temp_dir().join(format!("starforge-e01-boot-{}", std::process::id()));
        let source = root.join("source");
        let asset_root = root.join("assets");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&asset_root).expect("mkdir");
        for file in REQUIRED_ASSET_FILES {
            let path = asset_root.join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"stale").expect("write");
            let source_path = source.join(file);
            std::fs::create_dir_all(source_path.parent().expect("parent")).expect("mkdir");
            // Models keep the same bytes; shaders change.
            let bytes = if file.starts_with("shaders/") {
                b"fresh".as_slice()
            } else {
                b"stale".as_slice()
            };
            std::fs::write(&source_path, bytes).expect("write");
        }

        let report = bootstrap_assets(&asset_root, Some(&source));
        assert!(report.copied_from_source);
        let first_shader = REQUIRED_ASSET_FILES
            .iter()
            .find(|file| file.starts_with("shaders/"))
            .expect("shader in required list");
        assert_eq!(
            std::fs::read(asset_root.join(first_shader)).expect("read"),
            b"fresh"
        );
        assert_eq!(
            std::fs::read(asset_root.join(REQUIRED_ASSET_FILES[0])).expect("read"),
            b"stale",
            "models must not be recopied just because shaders changed"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
