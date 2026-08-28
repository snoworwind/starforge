//! STARFORGE 星穹熔炉 — Bevy 移植版 (main entry + game flow).
//!
//! 部分数据字段和辅助入口按移植规格完整保留，尚不一定被当前运行路径读取。
#![allow(dead_code)]
// Bevy systems naturally expose their resources and queries as function parameters.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod audio;
mod char;
mod creatures;
mod data;
mod daynight;
mod factory;
mod feedback;
mod inventory;
mod lod;
mod materials;
mod network;
mod planet_scale;
mod player;
mod quests;
mod rng;
mod save;
mod schedule;
mod space;
mod station;
mod textures;
mod ui;
mod weather;
mod world;

use bevy::camera::{Exposure, Hdr, ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{
    AtmosphereEnvironmentMapLight, DirectionalLightShadowMap, atmosphere::ScatteringMedium,
};
use bevy::pbr::{
    AtmosphereMode, AtmosphereSettings, ContactShadows, DistanceFog, FogFalloff,
    ScreenSpaceAmbientOcclusion,
};
use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::prelude::*;
use bevy::window::{CursorOptions, PresentMode};
use bevy_egui::{EguiContexts, EguiPlugin, egui};
use materials::TerrainMaterials;
use player::{Beam, Player};
pub use schedule::{
    GameSet, GameState, InGame, creature_mode, ground_mode, ground_scene_mode, in_planet_mode,
    walk_look_mode,
};
use space::{FlightCamera, FlightMode, ShipAsset, ShipState, SpaceGame, SpaceInput};
use textures::AtlasRes;
use ui::{Research, UiState};
use world::{ChunkMesh, World};

#[inline]
fn finite_clamp(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn safe_player_position(pos: [f32; 3]) -> Option<Vec3> {
    if !pos.iter().all(|value| value.is_finite()) {
        return None;
    }
    Some(Vec3::new(
        pos[0].clamp(-1_000_000.0, 1_000_000.0),
        pos[1].clamp(-256.0, planet_scale::PLANET_SCALE.atmosphere_top + 256.0),
        pos[2].clamp(-1_000_000.0, 1_000_000.0),
    ))
}

/// What the user asked the Loading state to build.
#[derive(Resource)]
struct WorldRequest {
    world_name: String,
    char_name: String,
    seed: u32,
    biome: String,
    difficulty: data::Difficulty,
    load: bool,
    appearance: save::Appearance,
}

#[derive(Resource)]
struct LoadingState {
    world: World,
    world_name: String,
    char_name: String,
    difficulty: data::Difficulty,
    char_data: Option<save::CharData>,
    world_data: Option<save::WorldData>,
    techs: Vec<String>,
    spawn: Vec3,
    mats: Option<TerrainMaterials>,
    done: bool,
    start_t: f32,
}

#[derive(Resource)]
struct SaveNames {
    world: String,
    char: String,
}

/// Present only in `--smoke` mode: auto-start a world, run a few seconds, exit.
#[derive(Resource)]
struct SmokeFlag {
    frames: u32,
}

/// 像素风渲染目标（设置开启时在 startup 创建）。
#[derive(Resource)]
struct PixelTarget(pub Handle<Image>);

#[derive(Component)]
struct PixelUpscale;

fn smoke_exit(
    flag: Option<ResMut<SmokeFlag>>,
    mut mode: ResMut<FlightMode>,
    mut ship: ResMut<ShipState>,
    game: Option<Res<SpaceGame>>,
    mut save_ev: MessageWriter<ui::SaveEvent>,
    chunks: Query<&Visibility, With<ChunkMesh>>,
    clear: Res<ClearColor>,
) {
    if let Some(mut f) = flag {
        f.frames += 1;
        // 自测第二阶段：120 帧后切换到太空，验证太空场景/飞行/相机管线。
        // 出球点取近赤道方向（Y≈0），使旧代码的"按玩家高度算太空因子"必然退化成大气色，
        // 以覆盖"太空背景变大气色"回归。
        if f.frames == 120
            && let Some(g) = game.as_ref()
        {
            let p0 = &g.galaxy.planets[0];
            let center = Vec3::from(p0.pos);
            ship.pos = center + Vec3::new(0.9, 0.1, 0.4).normalize() * (p0.radius + 400.0);
            ship.speed = 20.0;
            *mode = FlightMode::Space;
            println!("SMOKE_STAGE space");
        }
        // 太空回归检查：地形区块必须隐藏（平面地形残影/球壳错位）；ClearColor 必须是太空黑
        if f.frames == 200 {
            let n = chunks.iter().count();
            let all_hidden = chunks.iter().all(|v| *v == Visibility::Hidden);
            // daynight 以线性空间写 ClearColor（lerp_color 转 LinearRgba），按线性值断言
            let c = clear.0.to_linear();
            println!(
                "SMOKE_CHECK space mode={mode:?} chunks={n} hidden={all_hidden} clear=({:.4},{:.4},{:.4})",
                c.red, c.green, c.blue
            );
            assert_eq!(
                *mode,
                FlightMode::Space,
                "smoke: expected Space at frame 200"
            );
            assert!(
                n > 0,
                "smoke: no chunk meshes present for space visibility check"
            );
            assert!(
                all_hidden,
                "smoke: chunk terrain must be hidden in space mode"
            );
            assert!(
                c.red < 0.03 && c.green < 0.03 && c.blue < 0.06,
                "smoke: space clear color must be black, got {:?}",
                clear.0
            );
        }
        // 存档路径验证：地面存档 + 太空存档
        if f.frames == 60 || f.frames == 300 {
            save_ev.write(ui::SaveEvent { quit_after: false });
            println!("SMOKE_STAGE save@{}", f.frames);
        }
        if f.frames > 480 {
            println!("SMOKE_OK frames={}", f.frames);
            std::process::exit(0);
        }
    }
}

/// In --smoke/--play mode, immediately leave the menu and start loading a world.
fn smoke_boot(req: Option<Res<WorldRequest>>, mut next: ResMut<NextState<GameState>>) {
    if req.is_some() {
        next.set(GameState::Loading);
    }
}

// ---------- Plugin composition ----------

/// 全部游戏模块插件的装配顺序（依赖序）。
pub struct StarForgePlugins {
    settings: save::Settings,
    cloud_tuning: weather::CloudTuning,
    lighting_tuning: daynight::LightingTuning,
}

impl StarForgePlugins {
    pub fn new(
        settings: save::Settings,
        cloud_tuning: weather::CloudTuning,
        lighting_tuning: daynight::LightingTuning,
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
            .add(rng::RngPlugin)
            .add(data::DataPlugin)
            .add(planet_scale::PlanetScalePlugin)
            .add(inventory::InventoryPlugin)
            .add(save::SaveSettingsPlugin(self.settings))
            .add(audio::GameAudioPlugin)
            .add(textures::TexturePlugin)
            .add(feedback::FeedbackPlugin)
            .add(ui::UiPlugin)
            .add(quests::QuestsPlugin)
            .add(char::CharPlugin)
            .add(materials::MaterialsPlugin)
            .add(daynight::DayNightPlugin {
                lighting: self.lighting_tuning,
            })
            .add(weather::WeatherPlugin {
                cloud: self.cloud_tuning,
            })
            .add(player::PlayerPlugin)
            .add(creatures::CreaturesPlugin)
            .add(factory::FactoryPlugin)
            .add(station::StationPlugin)
            .add(space::SpacePlugin)
            .add(network::NetworkPlugin)
            .add(lod::LodPlugin)
            .add(world::WorldPlugin)
    }
}

/// 游戏流程插件：状态机、菜单/加载、进驻清理与调度契约配置。
pub struct GameFlowPlugin;

impl Plugin for GameFlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .add_systems(Startup, startup)
            .add_systems(OnEnter(GameState::Loading), on_enter_loading)
            .add_systems(OnExit(GameState::Loading), on_exit_loading)
            .add_systems(
                Update,
                (
                    (smoke_boot, menu_system)
                        .chain()
                        .in_set(GameSet::Menu)
                        .run_if(in_state(GameState::Menu)),
                    loading_system
                        .in_set(GameSet::Loading)
                        .run_if(in_state(GameState::Loading)),
                ),
            )
            // playing 后段：星球切换/可见性（天空同步归 daynight 插件）
            .add_systems(
                Update,
                ((planet_switch_system, ground_scene_visibility_system)
                    .chain()
                    .in_set(GameSet::LateSwitchFlow)
                    .run_if(in_state(GameState::Playing)),),
            )
            // playing 保存尾链（map → save → quit → smoke）
            .add_systems(
                Update,
                (
                    (save_settings_system, save_system)
                        .chain()
                        .in_set(GameSet::SaveWrite),
                    (quit_to_menu_system, smoke_exit)
                        .chain()
                        .in_set(GameSet::SaveQuit),
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
        crate::schedule::configure(app);
    }
}

fn main() {
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
    let smoke = std::env::args().any(|a| a == "--smoke");
    let play = std::env::args().any(|a| a == "--play");
    let asset_dir = executable_asset_dir();
    let mut settings = save::load_settings();
    // Read-only visual probe overrides. They never persist to settings.json.
    if std::env::args().any(|arg| arg == "--clouds-off") {
        settings.clouds = false;
    }
    if std::env::args().any(|arg| arg == "--legacy-lod") {
        settings.lod_mode = save::LodMode::Legacy;
    }
    let cloud_tuning = weather::CloudTuning::from_settings(&settings);
    let lighting_tuning = daynight::LightingTuning::from_settings(&settings);
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.05, 0.07, 0.1)))
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
                    file_path: asset_dir,
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
        )
        .add_plugins(EguiPlugin::default())
        .add_plugins(StarForgePlugins::new(
            settings,
            cloud_tuning,
            lighting_tuning,
        ))
        .add_plugins(GameFlowPlugin);
    if smoke || play {
        // 自动建世界进入游戏（--play 不退出，供交互验证；--smoke 额外自测退出）
        app.insert_resource(WorldRequest {
            world_name: "smoke".into(),
            char_name: "smoker".into(),
            seed: 4242,
            biome: "lush".into(),
            difficulty: data::Difficulty::Normal,
            load: false,
            appearance: save::Appearance::random(4242),
        });
    }
    if smoke {
        app.insert_resource(SmokeFlag { frames: 0 });
    }
    app.run();
}

