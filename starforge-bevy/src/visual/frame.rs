//! A02 public visual contracts (v1).
//!
//! These types are the frozen boundary between gameplay/rendering producers and
//! the visual workstreams. Units and coordinate spaces are part of the
//! contract, not an implementation detail:
//!
//! - positions are world-space meters (1 unit = 1 voxel);
//! - directions are normalized world-space vectors;
//! - `view`/`projection`/`view_proj` are Bevy clip-space matrices (reverse-Z
//!   included, so consumers must not "fix" the depth convention);
//! - `origin_offset` converts logic coordinates into render coordinates and is
//!   zero until C05/D05 prove a camera-relative origin is needed.
//!
//! `CelestialLighting` keeps the two sun vectors separate on purpose:
//! `travel_direction` is the direction light rays travel (sun toward the
//! scene), while `to_sun_direction` points from the scene toward the sun.
//! Mixing them up was a documented risk (R043).

use bevy::prelude::*;

use crate::planet_scale::PlanetVisualFrame;

/// Stable identifier for a visual camera. Only the primary 3D camera exists
/// today; E03 uses this to key per-camera cloud history.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct VisualCameraId(pub u32);

impl VisualCameraId {
    pub const PRIMARY: Self = Self(0);
}

/// Read-only snapshot of the finalized camera and the local planet frame.
///
/// A `VisualFrame` is produced exactly once per frame in the `Last` schedule
/// from the camera state left by every camera writer (player, photo, visual
/// QA, space). Consumers must not sample `Transform`/`Projection` directly for
/// frame-locked effects; they read this resource instead so clouds, terrain,
/// lighting and UI share one coordinate frame.
#[derive(Resource, Clone, Copy, Debug)]
pub struct VisualFrame {
    /// Monotonic snapshot counter. Starts at 0 for the identity default.
    pub frame_id: u64,
    /// Incremented for new/load world, planet switch and return-to-menu
    /// boundaries by A04's single lifecycle owner.
    pub world_epoch: u64,
    pub camera_id: VisualCameraId,
    /// Frame delta in seconds that produced this snapshot.
    pub dt: f32,
    /// True when virtual time is paused or no world is being simulated.
    pub paused: bool,
    /// World-space camera view matrix (inverse of the camera `GlobalTransform`).
    pub view: Mat4,
    /// View matrix of the previous snapshot.
    pub prev_view: Mat4,
    /// Clip-from-view projection matrix (reverse-Z depth as Bevy renders it).
    pub projection: Mat4,
    /// `projection * view` for the current snapshot.
    pub view_proj: Mat4,
    /// `projection * view` of the previous snapshot, for history reprojection.
    pub prev_view_proj: Mat4,
    /// Camera position in world space.
    pub eye: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    /// Vertical field of view in radians (0 for orthographic projections).
    pub fov_y_radians: f32,
    pub near: f32,
    pub far: f32,
    /// Physical viewport size in pixels.
    pub viewport: UVec2,
    /// Logic-to-render origin shift. Zero in A02; C05/D05 populate it only
    /// after proving precision benefits, and consumers use the helpers below
    /// instead of subtracting it themselves.
    pub origin_offset: Vec3,
    /// Local tangent planet frame used by ground rendering.
    pub planet: PlanetVisualFrame,
}

impl Default for VisualFrame {
    fn default() -> Self {
        Self {
            frame_id: 0,
            world_epoch: 0,
            camera_id: VisualCameraId::PRIMARY,
            dt: 0.0,
            paused: true,
            view: Mat4::IDENTITY,
            prev_view: Mat4::IDENTITY,
            projection: Mat4::IDENTITY,
            view_proj: Mat4::IDENTITY,
            prev_view_proj: Mat4::IDENTITY,
            eye: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            right: Vec3::X,
            up: Vec3::Y,
            fov_y_radians: 0.0,
            near: 0.1,
            far: 1000.0,
            viewport: UVec2::ONE,
            origin_offset: Vec3::ZERO,
            planet: PlanetVisualFrame::default(),
        }
    }
}

impl VisualFrame {
    /// Convert a logic-world position into the render coordinate frame.
    pub fn render_position(&self, world: Vec3) -> Vec3 {
        world - self.origin_offset
    }

    /// Convert a render position back into logic-world coordinates.
    pub fn logic_position(&self, render: Vec3) -> Vec3 {
        render + self.origin_offset
    }

    /// Directions are unaffected by origin shifts; the explicit method exists
    /// so mixed position/direction code stays readable.
    pub fn render_direction(&self, direction: Vec3) -> Vec3 {
        direction
    }

    /// Inverse of the world-to-view transform (camera world matrix).
    pub fn view_inverse(&self) -> Mat4 {
        self.view.inverse()
    }
}

