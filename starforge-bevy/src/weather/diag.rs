//! E01 cloud-shell diagnostics: the ray/sphere boundary model, the settings
//! truth check and the runtime debug-view switch.
//!
//! The shipping volume shader (`assets/shaders/cloud_shell.wgsl`) marches a
//! single interval per pixel. This module ports that interval math to Rust so
//! below/inside/above/outside cameras, tangent rays and the *far* shell
//! segment after crossing the cloud-bottom sphere can be asserted on the CPU
//! instead of being inferred from screenshots. The far segment is reported but
//! not yet marched by the shipping path; that decision is E04 scope and is
//! recorded in the run notes so the limitation cannot be forgotten.
//!
//! See `docs/art-overhaul/02-RENDERING-LIGHTING-CLOUDS.md` (2.7) and
//! `docs/art-overhaul/05-EXECUTION-BACKLOG.md` (E01).

use bevy::prelude::*;
use serde::Serialize;

/// Epsilon used by the shader's face filter (`outer + 16.0`).
pub const FACE_CULL_MARGIN: f32 = 16.0;

/// Version of the per-scene `clouds` block in `metrics.json`.
pub const CLOUD_DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

/// Debug views selectable at runtime. `Off` is the shipping shader path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum CloudDebugMode {
    #[default]
    Off,
    /// Grayscale of the maximum density found along the ray.
    Density,
    /// Red/green/blue encoding of the march interval (start/end/far segment).
    Interval,
    /// Transmittance `T` after the march (white = clear sky).
    Transmittance,
    /// Accumulated in-scattering radiance.
    Scattering,
    /// Fraction of the step budget actually used.
    Steps,
    /// Scene-depth truncation before/after (R053).
    Depth,
    /// Light-route transmittance at the first in-cloud sample.
    Light,
    /// Face-culling check: green = kept face, red = discarded side (R052).
    Faces,
}

impl CloudDebugMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Density => "density",
            Self::Interval => "interval",
            Self::Transmittance => "transmittance",
            Self::Scattering => "scattering",
            Self::Steps => "steps",
            Self::Depth => "depth",
            Self::Light => "light",
            Self::Faces => "faces",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        let value = value.strip_prefix("debug_").unwrap_or(&value);
        match value {
            "off" | "full" | "none" | "0" => Some(Self::Off),
            "density" | "1" => Some(Self::Density),
            "interval" | "2" => Some(Self::Interval),
            "transmittance" | "t" | "3" => Some(Self::Transmittance),
            "scattering" | "l" | "4" => Some(Self::Scattering),
            "steps" | "5" => Some(Self::Steps),
            "depth" | "6" => Some(Self::Depth),
            "light" | "7" => Some(Self::Light),
            "faces" | "cull" | "8" => Some(Self::Faces),
            _ => None,
        }
    }

    pub fn shader_mode(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::Density => 1,
            Self::Interval => 2,
            Self::Transmittance => 3,
            Self::Scattering => 4,
            Self::Steps => 5,
            Self::Depth => 6,
            Self::Light => 7,
            Self::Faces => 8,
        }
    }
}

/// Main-world debug switch; the uniform is written every frame by
/// `climate_system` so toggling applies without rebuilding the material.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CloudDebug {
    pub mode: CloudDebugMode,
}

/// Statistics of the procedural density texture, computed while it is built so
/// diagnostics do not rescan ~1.2M voxels per frame.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct DensityStats {
    pub min: u8,
    pub max: u8,
    pub mean: f32,
    pub non_zero_fraction: f32,
    pub samples: u32,
}

impl DensityStats {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::default();
        }
        let mut min = u8::MAX;
        let mut max = 0u8;
        let mut sum = 0u64;
        let mut non_zero = 0u64;
        for &value in bytes {
            min = min.min(value);
            max = max.max(value);
            sum += value as u64;
            if value > 0 {
                non_zero += 1;
            }
        }
        Self {
            min,
            max,
            mean: sum as f32 / bytes.len() as f32,
            non_zero_fraction: non_zero as f32 / bytes.len() as f32,
            samples: bytes.len() as u32,
        }
    }
}

