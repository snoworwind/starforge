//! A04 visual resource lifetime, epoch and asynchronous-result contracts.
//!
//! Visual resources are not all owned by the application.  The ownership
//! table below is deliberately executable data: diagnostics use the same
//! names and scopes that cleanup/rebuild code documents.  A world epoch is
//! advanced before a new/load world is built, when the active planet changes,
//! and when gameplay returns to the menu.  Camera history and every queued
//! visual task therefore have one authoritative invalidation token.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::Serialize;

use super::{VisualCameraId, WorldEpoch};

const DEFAULT_TASK_CAPACITY: usize = 256;

/// Lifetime tier for a visual resource or background job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualOwner {
    Application,
    World,
    Planet,
    Chunk,
    Camera,
    Entity,
}

/// Why the current visual world was invalidated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldTransition {
    /// A new game or saved game is entering the Loading state.
    WorldLoad,
    /// The voxel/planet scene is replaced without leaving Playing.
    PlanetSwitch,
    /// Playing is being torn down before returning to the menu.
    ReturnToMenu,
}

impl WorldTransition {
    fn invalidates(self, owner: VisualOwner) -> bool {
        match self {
            Self::WorldLoad | Self::ReturnToMenu => owner != VisualOwner::Application,
            Self::PlanetSwitch => matches!(
                owner,
                VisualOwner::Planet
                    | VisualOwner::Chunk
                    | VisualOwner::Camera
                    | VisualOwner::Entity
            ),
        }
    }
}

/// One row in the resource ownership table required by A04.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct VisualOwnership {
    pub key: &'static str,
    pub owner: VisualOwner,
    pub release: &'static str,
}

/// Current concrete ownership map. Future workstreams add rows when they add
/// caches; they must not invent a second lifetime hierarchy.
pub const VISUAL_OWNERSHIP: &[VisualOwnership] = &[
    VisualOwnership {
        key: "ui.icon_materials",
        owner: VisualOwner::Application,
        release: "application exit",
    },
    VisualOwnership {
        key: "particles.shared_assets",
        owner: VisualOwner::Application,
        release: "application exit",
    },
    VisualOwnership {
        key: "terrain.materials",
        owner: VisualOwner::Planet,
        release: "planet switch or menu",
    },
    VisualOwnership {
        key: "weather.climate",
        owner: VisualOwner::Planet,
        release: "epoch/fingerprint change",
    },
    VisualOwnership {
        key: "terrain.lod",
        owner: VisualOwner::Planet,
        release: "epoch change or menu",
    },
    VisualOwnership {
        key: "terrain.chunk_meshes",
        owner: VisualOwner::Chunk,
        release: "stream eviction, planet switch or menu",
    },
    VisualOwnership {
        key: "camera.frame_history",
        owner: VisualOwner::Camera,
        release: "epoch, resize, zero-size or camera removal",
    },
    VisualOwnership {
        key: "weather.entities",
        owner: VisualOwner::Entity,
        release: "climate rebuild, planet switch or menu",
    },
    VisualOwnership {
        key: "game.in_game_entities",
        owner: VisualOwner::Entity,
        release: "menu",
    },
];

/// Monotonic revision supplied by the producer of an asynchronous input.
/// C03/E02/G work can use a chunk, weather-map or structure revision without
/// changing the common epoch validation path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub struct VisualRevision(pub u64);

/// Token copied into immutable worker input and returned with its CPU result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct VisualTaskTicket {
    pub id: u64,
    pub owner: VisualOwner,
    pub world_epoch: u64,
    pub revision: VisualRevision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskSubmitError {
    UnknownOrCancelled,
    StaleEpoch,
    StaleRevision,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct VisualTaskStats {
    pub capacity: usize,
    pub pending: usize,
    pub accepted: u64,
    pub cancelled: u64,
    pub rejected_queue_full: u64,
    pub rejected_unknown: u64,
    pub rejected_stale_epoch: u64,
    pub rejected_stale_revision: u64,
}

#[derive(Clone, Copy, Debug)]
struct PendingTask {
    owner: VisualOwner,
    epoch: u64,
    revision: VisualRevision,
}

/// Bounded registry for visual background work. It does not prescribe a task
/// executor; it prescribes the safety boundary around any executor. Worker
/// code gets a ticket, returns it unchanged, and the main thread calls
/// `submit_result` before creating or replacing Bevy assets.
#[derive(Debug)]
pub struct VisualTaskRegistry {
    capacity: usize,
    next_id: u64,
    pending: HashMap<u64, PendingTask>,
    stats: VisualTaskStats,
}

impl Default for VisualTaskRegistry {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_TASK_CAPACITY)
    }
}

