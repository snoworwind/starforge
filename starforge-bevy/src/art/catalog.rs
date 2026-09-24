//! B02 surface-material catalog: stable IDs, face roles and variant rules.
//!
//! The legacy atlas (`textures::Atlas`) derived every tile layer from the
//! painter registration order in `Atlas::build()`, and the meshing/icon/far
//! reads forwarded the raw tile name. B02 freezes that mapping into a catalog
//! with explicit, order-independent [`SurfaceMaterialId`]s:
//!
//! - `data::Block::id` stays the u8 chunk/save ABI and is never used as a
//!   texture layer index;
//! - every block face resolves to a [`SurfaceMaterialId`] through
//!   [`face_material`]/[`cross_material`];
//! - the frozen `block key + face role` mapping is fingerprinted
//!   ([`face_fingerprint`]) so atlas re-ordering or a future texture
//!   array/layer layout cannot silently change a save, recipe or business key
//!   (risk R020);
//! - face roles and the deterministic variant rule are data, not call-site
//!   conventions.
//!
//! `--art-audit` emits coverage and both fingerprints; the full registry is
//! written to `target/art-audit/material-catalog.json` (the B03 input).

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::Serialize;

/// Bumped when a change alters the meaning of a material ID, family or the
/// face-role mapping. Consumers compare it before mixing cached artifacts.
pub const MATERIAL_CATALOG_VERSION: u32 = 1;

/// Fallback used by the legacy code path when a tile key is not registered.
/// The catalog audit turns any real occurrence into a hard failure.
pub const FALLBACK_TILE: &str = "grass_top";

pub const FACE_COUNT: usize = 6;

/// Stable handle for one surface material (a texture layer + its PBR/tag
/// contract). Explicit values: reordering the registry or the atlas never
/// changes an ID, and IDs are never reused for a different key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SurfaceMaterialId(pub u16);

impl SurfaceMaterialId {
    /// Not a material: unknown keys, empty cells and diagnostics.
    pub const UNKNOWN: Self = Self(0);

    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u16 {
        self.0
    }

    pub const fn slot(self) -> usize {
        self.0 as usize
    }

    pub const fn is_known(self) -> bool {
        self.0 >= 1 && (self.0 as usize) <= SURFACE_MATERIALS.len()
    }
}

/// Families from `01-ART-DIRECTION-AND-ASSETS.md` §1.4. Roughness, metallic
/// policy and variant budgets hang off the family so a material row only has
/// to declare what is genuinely per-material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialFamily {
    /// 土/砂/盐
    Soil,
    /// 岩石/玄武岩
    Rock,
    /// 草/苔藓
    Grass,
    /// 木/板材
    Wood,
    /// 涂漆设备
    Painted,
    /// 裸金属
    BareMetal,
    Glass,
    Water,
    Ice,
    Snow,
    Crystal,
    /// 熔岩/能量源
    Energy,
    /// 叶/菌/膜
    Foliage,
}

/// Metallic strategy, not a constant value: painted equipment is bare metal
/// under worn edges, which a single scalar cannot express.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetallicPolicy {
    /// Non-metal surface; keep metallic at 0.
    Zero,
    /// Metal-dominant (bare metal, rails, cable armor).
    High,
    /// Paint layer at 0 with an almost-1 metallic wear mask.
    PaintOverMetal,
}