/// 发布版资源根目录固定在可执行文件旁，避免受启动时当前工作目录或
/// `CARGO_MANIFEST_DIR` 环境变量影响。
fn executable_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
}

fn executable_asset_dir() -> String {
    executable_dir()
        .map(|dir| dir.join("assets"))
        .unwrap_or_else(|| std::path::PathBuf::from("assets"))
        .to_string_lossy()
        .into_owned()
}

// ---------- Startup ----------

fn startup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    settings: Res<save::Settings>,
) {
    // Self-extract the model assets next to the executable so the
    // AssetServer finds them regardless of the working directory.
    if let Some(dir) = executable_dir() {
        // 本地开发构建时把模型目录复制到 exe 旁（源码在
        // <crate>/target/<profile>/ 下时向上两级即 crate 根）。发布包则应
        // 直接携带 exe 旁的 assets/models，不依赖源代码目录。
        let models_dir = dir.join("assets").join("models");
        let required_model_files = [
            "creatures/quaternius_alpaca.gltf",
            "creatures/quaternius_deer.gltf",
            "creatures/quaternius_fox.gltf",
            "creatures/quaternius_wolf.gltf",
            "creatures/sentinel.glb",
            "asteroids/meteor.glb",
            "asteroids/meteor_detailed.glb",
            "external/ships/space_ship_b/scene.gltf",
            "external/stations/space_station/scene.gltf",
        ];
        if required_model_files
            .iter()
            .any(|path| !models_dir.join(path).exists())
        {
            let mut src: Option<std::path::PathBuf> = None;
            let via_exe = dir.join("..").join("..").join("assets").join("models");
            if via_exe.is_dir() {
                src = Some(via_exe);
            }
            if let Some(s) = src {
                let _ = copy_dir_all(&s, &models_dir);
            }
        }
        let shaders_dir = dir.join("assets").join("shaders");
        let via_exe = dir.join("..").join("..").join("assets").join("shaders");
        if via_exe.is_dir() {
            let _ = copy_dir_all(&via_exe, &shaders_dir);
        }
    }
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
                far: space::CAM_FAR,
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

// ---------- Menu ----------

#[derive(Default, PartialEq)]
enum MenuScreen {
    #[default]
    Title,
    NewWorld,
    CharCreate,
    LoadWorld,
}

#[derive(Resource, Default)]
struct MenuState {
    screen: MenuScreen,
    world_name: String,
    char_name: String,
    seed_text: String,
    biome_idx: usize,
    difficulty: u8,
    creative: bool,
    error: Option<String>,
    appearance: Option<save::Appearance>,
}

fn menu_system(
    mut contexts: EguiContexts,
    mut menu: Local<MenuState>,
    mut commands: Commands,
    mut next: ResMut<NextState<GameState>>,
    settings: Res<save::Settings>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !ui::egui_fonts_ready(ctx) {
        return;
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "root_panel".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::CentralPanel::default().show(&mut root, |ui| {
        match menu.screen {
            MenuScreen::Title => {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.label(
                        egui::RichText::new("STARFORGE")
                            .size(56.0)
                            .strong()
                            .color(egui::Color32::from_rgb(0x35, 0xe0, 0xe8)),
                    );
                    ui.label(egui::RichText::new("星穹熔炉 · 体素星际工厂").size(22.0));
                    ui.label(
                        egui::RichText::new("Bevy (Rust) 移植版")
                            .size(14.0)
                            .color(egui::Color32::GRAY),
                    );
                    ui.add_space(48.0);
                    let r_new = ui.add_sized([260.0, 40.0], egui::Button::new("🚀 新世界"));
                    if r_new.clicked() {
                        menu.screen = MenuScreen::NewWorld;
                        menu.world_name = random_planet_name();
                        menu.char_name = "探险家".into();
                        menu.seed_text = format!("{}", rand_seed());
                        menu.biome_idx = 0;
                        menu.difficulty = 1;
                        menu.creative = false;
                        menu.error = None;
                    }
                    if ui
                        .add_sized([260.0, 40.0], egui::Button::new("📂 读取世界"))
                        .clicked()
                    {
                        menu.screen = MenuScreen::LoadWorld;
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(format!("渲染距离: {} 区块", settings.view_dist));
                        ui.label(format!("灵敏度: {:.1}", settings.mouse_sens));
                        ui.label(format!(
                            "渲染: {}",
                            if settings.pixelated {
                                "像素"
                            } else {
                                "现代"
                            }
                        ));
                    });
                    ui.add_space(20.0);
                    if ui
                        .add_sized([260.0, 36.0], egui::Button::new("🚪 退出"))
                        .clicked()
                    {
                        std::process::exit(0);
                    }
                });
            }
            MenuScreen::NewWorld => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(egui::RichText::new("创建新世界").size(28.0).strong());
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.label("世界名");
                        ui.text_edit_singleline(&mut menu.world_name);
                    });
                    ui.horizontal(|ui| {
                        ui.label("角色名");
                        ui.text_edit_singleline(&mut menu.char_name);
                    });
                    ui.horizontal(|ui| {
                        ui.label("种子");
                        ui.text_edit_singleline(&mut menu.seed_text);
                    });
                    ui.horizontal(|ui| {
                        ui.label("星球生态");
                        egui::ComboBox::from_id_salt("biome")
                            .selected_text(data::BIOMES[menu.biome_idx].name)
                            .show_ui(ui, |ui| {
                                for (i, b) in data::BIOMES.iter().enumerate() {
                                    let label = format!(
                                        "{}  {}",
                                        b.name,
                                        if b.haz.is_some() { b.haz_name } else { "·" }
                                    );
                                    ui.selectable_value(&mut menu.biome_idx, i, label);
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("难度");
                        for (i, d) in [
                            data::Difficulty::Easy,
                            data::Difficulty::Normal,
                            data::Difficulty::Hard,
                        ]
                        .iter()
                        .enumerate()
                        {
                            if ui
                                .selectable_label(
                                    menu.difficulty == i as u8 && !menu.creative,
                                    d.label(),
                                )
                                .clicked()
                            {
                                menu.difficulty = i as u8;
                                menu.creative = false;
                            }
                        }
                        if ui
                            .selectable_label(menu.creative, data::Difficulty::Creative.label())
                            .clicked()
                        {
                            menu.creative = true;
                        }
                    });
                    if let Some(e) = &menu.error {
                        ui.label(egui::RichText::new(e.clone()).color(egui::Color32::RED));
                    }
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        if ui.button("◀ 返回").clicked() {
                            menu.screen = MenuScreen::Title;
                        }
                        if ui.button("▶ 下一步").clicked() {
                            if menu.world_name.trim().is_empty() {
                                menu.error = Some("世界名不能为空".into());
                            } else {
                                let seed: u32 = menu
                                    .seed_text
                                    .trim()
                                    .parse()
                                    .unwrap_or_else(|_| rand_seed());
                                let _ = seed;
                                menu.error = None;
                                menu.screen = MenuScreen::CharCreate;
                                if menu.appearance.is_none() {
                                    menu.appearance = Some(save::Appearance::random(rand_seed()));
                                }
                            }
                        }
                    });
                });
            }
            MenuScreen::CharCreate => {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(egui::RichText::new("创建角色").size(26.0).strong());
                    ui.add_space(10.0);
                    let mut app = menu
                        .appearance
                        .clone()
                        .unwrap_or_else(|| save::Appearance::random(rand_seed()));
                    let mut name = menu.char_name.clone();
                    ui.horizontal(|ui| {
                        ui.label("角色名");
                        ui.text_edit_singleline(&mut name);
                    });
                    ui.add_space(8.0);
                    // 外观编辑
                    let opts_skin = [
                        "#e8c49a", "#d8b48a", "#c89878", "#8d5a3c", "#6b4630", "#f0d8b8",
                        "#b98e6a", "#e8d0b0",
                    ];
                    let opts_hair = [
                        "#4a3018", "#2e2620", "#5a4632", "#7a5a8a", "#a86a3a", "#d8c8a8",
                        "#c23a3a", "#1e2e4a",
                    ];
                    let opts_suit = [
                        "#4a5a6e", "#3fa8c9", "#5a3e3e", "#6e6a2a", "#3e5a6e", "#4a4258",
                        "#5a6a3a", "#7a3a2a",
                    ];
                    let opts_trim = [
                        "#35e0e8", "#ffb347", "#ff6a5e", "#b58aff", "#7dff8a", "#ffd94d",
                        "#f0f0f0", "#35b0ff",
                    ];
                    let opts_pants = [
                        "#33404c", "#4a3c2e", "#2e3a44", "#3a3248", "#3e3a2e", "#443430",
                    ];
                    let opts_boots = [
                        "#1e262e", "#2e2620", "#26221a", "#241e2e", "#2a221e", "#33261a",
                    ];
                    let opts_visor = [
                        "#ffb347", "#35e0e8", "#ff6a5e", "#b58aff", "#7dff8a", "#f0f0f0",
                    ];
                    let styles = ["none", "short", "long", "pony", "mohawk", "bun"];
                    egui::Grid::new("char_edit")
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            swatch_row(ui, "肤色", &opts_skin, &mut app.skin);
                            ui.end_row();
                            ui.label("发型");
                            egui::ComboBox::from_id_salt("hair_style")
                                .selected_text(save::Appearance::style_label(&app.hair_style))
                                .show_ui(ui, |ui| {
                                    for s in styles {
                                        ui.selectable_value(
                                            &mut app.hair_style,
                                            s.to_string(),
                                            save::Appearance::style_label(s),
                                        );
                                    }
                                });
                            ui.end_row();
                            swatch_row(ui, "发色", &opts_hair, &mut app.hair);
                            ui.end_row();
                            swatch_row(ui, "制服", &opts_suit, &mut app.suit);
                            ui.end_row();
                            swatch_row(ui, "饰条", &opts_trim, &mut app.trim);
                            ui.end_row();
                            swatch_row(ui, "裤装", &opts_pants, &mut app.pants);
                            ui.end_row();
                            swatch_row(ui, "靴子", &opts_boots, &mut app.boots);
                            ui.end_row();
                            swatch_row(ui, "目镜", &opts_visor, &mut app.visor);
                            ui.end_row();
                            ui.label("头盔");
                            ui.checkbox(&mut app.helmet, "开启");
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    let mut go = false;
                    let mut back = false;
                    ui.horizontal(|ui| {
                        if ui.button("🎲 随机外观").clicked() {
                            app = save::Appearance::random(rand_seed());
                        }
                        if ui.button("◀ 返回").clicked() {
                            back = true;
                        }
                        if ui.button("▶ 出发！").clicked() {
                            go = true;
                        }
                    });
                    menu.char_name = name;
                    menu.appearance = Some(app.clone());
                    if back {
                        menu.screen = MenuScreen::NewWorld;
                    }
                    if go {
                        let seed: u32 = menu
                            .seed_text
                            .trim()
                            .parse()
                            .unwrap_or_else(|_| rand_seed());
                        let difficulty = if menu.creative {
                            data::Difficulty::Creative
                        } else {
                            [
                                data::Difficulty::Easy,
                                data::Difficulty::Normal,
                                data::Difficulty::Hard,
                            ][menu.difficulty as usize]
                        };
                        commands.insert_resource(WorldRequest {
                            world_name: menu.world_name.trim().to_string(),
                            char_name: if menu.char_name.trim().is_empty() {
                                "旅行者".to_string()
                            } else {
                                menu.char_name.trim().to_string()
                            },
                            seed,
                            biome: data::BIOMES[menu.biome_idx].key.to_string(),
                            difficulty,
                            load: false,
                            appearance: app.clone(),
                        });
                        next.set(GameState::Loading);
                    }
                });
            }
            MenuScreen::LoadWorld => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(egui::RichText::new("读取世界").size(28.0).strong());
                    ui.add_space(16.0);
                    let worlds = save::world_summaries();
                    if worlds.is_empty() {
                        ui.label("（暂无存档）");
                    }
                    for (name, seed, biome) in worlds {
                        let biome_name = data::biome_by_key(&biome).name;
                        ui.horizontal(|ui| {
                            ui.label(format!("{name}  ·  种子 {seed}  ·  {biome_name}"));
                            if ui.button("进入").clicked() {
                                commands.insert_resource(WorldRequest {
                                    world_name: name.clone(),
                                    char_name: "探险家".into(),
                                    seed,
                                    biome: biome.clone(),
                                    difficulty: data::Difficulty::Normal,
                                    load: true,
                                    appearance: save::Appearance::random(rand_seed()),
                                });
                                next.set(GameState::Loading);
                            }
                        });
                    }
                    ui.add_space(16.0);
                    if ui.button("◀ 返回").clicked() {
                        menu.screen = MenuScreen::Title;
                    }
                });
            }
        }
    });
}

