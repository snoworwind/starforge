//! A05 unified visual diagnostics snapshot.
//!
//! One resource remembers what the renderer actually resolved, what the
//! lifecycle counters currently are and which stored settings had to be
//! repaired, so QA (K03/K05) can export a single file instead of asking every
//! subsystem to read every other subsystem. Counts here are asset/entity
//! entries, not GPU memory measurements.

use bevy::prelude::*;
use serde::Serialize;

use super::frame::VisualFrame;
use super::lifecycle::VisualLifecycleDiagnostics;
use super::quality::ResolvedQuality;
use super::settings::{
    LEGACY_SETTING_KEYS, SettingsLoadReport, VisualPreset, VisualSettings, detect_visual_preset,
    numeric_value,
};
use crate::save::Settings;

pub const VISUAL_DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

/// Small camera summary; the full matrices stay in `VisualFrame` because a
/// diagnostics file does not need 64 floats per frame.
#[derive(Clone, Debug, Default, Serialize)]
pub struct VisualCameraSnapshot {
    pub frame_id: u64,
    pub world_epoch: u64,
    pub camera_id: u32,
    pub viewport: [u32; 2],
    pub paused: bool,
    pub dt_seconds: f32,
    pub fov_y_degrees: f32,
    pub near: f32,
    pub far: f32,
}

/// Unified snapshot exported by `visual-qa` and shown in the advanced
/// diagnostics foldout.
#[derive(Resource, Clone, Debug, Default, Serialize)]
pub struct VisualDiagnostics {
    pub schema_version: u32,
    pub camera: VisualCameraSnapshot,
    pub quality: ResolvedQuality,
    pub lifecycle: VisualLifecycleDiagnostics,
    /// Effective settings exactly as the renderer reads them.
    pub settings: Settings,
    pub preset: VisualPreset,
    pub settings_load: SettingsLoadReport,
    /// Threshold violations from [`VisualSettings`]; empty when within budget.
    pub warnings: Vec<String>,
    /// Persisted parameters that no longer drive rendering (R061/R117).
    pub inert_settings: Vec<String>,
}

fn cache_entities(lifecycle: &VisualLifecycleDiagnostics, key: &str) -> usize {
    lifecycle
        .caches
        .iter()
        .find(|metric| metric.key == key)
        .map_or(0, |metric| metric.entities)
}

/// Compare lifecycle counters against the validated warning thresholds.
pub fn resource_warnings(
    settings: &VisualSettings,
    lifecycle: &VisualLifecycleDiagnostics,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if lifecycle.tasks.pending > settings.warn_pending_tasks as usize {
        warnings.push(format!(
            "视觉任务积压 {} > {}",
            lifecycle.tasks.pending, settings.warn_pending_tasks
        ));
    }
    let in_game = cache_entities(lifecycle, "game.in_game_entities");
    if in_game > settings.warn_in_game_entities as usize {
        warnings.push(format!(
            "InGame 实体 {} > {}",
            in_game, settings.warn_in_game_entities
        ));
    }
    let chunks = cache_entities(lifecycle, "terrain.chunk_meshes");
    if chunks > settings.warn_chunk_meshes as usize {
        warnings.push(format!(
            "区块网格 {} > {}",
            chunks, settings.warn_chunk_meshes
        ));
    }
    let assets = lifecycle.assets.meshes
        + lifecycle.assets.images
        + lifecycle.assets.standard_materials
        + lifecycle.assets.curved_materials
        + lifecycle.assets.cloud_materials;
    if assets > settings.warn_asset_total as usize {
        warnings.push(format!(
            "视觉资产 {} > {}",
            assets, settings.warn_asset_total
        ));
    }
    warnings
}

fn inert_settings(settings: &Settings) -> Vec<String> {
    LEGACY_SETTING_KEYS
        .iter()
        .filter_map(|key| {
            let value = numeric_value(settings, key)?;
            Some(format!("{key}={value}（已废弃，不影响渲染）"))
        })
        .collect()
}

