//! A05 settings registry, validation and quality presets.
//!
//! Every persisted visual/tuning parameter is described exactly once in
//! [`VISUAL_SETTING_SPECS`]. `load_settings`/`save_settings` both call
//! [`sanitize_settings`], so NaN, infinity and out-of-range values fall back
//! to a finite default and are *reported* instead of silently changing the
//! picture (06/R006). The same specs drive the UI slider ranges, which is what
//! keeps "displayed value == effective value" true instead of aspirational.
//!
//! Settings live in `saves/settings.json`, never in a world/character save.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::lifecycle::VisualLifecycle;
use crate::daynight::LightingTuning;
use crate::save::Settings;
use crate::weather::CloudTuning;

/// Coarse grouping used by diagnostics and the future J05 settings screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingGroup {
    Graphics,
    Clouds,
    Lighting,
    Audio,
    Input,
    Diagnostics,
}

/// When a changed value starts mattering, so the UI can say so instead of
/// letting the player assume a restart-free change took effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyMode {
    /// Read every frame (or written through to the owning runtime).
    Immediate,
    /// Stored now, read when the next world/planet is built.
    NextWorld,
    /// Only read during startup; the UI must label it "重启生效".
    Restart,
    /// Kept for save compatibility; no renderer reads it anymore (R117).
    Legacy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingValueKind {
    Float,
    Integer,
}

/// One authoritative numeric setting description.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SettingSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub group: SettingGroup,
    pub apply: ApplyMode,
    pub kind: SettingValueKind,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

impl SettingSpec {
    pub fn slider_range(&self) -> std::ops::RangeInclusive<f32> {
        (self.min as f32)..=(self.max as f32)
    }

    pub fn is_default(&self) -> bool {
        self.min == self.max && self.min == self.default
    }
}

