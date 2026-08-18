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
mod materials;
mod network;
mod player;
mod quests;
mod rng;
mod save;
mod space;
mod station;
mod textures;
mod ui;
mod weather;
mod world;

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoAutoAabb, NoFrustumCulling};
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::window::{CursorOptions, PresentMode};
use bevy_egui::{EguiContexts, EguiPlugin, egui};
use materials::{TerrainMat, TerrainMaterials};
use player::Player;
use space::{FlightCamera, FlightMode, ShipAsset, ShipState, SpaceGame, SpaceInput};
use ui::{Research, ScanPulse, UiState};
use world::{VoxelMesh, World};

/// Everything spawned in-game gets this marker (cleared when returning to menu).
#[derive(Component)]
pub struct InGame;

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
        pos[1].clamp(-256.0, 512.0),
        pos[2].clamp(-1_000_000.0, 1_000_000.0),
    ))
}

/// Chunk terrain meshes (cleared on planet switch).
#[derive(Component)]
pub struct ChunkMesh {
    /// 区块坐标用于高速移动时立即隐藏旧视距外网格，避免等待延迟 despawn
    /// 命令执行期间把旧地形和新地形叠在一起。
    pub cx: i32,
    pub cz: i32,
}

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum GameState {
    #[default]
    Menu,
    Loading,
    Playing,
}

#[derive(Component)]
struct Beam;

#[derive(Component)]
pub struct SunDisc;