fn swatch_row(ui: &mut egui::Ui, label: &str, opts: &[&str], current: &mut String) {
    ui.label(label);
    ui.horizontal(|ui| {
        for c in opts {
            let selected = current == c;
            let col = hex_to_egui(c);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(3), col);
            if selected {
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            if resp.clicked() {
                *current = (*c).to_string();
            }
        }
    });
}

fn hex_to_egui(hex: &str) -> egui::Color32 {
    let h = hex.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0x888888);
    egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn rand_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(7777)
}

fn random_planet_name() -> String {
    let mut rng = rng::Rng::new(rand_seed());
    let name = data::PLANET_NAME_POOL[rng.range(data::PLANET_NAME_POOL.len())];
    let suffix = data::GALAXY_SUFFIX[rng.range(data::GALAXY_SUFFIX.len())];
    format!("{name}{suffix}")
}

// ---------- Loading ----------

fn on_enter_loading(
    mut commands: Commands,
    request: Option<Res<WorldRequest>>,
    settings: Res<save::Settings>,
) {
    let Some(req) = request else { return };
    let mut world = World::new(req.seed, &req.biome, settings.view_dist);
    let mut world_data: Option<save::WorldData> = None;
    let char_data = if req.load {
        if let Some(wd) = save::load_world(&req.world_name) {
            world.saved_mods = wd.mods.clone();
            world_data = Some(wd.clone());
            let direct = save::load_char(&req.char_name);
            if direct.is_some() {
                direct
            } else {
                let mut found = None;
                for c in save::list_chars() {
                    if let Some(cd) = save::load_char(&c)
                        && cd.world.as_deref() == Some(req.world_name.as_str())
                    {
                        found = Some(cd);
                        break;
                    }
                }
                found
            }
        } else {
            None
        }
    } else {
        None
    };
    let techs = char_data
        .as_ref()
        .map(|c| c.techs.clone())
        .unwrap_or_default();
    // Generate an anchor area before searching for a spawn. Without this the
    // initial search saw only AIR and low-view-distance worlds started at y=2.
    let in_space = world_data
        .as_ref()
        .is_some_and(|w| matches!(w.state.as_str(), "space" | "warping"));
    let saved_position = char_data
        .as_ref()
        .filter(|_| !in_space)
        .and_then(|c| safe_player_position(c.pos));
    let anchor = saved_position
        .map(|pos| (pos.x as i32, pos.z as i32))
        .unwrap_or((96, 96));
    let anchor_cx = world::cf(anchor.0 as f32);
    let anchor_cz = world::cf(anchor.1 as f32);
    for cz in anchor_cz - 2..=anchor_cz + 2 {
        for cx in anchor_cx - 2..=anchor_cx + 2 {
            world.ensure_chunk(cx, cz);
        }
    }
    let spawn = saved_position.unwrap_or_else(|| world.find_spawn(anchor.0, anchor.1));
    commands.insert_resource(LoadingState {
        world,
        world_name: req.world_name.clone(),
        char_name: req.char_name.clone(),
        difficulty: req.difficulty,
        char_data,
        world_data,
        techs,
        spawn,
        mats: None,
        done: false,
        start_t: 0.0,
    });
}