/// All numeric settings the game persists today. Bool settings are covered by
/// [`apply_mode_for`]; the legacy cloud render-size pair is documented by
/// [`LEGACY_SETTING_KEYS`] because its sane values are an allowlist, not a
/// numeric interval.
pub const VISUAL_SETTING_SPECS: &[SettingSpec] = &[
    SettingSpec {
        key: "view_dist",
        label: "渲染距离",
        unit: "区块",
        group: SettingGroup::Graphics,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Integer,
        min: 3.0,
        max: 32.0,
        default: 10.0,
    },
    SettingSpec {
        key: "mouse_sens",
        label: "鼠标灵敏度",
        unit: "×",
        group: SettingGroup::Input,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.05,
        max: 5.0,
        default: 1.0,
    },
    SettingSpec {
        key: "volume",
        label: "主音量",
        unit: "",
        group: SettingGroup::Audio,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 1.0,
        default: 0.8,
    },
    SettingSpec {
        key: "music_volume",
        label: "音乐音量",
        unit: "",
        group: SettingGroup::Audio,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 1.0,
        default: 0.7,
    },
    SettingSpec {
        key: "ambience_volume",
        label: "环境氛围",
        unit: "",
        group: SettingGroup::Audio,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 1.0,
        default: 0.6,
    },
    SettingSpec {
        key: "cloud_coverage",
        label: "云量",
        unit: "",
        group: SettingGroup::Clouds,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 1.0,
        default: 0.61,
    },
    SettingSpec {
        key: "cloud_density",
        label: "云密度",
        unit: "",
        group: SettingGroup::Clouds,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 1.0,
        default: 0.09,
    },
    SettingSpec {
        key: "cloud_raymarch_steps",
        label: "云步数",
        unit: "步",
        group: SettingGroup::Clouds,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Integer,
        min: 4.0,
        max: 64.0,
        default: 24.0,
    },
    SettingSpec {
        key: "sunlight_boost",
        label: "太阳直射倍率",
        unit: "×",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 150.0,
        default: 1.0,
    },
    SettingSpec {
        key: "sun_disk_intensity",
        label: "太阳盘亮度",
        unit: "×",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 200.0,
        default: 1.0,
    },
    SettingSpec {
        key: "atmosphere_fill",
        label: "室外大气补光",
        unit: "×",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    SettingSpec {
        key: "ambient_multiplier",
        label: "全局环境光倍率",
        unit: "×",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 10.0,
        default: 1.0,
    },
    SettingSpec {
        key: "space_atmosphere_fill",
        label: "太空环境补光",
        unit: "×",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 2.0,
        default: 1.0,
    },
    SettingSpec {
        key: "bloom_intensity",
        label: "Bloom 溢出强度",
        unit: "",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 8.0,
        default: 0.12,
    },
    SettingSpec {
        key: "bloom_threshold",
        label: "Bloom 阈值",
        unit: "",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 50.0,
        default: 1.5,
    },
    SettingSpec {
        key: "bloom_threshold_softness",
        label: "Bloom 阈值柔化",
        unit: "",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 1.0,
        default: 0.2,
    },
    SettingSpec {
        key: "bloom_low_frequency_boost",
        label: "Bloom 低频扩散",
        unit: "",
        group: SettingGroup::Lighting,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Float,
        min: 0.0,
        max: 5.0,
        default: 0.35,
    },
    SettingSpec {
        key: "task_capacity",
        label: "视觉任务队列容量",
        unit: "任务",
        group: SettingGroup::Diagnostics,
        apply: ApplyMode::Restart,
        kind: SettingValueKind::Integer,
        min: 16.0,
        max: 4096.0,
        default: 256.0,
    },
    SettingSpec {
        key: "warn_pending_tasks",
        label: "任务积压告警",
        unit: "任务",
        group: SettingGroup::Diagnostics,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Integer,
        min: 1.0,
        max: 4096.0,
        default: 192.0,
    },
    SettingSpec {
        key: "warn_in_game_entities",
        label: "实体数告警",
        unit: "实体",
        group: SettingGroup::Diagnostics,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Integer,
        min: 100.0,
        max: 1_000_000.0,
        default: 6000.0,
    },
    SettingSpec {
        key: "warn_chunk_meshes",
        label: "区块网格告警",
        unit: "网格",
        group: SettingGroup::Diagnostics,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Integer,
        min: 16.0,
        max: 262_144.0,
        default: 4096.0,
    },
    SettingSpec {
        key: "warn_asset_total",
        label: "资产总数告警",
        unit: "资产",
        group: SettingGroup::Diagnostics,
        apply: ApplyMode::Immediate,
        kind: SettingValueKind::Integer,
        min: 100.0,
        max: 1_000_000.0,
        default: 12000.0,
    },
];

/// Persisted fields that no longer drive rendering. They are kept so old
/// settings files keep loading, but diagnostics list them instead of the UI
/// pretending they still do something (R061/R117).
pub const LEGACY_SETTING_KEYS: &[&str] = &["cloud_render_width", "cloud_render_height"];

/// Reasons recorded when a stored value was replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionReason {
    NonFinite,
    OutOfRange,
    UnsupportedValue,
}

/// One replacement of a stored value with its effective value.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SettingCorrection {
    pub key: String,
    /// `None` means the stored value was NaN/infinite (not representable in JSON).
    pub previous: Option<f64>,
    pub corrected: f64,
    pub reason: CorrectionReason,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SanitizeReport {
    pub corrections: Vec<SettingCorrection>,
}

impl SanitizeReport {
    pub fn is_clean(&self) -> bool {
        self.corrections.is_empty()
    }

    pub fn corrected(&self, key: &str) -> Option<&SettingCorrection> {
        self.corrections.iter().find(|item| item.key == key)
    }

    fn push(&mut self, key: &str, previous: Option<f64>, corrected: f64, reason: CorrectionReason) {
        self.corrections.push(SettingCorrection {
            key: key.to_string(),
            previous,
            corrected,
            reason,
        });
    }
}

/// Result of reading `saves/settings.json`: whether the whole file had to be
/// replaced and which individual values were repaired.
#[derive(Resource, Clone, Debug, Default, Serialize)]
pub struct SettingsLoadReport {
    /// The file existed but could not be parsed, so `Settings::default()` was
    /// used for every field.
    pub fallback_to_defaults: bool,
    pub corrections: Vec<SettingCorrection>,
}

