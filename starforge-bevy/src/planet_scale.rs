//! Shared dimensional contract for the local voxel patch and orbital scene.
//!
//! Keep every altitude transition here.  The old implementation spread
//! unrelated 78..220 unit thresholds across weather, sky and flight code,
//! which made the cloud deck, black-sky fade and orbital hand-off collapse
//! into the same very small vertical interval.

use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanetScaleProfile {
    /// Visual radius represented by the local tangent-space voxel scene.
    pub local_planet_radius: f32,
    pub cloud_bottom: f32,
    pub cloud_top: f32,
    pub sky_space_fade_start: f32,
    pub sky_space_fade_end: f32,
    /// Orbital -> local transition.  Kept below `exit_altitude` as hysteresis.
    pub reentry_altitude: f32,
    /// Local -> orbital transition.
    pub exit_altitude: f32,
    pub atmosphere_top: f32,
    /// Radius around the camera that remains visually flat and matches voxel
    /// physics exactly before distant rendering blends toward planet curvature.
    pub curvature_flat_radius: f32,
    pub curvature_full_radius: f32,
}

pub const PLANET_SCALE: PlanetScaleProfile = PlanetScaleProfile {
    local_planet_radius: 16_384.0,
    cloud_bottom: 420.0,
    cloud_top: 780.0,
    sky_space_fade_start: 1_200.0,
    sky_space_fade_end: 2_400.0,
    reentry_altitude: 1_800.0,
    exit_altitude: 2_400.0,
    atmosphere_top: 2_800.0,
    curvature_flat_radius: 256.0,
    curvature_full_radius: 1_500.0,
};

/// Continuous local planet frame used only by ground-scene rendering.
/// Canonical voxel/space coordinates remain unchanged; moving this visual
/// proxy keeps the local tangent patch, atmosphere and clouds aligned.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PlanetVisualFrame {
    pub focus: Vec2,
    pub center: Vec3,
    pub datum_y: f32,
    pub radius: f32,
    pub flat_radius: f32,
    pub full_radius: f32,
}

impl Default for PlanetVisualFrame {
    fn default() -> Self {
        let datum_y = crate::data::SEA_Y;
        let radius = PLANET_SCALE.local_planet_radius;
        Self {
            focus: Vec2::ZERO,
            center: Vec3::new(0.0, datum_y - radius, 0.0),
            datum_y,
            radius,
            flat_radius: PLANET_SCALE.curvature_flat_radius,
            full_radius: PLANET_SCALE.curvature_full_radius,
        }
    }
}

pub(crate) fn update_visual_frame(
    mode: Res<crate::space::FlightMode>,
    player: Query<&crate::player::Player>,
    ship: Res<crate::space::ShipState>,
    world: Option<Res<crate::world::World>>,
    mut frame: ResMut<PlanetVisualFrame>,
) {
    if !mode.ground_scene() {
        return;
    }
    let Ok(player) = player.single() else { return };
    let focus = if matches!(
        *mode,
        crate::space::FlightMode::Atmo | crate::space::FlightMode::AtmoLand
    ) {
        ship.pos.xz()
    } else {
        player.pos.xz()
    };
    frame.focus = focus;
    frame.center = Vec3::new(focus.x, frame.datum_y - frame.radius, focus.y);
    let exact_radius = world
        .as_ref()
        .map(|world| world.view_dist as f32 * crate::data::CHUNK as f32 + 8.0)
        .unwrap_or(PLANET_SCALE.curvature_flat_radius);
    frame.flat_radius = PLANET_SCALE.curvature_flat_radius.max(exact_radius);
    frame.full_radius = PLANET_SCALE
        .curvature_full_radius
        .max(frame.flat_radius + 1_244.0);
}

#[inline]
pub fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0).max(f32::EPSILON)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn space_fade(altitude: f32) -> f32 {
    smoothstep(
        PLANET_SCALE.sky_space_fade_start,
        PLANET_SCALE.sky_space_fade_end,
        altitude,
    )
}

/// Height-dependent atmospheric flight ceiling.  Low flight keeps its
/// familiar handling while the upper atmosphere opens up enough that the
/// larger scale does not turn launch into a multi-minute wait.
#[inline]
pub fn atmospheric_max_speed(altitude: f32, boost: bool) -> f32 {
    let mid = smoothstep(300.0, 1_400.0, altitude);
    if boost {
        55.0 + (180.0 - 55.0) * mid
    } else {
        30.0 + (90.0 - 30.0) * mid
    }
}