/// Snapshot of the active celestial light state produced by `daynight_system`.
///
/// Produced once per played frame; the default approximates noon so consumers
/// are safe before the first `daynight_system` run.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CelestialLighting {
    /// 0.0 = midnight, 1.0 = noon (matches `daynight::day_factor`).
    pub daylight: f32,
    /// Normalized direction sunlight travels (sun toward the scene).
    pub travel_direction: Vec3,
    /// Normalized direction from the scene toward the sun.
    pub to_sun_direction: Vec3,
    /// Linear sunlight color.
    pub sun_color: LinearRgba,
    /// Direct sunlight illuminance in lux.
    pub sun_illuminance_lux: f32,
    /// Visible sun-disk intensity (art control).
    pub sun_disk_intensity: f32,
    /// Linear global ambient color.
    pub ambient_color: LinearRgba,
    /// Global ambient brightness in lux.
    pub ambient_brightness: f32,
    /// Environment-map fill intensity applied to the camera.
    pub environment_fill: f32,
    /// Active exposure in EV100.
    pub exposure_ev100: f32,
    /// Whether the ground (non-orbital) sun drives the scene.
    pub ground_sun_visible: bool,
}

impl Default for CelestialLighting {
    fn default() -> Self {
        let travel = Vec3::new(0.25, -0.85, 0.35).normalize();
        Self {
            daylight: 1.0,
            travel_direction: travel,
            to_sun_direction: -travel,
            sun_color: LinearRgba::new(1.0, 0.97, 0.9, 1.0),
            sun_illuminance_lux: 120_000.0,
            sun_disk_intensity: 1.0,
            ambient_color: LinearRgba::new(0.75, 0.8, 0.9, 1.0),
            ambient_brightness: (12.0 + 68.0) * 0.75,
            environment_fill: 0.75,
            exposure_ev100: 13.0,
            ground_sun_visible: false,
        }
    }
}

impl CelestialLighting {
    /// Cloud shader energy input derived from physical illuminance. Kept in
    /// one place so cloud/terrain cannot drift to different units.
    pub fn cloud_energy(&self) -> f32 {
        (self.sun_illuminance_lux / 120_000.0).clamp(0.0, 2.0)
    }
}

/// Incremented at every world-lifetime boundary. Worker results, planet caches
/// and camera histories compare this value before committing data.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct WorldEpoch(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    fn transform_view(translation: Vec3, yaw: f32) -> Mat4 {
        let transform = GlobalTransform::from(
            Transform::from_translation(translation).with_rotation(Quat::from_rotation_y(yaw)),
        );
        transform.affine().inverse().into()
    }

    #[test]
    fn default_frame_maps_positions_unchanged() {
        let frame = VisualFrame::default();
        let position = Vec3::new(3.0, -2.0, 7.5);
        assert!(frame.render_position(position).distance(position) < 1e-6);
        assert!(frame.view.w_axis.w == 1.0);
        assert_eq!(frame.view_proj, Mat4::IDENTITY);
    }

    #[test]
    fn render_origin_roundtrips() {
        let frame = VisualFrame {
            origin_offset: Vec3::new(1000.0, 0.0, -500.0),
            ..Default::default()
        };
        let logic = Vec3::new(1005.0, 12.0, -499.0);
        let render = frame.render_position(logic);
        assert!(render.distance(Vec3::new(5.0, 12.0, 1.0)) < 1e-6);
        assert!(frame.logic_position(render).distance(logic) < 1e-6);
    }

    #[test]
    fn view_matrix_places_eye_at_view_origin() {
        let eye = Vec3::new(4.0, 5.0, 6.0);
        let view = transform_view(eye, 0.7);
        let view_eye = view.transform_point3(eye);
        assert!(view_eye.length() < 1e-4, "{view_eye:?}");
        // Forward (-Z) must stay a unit direction after the origin shift.
        let forward = view.transform_vector3(Vec3::NEG_Z);
        assert!((forward.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn celestial_directions_are_opposite_and_normalized() {
        let light = CelestialLighting::default();
        assert!((light.travel_direction.length() - 1.0).abs() < 1e-5);
        assert!((light.to_sun_direction.length() - 1.0).abs() < 1e-5);
        let sum = light.travel_direction + light.to_sun_direction;
        assert!(sum.length() < 1e-5, "{sum:?}");
        assert!((light.cloud_energy() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cloud_energy_is_bounded() {
        let night = CelestialLighting {
            sun_illuminance_lux: 0.0,
            ..Default::default()
        };
        assert_eq!(night.cloud_energy(), 0.0);
        let overbright = CelestialLighting {
            sun_illuminance_lux: 1.0e9,
            ..Default::default()
        };
        assert_eq!(overbright.cloud_energy(), 2.0);
    }
}
