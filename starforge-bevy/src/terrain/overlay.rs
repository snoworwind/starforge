//! Opt-in C01 debug overlay: draws the current terrain coverage so near
//! chunks, LOD sections, dirty chunks, the legacy far mesh and the curvature
//! radii can be compared visually (`--terrain-overlay`).
//!
//! Disabled by default; when disabled the system returns before creating any
//! gizmo, so normal runs are unaffected.

use bevy::prelude::*;

use crate::data::CHUNK;
use crate::lod::LodRuntime;
use crate::planet_scale::PlanetVisualFrame;
use crate::player::Player;
use crate::world::{ChunkMesh, FarMesh, World};

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainOverlay(pub bool);

/// Cap gizmo work: a 16384 m LOD ring can hold more sections than a readable
/// overlay; the report/DTO still lists every node, the overlay samples.
const MAX_CHUNK_RECTS: usize = 512;
const MAX_LOD_RECTS: usize = 512;
const CIRCLE_SEGMENTS: usize = 48;

fn rect_points(center: Vec3, half: Vec2, y: f32) -> [Vec3; 5] {
    [
        Vec3::new(center.x - half.x, y, center.z - half.y),
        Vec3::new(center.x + half.x, y, center.z - half.y),
        Vec3::new(center.x + half.x, y, center.z + half.y),
        Vec3::new(center.x - half.x, y, center.z + half.y),
        Vec3::new(center.x - half.x, y, center.z - half.y),
    ]
}

fn ring_points(center: Vec3, radius: f32, segments: usize) -> Vec<Vec3> {
    let mut points = Vec::with_capacity(segments + 1);
    for step in 0..segments {
        let angle = step as f32 / segments as f32 * std::f32::consts::TAU;
        points.push(Vec3::new(
            center.x + angle.cos() * radius,
            center.y,
            center.z + angle.sin() * radius,
        ));
    }
    if let Some(first) = points.first().copied() {
        points.push(first);
    }
    points
}

#[allow(clippy::too_many_arguments)]
pub fn terrain_overlay_system(
    overlay: Res<TerrainOverlay>,
    world: Res<World>,
    lod: Res<LodRuntime>,
    planet: Res<PlanetVisualFrame>,
    far: Query<&FarMesh>,
    player: Query<&Player>,
    chunks: Query<&ChunkMesh>,
    mut gizmos: Gizmos,
) {
    if !overlay.0 {
        return;
    }
    let focus = player
        .single()
        .map(|player| player.pos)
        .unwrap_or(Vec3::new(planet.focus.x, planet.datum_y, planet.focus.y));
    let ground_y = focus.y + 0.6;

    // Near chunks (deduplicated: solid and water are separate entities).
    let mut seen: Vec<(i32, i32)> = Vec::new();
    for chunk in chunks.iter() {
        if seen.len() >= MAX_CHUNK_RECTS {
            break;
        }
        if seen.contains(&(chunk.cx, chunk.cz)) {
            continue;
        }
        seen.push((chunk.cx, chunk.cz));
        let center = Vec3::new(
            chunk.cx as f32 * CHUNK as f32 + CHUNK as f32 * 0.5,
            ground_y,
            chunk.cz as f32 * CHUNK as f32 + CHUNK as f32 * 0.5,
        );
        let dirty = world
            .get_chunk(chunk.cx, chunk.cz)
            .map(|chunk| chunk.dirty)
            .unwrap_or(false);
        let color = if dirty {
            Color::srgb(1.0, 0.2, 0.15)
        } else {
            Color::srgba(0.2, 0.85, 0.95, 0.55)
        };
        gizmos.linestrip(
            rect_points(center, Vec2::splat(CHUNK as f32 * 0.5 - 0.2), ground_y),
            color,
        );
    }

    // Hierarchical LOD sections, colored by level (cyan -> amber).
    let range = crate::lod::LEVEL_RANGE;
    let span_range = (range.1 - range.0).max(1) as f32;
    for (index, node) in lod.node_views().iter().enumerate() {
        if index >= MAX_LOD_RECTS {
            break;
        }
        let t = (node.key.level.saturating_sub(range.0)) as f32 / span_range;
        let color = Color::srgb(0.2 + 0.8 * t, 0.75 - 0.35 * t, 0.95 - 0.75 * t);
        gizmos.linestrip(
            rect_points(node.center, Vec2::splat(node.span * 0.5), ground_y + 1.5),
            color,
        );
    }

    // Legacy far mesh extent.
    if let Some(mesh) = far.iter().next() {
        let info = mesh.info();
        let center = Vec3::new(info.cx, ground_y + 3.0, info.cz);
        gizmos.linestrip(
            rect_points(center, Vec2::splat(info.half_extent_meters), ground_y + 3.0),
            Color::srgba(0.4, 1.0, 0.4, 0.7),
        );
    }

    // Curvature blend rings around the streaming focus.
    let focus_y = Vec3::new(planet.focus.x, ground_y + 4.5, planet.focus.y);
    gizmos.linestrip(
        ring_points(focus_y, planet.flat_radius, CIRCLE_SEGMENTS),
        Color::srgba(1.0, 0.75, 0.2, 0.8),
    );
    gizmos.linestrip(
        ring_points(focus_y, planet.full_radius, CIRCLE_SEGMENTS),
        Color::srgba(1.0, 0.3, 0.8, 0.6),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_is_opt_in_by_default() {
        assert!(!TerrainOverlay::default().0);
    }

    #[test]
    fn rect_and_ring_helpers_are_closed_loops() {
        let rect = rect_points(Vec3::ZERO, Vec2::splat(2.0), 1.0);
        assert_eq!(rect.first(), rect.last(), "rect must be a closed loop");
        let ring = ring_points(Vec3::ZERO, 10.0, 8);
        assert_eq!(ring.first(), ring.last(), "ring must be a closed loop");
        assert_eq!(ring.len(), 9);
        for point in &ring {
            let radius = (point.x * point.x + point.z * point.z).sqrt();
            assert!((radius - 10.0).abs() < 1e-4);
        }
    }
}