/// The atlas structure (block tiles) shared by meshing + icons.
#[derive(Resource)]
struct AtlasRes {
    atlas: textures::Atlas,
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
            save_ev.write(ui::SaveEvent);
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
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.05, 0.07, 0.1)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "STARFORGE 星穹熔炉 · Bevy 移植版".into(),
                resolution: (1280, 720).into(),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(MaterialPlugin::<TerrainMat>::default())
        .insert_resource(save::load_settings())
        .insert_resource(UiState::default())
        .insert_resource(Research::default())
        .insert_resource(ui::EguiIcons::default())
        .insert_resource(ui::ScanState::default())
        .insert_resource(ui::MapState::default())
        .init_state::<GameState>()
        .add_message::<ui::SaveEvent>()
        .add_message::<ui::QuitToMenuEvent>()
        .add_message::<quests::PlacedEvent>()
        .add_message::<quests::FlagEvent>()
        .add_message::<quests::BigMessageEvent>()
        .add_message::<space::LandPlanetEvent>()
        .add_message::<space::WarpArriveEvent>()
        .add_message::<station::ShipSwitchEvent>()
        .add_message::<network::BlockChanged>()
        .insert_resource(FlightMode::default())
        .insert_resource(ShipState::default())
        .insert_resource(SpaceInput::default())
        .insert_resource(FlightCamera::default())
        .insert_resource(player::PlayerCameraMode::default())
        .insert_resource(feedback::FeedbackAssets::default())
        .insert_resource(station::StationState::default())
        .insert_resource(station::StationDefense::default())
        .insert_resource(quests::Quests::default())
        .insert_resource(factory::Power::default())
        .insert_resource(factory::TickAcc::default())
        .insert_resource(space::WarpAnim::default())
        .insert_resource(space::WarpVisuals::default())
        .insert_resource(space::VisitorRespawn::default())
        .insert_resource(space::VisitorTraffic::default())
        .insert_resource(weather::ClimateRuntime::default())
        .insert_resource(network::NetworkState::default())
        .insert_resource(creatures::SentinelSpawner::default())
        .init_resource::<char::NpcAnimationLibrary>()
        .init_resource::<creatures::CreatureAnimationLibrary>()
        .add_systems(Startup, startup)
        .add_systems(
            PreUpdate,
            egui_manual_pass
                .after(bevy_egui::EguiPreUpdateSet::InitContexts)
                .before(bevy_egui::EguiPreUpdateSet::BeginPass),
        )
        .add_systems(PostUpdate, ui::setup_egui)
        .add_systems(OnEnter(GameState::Loading), on_enter_loading)
        .add_systems(OnExit(GameState::Loading), on_exit_loading)
        .add_systems(OnEnter(GameState::Playing), on_enter_playing)
        .add_systems(
            OnExit(GameState::Playing),
            (on_exit_playing, network::disconnect_system),
        )
        // menu
        .add_systems(
            Update,
            (
                egui_begin_pass,
                (
                    (smoke_boot, menu_system)
                        .chain()
                        .run_if(in_state(GameState::Menu)),
                    // loading
                    loading_system.run_if(in_state(GameState::Loading)),
                    // playing
                    (
                        (
                            (
                                // 通用
                                ui::panel_hotkeys_system,
                                ui::quicksave_system,
                                ui::clear_input_on_focus_lost,
                                ui::big_message_system,
                                quests::quest_tick_system,
                                quests::side_quest_system,
                                quests::village_side_quest_system,
                                char::npc_idle_system,
                                ui::research_system,
                                materials::curve_system,
                                materials::lamp_pool_system,
                                daynight::daynight_system,
                                weather::climate_system,
                            )
                                .chain(),
                            (
                                // 地面
                                player::movement_system.run_if(ground_mode),
                                player::collision_system.run_if(ground_mode),
                                player::survival_system.run_if(ground_mode),
                                player::mining_system.run_if(ground_mode),
                                player::break_system.run_if(ground_mode),
                                player::placement_system.run_if(ground_mode),
                                player::hotbar_system.run_if(ground_mode),
                                stream_system.run_if(ground_scene_mode),
                                creatures::creature_spawn_system.run_if(creature_mode),
                                creatures::creature_system.run_if(creature_mode),
                                creatures::creature_sound_system.run_if(creature_mode),
                                creatures::creature_animation_system.run_if(creature_mode),
                                creatures::sentinel_system.run_if(creature_mode),
                            )
                                .chain(),
                            (
                                creatures::creature_despawn_system.run_if(creature_mode),
                                creatures::drops_system.run_if(creature_mode),
                                factory::factory_system.run_if(ground_mode),
                                factory::machine_sync_system.run_if(ground_mode),
                                factory::lumberbot_visual_system.run_if(ground_mode),
                                ui::scan_system.run_if(ground_mode),
                                // 视角
                                player::look_system.run_if(walk_look_mode),
                                player::camera_toggle_system.run_if(in_planet_mode),
                                player::camera_system.run_if(in_planet_mode),
                                // 太空
                                space::space_input_system,
                                space::ship_interact_system,
                                space::seated_system,
                                space::atmo_land_trigger_system,
                            )
                                .chain(),
                            (
                                station::station_system,
                                station::station_defense_system,
                                station::station_dialog_system,
                                station::station_npc_spawn_system,
                                station::ship_switch_system,
                                planet_switch_system,
                                space_sky_sync_system,
                                ground_scene_visibility_system,
                                // 光标管理：所有模式（含太空/空间站）都按面板状态锁定/解锁
                                player::cursor_system,
                            )
                                .chain(),
                        )
                            .chain(),
                        (
                            ui::ghost_system.run_if(in_planet_mode),
                            beam_system.run_if(in_planet_mode),
                            prompt_system.run_if(in_planet_mode),
                            (ui::hud_system, ui::ship_label_system).chain(),
                            ui::inventory_panel_system,
                            ui::tech_panel_system,
                            ui::machine_panel_system,
                            ui::pause_panel_system,
                            ui::trade_panel_system,
                            ui::garage_panel_system,
                            ui::galaxy_map_system,
                            network::network_ui_system,
                            ui::creative_panel_system,
                            ui::planet_map_system,
                            save_system,
                            quit_to_menu_system,
                            smoke_exit,
                        )
                            .chain(),
                    )
                        .chain()
                        .run_if(in_state(GameState::Playing)),
                ),
                egui_end_pass,
            )
                .chain(),
        )
        // 飞行系统（模式互斥，无需严格顺序；逐一注册规避 tuple 配置组合问题）
        .add_systems(
            Update,
            space::atmo_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::atmoland_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::seated_camera_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::space_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::warp_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::warp_visual_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::space_scene_sync_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::sphere_fade_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            weather::space_cloud_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::warp_arrive_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::flight_camera_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::ship_sync_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::ship_parked_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::bolt_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::space_drop_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::visitor_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::asteroid_spin_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            space::engine_loop_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(Update, far_mesh_system.run_if(in_state(GameState::Playing)))
        .add_systems(
            Update,
            feedback::particle_system.run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            network::network_system.run_if(in_state(GameState::Playing)),
        );
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

fn ground_mode(mode: Res<FlightMode>) -> bool {
    *mode == FlightMode::Planet
}

/// 生物在地面和座舱状态都应继续模拟；座舱只暂停玩家的地面操作。
fn creature_mode(mode: Res<FlightMode>) -> bool {
    *mode == FlightMode::Planet || *mode == FlightMode::Seated
}

fn in_planet_mode(mode: Res<FlightMode>) -> bool {
    *mode == FlightMode::Planet || *mode == FlightMode::Seated
}

fn ground_scene_mode(mode: Res<FlightMode>) -> bool {
    mode.ground_scene()
}

fn walk_look_mode(mode: Res<FlightMode>) -> bool {
    *mode == FlightMode::Planet || *mode == FlightMode::Seated || *mode == FlightMode::Station
}

// ---------- Startup ----------

fn startup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut stdmats: ResMut<Assets<StandardMaterial>>,
    mut audio_assets: ResMut<Assets<bevy::audio::AudioSource>>,
    settings: Res<save::Settings>,
) {
    // Self-extract the WGSL shader assets next to the executable so the
    // AssetServer finds them regardless of the working directory.
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        let shader_dir = dir.join("assets").join("shaders");
        let _ = std::fs::create_dir_all(&shader_dir);
        let _ = std::fs::write(
            shader_dir.join("terrain_vertex.wgsl"),
            include_str!("../assets/shaders/terrain_vertex.wgsl"),
        );
        let _ = std::fs::write(
            shader_dir.join("terrain_prepass_vertex.wgsl"),
            include_str!("../assets/shaders/terrain_prepass_vertex.wgsl"),
        );
        let _ = std::fs::write(
            shader_dir.join("terrain_fragment.wgsl"),
            include_str!("../assets/shaders/terrain_fragment.wgsl"),
        );
        // GLB 模型同样自解压到 exe 旁（仅首次；源码在 <crate>/target/<profile>/ 下时向上两级即 crate 根）
        let models_dir = dir.join("assets").join("models");
        // Existing target folders may come from an older build and therefore
        // already contain legacy NPC assets while missing the new creature set.
        // Copy when any required Quaternius file is absent, not only when the
        // directory itself is absent.
        let creature_files = ["alpaca", "deer", "fox", "wolf"];
        if creature_files.iter().any(|name| {
            !models_dir
                .join("creatures")
                .join(format!("quaternius_{name}.gltf"))
                .exists()
        }) {
            let mut src: Option<std::path::PathBuf> = None;
            let via_exe = dir.join("..").join("..").join("assets").join("models");
            if via_exe.is_dir() {
                src = Some(via_exe);
            } else if let Ok(cwd) = std::env::current_dir() {
                let via_cwd = cwd.join("assets").join("models");
                if via_cwd.is_dir() {
                    src = Some(via_cwd);
                }
            }
            if let Some(s) = src {
                let _ = copy_dir_all(&s, &models_dir);
            }
        }
    }
    let atlas = textures::Atlas::build();
    commands.insert_resource(AtlasRes { atlas });
    let (icon_mats, mut icon_imgs) = ui::build_icons(&mut meshes, &mut images, &mut stdmats);
    // white fallback icon (1×1) — egui texture registration happens lazily in ui::setup_egui
    let white = images.add(Image::new(
        bevy::render::render_resource::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        vec![255u8, 255, 255, 255],
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    ));
    icon_imgs.map.insert("fallback".to_string(), white);
    commands.insert_resource(icon_mats);
    commands.insert_resource(icon_imgs);
    commands.insert_resource(audio::Sfx::build(&mut audio_assets, settings.volume));
    // persistent camera: created now so bevy_egui's primary context exists in menus too.
    // The player camera system drives it during Playing.
    let cam = commands
        .spawn((
            Camera3d::default(),
            Msaa::Off,
            Projection::Perspective(PerspectiveProjection {
                fov: 75f32.to_radians(),
                far: space::CAM_FAR,
                ..default()
            }),
            Transform::from_xyz(96.0, 90.0, 96.0),
            // 高度雾（JS planetScene.fog 移植）：远景融入天穹，隐藏流式区块边缘与曲率变形
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
            vec![0u8; 640 * 360 * 4],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
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

/// egui 0.35 requires one contiguous begin_pass → draw → end_pass lifecycle per
/// frame (as egui's own `run_ui` does). bevy_egui 0.41.1's split calls
/// (begin_pass in PreUpdate, end_pass in PostUpdate) break egui 0.35's hit-test
/// data chain (`prev_pass.widgets` stays empty, so no widget is ever hovered or
/// clicked). Fix: switch the context to manual mode and run the whole pass
/// inside Update, with all UI systems chained between the two.
fn egui_manual_pass(mut q: Query<&mut bevy_egui::EguiContextSettings>) {
    for mut s in &mut q {
        if !s.run_manually {
            s.run_manually = true;
        }
    }
}

fn egui_begin_pass(mut contexts: EguiContexts, mut input: Query<&mut bevy_egui::EguiInput>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if let Ok(mut inp) = input.single_mut() {
        let raw = std::mem::take(&mut inp.0);
        ctx.begin_pass(raw);
    }
}

fn egui_end_pass(mut contexts: EguiContexts, mut full: Query<&mut bevy_egui::EguiFullOutput>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let out = ctx.end_pass();
    if let Ok(mut f) = full.single_mut() {
        f.0 = Some(out);
    }
}

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
    let in_space = world_data.as_ref().is_some_and(|w| w.state == "space");
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
    mut terrain_materials: ResMut<Assets<TerrainMat>>,
    mut images: ResMut<Assets<Image>>,
    mut stdmats: ResMut<Assets<StandardMaterial>>,
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
            &mut terrain_materials,
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
        done = stream_world_step(
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
    // spawn logical machine entities for machine blocks present in loaded chunks
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
        let key = data::block_by_id(id).key;
        factory::spawn_machine(commands, cell, key, 0);
    }
    let day_t = 0.30;
    commands.insert_resource(daynight::DayTime(day_t));
    commands.insert_resource(daynight::SpaceFactor::default());
    let pool = daynight::spawn_sky(commands, meshes, stdmats);
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
    let mut research_active: Option<(String, f32)> = None;

    if let Some(cd) = char_data.as_ref() {
        if let Some(saved_position) = safe_player_position(cd.pos) {
            p.pos = saved_position;
        }
        p.yaw = if cd.yaw.is_finite() {
            cd.yaw.clamp(-std::f32::consts::PI, std::f32::consts::PI)
        } else {
            0.0
        };
        p.pitch = if cd.pitch.is_finite() {
            cd.pitch.clamp(-1.55, 1.55)
        } else {
            0.0
        };
        p.stats = player::Stats {
            // A dead flag is not part of the save schema; never reload an
            // alive player with zero health and no respawn timer.
            hp: finite_clamp(cd.stats[0], 8.0, 0.1, 8.0),
            shield: finite_clamp(cd.stats[1], 6.0, 0.0, 6.0),
            o2: finite_clamp(cd.stats[2], 100.0, 0.0, 100.0),
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
        if let Some(sp) = wd.ship_pos
            && sp.iter().all(|v| v.is_finite())
        {
            game.ship_pos = Vec3::new(sp[0], sp[1], sp[2]);
        }
        // 地图标记 / 跃迁锁定 / 跨星系档案 / 放置计数（JS mapMarks/warpLock/galaxyArchives/placedCount）
        game.marks = wd.marks.clone();
        game.warp_lock = wd.warp_lock.clone();
        game.archives = wd.archives.clone();
        quests.placed = wd.placed.clone();
        if wd.state == "space" {
            start_mode = FlightMode::Space;
            if let Some(ss) = &wd.ship_state {
                if ss.pos.iter().all(|v| v.is_finite()) {
                    ship_state.pos = Vec3::new(ss.pos[0], ss.pos[1], ss.pos[2]);
                }
                ship_state.yaw = if ss.yaw.is_finite() { ss.yaw } else { 0.0 };
                ship_state.pitch = if ss.pitch.is_finite() { ss.pitch } else { 0.0 };
                ship_state.roll = if ss.roll.is_finite() { ss.roll } else { 0.0 };
                ship_state.speed = if ss.speed.is_finite() {
                    ss.speed.max(0.0)
                } else {
                    0.0
                };
            }
        }
    }

    // 初始飞船
    if start_mode == FlightMode::Space
        && ship_state.pos.is_finite()
        && ship_state.pos.length_squared() >= 1e-6
    {
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
    let mut normalized_ship_inv =
        crate::inventory::Inventory::from_slots(ship_data.inv.clone()).slots;
    normalized_ship_inv.truncate(12);
    normalized_ship_inv.resize(12, None);
    ship_data.inv = normalized_ship_inv;
    let (ship_ent, flames, ship_spawn_pos) =
        space::spawn_initial_ship(commands, meshes, stdmats, asset_server, &world, &ship_data);
    if game.ship_pos == Vec3::ZERO {
        game.ship_pos = ship_spawn_pos;
    }
    // A space save stores the active ship in ship_state, not in the
    // planetary parking position. Keep that position so loading in space
    // does not teleport the ship back to the planet-side spawn pad.
    if start_mode != FlightMode::Space
        || !ship_state.pos.is_finite()
        || ship_state.pos.length_squared() < 1e-6
    {
        ship_state.pos = game.ship_pos;
    }
    ship_state.board_yaw = 0.0;
    ship_state.hp = 20.0;
    ship_state.hp_max = 20.0;
    commands.insert_resource(world);
    commands.insert_resource(ShipAsset {
        entity: Some(ship_ent),
        flames,
        data: ship_data.clone(),
    });
    game.ship_inv = crate::inventory::Inventory::from_slots(ship_data.inv.clone())
        .slots
        .into_iter()
        .take(12)
        .chain(std::iter::repeat(None))
        .take(12)
        .collect();

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
    commands.insert_resource(player::BreakQueue::default());
    // 兽群恢复（MC 风格：位置/血量/领地/被杀记录随存档还原）
    {
        let mut spawner = creatures::CreatureSpawner::default();
        if let Some(wd) = &world_data {
            spawner.restore(world_seed, &wd.creatures, &wd.creature_cells);
        }
        commands.insert_resource(spawner);
    }
    commands.insert_resource(ScanPulse::default());
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

// ---------- Streaming ----------

fn stream_world_step(
    world: &mut World,
    pcx: i32,
    pcz: i32,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    atlas: &textures::Atlas,
    mats: &TerrainMaterials,
    gen_budget: usize,
    mesh_budget: usize,
) -> bool {
    let mut gen_left = gen_budget;
    let mut mesh_left = mesh_budget;
    // generation rings (inside-out, Chebyshev)：预生成到 view+2，
    // 玩家跨区块时新视距环的数据已就绪，只差网格化（不会出现"穿越区块→远处大量区块重新生成"）
    'outer: for r in 0..=world.view_dist + 2 {
        if gen_left == 0 {
            break;
        }
        for cz in pcz - r..=pcz + r {
            for cx in pcx - r..=pcx + r {
                if World::cheb(cx, cz, pcx, pcz) != r {
                    continue;
                }
                if world.get_chunk(cx, cz).is_none() {
                    world.ensure_chunk(cx, cz);
                    gen_left -= 1;
                    if gen_left == 0 {
                        break 'outer;
                    }
                }
            }
        }
    }
    // meshing rings (neighbors must exist)：网格化到 view+1（含预载环）——
    // 玩家跨区块后新视距环立即有网格，地平线不再短暂只剩粗糙远景
    'mesh_outer: for r in 0..=world.view_dist + 1 {
        if mesh_left == 0 {
            break;
        }
        for cz in pcz - r..=pcz + r {
            for cx in pcx - r..=pcx + r {
                if World::cheb(cx, cz, pcx, pcz) != r {
                    continue;
                }
                let key = world::ckey(cx, cz);
                let Some(c) = world.chunks.get(&key) else {
                    continue;
                };
                if (c.mesh.is_some() || c.water_mesh.is_some()) && !c.dirty {
                    continue;
                }
                let neighbors_exist = [(cx - 1, cz), (cx + 1, cz), (cx, cz - 1), (cx, cz + 1)]
                    .iter()
                    .all(|(nx, nz)| world.get_chunk(*nx, *nz).is_some());
                if !neighbors_exist {
                    continue;
                }
                let (solid_m, water_m) = world::build_chunk_meshes(world, c, atlas);
                if let Some(e) = c.mesh {
                    commands.entity(e).despawn();
                }
                if let Some(e) = c.water_mesh {
                    commands.entity(e).despawn();
                }
                // AABB 需覆盖曲率顶点位移（着色器按 0.002·r² 下压，视距外缘可达上百格），
                // 否则高空时远景区块被视锥剔除、边缘地形闪烁
                let y_min = -160.0
                    - 0.002
                        * ((world.view_dist + 2) as f32 * crate::data::CHUNK as f32 + 16.0).powi(2)
                    - 8.0;
                let c = world.chunks.get_mut(&key).unwrap();
                c.mesh = solid_m.map(|m| {
                    spawn_chunk_mesh(
                        commands,
                        meshes,
                        cx,
                        cz,
                        m,
                        mats.solid.clone(),
                        false,
                        y_min,
                    )
                });
                c.water_mesh = water_m.map(|m| {
                    spawn_chunk_mesh(commands, meshes, cx, cz, m, mats.water.clone(), true, y_min)
                });
                c.dirty = false;
                mesh_left -= 1;
                if mesh_left == 0 {
                    break 'mesh_outer;
                }
            }
        }
    }
    // unload far meshes (keep data)
    for c in world.chunks.values_mut() {
        if (c.mesh.is_some() || c.water_mesh.is_some())
            && World::cheb(c.cx, c.cz, pcx, pcz) > world.view_dist + 3
        {
            if let Some(e) = c.mesh.take() {
                commands.entity(e).despawn();
            }
            if let Some(e) = c.water_mesh.take() {
                commands.entity(e).despawn();
            }
            c.dirty = true;
        }
    }
    // data eviction
    world.chunks.retain(|_, c| {
        !(c.mesh.is_none()
            && c.water_mesh.is_none()
            && World::cheb(c.cx, c.cz, pcx, pcz) > world.view_dist + 9
            && !c.modified)
    });
    // completeness check: every chunk in view distance generated & meshed

    (0..=world.view_dist).all(|r| {
        let mut ok = true;
        for cz in pcz - r..=pcz + r {
            for cx in pcx - r..=pcx + r {
                if World::cheb(cx, cz, pcx, pcz) != r {
                    continue;
                }
                match world.get_chunk(cx, cz) {
                    Some(c) => {
                        if (c.mesh.is_none() && c.water_mesh.is_none()) || c.dirty {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
            }
        }
        ok
    })
}

fn spawn_chunk_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    cx: i32,
    cz: i32,
    vm: VoxelMesh,
    mat: Handle<TerrainMat>,
    _water: bool,
    y_min: f32,
) -> Entity {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vm.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vm.normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vm.uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vm.colors);
    mesh.insert_indices(Indices::U32(vm.indices));
    let handle = meshes.add(mesh);
    let aabb = Aabb::from_min_max(
        Vec3::new(cx as f32 * 16.0, y_min, cz as f32 * 16.0),
        Vec3::new(cx as f32 * 16.0 + 16.0, 130.0, cz as f32 * 16.0 + 16.0),
    );
    commands
        .spawn((
            Mesh3d(handle),
            MeshMaterial3d(mat),
            // 顶点已是绝对世界坐标（build_chunk_meshes 以 x0+lx / z0+lz 生成），
            // 不能再叠加区块原点平移，否则每块地形被双倍偏移、块间出现大空洞。
            Transform::default(),
            aabb,
            NoAutoAabb,
            Visibility::default(),
            ChunkMesh { cx, cz },
            InGame,
        ))
        .id()
}

fn stream_system(
    mut world: ResMut<World>,
    player: Query<&Player>,
    ship: Res<ShipState>,
    mode: Res<FlightMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    atlas: Res<AtlasRes>,
    mats: Res<TerrainMaterials>,
) {
    let (px, pz) = if *mode == FlightMode::Atmo || *mode == FlightMode::AtmoLand {
        (ship.pos.x, ship.pos.z)
    } else {
        match player.single() {
            Ok(p) => (p.pos.x, p.pos.z),
            Err(_) => return,
        }
    };
    let pcx = world::cf(px);
    let pcz = world::cf(pz);
    if !world.stream_dirty && pcx == world.last_pcx && pcz == world.last_pcz {
        return;
    }
    // During atmospheric flight the ship can cross several chunks per second.
    // Keep each streaming slice small so terrain generation never stalls the
    // render frame; walking keeps the larger budget for quick world loading.
    let jump = (pcx - world.last_pcx)
        .abs()
        .max((pcz - world.last_pcz).abs());
    let fast_recenter = jump > world.view_dist + 2;
    let atmo = matches!(*mode, FlightMode::Atmo | FlightMode::AtmoLand);
    let (normal_gen, normal_mesh) = if atmo { (4, 2) } else { (12, 6) };
    let (fast_gen, fast_mesh) = if atmo { (8, 2) } else { (48, 16) };
    stream_world_step(
        &mut world,
        pcx,
        pcz,
        &mut commands,
        &mut meshes,
        &atlas.atlas,
        &mats,
        if fast_recenter { fast_gen } else { normal_gen },
        if fast_recenter {
            fast_mesh
        } else {
            normal_mesh
        },
    );
    world.last_pcx = pcx;
    world.last_pcz = pcz;
    // 空闲门控：只统计网格化范围内（≤ view+1）的脏块——view+2 纯数据预载环保持脏状态
    // 但不唤醒扫描（旧实现统计到 view+3，预载环永远脏 → 每帧全环扫描永不空闲）
    world.stream_dirty = world
        .chunks
        .values()
        .any(|c| c.dirty && World::cheb(c.cx, c.cz, pcx, pcz) <= world.view_dist + 1);
}

// ---------- 远景模拟地形（JS farMesh 移植） ----------

/// 远景地形状态：±1536 格低细节高度场，跟随玩家分帧重建；
/// 缺失时高空/远望的地表在视距边缘戛然而止，曲率变形与区块流式边缘完全暴露（巨大闪动/残影）。
#[derive(Component)]
struct FarMesh {
    /// 已完成的网格中心（世界坐标，按 FAR_SNAP 对齐）
    cx: f32,
    cz: f32,
    seed: u32,
    /// 待填充的下一行（< FAR_N 表示正在重建）
    row: usize,
    target_cx: f32,
    target_cz: f32,
    mesh: Handle<Mesh>,
}

/// 远景挖空环（JS farHoleU 同口径）：玩家周围由真实区块覆盖，远景在 r0..r1 间淡出。
fn far_hole_radii(view_dist: i32) -> (f32, f32) {
    let r1 = view_dist as f32 * 16.0 - 8.0;
    let r0 = (r1 - 90.0).max(56.0);
    (r0, r1)
}

fn far_mesh_system(
    mut commands: Commands,
    world: Res<World>,
    player: Query<&Player>,
    ship: Res<ShipState>,
    mode: Res<FlightMode>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Res<TerrainMaterials>,
    atlas: Res<AtlasRes>,
    mut far_q: Query<(
        Entity,
        &mut FarMesh,
        &mut Visibility,
        &mut MeshMaterial3d<TerrainMat>,
    )>,
) {
    let (px, pz) = if *mode == FlightMode::Atmo || *mode == FlightMode::AtmoLand {
        (ship.pos.x, ship.pos.z)
    } else {
        match player.single() {
            Ok(p) => (p.pos.x, p.pos.z),
            Err(_) => return,
        }
    };
    let show = mode.ground_scene();
    let tcx = (px / world::FAR_SNAP).round() * world::FAR_SNAP;
    let tcz = (pz / world::FAR_SNAP).round() * world::FAR_SNAP;
    if let Ok((e, mut fm, mut vis, mut mmat)) = far_q.single_mut() {
        *vis = if show && fm.row >= world::FAR_N {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        // 换球/读档：世界种子变化 → 整体重刷
        if fm.seed != world.seed {
            fm.seed = world.seed;
            fm.target_cx = tcx;
            fm.target_cz = tcz;
            fm.row = 0;
            *vis = Visibility::Hidden;
        }
        if fm.row >= world::FAR_N && (tcx != fm.cx || tcz != fm.cz) {
            fm.target_cx = tcx;
            fm.target_cz = tcz;
            fm.row = 0;
            // 不把新中心的部分行写入当前可见网格，避免高速穿越时出现
            // 半幅旧地形 + 半幅新地形的撕裂/跳变。
            *vis = Visibility::Hidden;
        }
        if fm.row < world::FAR_N {
            let from = fm.row;
            let to = (fm.row + world::FAR_ROWS_PER_FRAME).min(world::FAR_N);
            if let Some(mut mesh) = meshes.get_mut(&fm.mesh) {
                world::fill_far_rows(
                    &world,
                    &atlas.atlas,
                    fm.target_cx,
                    fm.target_cz,
                    from,
                    to,
                    &mut mesh,
                );
            }
            fm.row = to;
            if fm.row >= world::FAR_N {
                fm.cx = fm.target_cx;
                fm.cz = fm.target_cz;
                if show {
                    *vis = Visibility::Visible;
                }
            }
        }
        // 挖空环已移入片元着色器（far_hole_* uniform，curve_system 每帧更新）——
        // 不再每帧在 CPU 上改写 129×129 顶点 alpha（旧实现每帧上传 16k 顶点，跨越区块时加剧卡顿）
        if mmat.0 != mats.far {
            mmat.0 = mats.far.clone();
        }
        let _ = e;
    } else if show {
        let handle = meshes.add(world::build_far_mesh(&world, &atlas.atlas, tcx, tcz));
        commands.spawn((
            Mesh3d(handle.clone()),
            MeshMaterial3d(mats.far.clone()),
            Transform::default(),
            Visibility::Visible,
            FarMesh {
                cx: tcx,
                cz: tcz,
                seed: world.seed,
                row: world::FAR_N,
                target_cx: tcx,
                target_cz: tcz,
                mesh: handle,
            },
            NoFrustumCulling,
            InGame,
        ));
    }
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
    creatures: Query<(Entity, &creatures::Creature, &Transform)>,
    mut spawner: ResMut<creatures::CreatureSpawner>,
    mut sent_spawner: ResMut<creatures::SentinelSpawner>,
    mut scan_state: ResMut<ui::ScanState>,
    scan_markers: Query<Entity, With<ui::ScanMarker>>,
    mut terrain_materials: ResMut<Assets<TerrainMat>>,
    mut images: ResMut<Assets<Image>>,
    atlas: Res<AtlasRes>,
) {
    for e in ev.read() {
        let pid = e.pid;
        // 归档当前星球
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
        // 清理当前场景
        for ent in &chunk_meshes {
            commands.entity(ent).despawn();
        }
        for (ent, _, _) in &machines {
            commands.entity(ent).despawn();
        }
        for (ent, _, _) in &creatures {
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
fn space_sky_sync_system(
    mode: Res<FlightMode>,
    mut sun_disc: Query<&mut Visibility, (With<SunDisc>, Without<daynight::Star>)>,
    mut stars: Query<&mut Visibility, (With<daynight::Star>, Without<SunDisc>)>,
) {
    let show = mode.ground_scene();
    for mut vis in &mut sun_disc {
        *vis = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for mut vis in &mut stars {
        *vis = if show && stars_ok() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn stars_ok() -> bool {
    true
}

/// 地面体素场景实体（区块地形/生物/掉落物/建造虚影/激光束）随模式显隐：
/// JS 原版太空态不渲染 planetScene（独立场景切换），Bevy 单相机下必须显式隐藏，
/// 否则冲出大气后平面地形残影留在宇宙里，与太空星球球形外壳错位同屏。
fn ground_scene_visibility_system(
    mode: Res<FlightMode>,
    world: Res<World>,
    player: Query<&Player>,
    ship: Res<ShipState>,
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
        )>,
    >,
) {
    let (px, pz) = if matches!(
        *mode,
        FlightMode::Atmo | FlightMode::AtmoLand | FlightMode::Seated
    ) {
        (ship.pos.x, ship.pos.z)
    } else {
        player
            .single()
            .map(|p| (p.pos.x, p.pos.z))
            .unwrap_or((ship.pos.x, ship.pos.z))
    };
    let pcx = world::cf(px);
    let pcz = world::cf(pz);
    let show = mode.ground_scene();
    for (e, v, chunk) in &mut q {
        // 视距外网格先隐藏，再由 stream_world_step 延迟回收；高速穿越时不会
        // 出现旧区块残影、与新位置地形重叠或闪烁。
        let chunk_show = show
            && chunk
                .map(|c| World::cheb(c.cx, c.cz, pcx, pcz) <= world.view_dist + 1)
                .unwrap_or(true);
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

#[allow(clippy::too_many_arguments)]
fn save_system(
    mut ev: MessageReader<ui::SaveEvent>,
    player: Query<&Player>,
    world: Res<World>,
    research: Res<Research>,
    day: Res<daynight::DayTime>,
    names: Res<SaveNames>,
    game: ResMut<SpaceGame>,
    ship: Res<ShipState>,
    mode: Res<FlightMode>,
    ship_asset: Res<ShipAsset>,
    quests: Res<quests::Quests>,
    station: Option<Res<station::StationState>>,
    spawner: Res<creatures::CreatureSpawner>,
    creatures_q: Query<(Entity, &creatures::Creature, &Transform)>,
    mut commands: Commands,
    sfx: Res<audio::Sfx>,
) {
    for _ in ev.read() {
        let Ok(p) = player.single() else { continue };
        let state_str = if matches!(
            *mode,
            FlightMode::Space | FlightMode::Warping | FlightMode::Station
        ) {
            "space"
        } else {
            "planet"
        };
        let ok_char = save::save_char(
            p,
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
        let ship_pos = if *mode == FlightMode::Planet || *mode == FlightMode::Seated {
            Some([game.ship_pos.x, game.ship_pos.y, game.ship_pos.z])
        } else {
            None
        };
        let ship_state = if matches!(
            *mode,
            FlightMode::Space | FlightMode::Warping | FlightMode::Station
        ) {
            let mut ss = space::serialize_ship_state(&ship);
            // 站内存档存机库出口（JS main.js:2770-2775），读档不会重新泊入
            if *mode == FlightMode::Station
                && let Some(st) = station.as_ref()
            {
                ss.pos = [
                    st.station_pos.x + station::station_exit_pos()[0],
                    st.station_pos.y + station::station_exit_pos()[1],
                    st.station_pos.z + station::station_exit_pos()[2],
                ];
            }
            Some(ss)
        } else {
            None
        };
        let (creatures_save, creature_cells_save) = spawner.serialize(&creatures_q);
        let ok_world = save::save_world_full(
            &world,
            &names.world,
            day.0,
            state_str,
            game.current_planet,
            game.galaxy.seed,
            game.galaxy_count,
            &game.galaxy.market,
            &quests.flags,
            ship_pos,
            ship_state.as_ref(),
            &game.marks,
            game.warp_lock.as_ref(),
            &quests.placed,
            &game.archives,
            &creatures_save,
            &creature_cells_save,
        );
        if ok_char && ok_world {
            audio::play(&mut commands, sfx.pickup.clone(), 0.5, None);
        }
    }
}

fn quit_to_menu_system(
    mut ev: MessageReader<ui::QuitToMenuEvent>,
    mut commands: Commands,
    in_game: Query<Entity, With<InGame>>,
    mut network: ResMut<network::NetworkState>,
    mut next: ResMut<NextState<GameState>>,
) {
    for _ in ev.read() {
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
        commands.remove_resource::<ScanPulse>();
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