/// Radial class of the camera relative to the shell. `Above` is also the
/// "outside the shell sphere" case used by the face filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ShellRegion {
    Below,
    Inside,
    Above,
}

impl ShellRegion {
    pub fn key(self) -> &'static str {
        match self {
            Self::Below => "below",
            Self::Inside => "inside",
            Self::Above => "above",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ShellSegment {
    pub enter: f32,
    pub exit: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ShellIntervals {
    pub camera_region: ShellRegion,
    pub camera_radius: f32,
    /// Segment the shipping shader marches (single interval).
    pub first: Option<ShellSegment>,
    /// Far segment after crossing the cloud-bottom sphere. Reported by E01,
    /// currently dropped by the single-interval march (E04 decides whether to
    /// march it or prove it is always depth-occluded).
    pub far: Option<ShellSegment>,
    /// Camera is inside the outer cull shell; the kept surface is the inner
    /// (back) face, otherwise the outer (front) face.
    pub camera_inside_outer: bool,
}

impl Default for ShellIntervals {
    fn default() -> Self {
        Self {
            camera_region: ShellRegion::Above,
            camera_radius: 0.0,
            first: None,
            far: None,
            camera_inside_outer: false,
        }
    }
}

/// Ray/sphere roots in `center`-local space. Mirrors `ray_sphere` in
/// `cloud_shell.wgsl`; `None` when the ray misses or is tangent in a way that
/// would produce NaN downstream.
pub fn ray_sphere(origin_local: Vec3, direction: Vec3, radius: f32) -> Option<(f32, f32)> {
    let b = origin_local.dot(direction);
    let c = origin_local.length_squared() - radius * radius;
    let discriminant = b * b - c;
    if !discriminant.is_finite() || discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let near = -b - root;
    let far = -b + root;
    if near.is_finite() && far.is_finite() {
        Some((near.min(far), near.max(far)))
    } else {
        None
    }
}

/// Full shell interval model. `first` matches the current shader logic; `far`
/// records the segment the single-interval march drops.
pub fn shell_intervals(
    origin: Vec3,
    direction: Vec3,
    center: Vec3,
    inner: f32,
    outer: f32,
) -> ShellIntervals {
    let local = origin - center;
    let radius = local.length();
    let camera_region = if radius < inner {
        ShellRegion::Below
    } else if radius < outer {
        ShellRegion::Inside
    } else {
        ShellRegion::Above
    };
    let mut result = ShellIntervals {
        camera_region,
        camera_radius: radius,
        first: None,
        far: None,
        camera_inside_outer: radius < outer + FACE_CULL_MARGIN,
    };

    let Some((outer_near, outer_far)) = ray_sphere(local, direction, outer) else {
        return result;
    };
    if outer_far <= 0.0 {
        return result;
    }
    let inner_hit = ray_sphere(local, direction, inner);

    match camera_region {
        ShellRegion::Below => {
            // Start where the ray leaves the clear band below the deck.
            let start = match inner_hit {
                Some((_, inner_far)) => outer_near.max(inner_far),
                None => outer_near,
            };
            if outer_far > start {
                result.first = Some(ShellSegment {
                    enter: start,
                    exit: outer_far,
                });
            }
        }
        ShellRegion::Inside => {
            let mut end = outer_far;
            if let Some((inner_near, _)) = inner_hit
                && inner_near > 0.0
            {
                end = end.min(inner_near);
            }
            if end > 0.0 {
                result.first = Some(ShellSegment {
                    enter: 0.0,
                    exit: end,
                });
            }
            // After crossing the cloud bottom the ray may rise into the shell
            // again on the far side; the shipping march stops at the first hit.
            if let Some((inner_near, inner_far)) = inner_hit
                && inner_near > 0.0
                && inner_far > inner_near
                && outer_far > inner_far
            {
                result.far = Some(ShellSegment {
                    enter: inner_far,
                    exit: outer_far,
                });
            }
        }
        ShellRegion::Above => {
            let mut end = outer_far;
            if let Some((inner_near, inner_far)) = inner_hit
                && inner_near > outer_near
                && inner_near < end
            {
                end = inner_near;
                if outer_far > inner_far && inner_far > inner_near {
                    result.far = Some(ShellSegment {
                        enter: inner_far,
                        exit: outer_far,
                    });
                }
            }
            let start = outer_near.max(0.0);
            if end > start {
                result.first = Some(ShellSegment {
                    enter: start,
                    exit: end,
                });
            }
        }
    }
    result
}

/// Scene-depth truncation. `sky_distance` follows the shader convention: the
/// depth prepass returns 0 for sky/unwritten depth, which must NOT truncate.
pub fn truncate_to_scene(segment: ShellSegment, sky_distance: Option<f32>) -> Option<ShellSegment> {
    let exit = match sky_distance {
        Some(distance) if distance > 0.0 && distance.is_finite() => segment.exit.min(distance),
        _ => segment.exit,
    };
    if exit > segment.enter {
        Some(ShellSegment {
            enter: segment.enter,
            exit,
        })
    } else {
        None
    }
}

/// Premultiplied-alpha composite used by `AlphaMode::Premultiplied`:
/// `out.rgb = L + (1 - out.a) * background` with `out.a = 1 - T`.
pub fn composite_premultiplied(radiance: Vec3, transmittance: f32, background: Vec3) -> Vec3 {
    radiance + background * transmittance.clamp(0.0, 1.0)
}

/// Mirrors the shader coverage threshold: `mix(0.62, 0.18, coverage)`.
pub fn density_threshold(coverage: f32) -> f32 {
    0.62 + (0.18 - 0.62) * coverage.clamp(0.0, 1.0)
}

/// Mirrors the shader density remap (smoothstep erosion + density multiplier).
pub fn density_remap(raw: f32, coverage: f32, density: f32) -> f32 {
    let threshold = density_threshold(coverage);
    let upper = (threshold + 0.22).min(0.98);
    let t = ((raw - threshold) / (upper - threshold).max(f32::EPSILON)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t) * density.clamp(0.0, 1.0)
}

/// Per-frame snapshot serialized into `metrics.json` by the visual QA runner.
#[derive(Resource, Clone, Debug, Serialize)]
pub struct CloudDiagnostics {
    pub schema_version: u32,
    pub cloud_active: bool,
    pub biome: String,
    pub world_seed: u32,
    pub coverage_requested: f32,
    pub density_requested: f32,
    pub steps_requested: u32,
    pub coverage_effective: f32,
    pub density_effective: f32,
    pub steps_effective: u32,
    pub legacy_render_resolution: [u32; 2],
    pub effective_render_resolution: [u32; 2],
    /// True: the legacy `cloud_render_width/height` pair does not resize any
    /// render target (R061); the volumetric pass runs at the viewport size.
    pub render_resolution_inert: bool,
    pub shell_inner_altitude: f32,
    pub shell_outer_altitude: f32,
    pub center_ray: ShellIntervals,
    pub density_stats: DensityStats,
    pub debug_mode: String,
    pub notes: Vec<String>,
}

impl Default for CloudDiagnostics {
    fn default() -> Self {
        Self {
            schema_version: CLOUD_DIAGNOSTICS_SCHEMA_VERSION,
            cloud_active: false,
            biome: String::new(),
            world_seed: 0,
            coverage_requested: 0.0,
            density_requested: 0.0,
            steps_requested: 0,
            coverage_effective: 0.0,
            density_effective: 0.0,
            steps_effective: 0,
            legacy_render_resolution: [0, 0],
            effective_render_resolution: [0, 0],
            render_resolution_inert: true,
            shell_inner_altitude: 0.0,
            shell_outer_altitude: 0.0,
            center_ray: ShellIntervals::default(),
            density_stats: DensityStats::default(),
            debug_mode: CloudDebugMode::Off.key().to_string(),
            notes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INNER: f32 = 1_000.0;
    const OUTER: f32 = 1_300.0;
    const CENTER: Vec3 = Vec3::ZERO;

    fn intervals(altitude: f32, direction: Vec3) -> ShellIntervals {
        shell_intervals(
            Vec3::new(0.0, altitude, 0.0),
            direction.normalize(),
            CENTER,
            INNER,
            OUTER,
        )
    }

    #[test]
    fn ray_sphere_hits_misses_and_tangents() {
        let (near, far) = ray_sphere(Vec3::new(-200.0, 0.0, 0.0), Vec3::X, 100.0).unwrap();
        assert!(near > 99.0 && near < 101.0);
        assert!(far > 299.0 && far < 301.0);
        // A ray pointing away still yields analytic roots, but both are behind
        // the origin; `shell_interval`/`shell_intervals` reject them via
        // `far <= 0`, so the roots themselves stay finite.
        let (_, behind) = ray_sphere(Vec3::new(-200.0, 0.0, 0.0), Vec3::NEG_X, 100.0).unwrap();
        assert!(behind < 0.0);
        // Tangent (discriminant ~0) must not produce NaN roots.
        let tangent = ray_sphere(Vec3::new(0.0, 100.0, -500.0), Vec3::Z, 100.0);
        assert!(tangent.is_none_or(|(a, b)| a.is_finite() && b.is_finite()));
    }

    #[test]
    fn below_camera_starts_above_the_cloud_bottom() {
        // Camera below the inner sphere looking up: the march must start at
        // the inner exit, not at the camera.
        let result = intervals(400.0, Vec3::new(0.2, 1.0, 0.0));
        assert_eq!(result.camera_region, ShellRegion::Below);
        let first = result.first.expect("ray enters the shell");
        assert!(first.enter > 0.0);
        // The inner far hit is where the ray leaves the clear band.
        let local = Vec3::new(0.0, 400.0, 0.0);
        let (_, inner_far) =
            ray_sphere(local, Vec3::new(0.2, 1.0, 0.0).normalize(), INNER).unwrap();
        assert!((first.enter - inner_far).abs() < 0.01);
        assert!(first.exit > first.enter);
        assert!(
            result.far.is_none(),
            "no far segment once the ray exits outer"
        );
    }

    #[test]
    fn inside_camera_marching_inward_reports_the_dropped_far_segment() {
        // Camera inside the shell, ray angled down: first segment ends at the
        // cloud bottom, far segment is the deck across the planet. The shipping
        // shader marches only `first`.
        let result = intervals(1_150.0, Vec3::new(0.55, -1.0, 0.0));
        assert_eq!(result.camera_region, ShellRegion::Inside);
        let first = result.first.expect("near segment");
        assert_eq!(first.enter, 0.0);
        let far = result.far.expect("far segment is reported by E01");
        assert!(far.enter >= first.exit - 0.01);
        assert!(far.exit > far.enter);
    }

    #[test]
    fn inside_camera_marching_outward_has_one_segment() {
        let result = intervals(1_150.0, Vec3::new(0.2, 1.0, 0.0));
        let first = result.first.expect("segment");
        assert_eq!(first.enter, 0.0);
        assert!(result.far.is_none());
    }

    #[test]
    fn above_camera_crossing_the_inner_sphere_reports_the_far_segment() {
        let result = intervals(2_000.0, Vec3::new(0.4, -1.0, 0.0));
        assert_eq!(result.camera_region, ShellRegion::Above);
        let first = result.first.expect("near shell crossing");
        assert!(first.enter > 0.0, "march starts at the outer surface");
        let far = result.far.expect("far segment");
        assert!(far.exit > far.enter);
        assert!(far.enter > first.exit - 0.01);
    }

    #[test]
    fn above_camera_tangent_and_away_rays_do_not_panic() {
        let tangent = intervals(1_320.0, Vec3::X);
        assert!(tangent.first.is_none_or(|s| s.exit >= s.enter));
        let away = intervals(2_000.0, Vec3::new(0.3, 1.0, 0.0));
        assert!(
            away.first.is_none(),
            "ray away from the shell has no interval"
        );
    }

    #[test]
    fn face_filter_classifies_the_outer_margin() {
        let below = intervals(400.0, Vec3::Y);
        assert!(below.camera_inside_outer);
        let above = intervals(5_000.0, Vec3::NEG_Y);
        assert!(!above.camera_inside_outer);
    }

    #[test]
    fn depth_truncation_keeps_sky_and_clips_geometry() {
        let segment = ShellSegment {
            enter: 10.0,
            exit: 100.0,
        };
        assert_eq!(truncate_to_scene(segment, None), Some(segment));
        assert_eq!(truncate_to_scene(segment, Some(0.0)), Some(segment));
        let clipped = truncate_to_scene(segment, Some(40.0)).unwrap();
        assert_eq!(clipped.exit, 40.0);
        assert!(truncate_to_scene(segment, Some(5.0)).is_none());
        assert!(truncate_to_scene(segment, Some(f32::NAN)).is_some());
    }

    #[test]
    fn premultiplied_composite_matches_the_shader_output_contract() {
        let background = Vec3::new(0.2, 0.3, 0.5);
        // Clear sky: the background passes through unchanged.
        assert!(composite_premultiplied(Vec3::ZERO, 1.0, background).abs_diff_eq(background, 1e-6));
        // Opaque cloud: only the in-scattering radiance remains.
        let radiance = Vec3::new(1.0, 0.8, 0.6);
        assert!(composite_premultiplied(radiance, 0.0, background).abs_diff_eq(radiance, 1e-6));
        // Half transparent mixes both: L + (1 - a) * background with a = 0.5.
        let mixed = composite_premultiplied(radiance, 0.5, background);
        assert!((mixed.x - 1.1).abs() < 1e-6);
        assert!((mixed.y - 0.95).abs() < 1e-6);
    }

    #[test]
    fn density_threshold_tracks_coverage_without_inverting() {
        assert!((density_threshold(0.0) - 0.62).abs() < 1e-6);
        assert!((density_threshold(1.0) - 0.18).abs() < 1e-6);
        assert!(density_threshold(0.0) > density_threshold(1.0));
        // Higher coverage lowers the threshold, so the same raw value turns
        // into more density.
        let low = density_remap(0.5, 0.1, 1.0);
        let high = density_remap(0.5, 0.9, 1.0);
        assert!(high >= low);
        assert_eq!(density_remap(0.0, 0.5, 1.0), 0.0);
        assert!(density_remap(1.0, 0.5, 0.5) <= 0.5 + 1e-6);
    }

    #[test]
    fn density_stats_describe_the_texture() {
        let stats = DensityStats::from_bytes(&[0, 0, 128, 255]);
        assert_eq!(stats.min, 0);
        assert_eq!(stats.max, 255);
        assert!((stats.non_zero_fraction - 0.5).abs() < 1e-6);
        assert!((stats.mean - 95.75).abs() < 0.01);
        assert_eq!(stats.samples, 4);
        assert_eq!(DensityStats::from_bytes(&[]).samples, 0);
    }

    #[test]
    fn debug_modes_parse_and_map_to_shader_modes() {
        for mode in [
            CloudDebugMode::Off,
            CloudDebugMode::Density,
            CloudDebugMode::Interval,
            CloudDebugMode::Transmittance,
            CloudDebugMode::Scattering,
            CloudDebugMode::Steps,
            CloudDebugMode::Depth,
            CloudDebugMode::Light,
            CloudDebugMode::Faces,
        ] {
            assert_eq!(CloudDebugMode::parse(mode.key()), Some(mode));
            assert_eq!(
                CloudDebugMode::parse(&format!("debug_{}", mode.key())),
                Some(mode)
            );
        }
        assert_eq!(CloudDebugMode::parse("nope"), None);
        assert_eq!(CloudDebugMode::Off.shader_mode(), 0);
        assert_eq!(CloudDebugMode::Faces.shader_mode(), 8);
    }
}
