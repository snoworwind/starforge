//! G01 stable structure plans: frozen legacy layouts, labeled random streams,
//! real AABB bounds, budget validation and canonical fingerprints.
//!
//! The legacy cell-hash generator stays the source of *layout* truth for
//! `generator_version = 1`; this module wraps each result in a
//! [`StructurePlan`] that carries a stable id, the real footprint, the module
//! list and one seed per random stream. New decoration, furniture, damage and
//! loot code must draw from their own stream so adding content cannot move a
//! house or reroll a chest.
//!
//! See `docs/art-overhaul/03-WORLD-AND-PROCEDURAL-BUILDINGS.md` (3.7) and
//! `docs/art-overhaul/05-EXECUTION-BACKLOG.md` (G01).

use serde::Serialize;

use crate::rng::{Rng, hash2_state};

/// Version of the serialized field set (`metrics`/future save payloads).
pub const STRUCTURE_PLAN_SCHEMA_VERSION: u16 = 1;
/// The frozen legacy generator shipped before G01.
pub const GENERATOR_VERSION_LEGACY: u16 = 1;
/// Salt of the legacy layout stream. Must never change for version 1.
pub const LEGACY_LAYOUT_SALT: u32 = 0x0057_A7C7;
/// Conservative query margin for legacy plans (largest legacy footprint).
pub const LEGACY_QUERY_MARGIN: i32 = 24;

/// Search margin per generator version. Large v2+ settlements must add a row
/// here (and a golden query test) instead of relying on the old `±1 cell`
/// assumption (R077).
pub const GENERATOR_SEARCH_MARGINS: &[(u16, i32)] =
    &[(GENERATOR_VERSION_LEGACY, LEGACY_QUERY_MARGIN)];

pub fn plan_search_margin(generator_version: u16) -> i32 {
    GENERATOR_SEARCH_MARGINS
        .iter()
        .find(|(version, _)| *version == generator_version)
        .map(|(_, margin)| *margin)
        .unwrap_or(LEGACY_QUERY_MARGIN)
}

/// Frozen v1 payload; `stamp_structure` keeps consuming this exactly as before.
#[derive(Clone, Debug, PartialEq)]
pub enum LegacyLayout {
    Village {
        x: i32,
        z: i32,
        h: i32,
        huts: Vec<(i32, i32, i32)>,
    },
    Ruin {
        x: i32,
        z: i32,
        kind: u32,
        h: i32,
        seed: u32,
    },
}

impl LegacyLayout {
    pub fn kind(&self) -> StructureKind {
        match self {
            Self::Village { .. } => StructureKind::Village,
            Self::Ruin { .. } => StructureKind::Ruin,
        }
    }

    pub fn center(&self) -> (i32, i32) {
        match self {
            Self::Village { x, z, .. } | Self::Ruin { x, z, .. } => (*x, *z),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum StructureKind {
    Village,
    Ruin,
}

impl StructureKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Village => "village",
            Self::Ruin => "ruin",
        }
    }
}

/// Stable id of one plan: world seed + generator version + region cell +
/// local index + kind. Load order, HashMap iteration and query order never
/// enter the id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StructureId {
    pub world_seed: u32,
    pub generator_version: u16,
    pub cell_x: i32,
    pub cell_z: i32,
    pub local_index: u16,
    pub kind: StructureKind,
}

impl StructureId {
    pub fn stable_hash(self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.u32(self.world_seed);
        hash.u16(self.generator_version);
        hash.i32(self.cell_x);
        hash.i32(self.cell_z);
        hash.u16(self.local_index);
        hash.byte(match self.kind {
            StructureKind::Village => 1,
            StructureKind::Ruin => 2,
        });
        hash.finish()
    }
}

/// One labeled random stream per responsibility. Streams are derived from the
/// stable id, not from call order, so consuming more of one stream cannot
/// shift another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum StructureStream {
    Site,
    Layout,
    Roof,
    Furniture,
    Damage,
    Decor,
    Loot,
    Terrain,
}

pub const STRUCTURE_STREAMS: [StructureStream; 8] = [
    StructureStream::Site,
    StructureStream::Layout,
    StructureStream::Roof,
    StructureStream::Furniture,
    StructureStream::Damage,
    StructureStream::Decor,
    StructureStream::Loot,
    StructureStream::Terrain,
];