fn load_warnings(load: &SettingsLoadReport) -> Vec<String> {
    let mut warnings = Vec::new();
    if load.fallback_to_defaults {
        warnings.push("settings.json 无法解析，已使用默认设置".to_string());
    }
    for correction in &load.corrections {
        let previous = correction
            .previous
            .map(|value| format!("{value}"))
            .unwrap_or_else(|| "非有限值".to_string());
        warnings.push(format!(
            "设置 {} 从 {} 修正为 {}",
            correction.key, previous, correction.corrected
        ));
    }
    warnings
}

pub(super) fn collect_visual_diagnostics(
    frame: Res<VisualFrame>,
    quality: Res<ResolvedQuality>,
    lifecycle: Res<VisualLifecycleDiagnostics>,
    settings: Res<Settings>,
    load_report: Option<Res<SettingsLoadReport>>,
    mut diagnostics: ResMut<VisualDiagnostics>,
) {
    let load_report = load_report.as_deref().cloned().unwrap_or_default();
    let mut warnings = resource_warnings(&settings.visual, &lifecycle);
    warnings.extend(load_warnings(&load_report));
    *diagnostics = VisualDiagnostics {
        schema_version: VISUAL_DIAGNOSTICS_SCHEMA_VERSION,
        camera: VisualCameraSnapshot {
            frame_id: frame.frame_id,
            world_epoch: frame.world_epoch,
            camera_id: frame.camera_id.0,
            viewport: [frame.viewport.x, frame.viewport.y],
            paused: frame.paused,
            dt_seconds: frame.dt,
            fov_y_degrees: frame.fov_y_radians.to_degrees(),
            near: frame.near,
            far: frame.far,
        },
        quality: quality.clone(),
        lifecycle: lifecycle.clone(),
        settings: settings.as_ref().clone(),
        preset: detect_visual_preset(&settings),
        settings_load: load_report,
        warnings,
        inert_settings: inert_settings(&settings),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visual::lifecycle::{VisualAssetTotals, VisualCacheMetric, VisualTaskStats};

    fn lifecycle_with(
        pending: usize,
        in_game: usize,
        chunks: usize,
        assets: usize,
    ) -> VisualLifecycleDiagnostics {
        VisualLifecycleDiagnostics {
            tasks: VisualTaskStats {
                pending,
                ..Default::default()
            },
            caches: vec![
                VisualCacheMetric {
                    key: "game.in_game_entities".to_string(),
                    entities: in_game,
                    ..Default::default()
                },
                VisualCacheMetric {
                    key: "terrain.chunk_meshes".to_string(),
                    entities: chunks,
                    ..Default::default()
                },
            ],
            assets: VisualAssetTotals {
                meshes: assets,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn thresholds_fire_above_the_limit_and_clear_below() {
        let limits = VisualSettings {
            warn_pending_tasks: 10,
            warn_in_game_entities: 100,
            warn_chunk_meshes: 50,
            warn_asset_total: 200,
            ..Default::default()
        };
        let over = lifecycle_with(11, 101, 51, 201);
        let warnings = resource_warnings(&limits, &over);
        assert_eq!(warnings.len(), 4, "{warnings:?}");

        let under = lifecycle_with(10, 100, 50, 200);
        assert!(resource_warnings(&limits, &under).is_empty());
    }

    #[test]
    fn load_report_warnings_are_human_readable() {
        let load = SettingsLoadReport {
            fallback_to_defaults: true,
            corrections: vec![crate::visual::settings::SettingCorrection {
                key: "mouse_sens".to_string(),
                previous: None,
                corrected: 1.0,
                reason: crate::visual::settings::CorrectionReason::NonFinite,
            }],
        };
        let warnings = load_warnings(&load);
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("settings.json"));
        assert!(warnings[1].contains("mouse_sens"));
    }

    #[test]
    fn legacy_settings_are_reported_as_inert() {
        let settings = Settings::default();
        let listed = inert_settings(&settings);
        assert_eq!(listed.len(), LEGACY_SETTING_KEYS.len());
        assert!(listed.iter().all(|item| item.contains("已废弃")));
    }
}