impl MaterialFamily {
    pub const ALL: [Self; 13] = [
        Self::Soil,
        Self::Rock,
        Self::Grass,
        Self::Wood,
        Self::Painted,
        Self::BareMetal,
        Self::Glass,
        Self::Water,
        Self::Ice,
        Self::Snow,
        Self::Crystal,
        Self::Energy,
        Self::Foliage,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Soil => "soil",
            Self::Rock => "rock",
            Self::Grass => "grass",
            Self::Wood => "wood",
            Self::Painted => "painted",
            Self::BareMetal => "bare_metal",
            Self::Glass => "glass",
            Self::Water => "water",
            Self::Ice => "ice",
            Self::Snow => "snow",
            Self::Crystal => "crystal",
            Self::Energy => "energy",
            Self::Foliage => "foliage",
        }
    }

    /// Roughness review range from the 01 §1.4 table (start point, not a
    /// physical guarantee).
    pub const fn roughness_range(self) -> (f32, f32) {
        match self {
            Self::Soil => (0.75, 1.0),
            Self::Rock => (0.60, 0.95),
            Self::Grass => (0.75, 1.0),
            Self::Wood => (0.50, 0.85),
            Self::Painted => (0.30, 0.65),
            Self::BareMetal => (0.20, 0.55),
            Self::Glass => (0.02, 0.20),
            Self::Water => (0.04, 0.25),
            Self::Ice => (0.10, 0.45),
            Self::Snow => (0.80, 1.0),
            Self::Crystal => (0.12, 0.40),
            Self::Energy => (0.20, 0.60),
            Self::Foliage => (0.55, 0.95),
        }
    }

    pub const fn metallic_policy(self) -> MetallicPolicy {
        match self {
            Self::Painted => MetallicPolicy::PaintOverMetal,
            Self::BareMetal => MetallicPolicy::High,
            _ => MetallicPolicy::Zero,
        }
    }

    /// Target micro-variant count when the family is produced (01 §1.3:
    /// 3–6 variants per main material). The current rows ship one variant
    /// until B03/B04 generates the rest.
    pub const fn variant_budget(self) -> u8 {
        match self {
            Self::Soil | Self::Rock => 6,
            Self::Grass | Self::Wood | Self::Painted | Self::Foliage => 4,
            Self::BareMetal
            | Self::Glass
            | Self::Water
            | Self::Ice
            | Self::Snow
            | Self::Crystal
            | Self::Energy => 3,
        }
    }
}

/// UV/layer strategy. `VoxelTile` is the legacy 16×16 atlas tile sampled per
/// voxel face; B03 adds the texture-array/classic paths without changing IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UvStrategy {
    VoxelTile,
}

impl UvStrategy {
    pub const fn key(self) -> &'static str {
        match self {
            Self::VoxelTile => "voxel_tile",
        }
    }
}

/// Footstep/break tag consumed by audio and particle adapters (B05/I05); the
/// catalog only declares the tag, it does not play anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceTag {
    Soft,
    Sand,
    Stone,
    Wood,
    Metal,
    Glass,
    Water,
    Ice,
    Snow,
    Plant,
    Crystal,
    Energy,
}

impl SurfaceTag {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Sand => "sand",
            Self::Stone => "stone",
            Self::Wood => "wood",
            Self::Metal => "metal",
            Self::Glass => "glass",
            Self::Water => "water",
            Self::Ice => "ice",
            Self::Snow => "snow",
            Self::Plant => "plant",
            Self::Crystal => "crystal",
            Self::Energy => "energy",
        }
    }
}

/// One registered surface material. `key` is the stable catalog key and the
/// frozen legacy painter key today; the ID is what survives a rename or an
/// atlas/layer reorder.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SurfaceMaterial {
    pub id: SurfaceMaterialId,
    pub key: &'static str,
    pub family: MaterialFamily,
    pub uv: UvStrategy,
    /// Variants currently shipped. `1` until B03/B04 produces more; the rule
    /// is already defined so production only bumps this number.
    pub variants: u8,
    pub tag: SurfaceTag,
    /// `EMISSION_TIERS` key (`crate::art::style`) when the material emits.
    pub emission: Option<&'static str>,
}

impl SurfaceMaterial {
    /// Deterministic micro-variant slot for one voxel face. Never derived
    /// from Entity id, load order or frame number (01 §1.3 / R021).
    pub fn variant_slot(&self, wx: i32, y: i32, wz: i32, face: usize) -> u8 {
        variant_slot(self.id, self.variants, wx, y, wz, face)
    }
}

macro_rules! surface {
    ($id:expr, $key:literal, $family:ident, $tag:ident, $emission:expr) => {
        SurfaceMaterial {
            id: SurfaceMaterialId($id),
            key: $key,
            family: MaterialFamily::$family,
            uv: UvStrategy::VoxelTile,
            variants: 1,
            tag: SurfaceTag::$tag,
            emission: $emission,
        }
    };
}