#[inline]
pub fn local_to_planet_direction(x: f32, z: f32) -> Vec3 {
    let lon = x / PLANET_SCALE.local_planet_radius;
    let lat = (z / PLANET_SCALE.local_planet_radius).clamp(
        -std::f32::consts::FRAC_PI_2 + 1.0e-4,
        std::f32::consts::FRAC_PI_2 - 1.0e-4,
    );
    Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin())
}

#[inline]
pub fn planet_direction_to_local(direction: Vec3) -> Vec2 {
    let direction = direction.normalize_or_zero();
    let lon = direction.z.atan2(direction.x);
    let lat = direction.y.clamp(-1.0, 1.0).asin();
    Vec2::new(
        lon * PLANET_SCALE.local_planet_radius,
        lat * PLANET_SCALE.local_planet_radius,
    )
}

#[inline]
pub fn curved_surface_position(flat: Vec3, frame: PlanetVisualFrame) -> Vec3 {
    let delta = flat.xz() - frame.focus;
    let distance = delta.length();
    if distance <= frame.flat_radius {
        return flat;
    }
    let direction = if distance > f32::EPSILON {
        delta / distance
    } else {
        Vec2::X
    };
    let angle = (distance / frame.radius).min(1.45);
    let radial = Vec3::new(
        direction.x * angle.sin(),
        angle.cos(),
        direction.y * angle.sin(),
    );
    let sphere = frame.center + radial * (frame.radius + flat.y - frame.datum_y);
    let blend = smoothstep(frame.flat_radius, frame.full_radius, distance);
    flat.lerp(sphere, blend)
}

/// Planetary scale contract and the transient visual tangent frame.
pub struct PlanetScalePlugin;

impl bevy::prelude::Plugin for PlanetScalePlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<PlanetVisualFrame>().add_systems(
            PostUpdate,
            update_visual_frame.before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_bands_are_ordered_and_separated() {
        let p = PLANET_SCALE;
        assert!(p.cloud_bottom > 96.0);
        assert!(p.cloud_top > p.cloud_bottom);
        assert!(p.sky_space_fade_start > p.cloud_top);
        assert!(p.reentry_altitude > p.sky_space_fade_start);
        assert!(p.exit_altitude > p.reentry_altitude);
        assert!(p.atmosphere_top > p.exit_altitude);
    }

    #[test]
    fn tangent_patch_roundtrip() {
        for local in [
            Vec2::ZERO,
            Vec2::new(123.4, -45.6),
            Vec2::new(8_000.0, 4_000.0),
            Vec2::new(-12_000.0, -7_500.0),
        ] {
            let direction = local_to_planet_direction(local.x, local.y);
            let decoded = planet_direction_to_local(direction);
            assert!(decoded.distance(local) < 0.01, "{decoded:?} vs {local:?}");
        }
    }

    #[test]
    fn upper_atmosphere_accelerates_flight() {
        assert_eq!(atmospheric_max_speed(0.0, false), 30.0);
        assert_eq!(atmospheric_max_speed(0.0, true), 55.0);
        assert!((atmospheric_max_speed(1_500.0, false) - 90.0).abs() < 0.01);
        assert!((atmospheric_max_speed(1_500.0, true) - 180.0).abs() < 0.01);
    }

    #[test]
    fn visual_center_stays_below_its_focus() {
        let frame = PlanetVisualFrame::default();
        assert_eq!(frame.center.xz(), frame.focus);
        assert!((frame.center.y + frame.radius - frame.datum_y).abs() < f32::EPSILON);
    }

    #[test]
    fn visual_curvature_is_continuous_when_focus_moves() {
        let mut frame = PlanetVisualFrame {
            flat_radius: 256.0,
            full_radius: 1_500.0,
            ..default()
        };
        let flat = Vec3::new(4_000.0, crate::data::SEA_Y, 0.0);
        let before = curved_surface_position(flat, frame);
        frame.focus.x += 0.01;
        frame.center.x += 0.01;
        let after = curved_surface_position(flat, frame);
        assert!(before.distance(after) < 0.02);
    }
}