impl VisualTaskRegistry {
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            next_id: 1,
            pending: HashMap::new(),
            stats: VisualTaskStats {
                capacity,
                ..Default::default()
            },
        }
    }

    pub fn issue(
        &mut self,
        owner: VisualOwner,
        epoch: WorldEpoch,
        revision: VisualRevision,
    ) -> Option<VisualTaskTicket> {
        if self.pending.len() >= self.capacity {
            self.stats.rejected_queue_full = self.stats.rejected_queue_full.saturating_add(1);
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.pending.insert(
            id,
            PendingTask {
                owner,
                epoch: epoch.0,
                revision,
            },
        );
        self.refresh_pending();
        Some(VisualTaskTicket {
            id,
            owner,
            world_epoch: epoch.0,
            revision,
        })
    }

    /// Validate a completed worker result before its payload reaches Assets or
    /// ECS. A rejected result is consumed exactly once and must be dropped.
    pub fn submit_result(
        &mut self,
        ticket: VisualTaskTicket,
        current_epoch: WorldEpoch,
        current_revision: VisualRevision,
    ) -> Result<(), TaskSubmitError> {
        let Some(pending) = self.pending.remove(&ticket.id) else {
            self.stats.rejected_unknown = self.stats.rejected_unknown.saturating_add(1);
            return Err(TaskSubmitError::UnknownOrCancelled);
        };
        self.refresh_pending();
        if pending.owner != ticket.owner
            || pending.epoch != ticket.world_epoch
            || pending.revision != ticket.revision
        {
            self.stats.rejected_unknown = self.stats.rejected_unknown.saturating_add(1);
            return Err(TaskSubmitError::UnknownOrCancelled);
        }
        if ticket.owner != VisualOwner::Application && ticket.world_epoch != current_epoch.0 {
            self.stats.rejected_stale_epoch = self.stats.rejected_stale_epoch.saturating_add(1);
            return Err(TaskSubmitError::StaleEpoch);
        }
        if ticket.revision != current_revision {
            self.stats.rejected_stale_revision =
                self.stats.rejected_stale_revision.saturating_add(1);
            return Err(TaskSubmitError::StaleRevision);
        }
        self.stats.accepted = self.stats.accepted.saturating_add(1);
        Ok(())
    }

    fn cancel_for(&mut self, transition: WorldTransition) -> usize {
        let before = self.pending.len();
        self.pending
            .retain(|_, task| !transition.invalidates(task.owner));
        let cancelled = before - self.pending.len();
        self.stats.cancelled = self.stats.cancelled.saturating_add(cancelled as u64);
        self.refresh_pending();
        cancelled
    }

    fn refresh_pending(&mut self) {
        self.stats.pending = self.pending.len();
    }

    pub fn stats(&self) -> VisualTaskStats {
        self.stats
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryResetReason {
    CameraCreated,
    WorldEpochChanged,
    ViewportResized,
    WindowSuspended,
    WindowRestored,
}

#[derive(Clone, Copy, Debug)]
struct CameraLifetime {
    epoch: u64,
    viewport: UVec2,
    suspended: bool,
}

/// Result of observing a camera at the final frame-sampling point.
#[derive(Clone, Copy, Debug)]
pub struct CameraObservation {
    pub viewport: UVec2,
    pub reset_history: bool,
    pub suspended: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TransitionRecord {
    pub sequence: u64,
    pub from_epoch: u64,
    pub to_epoch: u64,
    pub reason: WorldTransition,
    pub cancelled_tasks: usize,
}

/// Runtime owner of epoch changes, camera-history invalidation and bounded
/// background visual work.
#[derive(Resource, Debug, Default)]
pub struct VisualLifecycle {
    transition_sequence: u64,
    last_transition: Option<TransitionRecord>,
    cameras: HashMap<VisualCameraId, CameraLifetime>,
    tasks: VisualTaskRegistry,
    history_resets: u64,
    last_history_reset: Option<HistoryResetReason>,
}

impl VisualLifecycle {
    pub fn tasks_mut(&mut self) -> &mut VisualTaskRegistry {
        &mut self.tasks
    }

    /// Apply the A05 `visual.task_capacity` setting. Called during startup
    /// before any world exists, so replacing the registry cannot drop live
    /// work; re-issuing later is intentionally not supported (Restart mode).
    pub fn set_task_capacity(&mut self, capacity: usize) {
        self.tasks = VisualTaskRegistry::with_capacity(capacity);
    }

    pub fn task_stats(&self) -> VisualTaskStats {
        self.tasks.stats()
    }

    pub fn last_transition(&self) -> Option<TransitionRecord> {
        self.last_transition
    }

    pub fn observe_camera(
        &mut self,
        camera: VisualCameraId,
        epoch: WorldEpoch,
        physical_viewport: Option<UVec2>,
    ) -> CameraObservation {
        let valid_viewport = physical_viewport.filter(|size| size.x > 0 && size.y > 0);
        let mut reset = None;
        let mut viewport = valid_viewport.unwrap_or(UVec2::ONE);
        let suspended = valid_viewport.is_none();

        match self.cameras.get_mut(&camera) {
            None => {
                reset = Some(HistoryResetReason::CameraCreated);
                self.cameras.insert(
                    camera,
                    CameraLifetime {
                        epoch: epoch.0,
                        viewport,
                        suspended,
                    },
                );
            }
            Some(previous) => {
                if suspended {
                    viewport = previous.viewport;
                }
                if previous.epoch != epoch.0 {
                    reset = Some(HistoryResetReason::WorldEpochChanged);
                } else if !previous.suspended && suspended {
                    reset = Some(HistoryResetReason::WindowSuspended);
                } else if previous.suspended && !suspended {
                    reset = Some(HistoryResetReason::WindowRestored);
                } else if !suspended && previous.viewport != viewport {
                    reset = Some(HistoryResetReason::ViewportResized);
                }
                previous.epoch = epoch.0;
                previous.viewport = viewport;
                previous.suspended = suspended;
            }
        }

        if let Some(reason) = reset {
            self.history_resets = self.history_resets.saturating_add(1);
            self.last_history_reset = Some(reason);
        }
        CameraObservation {
            viewport,
            reset_history: reset.is_some(),
            suspended,
        }
    }

    fn advance(&mut self, epoch: &mut WorldEpoch, reason: WorldTransition) -> TransitionRecord {
        let from_epoch = epoch.0;
        epoch.0 = epoch.0.wrapping_add(1).max(1);
        self.transition_sequence = self.transition_sequence.wrapping_add(1).max(1);
        let record = TransitionRecord {
            sequence: self.transition_sequence,
            from_epoch,
            to_epoch: epoch.0,
            reason,
            cancelled_tasks: self.tasks.cancel_for(reason),
        };
        self.last_transition = Some(record);
        record
    }
}

/// Advance the one authoritative epoch. Flow code calls this immediately
/// before replacing a planet; state hooks below cover loading and menu return.
pub fn advance_world_epoch(
    epoch: &mut WorldEpoch,
    lifecycle: &mut VisualLifecycle,
    reason: WorldTransition,
) -> TransitionRecord {
    let record = lifecycle.advance(epoch, reason);
    info!(
        "visual world epoch {}→{} reason={:?} cancelled_tasks={}",
        record.from_epoch, record.to_epoch, record.reason, record.cancelled_tasks
    );
    record
}

/// Bundles the two authoritative lifecycle resources so already-large flow
/// systems do not exceed Bevy's system-parameter tuple limit.
#[derive(SystemParam)]
pub struct VisualLifecycleMut<'w> {
    epoch: ResMut<'w, WorldEpoch>,
    lifecycle: ResMut<'w, VisualLifecycle>,
}

impl VisualLifecycleMut<'_> {
    pub fn advance(&mut self, reason: WorldTransition) -> TransitionRecord {
        advance_world_epoch(&mut self.epoch, &mut self.lifecycle, reason)
    }
}

pub(super) fn begin_loading_epoch(
    mut epoch: ResMut<WorldEpoch>,
    mut lifecycle: ResMut<VisualLifecycle>,
) {
    advance_world_epoch(&mut epoch, &mut lifecycle, WorldTransition::WorldLoad);
}

pub(super) fn finish_playing_epoch(
    mut epoch: ResMut<WorldEpoch>,
    mut lifecycle: ResMut<VisualLifecycle>,
) {
    advance_world_epoch(&mut epoch, &mut lifecycle, WorldTransition::ReturnToMenu);
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct VisualCacheMetric {
    pub key: String,
    pub owner: Option<VisualOwner>,
    pub entities: usize,
    pub strong_handles: usize,
    pub queued: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct VisualAssetTotals {
    pub meshes: usize,
    pub images: usize,
    pub standard_materials: usize,
    pub curved_materials: usize,
    pub cloud_materials: usize,
}

/// Cheap, once-per-frame lifetime diagnostics. Counts are snapshots, not GPU
/// memory measurements; K03 will add device-specific resident-byte evidence.
#[derive(Resource, Clone, Debug, Default, Serialize)]
pub struct VisualLifecycleDiagnostics {
    pub world_epoch: u64,
    pub transition_count: u64,
    pub last_transition: Option<TransitionRecord>,
    pub history_resets: u64,
    pub last_history_reset: Option<HistoryResetReason>,
    pub camera_suspended: bool,
    pub tasks: VisualTaskStats,
    pub caches: Vec<VisualCacheMetric>,
    pub assets: VisualAssetTotals,
}

fn owner_for(key: &str) -> Option<VisualOwner> {
    VISUAL_OWNERSHIP
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.owner)
}

#[derive(SystemParam)]
pub(super) struct LifecycleDiagnosticInputs<'w, 's> {
    meshes: Res<'w, Assets<Mesh>>,
    images: Res<'w, Assets<Image>>,
    standard_materials: Res<'w, Assets<StandardMaterial>>,
    curved_materials: Res<'w, Assets<crate::materials::CurvedTerrainMaterial>>,
    cloud_materials: Res<'w, Assets<crate::weather::CloudShellMaterial>>,
    terrain_materials: Option<Res<'w, crate::materials::TerrainMaterials>>,
    climate: Option<Res<'w, crate::weather::ClimateRuntime>>,
    lod_runtime: Option<Res<'w, crate::lod::LodRuntime>>,
    particle_cache: Option<Res<'w, crate::particles::ParticleCache>>,
    icon_materials: Option<Res<'w, crate::ui::IconMaterials>>,
    chunks: Query<'w, 's, Entity, With<crate::world::ChunkMesh>>,
    lod_meshes: Query<'w, 's, Entity, With<crate::lod::LodMesh>>,
    clouds: Query<'w, 's, Entity, With<crate::weather::CloudVolume>>,
    weather: Query<'w, 's, Entity, With<crate::weather::WeatherParticle>>,
    in_game: Query<'w, 's, Entity, With<crate::InGame>>,
    cameras: Query<'w, 's, Entity, With<Camera3d>>,
}

pub(super) fn collect_lifecycle_diagnostics(
    epoch: Res<WorldEpoch>,
    lifecycle: Res<VisualLifecycle>,
    mut diagnostics: ResMut<VisualLifecycleDiagnostics>,
    inputs: LifecycleDiagnosticInputs,
) {
    let metric =
        |key: &str, entities: usize, strong_handles: usize, queued: usize| VisualCacheMetric {
            key: key.to_string(),
            owner: owner_for(key),
            entities,
            strong_handles,
            queued,
        };
    let particle_handles = inputs.particle_cache.as_ref().map_or(0, |cache| {
        cache.meshes.len() + cache.materials.len() + usize::from(cache.soft_texture.is_some())
    });
    let particle_entities = inputs
        .particle_cache
        .as_ref()
        .map_or(0, |cache| cache.active);
    let icon_handles = inputs
        .icon_materials
        .as_ref()
        .map_or(0, |icons| icons.map.len() + 2);
    let climate_handles = inputs
        .climate
        .as_ref()
        .map_or(0, |runtime| runtime.lifecycle_owned_handles());
    let lod_handles = inputs
        .lod_runtime
        .as_ref()
        .map_or(0, |runtime| runtime.lifecycle_owned_handles());
    let lod_queued = inputs
        .lod_runtime
        .as_ref()
        .map_or(0, |runtime| runtime.stats.queued_sections);
    let camera_suspended = lifecycle.cameras.values().any(|camera| camera.suspended);

    *diagnostics = VisualLifecycleDiagnostics {
        world_epoch: epoch.0,
        transition_count: lifecycle.transition_sequence,
        last_transition: lifecycle.last_transition,
        history_resets: lifecycle.history_resets,
        last_history_reset: lifecycle.last_history_reset,
        camera_suspended,
        tasks: lifecycle.task_stats(),
        caches: vec![
            metric("ui.icon_materials", 0, icon_handles, 0),
            metric(
                "particles.shared_assets",
                particle_entities,
                particle_handles,
                0,
            ),
            metric(
                "terrain.materials",
                0,
                inputs.terrain_materials.as_ref().map_or(0, |_| 5),
                0,
            ),
            metric(
                "weather.climate",
                inputs.clouds.iter().count(),
                climate_handles,
                0,
            ),
            metric(
                "terrain.lod",
                inputs.lod_meshes.iter().count(),
                lod_handles,
                lod_queued,
            ),
            metric("terrain.chunk_meshes", inputs.chunks.iter().count(), 0, 0),
            metric("camera.frame_history", inputs.cameras.iter().count(), 0, 0),
            metric("weather.entities", inputs.weather.iter().count(), 0, 0),
            metric("game.in_game_entities", inputs.in_game.iter().count(), 0, 0),
        ],
        assets: VisualAssetTotals {
            meshes: inputs.meshes.len(),
            images: inputs.images.len(),
            standard_materials: inputs.standard_materials.len(),
            curved_materials: inputs.curved_materials.len(),
            cloud_materials: inputs.cloud_materials.len(),
        },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_table_has_unique_keys_and_leaf_to_root_release_text() {
        let mut keys = std::collections::HashSet::new();
        for entry in VISUAL_OWNERSHIP {
            assert!(keys.insert(entry.key), "duplicate owner key {}", entry.key);
            assert!(!entry.release.is_empty());
        }
        assert!(
            VISUAL_OWNERSHIP
                .iter()
                .any(|e| e.owner == VisualOwner::Application)
        );
        assert!(
            VISUAL_OWNERSHIP
                .iter()
                .any(|e| e.owner == VisualOwner::Camera)
        );
    }

    #[test]
    fn task_capacity_setting_recreates_the_bounded_registry() {
        let mut lifecycle = VisualLifecycle::default();
        lifecycle.set_task_capacity(2);
        assert_eq!(lifecycle.task_stats().capacity, 2);
        assert!(
            lifecycle
                .tasks_mut()
                .issue(VisualOwner::Chunk, WorldEpoch(1), VisualRevision(0))
                .is_some()
        );
        assert!(
            lifecycle
                .tasks_mut()
                .issue(VisualOwner::Chunk, WorldEpoch(1), VisualRevision(1))
                .is_some()
        );
        assert!(
            lifecycle
                .tasks_mut()
                .issue(VisualOwner::Chunk, WorldEpoch(1), VisualRevision(2))
                .is_none()
        );
    }

    #[test]
    fn bounded_registry_rejects_excess_work() {
        let mut registry = VisualTaskRegistry::with_capacity(1);
        let epoch = WorldEpoch(3);
        assert!(
            registry
                .issue(VisualOwner::Chunk, epoch, VisualRevision(1))
                .is_some()
        );
        assert!(
            registry
                .issue(VisualOwner::Chunk, epoch, VisualRevision(2))
                .is_none()
        );
        assert_eq!(registry.stats().pending, 1);
        assert_eq!(registry.stats().rejected_queue_full, 1);
    }

    #[test]
    fn stale_revision_is_consumed_without_asset_commit() {
        let mut registry = VisualTaskRegistry::default();
        let ticket = registry
            .issue(VisualOwner::Chunk, WorldEpoch(7), VisualRevision(4))
            .unwrap();
        assert_eq!(
            registry.submit_result(ticket, WorldEpoch(7), VisualRevision(5)),
            Err(TaskSubmitError::StaleRevision)
        );
        assert_eq!(registry.stats().pending, 0);
        assert_eq!(registry.stats().rejected_stale_revision, 1);
    }

    #[test]
    fn delayed_old_epoch_result_is_rejected() {
        let mut registry = VisualTaskRegistry::default();
        let ticket = registry
            .issue(VisualOwner::Planet, WorldEpoch(9), VisualRevision(2))
            .unwrap();
        assert_eq!(
            registry.submit_result(ticket, WorldEpoch(10), VisualRevision(2)),
            Err(TaskSubmitError::StaleEpoch)
        );
        assert_eq!(registry.stats().rejected_stale_epoch, 1);
    }

    #[test]
    fn transition_cancels_only_invalidated_owners() {
        let mut lifecycle = VisualLifecycle::default();
        let mut epoch = WorldEpoch(1);
        let app_ticket = lifecycle
            .tasks_mut()
            .issue(VisualOwner::Application, epoch, VisualRevision(0))
            .unwrap();
        let planet_ticket = lifecycle
            .tasks_mut()
            .issue(VisualOwner::Planet, epoch, VisualRevision(0))
            .unwrap();
        let record = advance_world_epoch(&mut epoch, &mut lifecycle, WorldTransition::PlanetSwitch);
        assert_eq!(epoch.0, 2);
        assert_eq!(record.cancelled_tasks, 1);
        assert_eq!(lifecycle.task_stats().pending, 1);
        assert_eq!(
            lifecycle
                .tasks_mut()
                .submit_result(planet_ticket, epoch, VisualRevision(0)),
            Err(TaskSubmitError::UnknownOrCancelled)
        );
        assert_eq!(
            lifecycle
                .tasks_mut()
                .submit_result(app_ticket, epoch, VisualRevision(0)),
            Ok(())
        );
    }

    #[test]
    fn camera_history_resets_on_epoch_resize_suspend_and_restore() {
        let mut lifecycle = VisualLifecycle::default();
        let id = VisualCameraId::PRIMARY;
        let first = lifecycle.observe_camera(id, WorldEpoch(1), Some(UVec2::new(1280, 720)));
        assert!(first.reset_history);
        assert!(!first.suspended);
        let steady = lifecycle.observe_camera(id, WorldEpoch(1), Some(UVec2::new(1280, 720)));
        assert!(!steady.reset_history);
        let resized = lifecycle.observe_camera(id, WorldEpoch(1), Some(UVec2::new(1920, 1080)));
        assert!(resized.reset_history);
        let suspended = lifecycle.observe_camera(id, WorldEpoch(1), None);
        assert!(suspended.reset_history);
        assert_eq!(suspended.viewport, UVec2::new(1920, 1080));
        let restored = lifecycle.observe_camera(id, WorldEpoch(1), Some(UVec2::new(1920, 1080)));
        assert!(restored.reset_history);
        let epoch = lifecycle.observe_camera(id, WorldEpoch(2), Some(UVec2::new(1920, 1080)));
        assert!(epoch.reset_history);
        assert_eq!(
            lifecycle.last_history_reset,
            Some(HistoryResetReason::WorldEpochChanged)
        );
    }
}