impl SettingsLoadReport {
    pub fn is_clean(&self) -> bool {
        !self.fallback_to_defaults && self.corrections.is_empty()
    }

    pub fn into_sanitize_report(self) -> SanitizeReport {
        SanitizeReport {
            corrections: self.corrections,
        }
    }
}

/// Extra diagnostics knobs. Nested like `lighting` so old settings files load
/// without migration and gameplay saves never see these fields.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct VisualSettings {
    /// Bound for `VisualTaskRegistry`; applied during startup (Restart).
    pub task_capacity: u32,
    /// Warn when queued visual tasks exceed this.
    pub warn_pending_tasks: u32,
    /// Warn when the in-game entity count exceeds this.
    pub warn_in_game_entities: u32,
    /// Warn when resident chunk meshes exceed this.
    pub warn_chunk_meshes: u32,
    /// Warn when mesh+image+material asset entries exceed this.
    pub warn_asset_total: u32,
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            task_capacity: 256,
            warn_pending_tasks: 192,
            warn_in_game_entities: 6000,
            warn_chunk_meshes: 4096,
            warn_asset_total: 12000,
        }
    }
}

/// Selectable quality presets. `Custom` is the detected state after any manual
/// change, never an applied preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualPreset {
    Low,
    Medium,
    High,
    #[default]
    Custom,
}

impl VisualPreset {
    pub const SELECTABLE: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "低",
            Self::Medium => "中",
            Self::High => "高",
            Self::Custom => "自定义",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Custom => "custom",
        }
    }
}

pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    VISUAL_SETTING_SPECS.iter().find(|spec| spec.key == key)
}

/// Apply-mode lookup that also covers the bool settings and the legacy pair.
pub fn apply_mode_for(key: &str) -> Option<ApplyMode> {
    match key {
        "pixelated" => Some(ApplyMode::Restart),
        "clouds" | "weather" | "music" | "show_fps" | "lod_mode" => Some(ApplyMode::Immediate),
        "cloud_render_width" | "cloud_render_height" => Some(ApplyMode::Legacy),
        other => spec(other).map(|spec| spec.apply),
    }
}

/// Clamp a value that has a spec, without reporting (runtime tuning helpers).
pub fn clamp_f32(key: &str, value: f32) -> f32 {
    let Some(spec) = spec(key) else {
        return value;
    };
    if !value.is_finite() {
        return spec.default as f32;
    }
    value.clamp(spec.min as f32, spec.max as f32)
}

pub fn clamp_u32(key: &str, value: u32) -> u32 {
    let Some(spec) = spec(key) else {
        return value;
    };
    value.clamp(spec.min as u32, spec.max as u32)
}

fn sanitize_f32(key: &str, value: f32, report: &mut SanitizeReport) -> f32 {
    let Some(spec) = spec(key) else {
        return value;
    };
    if !value.is_finite() {
        report.push(key, None, spec.default, CorrectionReason::NonFinite);
        return spec.default as f32;
    }
    let clamped = value.clamp(spec.min as f32, spec.max as f32);
    if clamped != value {
        report.push(
            key,
            Some(value as f64),
            clamped as f64,
            CorrectionReason::OutOfRange,
        );
    }
    clamped
}

fn sanitize_i32(key: &str, value: i32, report: &mut SanitizeReport) -> i32 {
    let Some(spec) = spec(key) else {
        return value;
    };
    let clamped = value.clamp(spec.min as i32, spec.max as i32);
    if clamped != value {
        report.push(
            key,
            Some(value as f64),
            clamped as f64,
            CorrectionReason::OutOfRange,
        );
    }
    clamped
}

fn sanitize_u32(key: &str, value: u32, report: &mut SanitizeReport) -> u32 {
    let Some(spec) = spec(key) else {
        return value;
    };
    let clamped = value.clamp(spec.min as u32, spec.max as u32);
    if clamped != value {
        report.push(
            key,
            Some(value as f64),
            clamped as f64,
            CorrectionReason::OutOfRange,
        );
    }
    clamped
}