/// Frozen registry. IDs follow the legacy `textures.rs` painter order 1:1
/// (id 1 = first painter); neither the IDs nor the keys may be re-derived
/// from atlas registration order again.
pub const SURFACE_MATERIALS: &[SurfaceMaterial] = &[
    surface!(1, "grass_top", Grass, Plant, None),
    surface!(2, "dirt", Soil, Soft, None),
    surface!(3, "grass_side", Grass, Plant, None),
    surface!(4, "stone", Rock, Stone, None),
    surface!(5, "sand", Soil, Sand, None),
    surface!(6, "gravel", Soil, Sand, None),
    surface!(7, "log_side", Wood, Wood, None),
    surface!(8, "log_top", Wood, Wood, None),
    surface!(9, "leaves", Foliage, Plant, None),
    surface!(10, "planks", Wood, Wood, None),
    surface!(11, "water", Water, Water, None),
    surface!(12, "ice", Ice, Ice, None),
    surface!(13, "snow_top", Snow, Snow, None),
    surface!(14, "snow_side", Snow, Snow, None),
    surface!(15, "basalt", Rock, Stone, None),
    surface!(16, "alien_top", Grass, Plant, None),
    surface!(17, "alien_side", Grass, Plant, None),
    surface!(18, "barrier", Energy, Energy, Some("marker")),
    surface!(19, "crystal", Crystal, Crystal, Some("lamp")),
    surface!(20, "mush_stem", Foliage, Plant, None),
    surface!(21, "mush_cap", Foliage, Plant, None),
    surface!(22, "ash", Soil, Soft, None),
    surface!(23, "amber", Crystal, Crystal, Some("marker")),
    surface!(24, "rust", Rock, Stone, None),
    surface!(25, "salt", Soil, Sand, None),
    surface!(26, "obsidian", Rock, Stone, None),
    surface!(27, "redmoss_top", Grass, Plant, None),
    surface!(28, "redmoss_side", Grass, Plant, None),
    surface!(29, "hive", Foliage, Plant, None),
    surface!(30, "murk_top", Grass, Plant, Some("lamp")),
    surface!(31, "murk_side", Grass, Plant, None),
    surface!(32, "glow_shroom", Foliage, Plant, Some("lamp")),
    surface!(33, "coal_ore", Rock, Stone, None),
    surface!(34, "iron_ore", Rock, Stone, None),
    surface!(35, "copper_ore", Rock, Stone, None),
    surface!(36, "titanium_ore", Rock, Stone, None),
    surface!(37, "uranium_ore", Rock, Stone, Some("marker")),
    surface!(38, "gold_ore", Rock, Stone, None),
    surface!(39, "sodium_plant", Foliage, Plant, None),
    surface!(40, "oxygen_plant", Foliage, Plant, None),
    surface!(41, "carbon_fern", Foliage, Plant, None),
    surface!(42, "glass", Glass, Glass, None),
    surface!(43, "lamp_on", Energy, Glass, Some("lamp")),
    surface!(44, "metal", BareMetal, Metal, None),
    surface!(45, "metal_dark", BareMetal, Metal, None),
    surface!(46, "vent", Painted, Metal, None),
    surface!(47, "furnace_front", Painted, Metal, None),
    surface!(48, "furnace_on", Energy, Metal, Some("lamp")),
    surface!(49, "belt", Painted, Metal, None),
    surface!(50, "belt_turn", Painted, Metal, None),
    surface!(51, "wind_pole", BareMetal, Metal, None),
    surface!(52, "miner_top", Painted, Metal, None),
    surface!(53, "assembler_top", Painted, Metal, None),
    surface!(54, "solar_top", Painted, Metal, None),
    surface!(55, "chest_side", Wood, Wood, None),
    surface!(56, "refinery_side", Painted, Metal, None),
    surface!(57, "reactor_side", Painted, Metal, None),
    surface!(58, "launchpad_top", Painted, Metal, None),
    surface!(59, "storage_top", Wood, Wood, None),
    surface!(60, "medbay_top", Painted, Metal, None),
    surface!(61, "slab", Rock, Stone, None),
    surface!(62, "concrete", Painted, Stone, None),
];