fn on_exit_loading(mut commands: Commands) {
    commands.remove_resource::<LoadingState>();
    commands.remove_resource::<WorldRequest>();
}

#[allow(clippy::too_many_arguments)]
fn loading_system(
    mut commands: Commands,
    loading: Option<ResMut<LoadingState>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut stdmats: ResMut<Assets<StandardMaterial>>,
    mut curved_mats: ResMut<Assets<materials::CurvedTerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scattering_mediums: ResMut<Assets<ScatteringMedium>>,
    atlas: Res<AtlasRes>,
    asset_server: Res<AssetServer>,
    mut contexts: EguiContexts,
    mut next: ResMut<NextState<GameState>>,
    time: Res<Time>,
) {
    let Some(mut ls) = loading else { return };
    ls.start_t += time.delta_secs();
    // build terrain materials once
    if ls.mats.is_none() {
        let mats = TerrainMaterials::build(
            &mut stdmats,
            &mut curved_mats,
            &mut images,
            atlas.atlas.to_image(),
            ls.world.biome().water_tint,
        );
        commands.insert_resource(mats.clone());
        ls.mats = Some(mats);
    }
    let Some(mats) = ls.mats.clone() else { return };
    let mut done = false;
    for _ in 0..8 {
        let pcx = world::cf(ls.spawn.x);
        let pcz = world::cf(ls.spawn.z);
        done = world::stream_world_step(
            &mut ls.world,
            pcx,
            pcz,
            &mut commands,
            &mut meshes,
            &atlas.atlas,
            &mats,
            64,
            32,
        );
        if done {
            break;
        }
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !ui::egui_fonts_ready(ctx) {
        return;
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "root_panel".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::CentralPanel::default().show(&mut root, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(300.0);
            ui.label(egui::RichText::new("正在生成星球地形…").size(22.0));
            ui.add(egui::ProgressBar::new((ls.start_t / 1.5).clamp(0.0, 1.0)).desired_width(300.0));
        });
    });
    if done || ls.start_t > 8.0 {
        let spawn = ls.spawn;
        let difficulty = ls.difficulty;
        let char_data = ls.char_data.take();
        let world_data = ls.world_data.take();
        let techs = ls.techs.clone();
        let world_name = ls.world_name.clone();
        let char_name = ls.char_name.clone();
        let seed = ls.world.seed;
        let biome = ls.world.biome().key;
        let view_dist = ls.world.view_dist;
        let world = std::mem::replace(&mut ls.world, World::new(seed, biome, view_dist));
        spawn_scene(
            &mut commands,
            &mut meshes,
            &mut stdmats,
            &mut scattering_mediums,
            &asset_server,
            world,
            spawn,
            difficulty,
            char_data,
            world_data,
            techs,
            world_name,
            char_name,
        );
        next.set(GameState::Playing);
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    stdmats: &mut Assets<StandardMaterial>,
    scattering_mediums: &mut Assets<ScatteringMedium>,
    asset_server: &AssetServer,
    world: World,
    spawn: Vec3,
    difficulty: data::Difficulty,
    char_data: Option<save::CharData>,
    world_data: Option<save::WorldData>,
    techs: Vec<String>,
    world_name: String,
    char_name: String,
) {
    let biome = world.biome();
    let world_seed = world.seed;
    // `on_enter_loading` already generated and selected a valid spawn. Keep
    // it here; recomputing at the hard-coded default would discard a saved
    // position and could place the player in a different part of the world.
    let spawn = if spawn.is_finite() {
        spawn
    } else {
        world.find_spawn(96, 96)
    };
    // Restore persisted machine state first. Old saves did not store active-
    // planet machine data, so loaded block cells remain a compatibility
    // fallback below. Saved machines in unloaded chunks must be kept: their
    // blocks will be verified when those chunks stream back in.
    let mut restored_machine_positions = std::collections::HashSet::new();
    let mut valid_machine_saves = Vec::new();
    if let Some(saved) = world_data.as_ref().map(|data| data.machines.as_slice()) {
        for machine in saved {
            let pos = [machine.x, machine.y, machine.z];
            let kind = factory::MachineKind::from_block_key(&machine.kind);
            if machine.y < 0
                || machine.y >= data::WORLD_H
                || machine.x.unsigned_abs() > 1_000_000
                || machine.z.unsigned_abs() > 1_000_000
                || kind == factory::MachineKind::Other
                || !restored_machine_positions.insert(pos)
            {
                continue;
            }
            let cx = machine.x.div_euclid(data::CHUNK);
            let cz = machine.z.div_euclid(data::CHUNK);
            if world.get_chunk(cx, cz).is_some()
                && data::block_by_id(world.get(machine.x, machine.y, machine.z)).machine
                    != Some(kind.block_key())
            {
                restored_machine_positions.remove(&pos);
                continue;
            }
            valid_machine_saves.push(machine.clone());
        }
    }
    factory::deserialize_machines(commands, &valid_machine_saves);

    // Spawn default logical state for machine blocks not represented in old
    // or partially recovered saves.
    let machine_cells: Vec<([i32; 3], u8)> = world
        .chunks
        .values()
        .flat_map(|c| {
            c.data
                .iter()
                .enumerate()
                .filter(|(_, id)| factory::MACHINE_BLOCK_IDS.contains(*id))
                .map(|(i, &id)| {
                    (
                        [
                            c.cx * 16 + (i % 16) as i32,
                            (i / 256) as i32,
                            c.cz * 16 + ((i / 16) % 16) as i32,
                        ],
                        id,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for (cell, id) in machine_cells {
        if restored_machine_positions.contains(&cell) {
            continue;
        }
        let key = data::block_by_id(id).key;
        factory::spawn_machine(commands, cell, key, 0);
    }
    let day_t = world_data.as_ref().map(|saved| saved.day_t).unwrap_or(0.30);
    commands.insert_resource(daynight::DayTime(day_t));
    commands.insert_resource(daynight::SpaceFactor::default());
    let earth_medium = scattering_mediums.add(ScatteringMedium::earth(256, 256));
    commands.insert_resource(daynight::AtmosphereAssets {
        earth: earth_medium.clone(),
    });
    let pool = daynight::spawn_sky(commands, meshes, stdmats, earth_medium);
    commands.insert_resource(materials::LampPool {
        entities: pool,
        timer: 0.0,
    });

    // ---- 角色 / 任务 / 飞船 / 星系 ----
    let saved_difficulty = char_data.as_ref().and_then(|c| match c.difficulty {
        0 => Some(data::Difficulty::Easy),
        1 => Some(data::Difficulty::Normal),
        2 => Some(data::Difficulty::Hard),
        3 => Some(data::Difficulty::Creative),
        _ => None,
    });
    let mut p = Player::new(saved_difficulty.unwrap_or(difficulty));
    p.pos = spawn;
    let appearance = char_data
        .as_ref()
        .map(|c| c.appearance.clone())
        .unwrap_or_default();
    p.appearance = appearance.clone();
    let mut quests = quests::Quests::default();
    let mut game: SpaceGame;
    let mut start_mode = FlightMode::Planet;
    let mut ship_state = ShipState::default();
    let mut warp_anim = space::WarpAnim::default();
    let mut saved_ship_hp = None;
    let mut research_active: Option<(String, f32)> = None;

    if let Some(cd) = char_data.as_ref() {
        p.equipment = cd.equipment.clone();
        p.equipment.sanitize();
        if let Some(saved_position) = safe_player_position(cd.pos) {
            p.pos = saved_position;
        }
        p.yaw = if cd.yaw.is_finite() {
            cd.yaw.clamp(-std::f32::consts::PI, std::f32::consts::PI)
        } else {
            0.0
        };
        p.pitch = if cd.pitch.is_finite() {
            cd.pitch
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2)
        } else {
            0.0
        };
        let max_shield = p.stat_max("shield");
        let max_o2 = p.stat_max("o2");
        p.stats = player::Stats {
            // A dead flag is not part of the save schema; never reload an
            // alive player with zero health and no respawn timer.
            hp: finite_clamp(cd.stats[0], 8.0, 0.1, 8.0),
            shield: finite_clamp(cd.stats[1], 6.0, 0.0, max_shield),
            o2: finite_clamp(cd.stats[2], 100.0, 0.0, max_o2),
            haz: finite_clamp(cd.stats[3], 100.0, 0.0, 100.0),
            jet: finite_clamp(cd.stats[4], 100.0, 0.0, 100.0),
            laser: finite_clamp(cd.stats[5], 100.0, 0.0, 100.0),
        };
        p.inv = crate::inventory::Inventory::from_slots(cd.inv.clone());
        p.hot_idx = cd.hot_idx.clamp(-1, 8);
        p.credits = cd.credits.max(0);
        p.play_time = if cd.play_time.is_finite() {
            cd.play_time.max(0.0)
        } else {
            0.0
        };
        quests.idx = cd.quest_idx;
        game = SpaceGame::new(load_galaxy(&world_data));
        game.fuel_loaded = cd.fuel_loaded.max(0);
        game.garage = cd.ship_garage.clone();
        research_active = cd.researching.clone();
    } else {
        p.inv.add_item("carbon", 20);
        p.inv.add_item("oxygen", 5);
        p.inv.add_item("sodium", 5);
        game = SpaceGame::new(load_galaxy(&world_data));
        game.garage = Vec::new();
    }
    // 世界档恢复
    if let Some(wd) = &world_data {
        for (k, v) in &wd.flags {
            quests.flags.insert(k.clone(), *v);
        }
        if !game.galaxy.planets.is_empty() {
            game.current_planet = wd.current_planet.min(game.galaxy.planets.len() - 1);
        }
        game.galaxy_count = wd.galaxy_count.max(1);
        if !wd.market.is_empty() {
            game.galaxy.market = wd.market.clone();
        }
        if !wd.stock.is_empty() {
            game.galaxy.stock = wd.stock.clone();
        }
        if let Some(sp) = wd.ship_pos
            && sp.iter().all(|v| v.is_finite())
        {
            game.ship_pos = Vec3::new(sp[0], sp[1], sp[2]);
        }
        // 地图标记 / 跃迁锁定 / 跨星系档案 / 放置计数（JS mapMarks/warpLock/galaxyArchives/placedCount）
        game.marks = wd.marks.clone();
        game.warp_lock = wd.warp_lock.clone();
        game.visited = wd.visited.clone();
        game.archives = wd.archives.clone();
        quests.placed = wd.placed.clone();
        quests.side = wd.side_quest.clone();
        saved_ship_hp = wd
            .ship_state
            .as_ref()
            .and_then(|state| state.hp)
            .filter(|hp| hp.is_finite());
        start_mode = match wd.state.as_str() {
            "atmo" => FlightMode::Atmo,
            "space" => FlightMode::Space,
            "warping" => FlightMode::Warping,
            _ => FlightMode::Planet,
        };
        if matches!(
            start_mode,
            FlightMode::Atmo | FlightMode::Space | FlightMode::Warping
        ) {
            if let Some(ss) = &wd.ship_state {
                if ss.pos.iter().all(|v| v.is_finite()) {
                    ship_state.pos = if start_mode == FlightMode::Atmo {
                        safe_player_position(ss.pos).unwrap_or_default()
                    } else {
                        Vec3::new(ss.pos[0], ss.pos[1], ss.pos[2])
                            .clamp(Vec3::splat(-10_000_000.0), Vec3::splat(10_000_000.0))
                    };
                }
                ship_state.yaw = if ss.yaw.is_finite() {
                    ss.yaw.rem_euclid(std::f32::consts::TAU)
                } else {
                    0.0
                };
                ship_state.pitch = if ss.pitch.is_finite() {
                    ss.pitch.clamp(-1.55, 1.55)
                } else {
                    0.0
                };
                ship_state.roll = if ss.roll.is_finite() {
                    ss.roll.rem_euclid(std::f32::consts::TAU)
                } else {
                    0.0
                };
                ship_state.speed = if ss.speed.is_finite() {
                    ss.speed.clamp(0.0, 4_800.0)
                } else {
                    0.0
                };
            }
            if start_mode == FlightMode::Warping
                && let Some(saved) = &wd.warp_anim
            {
                warp_anim = space::WarpAnim {
                    active: true,
                    t: saved.t,
                    seed: saved.seed,
                    yaw: saved.yaw,
                    pitch: saved.pitch,
                    v0: saved.v0,
                };
            }
        }
    }
    // A save made in space immediately after a cross-galaxy warp still has
    // the departed galaxy's voxel world loaded. That world is also present in
    // the archived galaxy snapshot, which lets old saves (without an explicit
    // owner field) be identified safely. The next landing must rebuild rather
    // than treating it as planet 0 of the destination galaxy.
    let loaded_world_is_archived = world_data.as_ref().is_some_and(|saved| {
        matches!(saved.state.as_str(), "space" | "warping")
            && saved.archives.values().any(|galaxy| {
                galaxy
                    .planets
                    .values()
                    .any(|planet| planet.seed == world.seed && planet.biome == world.biome().key)
            })
    });
    game.landed_planet = if loaded_world_is_archived {
        -1
    } else {
        game.current_planet as i32
    };

    // 初始飞船
    let active_flight = matches!(
        start_mode,
        FlightMode::Atmo | FlightMode::Space | FlightMode::Warping
    );
    if active_flight && ship_state.pos.is_finite() && ship_state.pos.length_squared() >= 1e-6 {
        p.pos = ship_state.pos;
    }
    let mut ship_data = save::ShipSave {
        model: "ship".into(),
        cls: "C".into(),
        name: "拓荒者号".into(),
        inv: vec![None; 12],
    };
    if let Some(cd) = char_data.as_ref()
        && (!cd.player_ship.model.is_empty()
            || !cd.player_ship.cls.is_empty()
            || !cd.player_ship.inv.is_empty())
    {
        ship_data = cd.player_ship.clone();
    }
    let cargo_slots = data::ship_class_by_key(&ship_data.cls).slots;
    ship_data.inv =
        crate::inventory::Inventory::from_slots_with_capacity(ship_data.inv.clone(), cargo_slots)
            .slots;
    // 船放在玩家出生点旁边（太空开局用占位点，船随即被同步到存档太空位置）
    let ship_anchor = if active_flight {
        Vec3::new(96.0, 40.0, 96.0)
    } else {
        p.pos
    };
    let (ship_ent, flames, ship_spawn_pos) = space::spawn_initial_ship(
        commands,
        meshes,
        stdmats,
        asset_server,
        &world,
        ship_anchor,
        &ship_data,
    );
    if game.ship_pos == Vec3::ZERO {
        game.ship_pos = ship_spawn_pos;
    }
    // A space save stores the active ship in ship_state, not in the
    // planetary parking position. Keep that position so loading in space
    // does not teleport the ship back to the planet-side spawn pad.
    if !active_flight || !ship_state.pos.is_finite() || ship_state.pos.length_squared() < 1e-6 {
        ship_state.pos = game.ship_pos;
    }
    ship_state.board_yaw = 0.0;
    ship_state.hp_max = space::vis_hp(&ship_data.cls);
    ship_state.hp = saved_ship_hp
        .unwrap_or(ship_state.hp_max)
        .clamp(0.1, ship_state.hp_max);
    commands.insert_resource(world);
    commands.insert_resource(ShipAsset {
        entity: Some(ship_ent),
        flames,
        data: ship_data.clone(),
    });
    p.toast("欢迎来到星穹熔炉 · W A S D 移动 · Tab 背包");
    let player_pos = p.pos;
    commands.spawn((
        p,
        Transform::from_translation(player_pos),
        Visibility::default(),
        InGame,
    ));
    // ghost preview
    let ghost_mat = stdmats.add(StandardMaterial {
        base_color: Color::srgba(0.21, 0.88, 0.91, 0.2),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(ui::GhostMat(ghost_mat.clone()));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(ghost_mat),
        Transform::default(),
        Visibility::Hidden,
        ui::Ghost {
            pos: Vec3::ZERO,
            scale: Vec3::ONE,
            ok: true,
            active: false,
        },
        InGame,
    ));
    // laser beam
    let beam_mat = stdmats.add(StandardMaterial {
        base_color: Color::srgb(2.0, 0.6, 0.3),
        emissive: LinearRgba::new(1.0, 0.4, 0.15, 1.0) * 2.0,
        unlit: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.03, 0.03, 1.0))),
        MeshMaterial3d(beam_mat),
        Transform::default(),
        Visibility::Hidden,
        Beam,
        InGame,
    ));
    // state resources
    commands.insert_resource(UiState::default());
    commands.insert_resource(ui::MapState::default());
    commands.insert_resource(player::BreakQueue::default());
    // 兽群恢复（MC 风格：位置/血量/领地/被杀记录随存档还原）
    {
        let mut spawner = creatures::CreatureSpawner::default();
        if let Some(wd) = &world_data {
            spawner.restore(world_seed, &wd.creatures, &wd.creature_cells);
        }
        commands.insert_resource(spawner);
    }
    commands.insert_resource(ui::ScanState::default());
    commands.insert_resource(Research {
        techs,
        active: research_active,
    });
    commands.insert_resource(SaveNames {
        world: world_name,
        char: char_name,
    });
    commands.insert_resource(game);
    commands.insert_resource(quests);
    commands.insert_resource(ship_state);
    commands.insert_resource(space::ShipRecall::default());
    commands.insert_resource(warp_anim);
    commands.insert_resource(SpaceInput::default());
    commands.insert_resource(FlightCamera::default());
    commands.insert_resource(station::StationState::default());
    commands.insert_resource(factory::Power::default());
    commands.insert_resource(space::AtmoLand::default());
    commands.insert_resource(start_mode);
    let _ = biome;
}

/// 从世界档恢复星系。
fn load_galaxy(wd: &Option<save::WorldData>) -> data::Galaxy {
    let seed = wd
        .as_ref()
        .map(|w| w.galaxy_seed)
        .unwrap_or(data::HOME_GALAXY_SEED);
    if seed == data::HOME_GALAXY_SEED {
        data::home_galaxy()
    } else {
        data::generate_galaxy(seed)
    }
}

// ---------- Playing lifecycle ----------

fn on_enter_playing(mut windows: Query<&mut CursorOptions, With<bevy::window::PrimaryWindow>>) {
    for mut opts in &mut windows {
        opts.grab_mode = bevy::window::CursorGrabMode::Locked;
        opts.visible = false;
    }
}

/// 返回主菜单时释放鼠标（否则菜单里光标不可见/被锁）。
fn on_exit_playing(mut windows: Query<&mut CursorOptions, With<bevy::window::PrimaryWindow>>) {
    for mut opts in &mut windows {
        opts.grab_mode = bevy::window::CursorGrabMode::None;
        opts.visible = true;
    }
}

fn beam_system(
    mut q: Query<(&mut Transform, &mut Visibility), With<Beam>>,
    player: Query<&Player>,
    mouse: Res<ButtonInput<MouseButton>>,
    world: Res<World>,
    ui: Res<UiState>,
) {
    for (mut tf, mut vis) in &mut q {
        let Ok(p) = player.single() else {
            *vis = Visibility::Hidden;
            continue;
        };
        let firing = p.hot_idx == -1 && mouse.pressed(MouseButton::Left) && !ui.locked() && !p.dead;
        if !firing {
            *vis = Visibility::Hidden;
            continue;
        }
        let origin = p.eye();
        let dir = p.look_dir();
        let dist = world
            .raycast(origin, dir, 22.0)
            .map(|(_, _, d)| d)
            .unwrap_or(22.0)
            .max(0.3);
        let mid = origin + dir * (dist * 0.5);
        tf.translation = mid;
        tf.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
        tf.scale = Vec3::new(1.0, 1.0, dist);
        *vis = Visibility::Visible;
    }
}

fn prompt_system(
    mut ui_state: ResMut<UiState>,
    player: Query<&Player>,
    world: Res<World>,
    machines: Query<&factory::Machine>,
    game: Res<SpaceGame>,
) {
    if ui_state.locked() {
        ui_state.prompt = None;
        return;
    }
    let Ok(p) = player.single() else {
        ui_state.prompt = None;
        return;
    };
    // 飞船优先（与登船判定半径一致：JS 4.5）
    if p.pos.distance(game.ship_pos) < 4.5 {
        ui_state.prompt = Some("[E] 检查飞船 / 登船".into());
        return;
    }
    let mut prompt = None;
    if let Some((cell, _n, dist)) = world.raycast(p.eye(), p.look_dir(), 5.0)
        && dist <= 5.0
        && let Some(m) = machines.iter().find(|m| m.pos == cell)
    {
        prompt = Some(format!("[E] 打开{}", m.kind.label()));
    }
    ui_state.prompt = prompt;
}

// ---------- 星球切换（无缝再入） ----------

/// 响应 LandPlanetEvent：归档当前星球 → 重建新星球世界/材质/机器。
#[allow(clippy::too_many_arguments)]
fn planet_switch_system(
    mut ev: MessageReader<space::LandPlanetEvent>,
    mut game: ResMut<SpaceGame>,
    mut world: ResMut<World>,
    mut commands: Commands,
    chunk_meshes: Query<Entity, With<ChunkMesh>>,
    machines: Query<(Entity, &factory::Machine, &factory::MachineState)>,
    mut creatures: Query<(Entity, &mut creatures::Creature, &Transform)>,
    mut spawner: ResMut<creatures::CreatureSpawner>,
    mut sent_spawner: ResMut<creatures::SentinelSpawner>,
    mut scan_state: ResMut<ui::ScanState>,
    scan_markers: Query<Entity, With<ui::ScanMarker>>,
    mut terrain_materials: ResMut<Assets<StandardMaterial>>,
    mut curved_materials: ResMut<Assets<materials::CurvedTerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
    atlas: Res<AtlasRes>,
) {
    for e in ev.read() {
        let pid = e.pid;
        if e.archive_current {
            // 同一星系内换星：归档当前星球。跨星系时旧世界已在
            // warp_system 完成前归档，不能写进目标星系的 visited。
            let cur = game.current_planet;
            let machines_save = factory::serialize_machines(&machines);
            let mut archive = game.visited.get(&cur).cloned().unwrap_or_default();
            archive.machines = machines_save;
            archive.ship_pos = [game.ship_pos.x, game.ship_pos.y, game.ship_pos.z];
            archive.mods = world.serialize_mods();
            archive.seed = world.seed;
            archive.biome = world.biome().key.to_string();
            archive.marks = game.marks.clone();
            // 兽群随星球档案归档（MC 风格：位置/血量/领地/被杀记录）
            let (herds_save, cells_save) = spawner.serialize(&creatures);
            archive.creatures = herds_save;
            archive.creature_cells = cells_save;
            game.visited.insert(cur, archive);
        }
        // 清理当前场景
        for ent in &chunk_meshes {
            commands.entity(ent).despawn();
        }
        for (ent, _, _) in &machines {
            commands.entity(ent).despawn();
        }
        for (ent, mut c, _) in &mut creatures {
            // 先抬血再统一销毁：避免 creature_despawn_system 同帧对
            // hp<=0（淡出/已死）生物重复 despawn 报 Entity invalid。
            c.hp = 1.0;
            commands.entity(ent).despawn();
        }
        for ent in &scan_markers {
            commands.entity(ent).despawn();
        }
        *spawner = creatures::CreatureSpawner::default();
        *sent_spawner = creatures::SentinelSpawner::default();
        *scan_state = ui::ScanState::default();
        // 构建新星球世界
        let pd = game
            .galaxy
            .planets
            .iter()
            .find(|p| p.id == pid)
            .cloned()
            .unwrap_or_else(|| game.planet().clone());
        let archive_new = game.visited.get(&pid).cloned();
        let seed = archive_new
            .as_ref()
            .map(|a| a.seed)
            .unwrap_or_else(|| game.galaxy.seed ^ (pid as u32).wrapping_mul(0x9E37_79B9));
        let biome = archive_new
            .as_ref()
            .map(|a| a.biome.clone())
            .unwrap_or_else(|| pd.biome.to_string());
        let view_dist = world.view_dist;
        let mut new_world = World::new(seed, &biome, view_dist);
        if let Some(a) = &archive_new {
            new_world.saved_mods = a.mods.clone();
        }
        // 材质重建（水面染色随生态）
        let mats = TerrainMaterials::build(
            &mut terrain_materials,
            &mut curved_materials,
            &mut images,
            atlas.atlas.to_image(),
            data::biome_by_key(&biome).water_tint,
        );
        commands.insert_resource(mats);
        // 机器恢复 + 兽群恢复（新星球无档案则空）
        if let Some(a) = &archive_new {
            factory::deserialize_machines(&mut commands, &a.machines);
            game.ship_pos = Vec3::new(a.ship_pos[0], a.ship_pos[1], a.ship_pos[2]);
            game.marks = a.marks.clone();
            spawner.restore(seed, &a.creatures, &a.creature_cells);
        } else {
            // 新星球：占位停泊点（落地动画会写入真实位置）
            game.ship_pos = Vec3::new(96.0, 40.0, 96.0);
            game.marks = Vec::new();
            spawner.restore(seed, &[], &[]);
        }
        *world = new_world;
        game.current_planet = pid;
    }
}

// ---------- 太空天空切换 ----------

/// 太空/空间站模式隐藏地面天空（日盘与星光）。
/// JS 原版太空态不渲染 planetScene（独立场景切换），Bevy 单相机下必须显式隐藏，
/// 否则冲出大气后平面地形残影留在宇宙里，与太空星球球形外壳错位同屏。
fn ground_scene_visibility_system(
    mode: Res<FlightMode>,
    world: Res<World>,
    mut commands: Commands,
    mut q: Query<
        (Entity, Option<&mut Visibility>, Option<&ChunkMesh>),
        Or<(
            With<ChunkMesh>,
            With<creatures::Creature>,
            With<creatures::DropItem>,
            With<ui::Ghost>,
            With<ui::ScanMarker>,
            With<Beam>,
            With<factory::BotVis>,
        )>,
    >,
) {
    let show = mode.ground_scene();
    for (e, v, chunk) in &mut q {
        // Keep the old, already-rendered terrain visible while the next
        // streaming ring is generated.  Hiding it immediately at a chunk
        // boundary exposed the far mesh for a few frames, making the terrain
        // appear to turn into a different biome.  stream_world_step still
        // removes meshes outside the unload radius; the seed check only
        // suppresses stale meshes from a previous planet during the handoff.
        let chunk_show = show && chunk.map(|c| c.world_seed == world.seed).unwrap_or(true);
        let vis = if chunk_show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        match v {
            Some(mut v) => {
                if *v != vis {
                    *v = vis;
                }
            }
            None => {
                commands.entity(e).insert(vis);
            }
        }
    }
}

// ---------- Save / quit ----------

fn save_settings_system(mut ev: MessageReader<ui::SaveEvent>, settings: Res<save::Settings>) {
    if ev.read().next().is_some() {
        // F5 / pause saves also persist display, cloud and F3 lighting
        // settings; these are independent of the character/world JSON.
        let _ = save::save_settings(&settings);
    }
}

#[allow(clippy::too_many_arguments)]
fn save_system(
    mut ev: MessageReader<ui::SaveEvent>,
    mut player: Query<&mut Player>,
    world: Res<World>,
    research: Res<Research>,
    day: Res<daynight::DayTime>,
    names: Res<SaveNames>,
    game: ResMut<SpaceGame>,
    ship: Res<ShipState>,
    warp_anim: Res<space::WarpAnim>,
    mode: Res<FlightMode>,
    ship_asset: Res<ShipAsset>,
    quests: Res<quests::Quests>,
    station: Option<Res<station::StationState>>,
    spawner: Res<creatures::CreatureSpawner>,
    machines: Query<(Entity, &factory::Machine, &factory::MachineState)>,
    creatures_q: Query<(Entity, &mut creatures::Creature, &Transform)>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
    mut quit_ev: MessageWriter<ui::QuitToMenuEvent>,
) {
    for request in ev.read() {
        let Ok(mut p) = player.single_mut() else {
            continue;
        };
        let state_str = match *mode {
            FlightMode::Atmo | FlightMode::AtmoLand => "atmo",
            FlightMode::Space | FlightMode::Station => "space",
            FlightMode::Warping => "warping",
            FlightMode::Planet | FlightMode::Seated => "planet",
        };
        let Some(char_snapshot) = save::snapshot_char_file(&names.char) else {
            p.toast("保存失败：无法读取原角色档，已取消本次写入");
            audio::play(&mut commands, sfx.error.clone(), 0.5, None);
            continue;
        };
        let ok_char = save::save_char(
            &p,
            &names.char,
            Some(&names.world),
            &research.techs,
            &p.appearance,
            game.fuel_loaded,
            &ship_asset.data,
            &game.garage,
            quests.idx,
            research.active.as_ref(),
        );
        let ship_pos = if matches!(
            *mode,
            FlightMode::Planet | FlightMode::Seated | FlightMode::Atmo | FlightMode::AtmoLand
        ) {
            Some([game.ship_pos.x, game.ship_pos.y, game.ship_pos.z])
        } else {
            None
        };
        let mut ship_state = space::serialize_ship_state(&ship);
        if matches!(*mode, FlightMode::Planet | FlightMode::Seated) {
            ship_state.pos = [game.ship_pos.x, game.ship_pos.y, game.ship_pos.z];
        }
        // 站内存档存机库出口（JS main.js:2770-2775），读档不会重新泊入
        if *mode == FlightMode::Station
            && let Some(st) = station.as_ref()
        {
            let exit = station::station_exit_pos(st.station_pos, st.seed);
            ship_state.pos = [exit.x, exit.y, exit.z];
        }
        let warp_anim = if *mode == FlightMode::Warping && warp_anim.active {
            Some(save::WarpAnimSave {
                t: warp_anim.t,
                seed: warp_anim.seed,
                yaw: warp_anim.yaw,
                pitch: warp_anim.pitch,
                v0: warp_anim.v0,
            })
        } else {
            None
        };
        let (creatures_save, creature_cells_save) = spawner.serialize(&creatures_q);
        let machines_save = factory::serialize_machines(&machines);
        let ok_world = ok_char
            && save::save_world_full(
                &world,
                &names.world,
                day.0,
                state_str,
                game.current_planet,
                game.galaxy.seed,
                game.galaxy_count,
                &game.galaxy.market,
                &game.galaxy.stock,
                &quests.flags,
                ship_pos,
                Some(&ship_state),
                warp_anim.as_ref(),
                &game.marks,
                game.warp_lock.as_ref(),
                &quests.placed,
                quests.side.as_ref(),
                &machines_save,
                &game.visited,
                &game.archives,
                &creatures_save,
                &creature_cells_save,
            );
        let rollback_ok =
            !ok_char || ok_world || save::restore_char_file(&names.char, &char_snapshot);
        if ok_char && ok_world {
            audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
            if request.quit_after {
                quit_ev.write(ui::QuitToMenuEvent);
            }
        } else if rollback_ok {
            p.toast("保存失败：已留在游戏中，请检查磁盘空间或存档目录权限");
            audio::play(&mut commands, sfx.error.clone(), 0.5, None);
        } else {
            p.toast("保存失败且角色档回滚失败：请勿退出，并先备份存档目录");
            audio::play(&mut commands, sfx.error.clone(), 0.8, None);
        }
    }
}

fn quit_to_menu_system(
    mut ev: MessageReader<ui::QuitToMenuEvent>,
    mut commands: Commands,
    in_game: Query<Entity, With<InGame>>,
    mut network: ResMut<network::NetworkState>,
    mut rain_audio: ResMut<weather::RainAudio>,
    mut next: ResMut<NextState<GameState>>,
) {
    for _ in ev.read() {
        rain_audio.entity = None;
        for e in &in_game {
            commands.entity(e).despawn();
        }
        network.reset();
        commands.remove_resource::<World>();
        commands.remove_resource::<TerrainMaterials>();
        commands.remove_resource::<daynight::DayTime>();
        commands.remove_resource::<daynight::SpaceFactor>();
        commands.remove_resource::<materials::LampPool>();
        commands.remove_resource::<player::BreakQueue>();
        commands.remove_resource::<creatures::CreatureSpawner>();
        commands.remove_resource::<ui::GhostMat>();
        commands.remove_resource::<SaveNames>();
        commands.remove_resource::<Research>();
        commands.remove_resource::<SpaceGame>();
        commands.remove_resource::<ShipState>();
        commands.remove_resource::<ShipAsset>();
        commands.remove_resource::<SpaceInput>();
        commands.remove_resource::<FlightCamera>();
        commands.remove_resource::<station::StationState>();
        commands.remove_resource::<quests::Quests>();
        commands.remove_resource::<factory::Power>();
        commands.remove_resource::<space::AtmoLand>();
        commands.remove_resource::<space::SpaceScene>();
        commands.remove_resource::<space::VisitorRespawn>();
        commands.remove_resource::<space::VisitorTraffic>();
        commands.remove_resource::<station::StationDefense>();
        commands.remove_resource::<factory::TickAcc>();
        commands.remove_resource::<weather::ClimateRuntime>();
        commands.remove_resource::<creatures::SentinelSpawner>();
        commands.remove_resource::<ui::ScanState>();
        commands.insert_resource(creatures::SentinelSpawner::default());
        commands.insert_resource(weather::ClimateRuntime::default());
        commands.insert_resource(space::VisitorRespawn::default());
        commands.insert_resource(space::VisitorTraffic::default());
        commands.insert_resource(station::StationDefense::default());
        commands.insert_resource(factory::TickAcc::default());
        commands.insert_resource(space::WarpAnim::default());
        commands.insert_resource(space::WarpVisuals::default());
        commands.insert_resource(Research::default());
        commands.insert_resource(UiState::default());
        commands.insert_resource(FlightMode::default());
        commands.insert_resource(quests::Quests::default());
        next.set(GameState::Menu);
    }
}