impl StructureStream {
    pub fn tag(self) -> u32 {
        match self {
            Self::Site => 0x0000_517E,
            // The legacy cell RNG salt; version 1 layout is frozen to it.
            Self::Layout => LEGACY_LAYOUT_SALT,
            Self::Roof => 0x0000_200F,
            Self::Furniture => 0x0000_F0B1,
            Self::Damage => 0x0000_D4A6,
            Self::Decor => 0x0000_DEC0,
            Self::Loot => 0x0000_1007,
            Self::Terrain => 0x0000_7E44,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Layout => "layout",
            Self::Roof => "roof",
            Self::Furniture => "furniture",
            Self::Damage => "damage",
            Self::Decor => "decor",
            Self::Loot => "loot",
            Self::Terrain => "terrain",
        }
    }

    fn index(self) -> usize {
        STRUCTURE_STREAMS
            .iter()
            .position(|stream| *stream == self)
            .unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StreamSeeds {
    pub seeds: [u32; 8],
}

impl StreamSeeds {
    /// Derive every stream for one plan. `generator_version = 1` and
    /// `local_index = 0` reproduce the legacy layout RNG exactly, so wrapping
    /// the old generator does not change a single block.
    pub fn for_cell(
        world_seed: u32,
        generator_version: u16,
        cell_x: i32,
        cell_z: i32,
        local_index: u16,
    ) -> Self {
        let version_tag = if generator_version == GENERATOR_VERSION_LEGACY {
            0
        } else {
            (generator_version as u32).wrapping_mul(0x85EB_CA6B)
        };
        let index_tag = (local_index as u32).wrapping_mul(0x9E37_79B9);
        let mut seeds = [0u32; 8];
        for stream in STRUCTURE_STREAMS {
            let salt = stream.tag() ^ version_tag ^ index_tag;
            seeds[stream.index()] = hash2_state(cell_x, cell_z, salt, world_seed);
        }
        Self { seeds }
    }

    pub fn seed(&self, stream: StructureStream) -> u32 {
        self.seeds[stream.index()]
    }

    pub fn rng(&self, stream: StructureStream) -> Rng {
        Rng::new(self.seed(stream))
    }

    pub fn with_seed(mut self, stream: StructureStream, seed: u32) -> Self {
        self.seeds[stream.index()] = seed;
        self
    }
}

/// Inclusive world-space footprint. Height is validated separately against
/// `WORLD_H` by the plan validator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PlanAabb {
    pub min_x: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_z: i32,
}

impl PlanAabb {
    pub fn new(x0: i32, z0: i32, x1: i32, z1: i32) -> Self {
        Self {
            min_x: x0.min(x1),
            min_z: z0.min(z1),
            max_x: x0.max(x1),
            max_z: z0.max(z1),
        }
    }

    /// Real footprint of a legacy layout. Villages cover every hut plus its
    /// 5x5 wall ring; ruins cover the full +-10 stamp range.
    pub fn from_legacy(layout: &LegacyLayout) -> Self {
        let mut aabb = match layout {
            LegacyLayout::Village { x, z, .. } => Self::new(*x, *z, *x, *z),
            LegacyLayout::Ruin { x, z, .. } => Self::new(x - 10, z - 10, x + 10, z + 10),
        };
        if let LegacyLayout::Village { huts, .. } = layout {
            for &(hx, hz, _) in huts {
                aabb.min_x = aabb.min_x.min(hx - 2);
                aabb.max_x = aabb.max_x.max(hx + 2);
                aabb.min_z = aabb.min_z.min(hz - 2);
                aabb.max_z = aabb.max_z.max(hz + 2);
            }
        }
        aabb
    }

    pub fn width(self) -> i32 {
        self.max_x - self.min_x + 1
    }

    pub fn height(self) -> i32 {
        self.max_z - self.min_z + 1
    }

    pub fn radius(self) -> i32 {
        (self.width().max(self.height()) + 1) / 2
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.min_x <= other.max_x
            && other.min_x <= self.max_x
            && self.min_z <= other.max_z
            && other.min_z <= self.max_z
    }