pub const MATERIAL_COUNT: usize = SURFACE_MATERIALS.len();

pub fn material_by_key(key: &str) -> Option<&'static SurfaceMaterial> {
    SURFACE_MATERIALS
        .iter()
        .find(|material| material.key == key)
}

pub fn material_by_id(id: SurfaceMaterialId) -> Option<&'static SurfaceMaterial> {
    if !id.is_known() {
        return None;
    }
    SURFACE_MATERIALS.get(id.slot() - 1)
}

/// Stable key for a possibly-unknown id; diagnostics only.
pub fn material_key(id: SurfaceMaterialId) -> &'static str {
    material_by_id(id)
        .map(|material| material.key)
        .unwrap_or("unknown")
}

pub fn material_id(key: &str) -> Option<SurfaceMaterialId> {
    material_by_key(key).map(|material| material.id)
}

/// Face role from the 0..5 face order used by the mesher (+X, -X, +Y, -Y,
/// +Z, -Z). This is the contract the old `Tiles::for_face` happened to encode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceRole {
    Side,
    Top,
    Bottom,
    Front,
}

pub const FACE_ROLES: [FaceRole; FACE_COUNT] = [
    FaceRole::Side,
    FaceRole::Side,
    FaceRole::Top,
    FaceRole::Bottom,
    FaceRole::Front,
    FaceRole::Side,
];

pub const fn face_role(face: usize) -> FaceRole {
    if face < FACE_COUNT {
        FACE_ROLES[face]
    } else {
        FaceRole::Side
    }
}

/// Per-block-id face resolution, built once from `data::BLOCKS`. Entries for
/// unused ids and for AIR stay at the fallback material.
struct FaceTable {
    faces: [[u16; FACE_COUNT]; 256],
    cross: [u16; 256],
}

fn face_table() -> &'static FaceTable {
    static TABLE: OnceLock<FaceTable> = OnceLock::new();
    TABLE.get_or_init(build_face_table)
}

fn fallback_id() -> SurfaceMaterialId {
    material_id(FALLBACK_TILE).unwrap_or(SurfaceMaterialId::UNKNOWN)
}

fn build_face_table() -> FaceTable {
    let fallback = fallback_id().raw();
    let mut table = FaceTable {
        faces: [[fallback; FACE_COUNT]; 256],
        cross: [fallback; 256],
    };
    for block in crate::data::BLOCKS {
        if block.id == crate::data::ids::AIR {
            continue;
        }
        let cross_key = block
            .tiles
            .side
            .or(block.tiles.all)
            .unwrap_or(FALLBACK_TILE);
        table.cross[block.id as usize] = material_id(cross_key).unwrap_or(fallback_id()).raw();
        for face in 0..FACE_COUNT {
            let key = block.tiles.for_face(face);
            table.faces[block.id as usize][face] = material_id(key).unwrap_or(fallback_id()).raw();
        }
    }
    table
}

pub fn face_material(block_id: u8, face: usize) -> SurfaceMaterialId {
    let table = face_table();
    if face >= FACE_COUNT {
        return SurfaceMaterialId::UNKNOWN;
    }
    SurfaceMaterialId::new(table.faces[block_id as usize][face])
}

pub fn cross_material(block_id: u8) -> SurfaceMaterialId {
    SurfaceMaterialId::new(face_table().cross[block_id as usize])
}

/// Legacy `far_surface_sample`/`fill_far_rows` mapping from a biome ground key
/// to its top surface. Frozen here so the far path resolves through IDs too.
pub fn ground_material(ground_key: &str) -> SurfaceMaterialId {
    let tile = match ground_key {
        "grass" => "grass_top",
        "snow" => "snow_top",
        "alien" => "alien_top",
        "murk" => "murk_top",
        "redmoss" => "redmoss_top",
        other => other,
    };
    material_id(tile).unwrap_or(SurfaceMaterialId::UNKNOWN)
}