/// Runtime lighting panel sanitizes through the same specs as the settings
/// file so the slider range and the accepted range cannot drift apart.
pub fn sanitize_lighting_tuning(tuning: &mut LightingTuning, report: Option<&mut SanitizeReport>) {
    let mut local;
    let report = match report {
        Some(report) => report,
        None => {
            local = SanitizeReport::default();
            &mut local
        }
    };
    tuning.sunlight_boost = sanitize_f32("sunlight_boost", tuning.sunlight_boost, report);
    tuning.sun_disk_intensity =
        sanitize_f32("sun_disk_intensity", tuning.sun_disk_intensity, report);
    tuning.atmosphere_fill = sanitize_f32("atmosphere_fill", tuning.atmosphere_fill, report);
    tuning.ambient_multiplier =
        sanitize_f32("ambient_multiplier", tuning.ambient_multiplier, report);
    tuning.space_atmosphere_fill = sanitize_f32(
        "space_atmosphere_fill",
        tuning.space_atmosphere_fill,
        report,
    );
    tuning.bloom_intensity = sanitize_f32("bloom_intensity", tuning.bloom_intensity, report);
    tuning.bloom_threshold = sanitize_f32("bloom_threshold", tuning.bloom_threshold, report);
    tuning.bloom_threshold_softness = sanitize_f32(
        "bloom_threshold_softness",
        tuning.bloom_threshold_softness,
        report,
    );
    tuning.bloom_low_frequency_boost = sanitize_f32(
        "bloom_low_frequency_boost",
        tuning.bloom_low_frequency_boost,
        report,
    );
}

pub fn sanitize_cloud_tuning(tuning: &mut CloudTuning, report: Option<&mut SanitizeReport>) {
    let mut local;
    let report = match report {
        Some(report) => report,
        None => {
            local = SanitizeReport::default();
            &mut local
        }
    };
    tuning.coverage = sanitize_f32("cloud_coverage", tuning.coverage, report);
    tuning.density = sanitize_f32("cloud_density", tuning.density, report);
    tuning.raymarch_steps = sanitize_u32("cloud_raymarch_steps", tuning.raymarch_steps, report);
}

/// Canonical validation for every persisted setting. Both `load_settings` and
/// `save_settings` call this, which is what makes the value shown in the UI the
/// value the renderer reads.
pub fn sanitize_settings(settings: &mut Settings) -> SanitizeReport {
    let mut report = SanitizeReport::default();

    settings.view_dist = sanitize_i32("view_dist", settings.view_dist, &mut report);
    settings.mouse_sens = sanitize_f32("mouse_sens", settings.mouse_sens, &mut report);
    settings.volume = sanitize_f32("volume", settings.volume, &mut report);
    settings.music_volume = sanitize_f32("music_volume", settings.music_volume, &mut report);
    settings.ambience_volume =
        sanitize_f32("ambience_volume", settings.ambience_volume, &mut report);

    settings.cloud_coverage = sanitize_f32("cloud_coverage", settings.cloud_coverage, &mut report);
    settings.cloud_density = sanitize_f32("cloud_density", settings.cloud_density, &mut report);
    settings.cloud_raymarch_steps = sanitize_u32(
        "cloud_raymarch_steps",
        settings.cloud_raymarch_steps,
        &mut report,
    );
    sanitize_cloud_resolution(settings, &mut report);

    sanitize_lighting_tuning(&mut settings.lighting, Some(&mut report));

    settings.visual.task_capacity =
        sanitize_u32("task_capacity", settings.visual.task_capacity, &mut report);
    settings.visual.warn_pending_tasks = sanitize_u32(
        "warn_pending_tasks",
        settings.visual.warn_pending_tasks,
        &mut report,
    );
    settings.visual.warn_in_game_entities = sanitize_u32(
        "warn_in_game_entities",
        settings.visual.warn_in_game_entities,
        &mut report,
    );
    settings.visual.warn_chunk_meshes = sanitize_u32(
        "warn_chunk_meshes",
        settings.visual.warn_chunk_meshes,
        &mut report,
    );
    settings.visual.warn_asset_total = sanitize_u32(
        "warn_asset_total",
        settings.visual.warn_asset_total,
        &mut report,
    );

    report
}

