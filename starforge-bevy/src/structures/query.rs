//! G01 plan query geometry and bounded plan cache.
//!
//! The legacy query expanded `±1 cell` because one cell is much larger than a
//! structure. That assumption breaks for larger plans, so the query range is
//! now computed from the rectangle plus a versioned search margin, and the
//! result is filtered by each plan's real AABB. The cache replaces the old
//! unbounded `HashMap`, keeping determinism (eviction only regenerates the
//! same plan) while bounding memory for infinite exploration.
//!
//! See `docs/art-overhaul/03-WORLD-AND-PROCEDURAL-BUILDINGS.md` (3.7) and
//! `docs/art-overhaul/05-EXECUTION-BACKLOG.md` (G01).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::structures::plan::StructurePlan;

/// Default number of cached region cells. Cells hold at most one plan (v1),
/// so 4096 entries cover a huge explored area without unbounded growth.
pub const DEFAULT_PLAN_CACHE_CAPACITY: usize = 4096;

/// Inclusive cell range that can contain writers for a query rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CellRange {
    pub cx0: i32,
    pub cx1: i32,
    pub cz0: i32,
    pub cz1: i32,
}

impl CellRange {
    pub fn len(self) -> u64 {
        let width = (self.cx1 as i64 - self.cx0 as i64 + 1).max(0) as u64;
        let depth = (self.cz1 as i64 - self.cz0 as i64 + 1).max(0) as u64;
        width * depth
    }

    pub fn contains(self, cx: i32, cz: i32) -> bool {
        (self.cx0..=self.cx1).contains(&cx) && (self.cz0..=self.cz1).contains(&cz)
    }

    pub fn cells(self) -> impl Iterator<Item = (i32, i32)> {
        (self.cx0..=self.cx1).flat_map(move |cx| (self.cz0..=self.cz1).map(move |cz| (cx, cz)))
    }
}