/// Stable hash used by the variant rule; deliberately independent of Bevy's
/// `Entity` ids, atlas order and frame counters.
pub fn variant_slot(
    material: SurfaceMaterialId,
    variants: u8,
    wx: i32,
    y: i32,
    wz: i32,
    face: usize,
) -> u8 {
    let slots = variants.max(1) as u32;
    let salt = ((material.raw() as u32) << 8) ^ (face as u32 & 0xff);
    let seed = (y as u32) ^ 0x9E37_79B9;
    ((crate::rng::hash2_state(wx, wz, salt, seed) >> 8) % slots) as u8
}

// ---------- fingerprints and audit ----------

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn hash_field(hash: u64, value: &str) -> u64 {
    fnv1a(fnv1a(hash, value.as_bytes()), b"|")
}

/// Freezes the `SurfaceMaterialId -> key` mapping.
pub fn id_fingerprint() -> u64 {
    let mut hash = FNV_OFFSET;
    for material in SURFACE_MATERIALS {
        hash = hash_field(hash, material.key);
        hash = hash_field(hash, &material.id.raw().to_string());
    }
    hash
}

/// Freezes the `block key + face role -> material key` mapping for every
/// non-air block (this is the mapping that must survive atlas reorder).
pub fn face_fingerprint() -> u64 {
    let mut hash = FNV_OFFSET;
    for block in crate::data::BLOCKS {
        if block.id == crate::data::ids::AIR {
            continue;
        }
        hash = hash_field(hash, block.key);
        for face in 0..FACE_COUNT {
            hash = hash_field(hash, material_key(face_material(block.id, face)));
        }
        hash = hash_field(hash, material_key(cross_material(block.id)));
    }
    hash
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MaterialCatalogReport {
    pub catalog_version: u32,
    pub materials: usize,
    pub blocks: usize,
    pub face_bindings: usize,
    /// Tile names used by block faces/cross that are not registered.
    pub unknown_block_tiles: Vec<String>,
    /// Biome ground keys that resolve to no material.
    pub unknown_ground_keys: Vec<String>,
    /// Atlas painters with no catalog row.
    pub unregistered_painters: Vec<String>,
    /// Catalog rows with no atlas painter.
    pub unregistered_materials: Vec<String>,
    /// Registered rows no block face or biome ground key references yet
    /// (legacy reserve layers; informational, not a failure by itself).
    pub unused_materials: Vec<String>,
    pub id_fingerprint: u64,
    pub face_fingerprint: u64,
    pub notes: Vec<String>,
}

impl MaterialCatalogReport {
    /// Hard failures: a block face or biome ground key that cannot resolve
    /// (wrong tile / unknown layer, R020), or a registry/painter mismatch.
    pub fn failure_count(&self) -> usize {
        self.unknown_block_tiles.len()
            + self.unknown_ground_keys.len()
            + self.unregistered_painters.len()
            + self.unregistered_materials.len()
    }
}

/// Coverage audit over the real block/biome/atlas registries. Pure CPU, safe
/// to run from `--art-audit` before any Bevy app exists.
pub fn catalog_report() -> MaterialCatalogReport {
    let painters: BTreeSet<&'static str> =
        crate::textures::Atlas::painter_keys().into_iter().collect();
    let registered: BTreeSet<&'static str> = SURFACE_MATERIALS
        .iter()
        .map(|material| material.key)
        .collect();

    let mut unknown_block_tiles = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    let mut blocks = 0usize;
    let mut face_bindings = 0usize;
    for block in crate::data::BLOCKS {
        if block.id == crate::data::ids::AIR {
            continue;
        }
        blocks += 1;
        for face in 0..FACE_COUNT {
            face_bindings += 1;
            let key = block.tiles.for_face(face);
            if material_by_key(key).is_none() {
                unknown_block_tiles.insert(key);
            } else {
                referenced.insert(key);
            }
        }
        let cross = block
            .tiles
            .side
            .or(block.tiles.all)
            .unwrap_or(FALLBACK_TILE);
        if material_by_key(cross).is_none() {
            unknown_block_tiles.insert(cross);
        } else {
            referenced.insert(cross);
        }
    }

    let mut unknown_ground_keys = BTreeSet::new();
    for biome in crate::data::BIOMES {
        let keys =
            std::iter::once(biome.grass).chain(biome.sub.iter().map(|(ground, _, _)| *ground));
        for key in keys.filter(|key| !key.is_empty()) {
            let id = ground_material(key);
            if id == SurfaceMaterialId::UNKNOWN {
                unknown_ground_keys.insert(key);
            } else {
                referenced.insert(material_key(id));
            }
        }
    }

    MaterialCatalogReport {
        catalog_version: MATERIAL_CATALOG_VERSION,
        materials: MATERIAL_COUNT,
        blocks,
        face_bindings,
        unknown_block_tiles: unknown_block_tiles
            .into_iter()
            .map(str::to_string)
            .collect(),
        unknown_ground_keys: unknown_ground_keys
            .into_iter()
            .map(str::to_string)
            .collect(),
        unregistered_painters: painters
            .difference(&registered)
            .map(|key| (*key).to_string())
            .collect(),
        unregistered_materials: registered
            .difference(&painters)
            .map(|key| (*key).to_string())
            .collect(),
        unused_materials: registered
            .difference(&referenced)
            .map(|key| (*key).to_string())
            .collect(),
        id_fingerprint: id_fingerprint(),
        face_fingerprint: face_fingerprint(),
        notes: vec![
            "ids are explicit constants; atlas registration order is not an ID source".to_string(),
            "unused_materials are frozen legacy atlas layers kept for registration-order compatibility; B03/B04 may wire or retire them".to_string(),
        ],
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MaterialSnapshot {
    pub id: u16,
    pub key: &'static str,
    pub family: &'static str,
    pub uv: &'static str,
    pub variants: u8,
    pub variant_budget: u8,
    pub metallic_policy: MetallicPolicy,
    pub tag: &'static str,
    pub emission: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlockSnapshot {
    pub id: u8,
    pub key: &'static str,
    pub cross: &'static str,
    pub faces: [&'static str; FACE_COUNT],
}

/// Machine-readable catalog artifact for B03's texture production and for
/// K01/K05 diffing.
#[derive(Clone, Debug, Serialize)]
pub struct MaterialCatalogSnapshot {
    pub catalog_version: u32,
    pub id_fingerprint: u64,
    pub face_fingerprint: u64,
    pub materials: Vec<MaterialSnapshot>,
    pub blocks: Vec<BlockSnapshot>,
}

pub fn catalog_snapshot() -> MaterialCatalogSnapshot {
    let materials = SURFACE_MATERIALS
        .iter()
        .map(|material| MaterialSnapshot {
            id: material.id.raw(),
            key: material.key,
            family: material.family.key(),
            uv: material.uv.key(),
            variants: material.variants,
            variant_budget: material.family.variant_budget(),
            metallic_policy: material.family.metallic_policy(),
            tag: material.tag.key(),
            emission: material.emission,
        })
        .collect();
    let blocks = crate::data::BLOCKS
        .iter()
        .filter(|block| block.id != crate::data::ids::AIR)
        .map(|block| BlockSnapshot {
            id: block.id,
            key: block.key,
            cross: material_key(cross_material(block.id)),
            faces: std::array::from_fn(|face| material_key(face_material(block.id, face))),
        })
        .collect();
    MaterialCatalogSnapshot {
        catalog_version: MATERIAL_CATALOG_VERSION,
        id_fingerprint: id_fingerprint(),
        face_fingerprint: face_fingerprint(),
        materials,
        blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B02 golden values: captured when the catalog froze the legacy mapping.
    /// A change here means saves/business IDs or the block→material binding
    /// moved; that requires an ADR, not a golden update.
    const GOLDEN_ID_FINGERPRINT: u64 = 15794410180736732047;
    const GOLDEN_FACE_FINGERPRINT: u64 = 16956356181302620687;

    #[test]
    fn ids_and_keys_are_unique_and_contiguous() {
        let mut keys = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for material in SURFACE_MATERIALS {
            assert!(keys.insert(material.key), "duplicate key {}", material.key);
            assert!(
                ids.insert(material.id.raw()),
                "duplicate id {}",
                material.id.raw()
            );
            assert!(material.id.is_known());
            assert!(material.variants >= 1);
            assert!(material.variants <= material.family.variant_budget());
        }
        for (index, raw) in (1u16..=MATERIAL_COUNT as u16).enumerate() {
            assert_eq!(raw as usize, index + 1, "ids must be contiguous from 1");
            assert!(ids.contains(&raw));
        }
        assert_eq!(material_by_id(SurfaceMaterialId::UNKNOWN), None);
        assert_eq!(material_by_id(SurfaceMaterialId::new(9999)), None);
    }

    #[test]
    fn catalog_fingerprints_are_frozen() {
        assert_eq!(
            id_fingerprint(),
            GOLDEN_ID_FINGERPRINT,
            "material ID mapping moved"
        );
        assert_eq!(
            face_fingerprint(),
            GOLDEN_FACE_FINGERPRINT,
            "block face mapping moved"
        );
    }

    #[test]
    fn every_block_face_resolves_and_matches_legacy_tiles() {
        let mut bindings = 0usize;
        for block in crate::data::BLOCKS {
            if block.id == crate::data::ids::AIR {
                continue;
            }
            for face in 0..FACE_COUNT {
                bindings += 1;
                let id = face_material(block.id, face);
                assert!(id.is_known(), "{} face {face} unresolved", block.key);
                assert_eq!(
                    material_key(id),
                    block.tiles.for_face(face),
                    "{} face {face} changed tile",
                    block.key
                );
            }
            let cross = cross_material(block.id);
            assert!(cross.is_known(), "{} cross unresolved", block.key);
            assert_eq!(
                material_key(cross),
                block
                    .tiles
                    .side
                    .or(block.tiles.all)
                    .unwrap_or(FALLBACK_TILE),
                "{} cross changed tile",
                block.key
            );
        }
        // 86 non-air blocks × 6 faces.
        assert_eq!(bindings, 516);
    }

    #[test]
    fn face_roles_match_legacy_for_face_precedence() {
        assert_eq!(face_role(0), FaceRole::Side);
        assert_eq!(face_role(1), FaceRole::Side);
        assert_eq!(face_role(2), FaceRole::Top);
        assert_eq!(face_role(3), FaceRole::Bottom);
        assert_eq!(face_role(4), FaceRole::Front);
        assert_eq!(face_role(5), FaceRole::Side);
    }

    #[test]
    fn ground_mapping_matches_legacy_far_path() {
        for biome in crate::data::BIOMES {
            for (key, _, _) in biome
                .sub
                .iter()
                .chain(std::iter::once(&(biome.grass, 0.0, 0.0)))
            {
                if key.is_empty() {
                    continue;
                }
                let id = ground_material(key);
                assert!(id.is_known(), "biome {} ground {key} unresolved", biome.key);
            }
        }
        assert_eq!(material_key(ground_material("grass")), "grass_top");
        assert_eq!(material_key(ground_material("snow")), "snow_top");
        assert_eq!(material_key(ground_material("alien")), "alien_top");
        assert_eq!(material_key(ground_material("murk")), "murk_top");
        assert_eq!(material_key(ground_material("redmoss")), "redmoss_top");
        assert_eq!(material_key(ground_material("basalt")), "basalt");
        assert_eq!(ground_material("not_a_tile"), SurfaceMaterialId::UNKNOWN);
    }

    #[test]
    fn variant_rule_is_deterministic_bounded_and_cell_dependent() {
        let material = material_by_key("stone").unwrap();
        let slot = |wx, y, wz, face| {
            variant_slot(
                material.id,
                material.family.variant_budget(),
                wx,
                y,
                wz,
                face,
            )
        };
        for face in 0..FACE_COUNT {
            let a = slot(10, 40, -3, face);
            let b = slot(10, 40, -3, face);
            assert_eq!(a, b, "same inputs must give the same slot");
            assert!(a < material.family.variant_budget());
        }
        let slots: BTreeSet<u8> = (-8..8)
            .flat_map(|x| (-8..8).map(move |z| slot(x, 30, z, 2)))
            .collect();
        assert!(slots.len() > 1, "6-variant budget must vary across cells");
        assert_ne!(
            slot(10, 30, 10, 2),
            slot(10, 31, 10, 2),
            "y must contribute"
        );
        assert_ne!(
            slot(10, 40, 10, 2),
            slot(10, 40, 10, 4),
            "face must contribute"
        );
        assert_eq!(variant_slot(material.id, 0, 1, 2, 3, 4), 0);
    }

    #[test]
    fn families_follow_the_art_guide_ranges() {
        assert_eq!(MaterialFamily::ALL.len(), 13);
        for family in MaterialFamily::ALL {
            let (min, max) = family.roughness_range();
            assert!(0.0 <= min && min <= max && max <= 1.0, "{}", family.key());
            let budget = family.variant_budget();
            assert!(
                (3..=6).contains(&budget),
                "{} budget {budget}",
                family.key()
            );
        }
        let tiers: BTreeSet<&str> = crate::art::style::EMISSION_TIERS
            .iter()
            .map(|tier| tier.key)
            .collect();
        for material in SURFACE_MATERIALS {
            if let Some(tier) = material.emission {
                assert!(tiers.contains(tier), "{} unknown tier {tier}", material.key);
            }
        }
    }

    #[test]
    fn atlas_reorder_keeps_ids_keys_and_save_abi() {
        let order: Vec<usize> = (0..crate::textures::Atlas::painter_keys().len())
            .rev()
            .collect();
        let original = crate::textures::Atlas::build();
        let permuted = crate::textures::Atlas::build_with_order(&order).expect("permuted atlas");
        assert_ne!(
            original.index.get("grass_top"),
            permuted.index.get("grass_top"),
            "the permutation must actually reorder the atlas"
        );
        // The atlas layout (UVs) moved, but each ID still resolves through
        // the catalog to its own key and layer.
        let mut moved = 0usize;
        for material in SURFACE_MATERIALS {
            let original_uv = original.uv_rect_material(material.id).expect("original UV");
            let permuted_uv = permuted.uv_rect_material(material.id).expect("permuted UV");
            assert_eq!(original_uv, original.uv_rect(material.key));
            assert_eq!(
                original.tile_material(material.id),
                original
                    .index
                    .get(material.key)
                    .map(|&i| &original.tiles[i]),
                "{} layer must follow its ID",
                material.key
            );
            if original_uv != permuted_uv {
                moved += 1;
            }
        }
        assert!(moved > 0, "a reversed atlas must move at least one UV rect");
        // Business mapping and save ABI are independent of atlas layout.
        for block in crate::data::BLOCKS {
            let again = crate::data::block_by_id(block.id);
            assert_eq!(block.key, again.key, "u8 block ABI changed");
        }
    }

    #[test]
    fn catalog_report_covers_registry_without_failures() {
        let report = catalog_report();
        assert_eq!(report.materials, MATERIAL_COUNT);
        assert_eq!(report.blocks, 86);
        assert_eq!(report.face_bindings, 86 * FACE_COUNT);
        assert_eq!(report.failure_count(), 0, "{report:?}");
        assert_eq!(
            report.unused_materials,
            vec![
                "belt_turn".to_string(),
                "furnace_on".to_string(),
                "gravel".to_string(),
                "wind_pole".to_string(),
            ],
            "reserve layers changed; wire or retire them deliberately"
        );
        assert_ne!(report.id_fingerprint, 0);
        assert_ne!(report.face_fingerprint, 0);
    }

    #[test]
    fn snapshot_lists_every_material_and_block() {
        let snapshot = catalog_snapshot();
        assert_eq!(snapshot.materials.len(), MATERIAL_COUNT);
        assert_eq!(snapshot.blocks.len(), 86);
        assert_eq!(snapshot.id_fingerprint, id_fingerprint());
        assert_eq!(snapshot.face_fingerprint, face_fingerprint());
        for block in &snapshot.blocks {
            assert_eq!(block.faces.len(), FACE_COUNT);
            assert!(block.faces.iter().all(|key| material_by_key(key).is_some()));
            assert!(material_by_key(block.cross).is_some());
        }
        let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
        assert!(json.contains("\"grass_top\""));
    }
}