fn sanitize_cloud_resolution(settings: &mut Settings, report: &mut SanitizeReport) {
    const ALLOWED: [(u32, u32); 4] = [(1280, 720), (1536, 864), (1920, 1080), (2560, 1600)];
    let pair = (settings.cloud_render_width, settings.cloud_render_height);
    if ALLOWED.contains(&pair) {
        return;
    }
    let defaults = Settings::default();
    report.push(
        "cloud_render_width",
        Some(pair.0 as f64),
        defaults.cloud_render_width as f64,
        CorrectionReason::UnsupportedValue,
    );
    report.push(
        "cloud_render_height",
        Some(pair.1 as f64),
        defaults.cloud_render_height as f64,
        CorrectionReason::UnsupportedValue,
    );
    settings.cloud_render_width = defaults.cloud_render_width;
    settings.cloud_render_height = defaults.cloud_render_height;
}

/// Reset only the visual fields, leaving audio/input preferences untouched.
pub fn reset_visual_settings(settings: &mut Settings) {
    let defaults = Settings::default();
    settings.view_dist = defaults.view_dist;
    settings.clouds = defaults.clouds;
    settings.weather = defaults.weather;
    settings.pixelated = defaults.pixelated;
    settings.cloud_coverage = defaults.cloud_coverage;
    settings.cloud_density = defaults.cloud_density;
    settings.cloud_raymarch_steps = defaults.cloud_raymarch_steps;
    settings.cloud_render_width = defaults.cloud_render_width;
    settings.cloud_render_height = defaults.cloud_render_height;
    settings.lighting = defaults.lighting;
    settings.visual = defaults.visual;
}

/// Performance-oriented preset application. Art controls (coverage, density,
/// lighting) are intentionally not touched; the player keeps their look while
/// the preset changes cost.
pub fn apply_visual_preset(settings: &mut Settings, preset: VisualPreset) {
    let (view_dist, clouds, weather, steps) = match preset {
        VisualPreset::Low => (6, false, false, 16),
        VisualPreset::Medium => (10, true, true, 24),
        VisualPreset::High => (16, true, true, 40),
        VisualPreset::Custom => return,
    };
    settings.view_dist = view_dist;
    settings.clouds = clouds;
    settings.weather = weather;
    settings.cloud_raymarch_steps = steps;
}

pub fn detect_visual_preset(settings: &Settings) -> VisualPreset {
    for preset in VisualPreset::SELECTABLE {
        let mut candidate = settings.clone();
        apply_visual_preset(&mut candidate, preset);
        if candidate.view_dist == settings.view_dist
            && candidate.clouds == settings.clouds
            && candidate.weather == settings.weather
            && candidate.cloud_raymarch_steps == settings.cloud_raymarch_steps
        {
            return preset;
        }
    }
    VisualPreset::Custom
}

/// Startup wiring: apply the task-capacity setting to the lifecycle registry
/// and log everything the load report repaired. Runs after the settings
/// resource exists but before any frame, so the bounded registry is never
/// resized while work is pending.
pub(super) fn configure_visual_settings(
    settings: Res<Settings>,
    load_report: Option<Res<SettingsLoadReport>>,
    mut lifecycle: ResMut<VisualLifecycle>,
) {
    lifecycle.set_task_capacity(settings.visual.task_capacity as usize);
    if let Some(report) = load_report.as_deref() {
        if report.fallback_to_defaults {
            warn!("settings.json 无法解析，已使用默认设置");
        }
        for correction in &report.corrections {
            let previous = correction
                .previous
                .map(|value| format!("{value}"))
                .unwrap_or_else(|| "non-finite".to_string());
            warn!(
                "settings repaired: {} {} → {} ({:?})",
                correction.key, previous, correction.corrected, correction.reason
            );
        }
    }
    info!(
        "visual settings ready: task_capacity={} preset={}",
        settings.visual.task_capacity,
        detect_visual_preset(&settings).key()
    );
}

