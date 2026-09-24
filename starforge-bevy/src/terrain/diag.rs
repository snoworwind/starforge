//! C01 terrain diagnostics: one coherent snapshot of what each terrain owner
//! currently covers, plus a deterministic input signature so mesh/LOD
//! regressions can be separated from input drift.

use bevy::prelude::*;
use serde::Serialize;

use crate::data::{CHUNK, WORLD_H};
use crate::lod::{self, LodRuntime, LodStats};
use crate::planet_scale::{PLANET_SCALE, PlanetVisualFrame};
use crate::player::Player;
use crate::visual::WorldEpoch;
use crate::world::{self, ChunkMesh, FarMesh, World};

pub const TERRAIN_DIAGNOSTICS_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NearCoverage {
    pub view_dist: i32,
    /// Generation ring (`view_dist + 2` in `stream_world_step`).
    pub load_radius_chunks: i32,
    /// Meshing ring (`view_dist + 1`).
    pub mesh_radius_chunks: i32,
    /// Meshes are despawned beyond this Chebyshev distance (`view_dist + 3`).
    pub unload_radius_chunks: i32,
    pub mesh_radius_meters: f32,
    pub resident_chunks: usize,
    pub solid_meshes: usize,
    pub cutout_meshes: usize,
    pub transparent_meshes: usize,
    pub emissive_meshes: usize,
    pub special_meshes: usize,
    pub water_meshes: usize,
    pub mesh_entities: usize,
    /// Chunks inside `view_dist` with no mesh or a dirty mesh.
    pub missing_view_chunks: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct LodCoverage {
    pub coverage_ready: bool,
    pub radius_meters: f32,
    /// Surface LOD preserves only the top surface; caves/bridges/overhangs are
    /// out of scope until C06 adds structural proxies.
    pub surface_only: bool,
    pub level_range: [u8; 2],
    pub stats: LodStats,
    pub resident_by_level: Vec<usize>,
    pub world_epoch: Option<u64>,
}

impl Default for LodCoverage {
    fn default() -> Self {
        Self {
            coverage_ready: false,
            radius_meters: lod::COVERAGE_RADIUS_METERS,
            surface_only: true,
            level_range: [lod::LEVEL_RANGE.0, lod::LEVEL_RANGE.1],
            stats: LodStats::default(),
            resident_by_level: Vec::new(),
            world_epoch: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct FarCoverage {
    pub present: bool,
    pub visible: bool,
    pub cx: f32,
    pub cz: f32,
    pub seed: u32,
    pub epoch: u64,
    pub staging: bool,
    pub half_extent_meters: f32,
    pub sink: f32,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct CurvatureCoverage {
    pub planet_radius: f32,
    pub flat_radius: f32,
    pub full_radius: f32,
    pub datum_y: f32,
    pub focus: [f32; 3],
}

/// One ownership band in the near/LOD/far chain. `min == 0` means the owner
/// starts at the streaming center; overlapping `max` values are intentional
/// fallback coverage, listed in `TerrainDiagnostics::notes`.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CoverageBand {
    pub owner: &'static str,
    pub min_meters: f32,
    pub max_meters: f32,
    pub source: &'static str,
    pub surface_only: bool,
}

#[derive(Resource, Clone, Debug, Default, Serialize, PartialEq)]
pub struct TerrainDiagnostics {
    pub schema_version: u32,
    pub world_epoch: u64,
    pub seed: u32,
    pub biome: String,
    pub view_dist: i32,
    pub near: NearCoverage,
    pub lod: LodCoverage,
    pub far: FarCoverage,
    pub curvature: CurvatureCoverage,
    pub ownership: Vec<CoverageBand>,
    pub dirty_chunks_total: usize,
    pub dirty_chunks_sample: Vec<[i32; 2]>,
    pub input_signature: u64,
    pub notes: Vec<String>,
}

impl Default for NearCoverage {
    fn default() -> Self {
        Self {
            view_dist: 8,
            load_radius_chunks: 10,
            mesh_radius_chunks: 9,
            unload_radius_chunks: 11,
            mesh_radius_meters: 9.0 * CHUNK as f32,
            resident_chunks: 0,
            solid_meshes: 0,
            cutout_meshes: 0,
            transparent_meshes: 0,
            emissive_meshes: 0,
            special_meshes: 0,
            water_meshes: 0,
            mesh_entities: 0,
            missing_view_chunks: 0,
        }
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

fn hash_f32(hash: &mut u64, value: f32) {
    hash_bytes(hash, &value.to_bits().to_le_bytes());
}

/// Deterministic signature of the terrain *inputs* (seed/biome/view distance
/// and the frozen scale/LOD/far constants). C01 compares it across runs before
/// blaming a mesh hash change on the builder; a differing signature means the
/// world itself changed.
pub fn input_signature(seed: u32, biome: &str, view_dist: i32) -> u64 {
    let mut hash = FNV_OFFSET;
    hash_bytes(&mut hash, &seed.to_le_bytes());
    hash_bytes(&mut hash, biome.as_bytes());
    hash_bytes(&mut hash, &view_dist.to_le_bytes());
    hash_bytes(&mut hash, &WORLD_H.to_le_bytes());
    hash_f32(&mut hash, PLANET_SCALE.local_planet_radius);
    hash_f32(&mut hash, PLANET_SCALE.curvature_flat_radius);
    hash_f32(&mut hash, PLANET_SCALE.curvature_full_radius);
    hash_f32(&mut hash, lod::COVERAGE_RADIUS_METERS);
    hash_bytes(&mut hash, &lod::LEVEL_RANGE.0.to_le_bytes());
    hash_bytes(&mut hash, &lod::LEVEL_RANGE.1.to_le_bytes());
    hash_bytes(&mut hash, &(world::FAR_N as u32).to_le_bytes());
    hash_f32(&mut hash, world::FAR_STEP);
    hash_f32(&mut hash, world::FAR_SINK);
    hash
}

/// Build the ownership table for the current view distance. Pure so tests and
/// the report can assert the band order without an `App`.
pub fn coverage_bands(view_dist: i32) -> Vec<CoverageBand> {
    let mesh_radius_meters = (view_dist + 1) as f32 * CHUNK as f32;
    vec![
        CoverageBand {
            owner: "near_chunks",
            min_meters: 0.0,
            max_meters: mesh_radius_meters,
            source: "world::stream_world_step (chunk meshes)",
            surface_only: false,
        },
        CoverageBand {
            owner: "hierarchical_lod",
            min_meters: mesh_radius_meters,
            max_meters: lod::COVERAGE_RADIUS_METERS,
            source: "lod::hierarchical_lod_system",
            surface_only: true,
        },
        CoverageBand {
            owner: "legacy_far_mesh",
            min_meters: (mesh_radius_meters - 120.0).max(0.0),
            max_meters: (world::FAR_N as f32 - 1.0) * 0.5 * world::FAR_STEP,
            source: "world::far_mesh_system (fallback under LOD)",
            surface_only: true,
        },
        CoverageBand {
            owner: "curvature",
            min_meters: PLANET_SCALE.curvature_flat_radius,
            max_meters: PLANET_SCALE.curvature_full_radius,
            source: "planet_scale::update_visual_frame + planet_curvature.wgsl",
            surface_only: false,
        },
    ]
}

#[allow(clippy::too_many_arguments)]
pub fn collect_terrain_diagnostics(
    world: Option<Res<World>>,
    player: Query<&Player>,
    lod: Res<LodRuntime>,
    planet: Res<PlanetVisualFrame>,
    epoch: Res<WorldEpoch>,
    chunk_meshes: Query<(), With<ChunkMesh>>,
    far: Query<(&FarMesh, &Visibility)>,
    mut diagnostics: ResMut<TerrainDiagnostics>,
) {
    let Some(world) = world else {
        return;
    };
    let view_dist = world.view_dist;
    let mut near = NearCoverage {
        view_dist,
        load_radius_chunks: view_dist + 2,
        mesh_radius_chunks: view_dist + 1,
        unload_radius_chunks: view_dist + 3,
        mesh_radius_meters: (view_dist + 1) as f32 * CHUNK as f32,
        ..default()
    };
    let mut dirty_total = 0usize;
    let mut dirty_sample = Vec::new();
    for chunk in world.chunks.values() {
        near.resident_chunks += 1;
        if chunk.mesh.is_some() {
            near.solid_meshes += 1;
        }
        if chunk.cutout_mesh.is_some() {
            near.cutout_meshes += 1;
        }
        if chunk.transparent_mesh.is_some() {
            near.transparent_meshes += 1;
        }
        if chunk.emissive_mesh.is_some() {
            near.emissive_meshes += 1;
        }
        if chunk.special_mesh.is_some() {
            near.special_meshes += 1;
        }
        if chunk.water_mesh.is_some() {
            near.water_meshes += 1;
        }
        if chunk.dirty {
            dirty_total += 1;
            if dirty_sample.len() < 32 {
                dirty_sample.push([chunk.cx, chunk.cz]);
            }
        }
    }
    dirty_sample.sort();
    near.mesh_entities = chunk_meshes.iter().count();
    if let Ok(player) = player.single() {
        let pcx = crate::world::cf(player.pos.x);
        let pcz = crate::world::cf(player.pos.z);
        for cz in pcz - view_dist..=pcz + view_dist {
            for cx in pcx - view_dist..=pcx + view_dist {
                if let Some(chunk) = world.get_chunk(cx, cz) {
                    if !chunk.has_render_mesh() || chunk.dirty {
                        near.missing_view_chunks += 1;
                    }
                } else {
                    near.missing_view_chunks += 1;
                }
            }
        }
    }

    let far = far
        .iter()
        .next()
        .map(|(mesh, visibility)| {
            let info = mesh.info();
            FarCoverage {
                present: true,
                visible: !matches!(*visibility, Visibility::Hidden),
                cx: info.cx,
                cz: info.cz,
                seed: info.seed,
                epoch: info.epoch,
                staging: info.staging,
                half_extent_meters: info.half_extent_meters,
                sink: info.sink,
            }
        })
        .unwrap_or_default();

    diagnostics.schema_version = TERRAIN_DIAGNOSTICS_SCHEMA_VERSION;
    diagnostics.world_epoch = epoch.0;
    diagnostics.seed = world.seed;
    diagnostics.biome = world.biome().key.to_string();
    diagnostics.view_dist = view_dist;
    diagnostics.near = near;
    diagnostics.lod = LodCoverage {
        coverage_ready: lod.coverage_ready,
        radius_meters: lod::COVERAGE_RADIUS_METERS,
        surface_only: true,
        level_range: [lod::LEVEL_RANGE.0, lod::LEVEL_RANGE.1],
        stats: lod.stats,
        resident_by_level: lod.level_histogram(),
        world_epoch: lod.world_epoch(),
    };
    diagnostics.far = far;
    diagnostics.curvature = CurvatureCoverage {
        planet_radius: planet.radius,
        flat_radius: planet.flat_radius,
        full_radius: planet.full_radius,
        datum_y: planet.datum_y,
        focus: [planet.focus.x, planet.datum_y, planet.focus.y],
    };
    diagnostics.ownership = coverage_bands(view_dist);
    diagnostics.dirty_chunks_total = dirty_total;
    diagnostics.dirty_chunks_sample = dirty_sample;
    diagnostics.input_signature = input_signature(world.seed, world.biome().key, view_dist);
    diagnostics.notes = vec![
        "hierarchical LOD is a surface approximation; caves, bridges, overhangs and towers are not covered until C06 proxies".to_string(),
        "legacy far mesh intentionally overlaps the LOD ring as a fallback; C06 decides removal".to_string(),
        "near/LOD/far radii are diagnostics only; selection logic is unchanged by C01".to_string(),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_bands_are_ordered_and_sourced() {
        let bands = coverage_bands(8);
        assert_eq!(bands.len(), 4);
        for band in &bands {
            assert!(band.max_meters > band.min_meters, "{}", band.owner);
            assert!(!band.source.is_empty(), "{}", band.owner);
        }
        // Near meshes must be the first band and end at the meshing ring.
        assert_eq!(bands[0].owner, "near_chunks");
        assert_eq!(bands[0].max_meters, 9.0 * CHUNK as f32);
        assert_eq!(bands[1].owner, "hierarchical_lod");
        assert_eq!(bands[1].max_meters, lod::COVERAGE_RADIUS_METERS);
        assert!(bands[1].surface_only);
        assert_eq!(bands[2].owner, "legacy_far_mesh");
        assert!(bands[2].max_meters > 0.0);
        assert_eq!(bands[3].owner, "curvature");
    }

    #[test]
    fn input_signature_is_deterministic_and_sensitive() {
        let base = input_signature(11, "lush", 8);
        assert_eq!(base, input_signature(11, "lush", 8));
        assert_ne!(base, input_signature(12, "lush", 8));
        assert_ne!(base, input_signature(11, "frozen", 8));
        assert_ne!(base, input_signature(11, "lush", 9));
    }

    #[test]
    fn default_diagnostics_declare_surface_only_lod() {
        let diagnostics = TerrainDiagnostics::default();
        assert!(diagnostics.lod.surface_only);
        assert!(diagnostics.notes.is_empty());
    }
}
