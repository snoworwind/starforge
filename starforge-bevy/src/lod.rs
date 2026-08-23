//! Hierarchical, Voxy-style distant terrain.
//!
//! This is an independent implementation of the architectural ideas: fixed
//! 32-cube sections, 2×2×2 mip reduction, projected-error selection and parent
//! fallback.  The first renderer specializes the hierarchy to the planet's
//! visible surface; the volumetric section representation below is also used
//! to preserve a clean path for caves, structures and edited voxels.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::materials::TerrainMaterials;
use crate::player::Player;
use crate::save::{LodMode, Settings};
use crate::space::FlightMode;
use crate::world::{self, World};

pub const SECTION_EDGE: usize = 32;
const SECTION_VOLUME: usize = SECTION_EDGE * SECTION_EDGE * SECTION_EDGE;
const SURFACE_MIN_LEVEL: u8 = 2;
const SURFACE_MAX_LEVEL: u8 = 8;
const LOD_RADIUS: f32 = 16_384.0;
// At a 75°/1080p camera this corresponds to roughly seven pixels of projected
// cell error. Standard Bevy mesh entities are intentionally coarser than
// Voxy's batched GPU arena so the initial implementation stays draw-call safe.
const PROJECTED_ERROR_DISTANCE: f32 = 100.0;
const BUILD_BUDGET_PER_FRAME: usize = 4;
const EVICT_AFTER_FRAMES: u64 = 900;