/// Numeric value of a setting by registry key, for tests and diagnostics.
pub fn numeric_value(settings: &Settings, key: &str) -> Option<f64> {
    let value = match key {
        "view_dist" => settings.view_dist as f64,
        "mouse_sens" => settings.mouse_sens as f64,
        "volume" => settings.volume as f64,
        "music_volume" => settings.music_volume as f64,
        "ambience_volume" => settings.ambience_volume as f64,
        "cloud_coverage" => settings.cloud_coverage as f64,
        "cloud_density" => settings.cloud_density as f64,
        "cloud_raymarch_steps" => settings.cloud_raymarch_steps as f64,
        "cloud_render_width" => settings.cloud_render_width as f64,
        "cloud_render_height" => settings.cloud_render_height as f64,
        "sunlight_boost" => settings.lighting.sunlight_boost as f64,
        "sun_disk_intensity" => settings.lighting.sun_disk_intensity as f64,
        "atmosphere_fill" => settings.lighting.atmosphere_fill as f64,
        "ambient_multiplier" => settings.lighting.ambient_multiplier as f64,
        "space_atmosphere_fill" => settings.lighting.space_atmosphere_fill as f64,
        "bloom_intensity" => settings.lighting.bloom_intensity as f64,
        "bloom_threshold" => settings.lighting.bloom_threshold as f64,
        "bloom_threshold_softness" => settings.lighting.bloom_threshold_softness as f64,
        "bloom_low_frequency_boost" => settings.lighting.bloom_low_frequency_boost as f64,
        "task_capacity" => settings.visual.task_capacity as f64,
        "warn_pending_tasks" => settings.visual.warn_pending_tasks as f64,
        "warn_in_game_entities" => settings.visual.warn_in_game_entities as f64,
        "warn_chunk_meshes" => settings.visual.warn_chunk_meshes as f64,
        "warn_asset_total" => settings.visual.warn_asset_total as f64,
        _ => return None,
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_keys_are_unique_and_ranges_are_sane() {
        let mut keys = std::collections::HashSet::new();
        for spec in VISUAL_SETTING_SPECS {
            assert!(keys.insert(spec.key), "duplicate setting spec {}", spec.key);
            assert!(!spec.label.is_empty());
            assert!(spec.min <= spec.max, "{} range inverted", spec.key);
            assert!(
                spec.default >= spec.min && spec.default <= spec.max,
                "{} default {} outside [{}, {}]",
                spec.key,
                spec.default,
                spec.min,
                spec.max
            );
            assert!(!spec.is_default(), "{} has a degenerate range", spec.key);
        }
    }

    #[test]
    fn spec_defaults_match_settings_defaults() {
        let settings = Settings::default();
        for spec in VISUAL_SETTING_SPECS {
            let value = numeric_value(&settings, spec.key)
                .unwrap_or_else(|| panic!("{} has no settings accessor", spec.key));
            assert!(
                (value - spec.default).abs() < 1e-6,
                "{} default {value} drifted from Settings::default {}",
                spec.key,
                spec.default
            );
        }
    }

    #[test]
    fn legacy_setting_keys_have_no_slider_spec_but_have_an_apply_mode() {
        for key in LEGACY_SETTING_KEYS {
            assert!(spec(key).is_none());
            assert_eq!(apply_mode_for(key), Some(ApplyMode::Legacy));
        }
    }

    #[test]
    fn non_finite_values_fall_back_to_defaults_and_are_reported() {
        let mut settings = Settings {
            view_dist: 999,
            mouse_sens: f32::NAN,
            volume: f32::INFINITY,
            cloud_density: f32::NEG_INFINITY,
            lighting: LightingTuning {
                bloom_threshold: f32::NAN,
                ..Default::default()
            },
            visual: VisualSettings {
                warn_chunk_meshes: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let report = sanitize_settings(&mut settings);
        assert!(!report.is_clean());
        assert_eq!(settings.view_dist, 32);
        assert_eq!(settings.mouse_sens, 1.0);
        assert_eq!(settings.volume, 0.8);
        assert_eq!(settings.cloud_density, 0.09);
        assert_eq!(settings.lighting.bloom_threshold, 1.5);
        assert_eq!(settings.visual.warn_chunk_meshes, 16);
        assert_eq!(
            report.corrected("mouse_sens").map(|item| item.reason),
            Some(CorrectionReason::NonFinite)
        );
        assert_eq!(
            report.corrected("view_dist").map(|item| item.reason),
            Some(CorrectionReason::OutOfRange)
        );
    }

    #[test]
    fn clean_settings_produce_no_corrections() {
        let mut settings = Settings::default();
        let report = sanitize_settings(&mut settings);
        assert!(report.is_clean(), "{:?}", report.corrections);
    }

    #[test]
    fn invalid_cloud_resolution_is_replaced_with_the_default_pair() {
        let mut settings = Settings {
            cloud_render_width: 123,
            cloud_render_height: 456,
            ..Default::default()
        };
        let report = sanitize_settings(&mut settings);
        assert_eq!(settings.cloud_render_width, 1536);
        assert_eq!(settings.cloud_render_height, 864);
        assert_eq!(
            report
                .corrected("cloud_render_width")
                .map(|item| item.reason),
            Some(CorrectionReason::UnsupportedValue)
        );
        assert_eq!(report.corrections.len(), 2);
    }

    #[test]
    fn presets_roundtrip_and_manual_changes_detect_custom() {
        for preset in VisualPreset::SELECTABLE {
            let mut settings = Settings::default();
            apply_visual_preset(&mut settings, preset);
            assert_eq!(detect_visual_preset(&settings), preset);
            assert!(sanitize_settings(&mut settings).is_clean());
        }
        let mut settings = Settings::default();
        apply_visual_preset(&mut settings, VisualPreset::Low);
        settings.view_dist = 31;
        assert_eq!(detect_visual_preset(&settings), VisualPreset::Custom);
    }

    #[test]
    fn reset_visual_settings_preserves_audio_and_input() {
        let mut settings = Settings {
            volume: 0.25,
            mouse_sens: 2.5,
            music_volume: 0.1,
            view_dist: 30,
            clouds: false,
            lighting: LightingTuning {
                bloom_intensity: 5.0,
                ..Default::default()
            },
            visual: VisualSettings {
                warn_asset_total: 999,
                ..Default::default()
            },
            ..Default::default()
        };
        reset_visual_settings(&mut settings);
        assert_eq!(settings.view_dist, 10);
        assert!(settings.clouds);
        assert_eq!(settings.lighting.bloom_intensity, 0.12);
        assert_eq!(settings.visual.warn_asset_total, 12000);
        assert_eq!(settings.volume, 0.25);
        assert_eq!(settings.mouse_sens, 2.5);
        assert_eq!(settings.music_volume, 0.1);
    }

    #[test]
    fn old_settings_json_without_visual_group_still_loads() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "view_dist": 12,
                "mouse_sens": 1.2,
                "volume": 0.5,
                "show_fps": true
            }"#,
        )
        .unwrap();
        assert_eq!(settings.view_dist, 12);
        assert_eq!(settings.visual, VisualSettings::default());
        assert_eq!(settings.cloud_raymarch_steps, 24);
        assert_eq!(settings.lighting, LightingTuning::default());
    }

    #[test]
    fn tuning_helpers_share_the_registry_ranges() {
        let mut lighting = LightingTuning {
            sunlight_boost: -4.0,
            bloom_threshold: f32::NAN,
            ..Default::default()
        };
        sanitize_lighting_tuning(&mut lighting, None);
        assert_eq!(lighting.sunlight_boost, 0.0);
        assert_eq!(lighting.bloom_threshold, 1.5);

        let mut cloud = CloudTuning {
            coverage: 2.0,
            density: f32::NAN,
            raymarch_steps: 900,
            ..Default::default()
        };
        sanitize_cloud_tuning(&mut cloud, None);
        assert_eq!(cloud.coverage, 1.0);
        assert_eq!(cloud.density, 0.09);
        assert_eq!(cloud.raymarch_steps, 64);
    }

    #[test]
    fn slider_range_matches_sanitize_range() {
        let range = spec("view_dist").unwrap().slider_range();
        assert_eq!(*range.start(), 3.0);
        assert_eq!(*range.end(), 32.0);
        assert_eq!(clamp_f32("view_dist", -5.0), 3.0);
        assert_eq!(clamp_u32("cloud_raymarch_steps", 1), 4);
    }
}
