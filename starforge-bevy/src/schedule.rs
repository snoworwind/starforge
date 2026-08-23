//! Game-wide scheduling contract + shared flow types.
//!
//! `GameSet` mirrors the total update order that `main.rs` used to enforce
//! with a single deeply nested `.chain()` tuple. Every plugin that registers
//! systems into `Update` tags them with one of these sets; the set chain in
//! [`configure`] restores the original deterministic order after modules are
//! split into their own plugins.

use bevy::prelude::*;

use crate::space::FlightMode;

/// Everything spawned in-game gets this marker (cleared when returning to menu).
#[derive(Component)]
pub struct InGame;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Menu,
    Loading,
    Playing,
}

/// Update schedule set labels, ordered via [`configure`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameSet {
    /// egui `begin_pass` — must run before any UI system draws.
    UiBeginPass,
    /// Main menu flow (`smoke_boot` → `menu_system`).
    Menu,
    /// World pre-generation screen (`loading_system`).
    Loading,
    // ---- playing chain (mirrors the original main.rs chain order) ----
    /// 通用: panel hotkeys → quicksave → focus clear → big message.
    CommonUi,
    /// quest_tick → side_quest → village_side_quest.
    CommonQuests,
    /// NPC idle animation.
    CommonNpc,
    /// Research progression.
    CommonResearch,
    /// Lamp pool follow.
    CommonLamp,
    /// Day/night + sky/sun/stars (writes `ClearColor` + `SpaceFactor`).
    CommonDaynight,
    /// Climate + rain audio (reads `ClearColor`/`SpaceFactor` — must follow daynight).
    CommonWeather,
    /// Player movement → collision → survival → mining → break → placement → hotbar.
    GroundPlayer,
    /// Terrain streaming around the player/ship.
    GroundStream,
    /// Creature spawn → AI → sound → animation → sentinel.
    GroundCreatures,
    /// Creature despawn → drops.
    LateCreatures,
    /// Factory ticks → machine sync → lumberbot visual.
    LateFactory,
    /// Scan markers/rings.
    LateScan,
    /// Player look → camera toggle → camera.
    LateLook,
    /// Space input → ship interact → recall → seated → atmo-land trigger.
    LateSpaceInput,
    /// Station state machine → defense → NPC spawn → ship switch.
    LateStation,
    /// Planet switch + ground-scene visibility (flow-owned).
    LateSwitchFlow,
    /// Space-mode sky sync (daynight-owned).
    LateSwitchSky,
    /// Cursor grab/visibility (player-owned).
    LateSwitchCursor,
    /// Build ghost preview.
    HudGhostUi,
    /// Laser beam + interact prompt.
    HudGhostPlayer,
    /// HUD + ship label.
    HudMain,
    /// Panels: lighting → inventory → tech → machine → pause → trade → garage → station services.
    PanelLighting,
    PanelInventory,
    PanelTech,
    PanelMachine,
    PanelPause,
    PanelTrade,
    PanelGarage,
    PanelStationServices,
    /// Buy-ship → galaxy map.
    SaveBuy,
    /// Network panel.
    SaveNetworkUi,
    /// Creative spawner panel.
    SaveCreative,
    /// Planet map (writes marks).
    SaveMap,
    /// Save settings + full save (after marks).
    SaveWrite,
    /// Quit-to-menu → smoke exit (last).
    SaveQuit,
    /// Long-distance terrain LOD selection before the legacy far-mesh fallback.
    FarLod,
    /// Legacy far mesh rebuild.
    FarMesh,
    /// egui `end_pass` — must run after every UI system.
    UiEndPass,
}

/// Applies the total `Update` order defined by [`GameSet`].
pub fn configure(app: &mut App) {
    app.configure_sets(
        Update,
        (
            (
                GameSet::UiBeginPass,
                GameSet::Menu,
                GameSet::Loading,
                GameSet::CommonUi,
                GameSet::CommonQuests,
                GameSet::CommonNpc,
                GameSet::CommonResearch,
                GameSet::CommonLamp,
            )
                .chain(),
            (
                GameSet::CommonDaynight,
                GameSet::CommonWeather,
                GameSet::GroundPlayer,
                GameSet::GroundStream,
                GameSet::GroundCreatures,
                GameSet::LateCreatures,
                GameSet::LateFactory,
                GameSet::LateScan,
            )
                .chain(),
            (
                GameSet::LateLook,
                GameSet::LateSpaceInput,
                GameSet::LateStation,
                GameSet::LateSwitchFlow,
                GameSet::LateSwitchSky,
                GameSet::LateSwitchCursor,
                GameSet::HudGhostUi,
                GameSet::HudGhostPlayer,
            )
                .chain(),
            (
                GameSet::HudMain,
                GameSet::PanelLighting,
                GameSet::PanelInventory,
                GameSet::PanelTech,
                GameSet::PanelMachine,
                GameSet::PanelPause,
                GameSet::PanelTrade,
                GameSet::PanelGarage,
                GameSet::PanelStationServices,
            )
                .chain(),
            (
                GameSet::SaveBuy,
                GameSet::SaveNetworkUi,
                GameSet::SaveCreative,
                GameSet::SaveMap,
                GameSet::SaveWrite,
                GameSet::SaveQuit,
                GameSet::FarLod,
                GameSet::FarMesh,
                GameSet::UiEndPass,
            )
                .chain(),
        )
            .chain(),
    );
}

pub fn ground_mode(mode: Res<FlightMode>) -> bool {
    *mode == FlightMode::Planet
}

/// 只要地表场景仍可见，生物就继续运行完整 AI；飞船飞过大气层时不能
/// 只播放骨骼动画而冻结位置。
pub fn creature_mode(mode: Res<FlightMode>) -> bool {
    mode.ground_scene()
}

pub fn in_planet_mode(mode: Res<FlightMode>) -> bool {
    *mode == FlightMode::Planet || *mode == FlightMode::Seated
}

pub fn ground_scene_mode(mode: Res<FlightMode>) -> bool {
    mode.ground_scene()
}

pub fn walk_look_mode(mode: Res<FlightMode>) -> bool {
    *mode == FlightMode::Planet || *mode == FlightMode::Seated || *mode == FlightMode::Station
}