/// Cell range for a world rectangle plus search margin. Uses Euclidean
/// division so negative coordinates behave exactly like positive ones.
pub fn cell_range_for_rect(
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
    margin: i32,
    cell_x: i32,
    cell_z: i32,
) -> CellRange {
    let cell_x = cell_x.max(1);
    let cell_z = cell_z.max(1);
    let margin = margin.max(0);
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (z0, z1) = (z0.min(z1), z0.max(z1));
    CellRange {
        cx0: x0.saturating_sub(margin).div_euclid(cell_x),
        cx1: x1.saturating_add(margin).div_euclid(cell_x),
        cz0: z0.saturating_sub(margin).div_euclid(cell_z),
        cz1: z1.saturating_add(margin).div_euclid(cell_z),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PlanCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub len: usize,
    pub capacity: usize,
}

struct PlanCacheInner {
    entries: HashMap<(i32, i32), Option<Arc<StructurePlan>>>,
    order: VecDeque<(i32, i32)>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

/// Bounded FIFO cache of generated plans (including deterministic "no
/// structure" results). A `Mutex` keeps `WorldGen` usable through `&self`
/// while multiple queries run.
pub struct PlanCache {
    capacity: usize,
    inner: Mutex<PlanCacheInner>,
}

impl PlanCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            inner: Mutex::new(PlanCacheInner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                hits: 0,
                misses: 0,
                evictions: 0,
            }),
        }
    }

    /// Return the cached plan for `cell`, or generate it. Generation happens
    /// outside the lock; if another thread produced the same cell meanwhile,
    /// the existing entry wins and the duplicate result is dropped.
    pub fn get_or_generate<F>(&self, cell: (i32, i32), generate: F) -> Option<Arc<StructurePlan>>
    where
        F: FnOnce() -> Option<StructurePlan>,
    {
        {
            let mut inner = self.inner.lock().expect("plan cache poisoned");
            if let Some(existing) = inner.entries.get(&cell) {
                let existing = existing.clone();
                inner.hits += 1;
                return existing;
            }
            inner.misses += 1;
        }

        let produced = generate().map(Arc::new);
        let mut inner = self.inner.lock().expect("plan cache poisoned");
        if let Some(existing) = inner.entries.get(&cell) {
            return existing.clone();
        }
        while inner.entries.len() >= self.capacity {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            if inner.entries.remove(&oldest).is_some() {
                inner.evictions += 1;
            }
        }
        inner.entries.insert(cell, produced.clone());
        inner.order.push_back(cell);
        produced
    }

    pub fn stats(&self) -> PlanCacheStats {
        let inner = self.inner.lock().expect("plan cache poisoned");
        PlanCacheStats {
            hits: inner.hits,
            misses: inner.misses,
            evictions: inner.evictions,
            len: inner.entries.len(),
            capacity: self.capacity,
        }
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("plan cache poisoned")
            .entries
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().expect("plan cache poisoned");
        inner.entries.clear();
        inner.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structures::plan::{
        GENERATOR_VERSION_LEGACY, LegacyLayout, PlanStats, StreamSeeds, StructurePlan,
    };

    fn plan(x: i32, z: i32) -> StructurePlan {
        StructurePlan::from_legacy(
            11,
            GENERATOR_VERSION_LEGACY,
            (x, z),
            LegacyLayout::Ruin {
                x: x * 10,
                z: z * 10,
                kind: 2,
                h: 20,
                seed: 1,
            },
            StreamSeeds::for_cell(11, GENERATOR_VERSION_LEGACY, x, z, 0),
            PlanStats::default(),
        )
    }

    #[test]
    fn cell_range_handles_negative_and_boundary_coordinates() {
        let range = cell_range_for_rect(0, 0, 16, 16, 24, 640, 320);
        assert_eq!(
            range,
            CellRange {
                cx0: -1,
                cx1: 0,
                cz0: -1,
                cz1: 0
            }
        );
        assert_eq!(range.len(), 4);

        let negative = cell_range_for_rect(-700, -400, -650, -330, 24, 640, 320);
        assert_eq!(negative.cx0, -2);
        assert_eq!(negative.cx1, -1);
        assert_eq!(negative.cz0, -2);
        assert_eq!(negative.cz1, -1);

        let huge = cell_range_for_rect(i32::MIN + 10, 0, i32::MAX - 10, 0, 24, 640, 320);
        assert!(huge.cx0 < 0 && huge.cx1 > 0, "saturating arithmetic");
        assert!(huge.cells().count() > 0);

        let single = cell_range_for_rect(700, 400, 700, 400, 24, 640, 320);
        assert_eq!(single.cx0, 1);
        assert_eq!(single.cx1, 1);
        assert_eq!(single.cz0, 1);
        assert_eq!(single.cz1, 1);
    }

    fn load(
        cache: &PlanCache,
        generated: &mut u32,
        cell: (i32, i32),
    ) -> Option<Arc<StructurePlan>> {
        cache.get_or_generate(cell, || {
            *generated += 1;
            Some(plan(cell.0, cell.1))
        })
    }

    #[test]
    fn cache_hits_misses_and_evicts_without_changing_content() {
        let cache = PlanCache::new(2);
        let mut generated = 0;
        let first = load(&cache, &mut generated, (0, 0)).expect("plan");
        assert_eq!(first.id.cell_x, 0);
        assert_eq!(first.id.cell_z, 0);
        let again = load(&cache, &mut generated, (0, 0)).expect("cached plan");
        assert_eq!(again.layout_fingerprint(), first.layout_fingerprint());
        assert_eq!(generated, 1, "second access must be a cache hit");

        load(&cache, &mut generated, (1, 0));
        load(&cache, &mut generated, (2, 0));
        let stats = cache.stats();
        assert_eq!(stats.len, 2, "capacity bound");
        assert_eq!(stats.evictions, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 3);

        // An evicted cell regenerates the identical plan.
        let regenerated = load(&cache, &mut generated, (0, 0)).expect("plan");
        assert_eq!(regenerated.layout_fingerprint(), first.layout_fingerprint());
        assert!(cache.stats().evictions >= 2);
    }

    #[test]
    fn cache_stores_empty_cells_too() {
        let cache = PlanCache::new(4);
        assert!(cache.get_or_generate((9, 9), || None).is_none());
        let mut calls = 0;
        assert!(
            cache
                .get_or_generate((9, 9), || {
                    calls += 1;
                    None
                })
                .is_none()
        );
        assert_eq!(calls, 0, "empty cell result is cached");
        assert_eq!(cache.len(), 1);
    }
}