pub const VOXEL_OPAQUE: u8 = 1 << 0;
pub const VOXEL_WATER: u8 = 1 << 1;
pub const VOXEL_EMISSIVE: u8 = 1 << 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LodVoxel {
    pub material: u8,
    pub coverage: u8,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LodSectionKey {
    pub level: u8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl LodSectionKey {
    #[inline]
    pub fn cell_size(self) -> f32 {
        (1u32 << self.level) as f32
    }

    #[inline]
    pub fn span(self) -> f32 {
        SECTION_EDGE as f32 * self.cell_size()
    }

    #[inline]
    fn children_2d(self) -> [Self; 4] {
        let level = self.level.saturating_sub(1);
        [
            Self {
                level,
                x: self.x * 2,
                y: self.y,
                z: self.z * 2,
            },
            Self {
                level,
                x: self.x * 2 + 1,
                y: self.y,
                z: self.z * 2,
            },
            Self {
                level,
                x: self.x * 2,
                y: self.y,
                z: self.z * 2 + 1,
            },
            Self {
                level,
                x: self.x * 2 + 1,
                y: self.y,
                z: self.z * 2 + 1,
            },
        ]
    }
}

/// Packed logical data for a complete 32³ node.  Runtime surface meshes do
/// not need to allocate this full block, but edits and future volumetric LOD
/// use the exact same hierarchy and reducer.
#[derive(Clone, Debug)]
pub struct LodSectionData {
    pub key: LodSectionKey,
    voxels: Box<[LodVoxel]>,
}

impl LodSectionData {
    pub fn empty(key: LodSectionKey) -> Self {
        Self {
            key,
            voxels: vec![LodVoxel::default(); SECTION_VOLUME].into_boxed_slice(),
        }
    }

    #[inline]
    fn index(x: usize, y: usize, z: usize) -> usize {
        (y * SECTION_EDGE + z) * SECTION_EDGE + x
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> LodVoxel {
        self.voxels[Self::index(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, voxel: LodVoxel) {
        self.voxels[Self::index(x, y, z)] = voxel;
    }

    /// Reduces eight 32³ children into one parent. Child order is
    /// `x + z*2 + y*4`, matching a conventional octree bit layout.
    pub fn reduce_children(key: LodSectionKey, children: [&Self; 8]) -> Self {
        let mut parent = Self::empty(key);
        for y in 0..SECTION_EDGE {
            for z in 0..SECTION_EDGE {
                for x in 0..SECTION_EDGE {
                    let mut representative = LodVoxel::default();
                    let mut best_score = 0u16;
                    let mut coverage_sum = 0u16;
                    let mut flags = 0u8;
                    for sy in 0..2 {
                        for sz in 0..2 {
                            for sx in 0..2 {
                                let gx = x * 2 + sx;
                                let gy = y * 2 + sy;
                                let gz = z * 2 + sz;
                                let child_index = (gx / SECTION_EDGE)
                                    + (gz / SECTION_EDGE) * 2
                                    + (gy / SECTION_EDGE) * 4;
                                let voxel = children[child_index].get(
                                    gx % SECTION_EDGE,
                                    gy % SECTION_EDGE,
                                    gz % SECTION_EDGE,
                                );
                                coverage_sum += voxel.coverage as u16;
                                flags |= voxel.flags;
                                let semantic_bonus = if voxel.flags & VOXEL_EMISSIVE != 0 {
                                    768
                                } else if voxel.flags & VOXEL_WATER != 0 {
                                    512
                                } else if voxel.flags & VOXEL_OPAQUE != 0 {
                                    256
                                } else {
                                    0
                                };
                                let score = semantic_bonus + voxel.coverage as u16;
                                if score > best_score {
                                    best_score = score;
                                    representative = voxel;
                                }
                            }
                        }
                    }
                    representative.coverage = (coverage_sum / 8) as u8;
                    representative.flags = flags;
                    parent.set(x, y, z, representative);
                }
            }
        }
        parent
    }
}

#[derive(Component)]
pub struct LodMesh;

struct ResidentNode {
    entity: Entity,
    mesh: Handle<Mesh>,
    visible: bool,
    last_used_frame: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LodStats {
    pub target_sections: usize,
    pub resident_sections: usize,
    pub visible_sections: usize,
    pub queued_sections: usize,
    pub generated_this_frame: usize,
    pub build_ms: f32,
    pub parent_fallbacks: usize,
}

#[derive(Resource, Default)]
pub struct LodRuntime {
    world_seed: Option<u32>,
    frame: u64,
    nodes: HashMap<LodSectionKey, ResidentNode>,
    pub coverage_ready: bool,
    pub stats: LodStats,
}

impl LodRuntime {
    fn clear(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        for (_, node) in self.nodes.drain() {
            commands.entity(node.entity).despawn();
            meshes.remove(&node.mesh);
        }
        self.coverage_ready = false;
        self.stats = LodStats::default();
    }
}

#[derive(Default)]
struct Selection {
    roots: Vec<LodSectionKey>,
    active: HashSet<LodSectionKey>,
    split: HashSet<LodSectionKey>,
    targets: HashSet<LodSectionKey>,
}

#[inline]
fn aabb_distance_2d(point: Vec2, min: Vec2, max: Vec2) -> f32 {
    let dx = if point.x < min.x {
        min.x - point.x
    } else if point.x > max.x {
        point.x - max.x
    } else {
        0.0
    };
    let dz = if point.y < min.y {
        min.y - point.y
    } else if point.y > max.y {
        point.y - max.y
    } else {
        0.0
    };
    Vec2::new(dx, dz).length()
}

fn select_node(key: LodSectionKey, player: Vec2, exact_radius: f32, selection: &mut Selection) {
    let span = key.span();
    let min = Vec2::new(key.x as f32 * span, key.z as f32 * span);
    let max = min + Vec2::splat(span);
    if aabb_distance_2d(player, min, max) > LOD_RADIUS {
        return;
    }

    // Complete nodes under the exact chunk square are omitted. Boundary nodes
    // remain slightly sunk under exact terrain, which is safer than a crack.
    if min.x >= player.x - exact_radius
        && max.x <= player.x + exact_radius
        && min.y >= player.y - exact_radius
        && max.y <= player.y + exact_radius
    {
        return;
    }

    selection.active.insert(key);
    let distance = aabb_distance_2d(player, min, max).max(1.0);
    let split_distance = key.cell_size() * PROJECTED_ERROR_DISTANCE;
    if key.level > SURFACE_MIN_LEVEL && distance < split_distance {
        selection.split.insert(key);
        for child in key.children_2d() {
            select_node(child, player, exact_radius, selection);
        }
    } else {
        selection.targets.insert(key);
    }
}

fn build_selection(player: Vec2, exact_radius: f32) -> Selection {
    let mut selection = Selection::default();
    let root_span = SECTION_EDGE as f32 * (1u32 << SURFACE_MAX_LEVEL) as f32;
    let min_x = ((player.x - LOD_RADIUS) / root_span).floor() as i32;
    let max_x = ((player.x + LOD_RADIUS) / root_span).floor() as i32;
    let min_z = ((player.y - LOD_RADIUS) / root_span).floor() as i32;
    let max_z = ((player.y + LOD_RADIUS) / root_span).floor() as i32;
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            let root = LodSectionKey {
                level: SURFACE_MAX_LEVEL,
                x,
                y: 0,
                z,
            };
            let before = selection.active.len();
            select_node(root, player, exact_radius, &mut selection);
            if selection.active.len() != before {
                selection.roots.push(root);
            }
        }
    }
    selection
}

/// Resolves a selected subtree atomically. Children are only shown when all
/// required siblings have a GPU-ready mesh; otherwise the ready parent stays
/// visible and covers the complete area.
fn resolve_visible(
    key: LodSectionKey,
    selection: &Selection,
    resident: &HashMap<LodSectionKey, ResidentNode>,
    visible: &mut Vec<LodSectionKey>,
    fallbacks: &mut usize,
) -> bool {
    if !selection.active.contains(&key) {
        return true;
    }
    if !selection.split.contains(&key) {
        if resident.contains_key(&key) {
            visible.push(key);
            return true;
        }
        return false;
    }

    let start = visible.len();
    let mut children_ready = true;
    for child in key.children_2d() {
        children_ready &= resolve_visible(child, selection, resident, visible, fallbacks);
    }
    if children_ready {
        return true;
    }
    visible.truncate(start);
    if resident.contains_key(&key) {
        visible.push(key);
        *fallbacks += 1;
        true
    } else {
        false
    }
}

fn build_surface_mesh(world: &World, atlas: &crate::textures::Atlas, key: LodSectionKey) -> Mesh {
    let edge = SECTION_EDGE + 1;
    let cell = key.cell_size();
    let span = key.span();
    let origin_x = key.x as f32 * span;
    let origin_z = key.z as f32 * span;
    let top_count = edge * edge;
    let mut positions = Vec::with_capacity(top_count + SECTION_EDGE * 4);
    let mut normals = vec![[0.0, 1.0, 0.0]; top_count];
    let mut colors = Vec::with_capacity(top_count + SECTION_EDGE * 4);

    for z in 0..edge {
        for x in 0..edge {
            let wx = origin_x + x as f32 * cell;
            let wz = origin_z + z as f32 * cell;
            let (height, color) = world::far_surface_sample(world, atlas, wx, wz);
            positions.push([wx, height - world::FAR_SINK, wz]);
            colors.push(color);
        }
    }

    let height = |x: usize, z: usize| positions[z * edge + x][1];
    for z in 0..edge {
        for x in 0..edge {
            let left = height(x.saturating_sub(1), z);
            let right = height((x + 1).min(SECTION_EDGE), z);
            let down = height(x, z.saturating_sub(1));
            let up = height(x, (z + 1).min(SECTION_EDGE));
            normals[z * edge + x] = Vec3::new(left - right, 2.0 * cell, down - up)
                .normalize_or_zero()
                .to_array();
        }
    }

    let mut indices = Vec::with_capacity(SECTION_EDGE * SECTION_EDGE * 6 + SECTION_EDGE * 24);
    for z in 0..SECTION_EDGE as u32 {
        for x in 0..SECTION_EDGE as u32 {
            let a = z * edge as u32 + x;
            let b = a + 1;
            let c = a + edge as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    // Perimeter skirts hide the remaining T-junction at a one-level neighbor
    // transition. Boundary positions are shared exactly by the power-of-two
    // grid; the skirt only becomes visible if raster precision opens a crack.
    let mut perimeter = Vec::with_capacity(SECTION_EDGE * 4);
    for x in 0..=SECTION_EDGE {
        perimeter.push(x);
    }
    for z in 1..=SECTION_EDGE {
        perimeter.push(z * edge + SECTION_EDGE);
    }
    for x in (0..SECTION_EDGE).rev() {
        perimeter.push(SECTION_EDGE * edge + x);
    }
    for z in (1..SECTION_EDGE).rev() {
        perimeter.push(z * edge);
    }
    let skirt_depth = (cell * 0.25).clamp(4.0, 24.0);
    let skirt_start = positions.len() as u32;
    for &top in &perimeter {
        let mut position = positions[top];
        position[1] -= skirt_depth;
        positions.push(position);
        normals.push(normals[top]);
        colors.push(colors[top]);
    }
    for i in 0..perimeter.len() {
        let next = (i + 1) % perimeter.len();
        let top_a = perimeter[i] as u32;
        let top_b = perimeter[next] as u32;
        let low_a = skirt_start + i as u32;
        let low_b = skirt_start + next as u32;
        indices.extend_from_slice(&[top_a, low_a, top_b, top_b, low_a, low_b]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_indices(Indices::U32(indices));
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

fn set_node_visible(commands: &mut Commands, node: &mut ResidentNode, visible: bool) {
    if node.visible == visible {
        return;
    }
    node.visible = visible;
    commands.entity(node.entity).insert(if visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    });
}

#[allow(clippy::too_many_arguments)]
pub fn hierarchical_lod_system(
    mut commands: Commands,
    settings: Res<Settings>,
    mode: Res<FlightMode>,
    player: Query<&Player>,
    world: Res<World>,
    atlas: Res<crate::textures::AtlasRes>,
    materials: Res<TerrainMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut runtime: ResMut<LodRuntime>,
) {
    runtime.frame = runtime.frame.wrapping_add(1);
    if settings.lod_mode != LodMode::Hierarchical || !mode.ground_scene() {
        for node in runtime.nodes.values_mut() {
            set_node_visible(&mut commands, node, false);
        }
        runtime.coverage_ready = false;
        runtime.stats.visible_sections = 0;
        return;
    }
    let Ok(player) = player.single() else { return };
    let player_xz = player.pos.xz();
    if runtime.world_seed != Some(world.seed) {
        runtime.clear(&mut commands, &mut meshes);
        runtime.world_seed = Some(world.seed);
    }

    let exact_radius = world.view_dist as f32 * crate::data::CHUNK as f32 - 8.0;
    let selection = build_selection(player_xz, exact_radius.max(64.0));
    let mut missing: Vec<_> = selection
        .active
        .iter()
        .copied()
        .filter(|key| !runtime.nodes.contains_key(key))
        .collect();
    missing.sort_by(|a, b| {
        b.level.cmp(&a.level).then_with(|| {
            let ac = Vec2::new((a.x as f32 + 0.5) * a.span(), (a.z as f32 + 0.5) * a.span());
            let bc = Vec2::new((b.x as f32 + 0.5) * b.span(), (b.z as f32 + 0.5) * b.span());
            ac.distance_squared(player_xz)
                .total_cmp(&bc.distance_squared(player_xz))
        })
    });

    let build_start = Instant::now();
    let mut generated = 0;
    let current_frame = runtime.frame;
    for key in missing.iter().take(BUILD_BUDGET_PER_FRAME).copied() {
        let mesh = meshes.add(build_surface_mesh(&world, &atlas.atlas, key));
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.lod.clone()),
                Transform::IDENTITY,
                Visibility::Hidden,
                NotShadowCaster,
                NoFrustumCulling,
                LodMesh,
                crate::InGame,
            ))
            .id();
        runtime.nodes.insert(
            key,
            ResidentNode {
                entity,
                mesh,
                visible: false,
                last_used_frame: current_frame,
            },
        );
        generated += 1;
    }

    let mut visible = Vec::new();
    let mut fallbacks = 0;
    let mut complete = true;
    for root in &selection.roots {
        complete &= resolve_visible(
            *root,
            &selection,
            &runtime.nodes,
            &mut visible,
            &mut fallbacks,
        );
    }
    let visible_set: HashSet<_> = visible.iter().copied().collect();
    let frame = runtime.frame;
    for (key, node) in &mut runtime.nodes {
        let show = visible_set.contains(key);
        set_node_visible(&mut commands, node, show);
        if selection.active.contains(key) || show {
            node.last_used_frame = frame;
        }
    }

    let stale: Vec<_> = runtime
        .nodes
        .iter()
        .filter_map(|(key, node)| {
            (frame.saturating_sub(node.last_used_frame) > EVICT_AFTER_FRAMES).then_some(*key)
        })
        .take(8)
        .collect();
    for key in stale {
        if let Some(node) = runtime.nodes.remove(&key) {
            commands.entity(node.entity).despawn();
            meshes.remove(&node.mesh);
        }
    }

    runtime.coverage_ready = complete && !selection.roots.is_empty();
    runtime.stats = LodStats {
        target_sections: selection.targets.len(),
        resident_sections: runtime.nodes.len(),
        visible_sections: visible.len(),
        queued_sections: missing.len().saturating_sub(generated),
        generated_this_frame: generated,
        build_ms: build_start.elapsed().as_secs_f32() * 1_000.0,
        parent_fallbacks: fallbacks,
    };
    static LOG_STATS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *LOG_STATS.get_or_init(|| std::env::args().any(|arg| arg == "--lod-log"))
        && runtime.frame.is_multiple_of(60)
    {
        let stats = runtime.stats;
        println!(
            "LOD target={} resident={} visible={} queued={} generated={} build_ms={:.2} fallback={} ready={}",
            stats.target_sections,
            stats.resident_sections,
            stats.visible_sections,
            stats.queued_sections,
            stats.generated_this_frame,
            stats.build_ms,
            stats.parent_fallbacks,
            runtime.coverage_ready,
        );
    }
}

/// Distant terrain plugin: hierarchical LOD selection (legacy far mesh fallback runs
/// in `GameSet::FarMesh`, after this set).
pub struct LodPlugin;

impl Plugin for LodPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LodRuntime>().add_systems(
            Update,
            hierarchical_lod_system
                .in_set(crate::schedule::GameSet::FarLod)
                .before(crate::schedule::GameSet::FarMesh)
                .run_if(in_state(crate::schedule::GameState::Playing)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_uses_power_of_two_sections() {
        let low = LodSectionKey {
            level: 0,
            x: 1,
            y: 0,
            z: -2,
        };
        let high = LodSectionKey { level: 5, ..low };
        assert_eq!(low.span(), 32.0);
        assert_eq!(high.span(), 1_024.0);
    }

    #[test]
    fn mip_reduction_preserves_semantic_materials() {
        let child_keys = [
            LodSectionKey::default(),
            LodSectionKey::default(),
            LodSectionKey::default(),
            LodSectionKey::default(),
            LodSectionKey::default(),
            LodSectionKey::default(),
            LodSectionKey::default(),
            LodSectionKey::default(),
        ];
        let mut children: Vec<_> = child_keys.into_iter().map(LodSectionData::empty).collect();
        children[0].set(
            0,
            0,
            0,
            LodVoxel {
                material: 7,
                coverage: 255,
                flags: VOXEL_OPAQUE,
            },
        );
        children[7].set(
            31,
            31,
            31,
            LodVoxel {
                material: 9,
                coverage: 100,
                flags: VOXEL_EMISSIVE,
            },
        );
        let refs = [
            &children[0],
            &children[1],
            &children[2],
            &children[3],
            &children[4],
            &children[5],
            &children[6],
            &children[7],
        ];
        let parent = LodSectionData::reduce_children(
            LodSectionKey {
                level: 1,
                ..default()
            },
            refs,
        );
        let solid = parent.get(0, 0, 0);
        assert_eq!(solid.material, 7);
        assert_ne!(solid.flags & VOXEL_OPAQUE, 0);
        let emissive = parent.get(31, 31, 31);
        assert_eq!(emissive.material, 9);
        assert_ne!(emissive.flags & VOXEL_EMISSIVE, 0);
    }

    #[test]
    fn selection_has_no_levels_outside_contract() {
        let selection = build_selection(Vec2::new(96.0, 96.0), 160.0);
        assert!(!selection.targets.is_empty());
        assert!(
            selection
                .targets
                .iter()
                .all(|key| (SURFACE_MIN_LEVEL..=SURFACE_MAX_LEVEL).contains(&key.level))
        );
        assert!(
            selection.targets.len() < 900,
            "too many unbatched target sections: {}",
            selection.targets.len()
        );
    }
}

// ---------- Plugin ----------
