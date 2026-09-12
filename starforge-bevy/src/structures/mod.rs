//! G01 structures: stable plans, labeled random streams, real AABB queries and
//! a bounded plan cache.
//!
//! The legacy cell generator is frozen as `generator_version = 1`:
//! `world.rs` still produces the identical layouts, but every plan now carries
//! a stable id, its true footprint, module/anchor lists and one seed per
//! responsibility stream. `stamp_structure` is intentionally unchanged.
//!
//! See `docs/art-overhaul/03-WORLD-AND-PROCEDURAL-BUILDINGS.md` (3.7).

mod plan;
mod query;

#[allow(unused_imports)]
pub use plan::{
    GENERATOR_SEARCH_MARGINS, GENERATOR_VERSION_LEGACY, LEGACY_LIMITS, LEGACY_QUERY_MARGIN,
    LegacyLayout, PlanAabb, PlanAnchor, PlanIssue, PlanLimits, PlanModule, PlanStats,
    STRUCTURE_PLAN_SCHEMA_VERSION, STRUCTURE_STREAMS, StreamSeeds, StructureId, StructureKind,
    StructurePlan, StructureStream, estimate_voxel_writes, plan_search_margin, ruin_module_kind,
};
#[allow(unused_imports)]
pub use query::{
    CellRange, DEFAULT_PLAN_CACHE_CAPACITY, PlanCache, PlanCacheStats, cell_range_for_rect,
};