    pub fn contains_xz(self, x: i32, z: i32) -> bool {
        (self.min_x..=self.max_x).contains(&x) && (self.min_z..=self.max_z).contains(&z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PlanModule {
    pub kind: &'static str,
    pub x: i32,
    pub z: i32,
    pub h: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PlanAnchor {
    pub kind: &'static str,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PlanStats {
    pub site_candidates: u32,
    pub rejected_sites: u32,
    pub recursion_depth: u32,
    pub modules: u32,
    pub estimated_voxel_writes: u32,
}

/// Hard budgets for one plan. `LEGACY_LIMITS` describes the frozen v1
/// generator; new versions get their own row so a v2 settlement cannot silently
/// exceed the v1 query margin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PlanLimits {
    pub max_footprint_radius: i32,
    pub max_vertical_extent: i32,
    pub max_modules: u32,
    pub max_voxel_writes: u32,
    pub max_recursion_depth: u32,
    pub max_site_candidates: u32,
}

pub const LEGACY_LIMITS: PlanLimits = PlanLimits {
    max_footprint_radius: LEGACY_QUERY_MARGIN,
    max_vertical_extent: 32,
    max_modules: 16,
    max_voxel_writes: 6_000,
    max_recursion_depth: 2,
    max_site_candidates: 70,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum PlanIssue {
    FootprintTooLarge,
    VerticalExtentTooLarge,
    TooManyModules,
    VoxelBudget,
    RecursionBudget,
    CandidateBudget,
    EmptyLayout,
}

impl PlanIssue {
    pub fn key(self) -> &'static str {
        match self {
            Self::FootprintTooLarge => "footprint_too_large",
            Self::VerticalExtentTooLarge => "vertical_extent_too_large",
            Self::TooManyModules => "too_many_modules",
            Self::VoxelBudget => "voxel_budget",
            Self::RecursionBudget => "recursion_budget",
            Self::CandidateBudget => "candidate_budget",
            Self::EmptyLayout => "empty_layout",
        }
    }
}

pub fn ruin_module_kind(kind: u32) -> &'static str {
    match kind {
        0 => "ring",
        1 => "tower",
        _ => "walled_court",
    }
}

pub fn estimate_voxel_writes(layout: &LegacyLayout) -> u32 {
    match layout {
        LegacyLayout::Village { huts, .. } => 3 + huts.len() as u32 * 260,
        // Tower footprint is small but up to 13 blocks tall; ring and walled
        // court are broad and low.
        LegacyLayout::Ruin { kind: 1, .. } => 64,
        LegacyLayout::Ruin { .. } => 420,
    }
}

fn modules_for(layout: &LegacyLayout) -> Vec<PlanModule> {
    match layout {
        LegacyLayout::Village { huts, .. } => huts
            .iter()
            .map(|&(x, z, h)| PlanModule {
                kind: "hut",
                x,
                z,
                h,
            })
            .collect(),
        LegacyLayout::Ruin { x, z, h, kind, .. } => vec![PlanModule {
            kind: ruin_module_kind(*kind),
            x: *x,
            z: *z,
            h: *h,
        }],
    }
}

fn anchors_for(layout: &LegacyLayout) -> Vec<PlanAnchor> {
    match layout {
        LegacyLayout::Village { x, z, h, .. } => vec![PlanAnchor {
            kind: "beacon",
            x: *x,
            y: h + 3,
            z: *z,
        }],
        LegacyLayout::Ruin {
            x, z, h, kind: 1, ..
        } => vec![PlanAnchor {
            kind: "lamp",
            x: *x,
            y: h + 13,
            z: *z,
        }],
        LegacyLayout::Ruin {
            x, z, h, kind: 0, ..
        } => vec![PlanAnchor {
            kind: "lamp",
            x: *x,
            y: h + 2,
            z: *z,
        }],
        LegacyLayout::Ruin { .. } => Vec::new(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructurePlan {
    pub id: StructureId,
    pub aabb: PlanAabb,
    pub layout: LegacyLayout,
    pub modules: Vec<PlanModule>,
    pub anchors: Vec<PlanAnchor>,
    pub streams: StreamSeeds,
    pub stats: PlanStats,
}

impl StructurePlan {
    pub fn from_legacy(
        world_seed: u32,
        generator_version: u16,
        cell: (i32, i32),
        layout: LegacyLayout,
        streams: StreamSeeds,
        stats: PlanStats,
    ) -> Self {
        let id = StructureId {
            world_seed,
            generator_version,
            cell_x: cell.0,
            cell_z: cell.1,
            local_index: 0,
            kind: layout.kind(),
        };
        let aabb = PlanAabb::from_legacy(&layout);
        let modules = modules_for(&layout);
        let anchors = anchors_for(&layout);
        let mut stats = stats;
        stats.modules = modules.len() as u32;
        stats.estimated_voxel_writes = estimate_voxel_writes(&layout);
        Self {
            id,
            aabb,
            layout,
            modules,
            anchors,
            streams,
            stats,
        }
    }

    pub fn kind(&self) -> StructureKind {
        self.id.kind
    }

    /// Margin a query must use so every writer of this plan is found. Never
    /// smaller than the version's frozen legacy margin.
    pub fn query_margin(&self) -> i32 {
        self.aabb.radius().max(LEGACY_QUERY_MARGIN)
    }

    /// Canonical layout fingerprint: excludes decor/loot streams, so adding
    /// decoration or loot cannot change it.
    pub fn layout_fingerprint(&self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.u64(self.id.stable_hash());
        hash.i32(self.aabb.min_x);
        hash.i32(self.aabb.min_z);
        hash.i32(self.aabb.max_x);
        hash.i32(self.aabb.max_z);
        hash_legacy(&mut hash, &self.layout);
        hash.u32(self.modules.len() as u32);
        for module in &self.modules {
            hash.str(module.kind);
            hash.i32(module.x);
            hash.i32(module.z);
            hash.i32(module.h);
        }
        for anchor in &self.anchors {
            hash.str(anchor.kind);
            hash.i32(anchor.x);
            hash.i32(anchor.y);
            hash.i32(anchor.z);
        }
        for stream in [
            StructureStream::Site,
            StructureStream::Layout,
            StructureStream::Roof,
            StructureStream::Furniture,
            StructureStream::Damage,
            StructureStream::Terrain,
        ] {
            hash.u32(self.streams.seed(stream));
        }
        hash.finish()
    }

    /// Loot is its own responsibility: it must be stable per plan, but decor
    /// changes must not move it and layout changes must not reroll it.
    pub fn loot_fingerprint(&self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.u64(self.id.stable_hash());
        hash.u32(self.streams.seed(StructureStream::Loot));
        hash.u32(self.anchors.len() as u32);
        hash.finish()
    }

    /// Everything, including decor: used for "same seed/cell -> same plan".
    pub fn plan_fingerprint(&self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.u64(self.layout_fingerprint());
        hash.u64(self.loot_fingerprint());
        for stream in [StructureStream::Decor, StructureStream::Loot] {
            hash.u32(self.streams.seed(stream));
        }
        hash.u32(self.stats.site_candidates);
        hash.u32(self.stats.rejected_sites);
        hash.u32(self.stats.recursion_depth);
        hash.finish()
    }

    pub fn validate(&self, limits: &PlanLimits) -> Result<(), Vec<PlanIssue>> {
        let mut issues = Vec::new();
        if self.aabb.radius() > limits.max_footprint_radius {
            issues.push(PlanIssue::FootprintTooLarge);
        }
        let top = self
            .modules
            .iter()
            .map(|module| module.h)
            .chain(self.anchors.iter().map(|anchor| anchor.y))
            .max()
            .unwrap_or(0);
        let bottom = self
            .modules
            .iter()
            .map(|module| module.h)
            .chain(self.anchors.iter().map(|anchor| anchor.y))
            .min()
            .unwrap_or(0);
        if top.saturating_sub(bottom) > limits.max_vertical_extent {
            issues.push(PlanIssue::VerticalExtentTooLarge);
        }
        if self.modules.len() as u32 > limits.max_modules {
            issues.push(PlanIssue::TooManyModules);
        }
        if self.stats.estimated_voxel_writes > limits.max_voxel_writes {
            issues.push(PlanIssue::VoxelBudget);
        }
        if self.stats.recursion_depth > limits.max_recursion_depth {
            issues.push(PlanIssue::RecursionBudget);
        }
        if self.stats.site_candidates > limits.max_site_candidates {
            issues.push(PlanIssue::CandidateBudget);
        }
        if self.modules.is_empty() {
            issues.push(PlanIssue::EmptyLayout);
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }
}

fn hash_legacy(hash: &mut Fnv1a, layout: &LegacyLayout) {
    match layout {
        LegacyLayout::Village { x, z, h, huts } => {
            hash.byte(1);
            hash.i32(*x);
            hash.i32(*z);
            hash.i32(*h);
            hash.u32(huts.len() as u32);
            for &(hx, hz, hh) in huts {
                hash.i32(hx);
                hash.i32(hz);
                hash.i32(hh);
            }
        }
        LegacyLayout::Ruin {
            x,
            z,
            kind,
            h,
            seed,
        } => {
            hash.byte(2);
            hash.i32(*x);
            hash.i32(*z);
            hash.u32(*kind);
            hash.i32(*h);
            hash.u32(*seed);
        }
    }
}

/// FNV-1a over a canonical little-endian field encoding. Private on purpose:
/// callers use the `*_fingerprint` functions so field order cannot drift.
pub(crate) struct Fnv1a(u64);

impl Fnv1a {
    pub(crate) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    pub(crate) fn byte(&mut self, value: u8) {
        self.0 ^= value as u64;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    pub(crate) fn u16(&mut self, value: u16) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    pub(crate) fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    pub(crate) fn i32(&mut self, value: i32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    pub(crate) fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    pub(crate) fn str(&mut self, value: &str) {
        self.u32(value.len() as u32);
        for byte in value.bytes() {
            self.byte(byte);
        }
    }

    pub(crate) fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::hash2;

    fn cell_seeds(cell: (i32, i32)) -> StreamSeeds {
        StreamSeeds::for_cell(11, GENERATOR_VERSION_LEGACY, cell.0, cell.1, 0)
    }

    fn sample_village() -> LegacyLayout {
        LegacyLayout::Village {
            x: 100,
            z: -50,
            h: 40,
            huts: vec![(96, -52, 40), (104, -47, 41), (99, -45, 39)],
        }
    }

    fn sample_plan(layout: LegacyLayout) -> StructurePlan {
        let mut rnd = cell_seeds((0, 0)).rng(StructureStream::Layout);
        let stats = PlanStats {
            site_candidates: 3 + (rnd.next() * 10.0) as u32,
            rejected_sites: 1,
            recursion_depth: 0,
            modules: 0,
            estimated_voxel_writes: 0,
        };
        StructurePlan::from_legacy(
            11,
            GENERATOR_VERSION_LEGACY,
            (0, 0),
            layout,
            cell_seeds((0, 0)),
            stats,
        )
    }

    #[test]
    fn legacy_layout_stream_matches_the_frozen_hash2_rng() {
        // Wrapping the old generator is only safe if the layout stream draws
        // the exact sequence the old `hash2(ccx, ccz, 0x57A7C7, seed)` did.
        for cell in [(0, 0), (-1, -1), (-2, 3), (100, 50)] {
            let seeds = StreamSeeds::for_cell(12345, GENERATOR_VERSION_LEGACY, cell.0, cell.1, 0);
            let mut stream = seeds.rng(StructureStream::Layout);
            let mut legacy = hash2(cell.0, cell.1, LEGACY_LAYOUT_SALT, 12345);
            for _ in 0..16 {
                assert_eq!(stream.next(), legacy.next());
            }
        }
    }

    #[test]
    fn decor_changes_do_not_move_layout_and_loot_is_independent() {
        let plan = sample_plan(sample_village());
        let layout = plan.layout_fingerprint();
        let loot = plan.loot_fingerprint();

        let decorated = StructurePlan {
            streams: plan.streams.with_seed(StructureStream::Decor, 0xDEAD_BEEF),
            ..plan.clone()
        };
        assert_eq!(decorated.layout_fingerprint(), layout);
        assert_eq!(decorated.loot_fingerprint(), loot);

        let relooted = StructurePlan {
            streams: plan.streams.with_seed(StructureStream::Loot, 0x1234_5678),
            ..plan.clone()
        };
        assert_eq!(relooted.layout_fingerprint(), layout);
        assert_ne!(relooted.loot_fingerprint(), loot);

        let moved_stream = StructurePlan {
            streams: plan.streams.with_seed(StructureStream::Layout, 0x0BAD_F00D),
            ..plan.clone()
        };
        assert_ne!(moved_stream.layout_fingerprint(), layout);
    }

    #[test]
    fn plan_fingerprint_is_stable_and_seed_sensitive() {
        let a = sample_plan(sample_village());
        let b = sample_plan(sample_village());
        assert_eq!(a.plan_fingerprint(), b.plan_fingerprint());

        let other_cell = StreamSeeds::for_cell(11, GENERATOR_VERSION_LEGACY, 1, -1, 0);
        let c = StructurePlan::from_legacy(
            11,
            GENERATOR_VERSION_LEGACY,
            (1, -1),
            sample_village(),
            other_cell,
            PlanStats::default(),
        );
        assert_ne!(a.plan_fingerprint(), c.plan_fingerprint());
        assert_ne!(
            a.id.stable_hash(),
            c.id.stable_hash(),
            "ids must include the cell"
        );
    }

    #[test]
    fn aabb_covers_every_legacy_write_footprint() {
        let village = PlanAabb::from_legacy(&sample_village());
        for (hx, hz) in [(96i32, -52i32), (104, -47), (99, -45)] {
            for dx in -2..=2 {
                for dz in -2..=2 {
                    assert!(
                        village.contains_xz(hx + dx, hz + dz),
                        "hut block ({hx},{hz}) + ({dx},{dz}) outside {village:?}"
                    );
                }
            }
        }
        assert!(village.contains_xz(100, -50), "beacon center");
        let ruin = PlanAabb::from_legacy(&LegacyLayout::Ruin {
            x: -7,
            z: 13,
            kind: 2,
            h: 30,
            seed: 5,
        });
        assert!(!ruin.contains_xz(-18, 13));
        assert!(ruin.contains_xz(-17, 13));
        assert!(ruin.contains_xz(3, 23));
    }

    #[test]
    fn legacy_plans_pass_their_own_limits_and_bad_plans_are_reported() {
        let plan = sample_plan(sample_village());
        assert_eq!(plan.validate(&LEGACY_LIMITS), Ok(()));

        let oversized = StructurePlan {
            aabb: PlanAabb::new(-1000, 0, 1000, 0),
            ..sample_plan(sample_village())
        };
        assert!(
            oversized
                .validate(&LEGACY_LIMITS)
                .unwrap_err()
                .contains(&PlanIssue::FootprintTooLarge)
        );

        let empty = StructurePlan {
            modules: Vec::new(),
            layout: LegacyLayout::Ruin {
                x: 0,
                z: 0,
                kind: 2,
                h: 0,
                seed: 0,
            },
            ..sample_plan(sample_village())
        };
        assert!(
            empty
                .validate(&LEGACY_LIMITS)
                .unwrap_err()
                .contains(&PlanIssue::EmptyLayout)
        );

        let over_budget = StructurePlan {
            stats: PlanStats {
                site_candidates: 1_000,
                ..plan.stats
            },
            ..sample_plan(sample_village())
        };
        assert!(
            over_budget
                .validate(&LEGACY_LIMITS)
                .unwrap_err()
                .contains(&PlanIssue::CandidateBudget)
        );
    }

    #[test]
    fn module_and_anchor_lists_describe_the_layout() {
        let village = sample_plan(sample_village());
        assert_eq!(village.modules.len(), 3);
        assert!(village.modules.iter().all(|module| module.kind == "hut"));
        assert_eq!(village.anchors.len(), 1);
        assert_eq!(village.anchors[0].kind, "beacon");
        assert_eq!(village.anchors[0].y, 40 + 3);

        let tower = sample_plan(LegacyLayout::Ruin {
            x: 5,
            z: 5,
            kind: 1,
            h: 33,
            seed: 9,
        });
        assert_eq!(tower.modules[0].kind, "tower");
        assert_eq!(tower.anchors[0].y, 33 + 13);
    }

    #[test]
    fn stream_seeds_differ_per_stream_and_include_version_and_index() {
        let seeds = cell_seeds((3, 4));
        let mut seen = Vec::new();
        for stream in STRUCTURE_STREAMS {
            let seed = seeds.seed(stream);
            assert!(!seen.contains(&seed), "stream {stream:?} collides");
            seen.push(seed);
        }
        let v2 = StreamSeeds::for_cell(11, 2, 3, 4, 0);
        assert_ne!(
            seeds.seed(StructureStream::Layout),
            v2.seed(StructureStream::Layout)
        );
        let indexed = StreamSeeds::for_cell(11, GENERATOR_VERSION_LEGACY, 3, 4, 2);
        assert_ne!(
            seeds.seed(StructureStream::Decor),
            indexed.seed(StructureStream::Decor)
        );
    }
}
