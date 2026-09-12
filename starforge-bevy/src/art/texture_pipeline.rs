//! B03 texture production pipeline: per-tile mip chains with channel-correct
//! downsampling, the padded-atlas fallback, texture-array planning and the
//! quality-route decision. Pure CPU so `--texture-audit` can run headless.
//!
//! Why per-tile chains instead of `Image` mip generation: filtering the whole
//! atlas bleeds neighbouring tiles across cell borders (R013), and it is wrong
//! for sRGB color, alpha cutouts and normal vectors (R016/R022). Each
//! `SurfaceMaterialId` owns one chain; the atlas fallback packs every level
//! with an edge-extruded gutter, and the array route uses the chains directly.
//!
//! The channel classes are already generic so B04/B05 can declare normal/ORM
//! maps without touching this module. Albedo mips are averaged in linear light
//! and premultiplied by alpha, and the alpha channel is remapped per level to
//! preserve the `Mask(0.4)` thresholded coverage (main and shadow read the
//! same alpha, so the cutoff is not allowed to drift per consumer).

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::catalog::{SURFACE_MATERIALS, SurfaceMaterialId, material_key};
use crate::textures::Atlas;

pub const PIPELINE_VERSION: u32 = 1;
/// Source-resolution gutter for the padded atlas fallback. Halves exactly per
/// level, which is what keeps the inner UV rect identical across levels; the
/// deepest padded level is therefore `floor(log2(gutter))`.
pub const BASE_GUTTER: u32 = 4;
/// `StandardMaterial`'s `AlphaMode::Mask(0.4)`, shared by main and shadow.
pub const ALPHA_CUTOFF: u8 = 102;
/// Alpha coverage correction is meaningful while a tile still has enough
/// pixels; at 2×2/1×1 the thresholded coverage can only quantize to a few
/// values, so those levels are reported but not corrected.
pub const CUTOUT_MIN_LEVEL_SIZE: u32 = 4;
/// Byte size of the legacy single 256×256 atlas before this pipeline exists.
pub const LEGACY_ATLAS_BYTES: u64 = 256 * 256 * 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelClass {
    /// sRGB color + linear alpha (all current terrain surfaces).
    Albedo,
    /// Raw linear data channels (ORM/roughness/metalness/height).
    LinearData,
    /// Tangent-space normal map (xy encoded, z reconstructed).
    Normal,
}

impl ChannelClass {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Albedo => "albedo",
            Self::LinearData => "linear_data",
            Self::Normal => "normal",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeMode {
    /// Tiling surfaces: filter across the wrap seam.
    Wrap,
    /// Atlas / one-shot surfaces: clamp at the border.
    Clamp,
}

impl EdgeMode {
    fn sample(self, coord: i64, size: u32) -> u32 {
        let size = size.max(1) as i64;
        match self {
            Self::Wrap => coord.rem_euclid(size) as u32,
            Self::Clamp => coord.clamp(0, size - 1) as u32,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl SourceImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert_eq!(
            pixels.len(),
            (width * height * 4) as usize,
            "RGBA8 source must be width*height*4"
        );
        Self {
            width,
            height,
            pixels,
        }
    }

    /// From one legacy 16×16 atlas tile.
    pub fn from_tile(tile: &[crate::textures::Pixel; 256]) -> Self {
        let mut pixels = Vec::with_capacity(256 * 4);
        for pixel in tile {
            pixels.extend_from_slice(pixel);
        }
        Self::new(16, 16, pixels)
    }

    fn pixel(&self, x: i64, y: i64, edge: EdgeMode) -> [u8; 4] {
        let x = edge.sample(x, self.width) as usize;
        let y = edge.sample(y, self.height) as usize;
        let i = (y * self.width as usize + x) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    fn set(&mut self, x: u32, y: u32, pixel: [u8; 4]) {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[i..i + 4].copy_from_slice(&pixel);
    }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn u8_to_linear(v: u8) -> f32 {
    srgb_to_linear(v as f32 / 255.0)
}

fn linear_to_u8(v: f32) -> u8 {
    (linear_to_srgb(v) * 255.0).round().clamp(0.0, 255.0) as u8
}

/// 2×2 box downsample in the correct space for `class`.
pub fn downsample(source: &SourceImage, class: ChannelClass, edge: EdgeMode) -> SourceImage {
    let width = (source.width / 2).max(1);
    let height = (source.height / 2).max(1);
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let footprint = [
                source.pixel(x as i64 * 2, y as i64 * 2, edge),
                source.pixel(x as i64 * 2 + 1, y as i64 * 2, edge),
                source.pixel(x as i64 * 2, y as i64 * 2 + 1, edge),
                source.pixel(x as i64 * 2 + 1, y as i64 * 2 + 1, edge),
            ];
            let out = match class {
                ChannelClass::Albedo => downsample_albedo(&footprint),
                ChannelClass::LinearData => downsample_linear(&footprint),
                ChannelClass::Normal => downsample_normal(&footprint),
            };
            let i = (y as usize * width as usize + x as usize) * 4;
            pixels[i..i + 4].copy_from_slice(&out);
        }
    }
    SourceImage::new(width, height, pixels)
}

fn downsample_albedo(footprint: &[[u8; 4]; 4]) -> [u8; 4] {
    let mut weighted = [0.0f32; 3];
    let mut weight = 0.0f32;
    let mut alpha = 0.0f32;
    for pixel in footprint {
        let a = pixel[3] as f32 / 255.0;
        weighted[0] += u8_to_linear(pixel[0]) * a;
        weighted[1] += u8_to_linear(pixel[1]) * a;
        weighted[2] += u8_to_linear(pixel[2]) * a;
        weight += a;
        alpha += a;
    }
    // Premultiplied color average: transparent texels contribute no color, so
    // an alpha-cutout edge cannot darken into a halo (R019).
    let rgb = if weight > 0.0 {
        [
            linear_to_u8(weighted[0] / weight),
            linear_to_u8(weighted[1] / weight),
            linear_to_u8(weighted[2] / weight),
        ]
    } else {
        [0, 0, 0]
    };
    [
        rgb[0],
        rgb[1],
        rgb[2],
        ((alpha / footprint.len() as f32) * 255.0).round() as u8,
    ]
}

fn downsample_linear(footprint: &[[u8; 4]; 4]) -> [u8; 4] {
    let mut out = [0u8; 4];
    for channel in 0..4 {
        let sum: u32 = footprint.iter().map(|pixel| pixel[channel] as u32).sum();
        out[channel] = ((sum as f32) / footprint.len() as f32).round() as u8;
    }
    out
}

fn downsample_normal(footprint: &[[u8; 4]; 4]) -> [u8; 4] {
    let mut sum = [0.0f32; 3];
    for pixel in footprint {
        let x = pixel[0] as f32 / 255.0 * 2.0 - 1.0;
        let y = pixel[1] as f32 / 255.0 * 2.0 - 1.0;
        // Reconstruct +Z, average vector coordinates, renormalize (R022).
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        sum[0] += x;
        sum[1] += y;
        sum[2] += z;
    }
    let length = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
    let (x, y, z) = if length > 1e-6 {
        (sum[0] / length, sum[1] / length, sum[2] / length)
    } else {
        (0.0, 0.0, 1.0)
    };
    [
        ((x * 0.5 + 0.5) * 255.0).round() as u8,
        ((y * 0.5 + 0.5) * 255.0).round() as u8,
        ((z * 0.5 + 0.5) * 255.0).round() as u8,
        255,
    ]
}

/// Fraction of pixels whose alpha passes `cutoff` (the `Mask(0.4)` test).
pub fn threshold_coverage(pixels: &[u8], cutoff: u8) -> f32 {
    let count = pixels.len() / 4;
    if count == 0 {
        return 0.0;
    }
    let covered = pixels.chunks_exact(4).filter(|p| p[3] >= cutoff).count();
    covered as f32 / count as f32
}

/// Ordered dither matrix for coverage-preserving alpha mips (4×4 Bayer).
const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Remap a cutout level's alpha so its `Mask(0.4)` thresholded coverage stays
/// close to `target`. A scale around the cutoff cannot change the predicate
/// (any monotone map keeps the ordering), so this binarizes with an ordered
/// dither pattern scaled to the target; without it a box-averaged leaf edge
/// crosses the cutoff and the foliage either vanishes or turns solid at range
/// (R016). Returns the resulting coverage error.
pub fn preserve_alpha_coverage(level: &mut SourceImage, target: f32) -> f32 {
    if target <= 0.0 || target >= 1.0 {
        return 0.0;
    }
    let count = level.pixels.len() / 4;
    if count == 0 {
        return target;
    }
    let mean: f32 = level
        .pixels
        .chunks_exact(4)
        .map(|pixel| pixel[3] as f32 / 255.0)
        .sum::<f32>()
        / count as f32;
    if mean <= 0.0 {
        return target;
    }
    let scale = (target / mean).clamp(0.0, 4.0);
    let width = level.width.max(1) as usize;
    for (index, pixel) in level.pixels.chunks_exact_mut(4).enumerate() {
        let x = index % width;
        let y = index / width;
        let probability = (pixel[3] as f32 / 255.0 * scale).clamp(0.0, 1.0);
        let threshold = (BAYER4[y % 4][x % 4] as f32 + 0.5) / 16.0;
        pixel[3] = if probability > threshold { 255 } else { 0 };
    }
    let after = threshold_coverage(&level.pixels, ALPHA_CUTOFF);
    (after - target).abs()
}

#[derive(Clone, Debug, PartialEq)]
pub struct MipChain {
    pub material: SurfaceMaterialId,
    pub class: ChannelClass,
    pub edge: EdgeMode,
    /// Unpadded levels, level 0 first. Albedo chains already carry the
    /// coverage-preserving alpha fix.
    pub levels: Vec<SourceImage>,
    /// Worst thresholded-coverage delta introduced by the alpha fix.
    pub coverage_error: f32,
}

pub fn build_chain(
    material: SurfaceMaterialId,
    class: ChannelClass,
    source: SourceImage,
    edge: EdgeMode,
) -> MipChain {
    let mut levels = vec![source];
    while levels.last().map(|level| level.width.max(level.height)) > Some(1) {
        let next = downsample(levels.last().expect("level"), class, edge);
        levels.push(next);
    }
    let mut coverage_error: f32 = 0.0;
    if class == ChannelClass::Albedo {
        let target = threshold_coverage(&levels[0].pixels, ALPHA_CUTOFF);
        for level in levels.iter_mut().skip(1) {
            if level.width.max(level.height) < CUTOUT_MIN_LEVEL_SIZE {
                continue;
            }
            coverage_error = coverage_error.max(preserve_alpha_coverage(level, target));
        }
    }
    MipChain {
        material,
        class,
        edge,
        levels,
        coverage_error,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaddedTile {
    pub size: u32,
    pub gutter: u32,
    pub pixels: Vec<u8>,
}

impl PaddedTile {
    pub fn padded_size(&self) -> u32 {
        self.size + self.gutter * 2
    }

    fn pixel(&self, x: i64, y: i64) -> [u8; 4] {
        let side = self.padded_size() as i64;
        let x = x.clamp(0, side - 1) as usize;
        let y = y.clamp(0, side - 1) as usize;
        let i = (y * side as usize + x) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

/// Extrude the tile border into a `gutter`-wide frame so any bilinear footprint
/// that stays inside the inner rect reads tile content only (R013).
pub fn pad_tile(source: &SourceImage, gutter: u32) -> PaddedTile {
    let size = source.width.max(source.height);
    let padded = size + gutter * 2;
    let mut pixels = vec![0u8; (padded * padded * 4) as usize];
    for y in 0..padded {
        for x in 0..padded {
            let inner_x = (x as i64 - gutter as i64).clamp(0, size as i64 - 1) as u32;
            let inner_y = (y as i64 - gutter as i64).clamp(0, size as i64 - 1) as u32;
            let pixel = source.pixel(inner_x as i64, inner_y as i64, EdgeMode::Clamp);
            let i = (y as usize * padded as usize + x as usize) * 4;
            pixels[i..i + 4].copy_from_slice(&pixel);
        }
    }
    PaddedTile {
        size,
        gutter,
        pixels,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct AtlasLevelInfo {
    pub level: u32,
    pub tile_size: u32,
    pub gutter: u32,
    pub cell: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct PaddedAtlasLevel {
    pub info: AtlasLevelInfo,
    pub pixels: Vec<u8>,
    /// Aligned with `PaddedAtlas::slots`.
    pub tiles: Vec<PaddedTile>,
}

#[derive(Clone, Debug)]
pub struct PaddedAtlas {
    pub columns: u32,
    pub rows: u32,
    pub base_gutter: u32,
    pub max_padded_level: u32,
    pub levels: Vec<PaddedAtlasLevel>,
    /// Inner UV rect per material slot; identical at every packed level.
    pub uv: Vec<[f32; 4]>,
    pub slots: Vec<(SurfaceMaterialId, u32, u32)>,
    pub uv_stable: bool,
}

pub fn gutter_for_level(base_gutter: u32, level: u32) -> u32 {
    base_gutter >> level
}

pub fn max_padded_level(base_gutter: u32, chain_levels: usize) -> u32 {
    let mut level = 0;
    while level + 1 < chain_levels as u32 && gutter_for_level(base_gutter, level + 1) >= 1 {
        level += 1;
    }
    level
}

fn next_power_of_two(value: u32) -> u32 {
    let mut out = 1;
    while out < value {
        out *= 2;
    }
    out
}

/// Pack per-material chains into a padded atlas. `chains` must all share one
/// base size (the catalog pipeline builds them from one source size).
pub fn build_padded_atlas(chains: &[MipChain], base_gutter: u32) -> Result<PaddedAtlas, String> {
    if chains.is_empty() {
        return Err("no chains to pack".to_string());
    }
    let base_size = chains[0].levels[0].width.max(chains[0].levels[0].height);
    for chain in chains {
        let size = chain.levels[0].width.max(chain.levels[0].height);
        if size != base_size {
            return Err(format!(
                "{} base size {size} != {base_size}",
                material_key(chain.material)
            ));
        }
    }
    let count = chains.len() as u32;
    let columns = next_power_of_two(count.isqrt() + u32::from(count.isqrt().pow(2) != count));
    let rows = count.div_ceil(columns);
    let max_level = chains
        .iter()
        .map(|chain| max_padded_level(base_gutter, chain.levels.len()))
        .min()
        .unwrap_or(0);

    let mut slots = Vec::with_capacity(chains.len());
    for (index, chain) in chains.iter().enumerate() {
        slots.push((
            chain.material,
            index as u32 % columns,
            index as u32 / columns,
        ));
    }

    let mut levels = Vec::new();
    for level in 0..=max_level {
        let gutter = gutter_for_level(base_gutter, level);
        let tile_size = chains[0].levels[level as usize].width;
        let cell = tile_size + gutter * 2;
        let info = AtlasLevelInfo {
            level,
            tile_size,
            gutter,
            cell,
            width: columns * cell,
            height: rows * cell,
        };
        let mut pixels = vec![0u8; (info.width * info.height * 4) as usize];
        let mut tiles = Vec::with_capacity(chains.len());
        for (index, chain) in chains.iter().enumerate() {
            let source = chain
                .levels
                .get(level as usize)
                .or_else(|| chain.levels.last())
                .expect("chain has level 0");
            let padded = pad_tile(source, gutter);
            let col = index as u32 % columns;
            let row = index as u32 / columns;
            let origin_x = col * cell;
            let origin_y = row * cell;
            for y in 0..cell {
                for x in 0..cell {
                    let pixel = padded.pixel(x as i64, y as i64);
                    let i = ((origin_y + y) as usize * info.width as usize
                        + (origin_x + x) as usize)
                        * 4;
                    pixels[i..i + 4].copy_from_slice(&pixel);
                }
            }
            tiles.push(padded);
        }
        levels.push(PaddedAtlasLevel {
            info,
            pixels,
            tiles,
        });
    }

    let base = &levels[0].info;
    let uv: Vec<[f32; 4]> = slots
        .iter()
        .map(|(_, col, row)| {
            let u0 = (*col as f32 * base.cell as f32 + base.gutter as f32) / base.width as f32;
            let v0 = (*row as f32 * base.cell as f32 + base.gutter as f32) / base.height as f32;
            [
                u0,
                v0,
                u0 + base.tile_size as f32 / base.width as f32,
                v0 + base.tile_size as f32 / base.height as f32,
            ]
        })
        .collect();

    let mut uv_stable = true;
    for level in &levels[1..] {
        for ((_, col, row), expected) in slots.iter().zip(uv.iter()) {
            let u0 = (*col as f32 * level.info.cell as f32 + level.info.gutter as f32)
                / level.info.width as f32;
            let v0 = (*row as f32 * level.info.cell as f32 + level.info.gutter as f32)
                / level.info.height as f32;
            let plain_u1 = u0 + level.info.tile_size as f32 / level.info.width as f32;
            let plain_v1 = v0 + level.info.tile_size as f32 / level.info.height as f32;
            if (u0 - expected[0]).abs() > 1e-6
                || (v0 - expected[1]).abs() > 1e-6
                || (plain_u1 - expected[2]).abs() > 1e-6
                || (plain_v1 - expected[3]).abs() > 1e-6
            {
                uv_stable = false;
            }
        }
    }

    Ok(PaddedAtlas {
        columns,
        rows,
        base_gutter,
        max_padded_level: max_level,
        levels,
        uv,
        slots,
        uv_stable,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct BleedLevelReport {
    pub level: u32,
    pub gutter: u32,
    pub checked_pixels: usize,
    pub violations: usize,
}

/// Verify the extrusion invariant on every packed level: each padded pixel
/// must equal the clamped inner pixel, so a bilinear footprint inside the
/// inner rect can never touch a neighbouring cell.
pub fn verify_bleed(atlas: &PaddedAtlas) -> Vec<BleedLevelReport> {
    let mut reports = Vec::new();
    for level in &atlas.levels {
        let mut checked = 0usize;
        let mut violations = 0usize;
        for tile in &level.tiles {
            let side = tile.padded_size();
            let gutter = tile.gutter as i64;
            let max_inner = tile.size as i64 - 1;
            for y in 0..side {
                for x in 0..side {
                    let inner_x = (x as i64 - gutter).clamp(0, max_inner);
                    let inner_y = (y as i64 - gutter).clamp(0, max_inner);
                    checked += 1;
                    // The extruded pixel must equal the clamped inner pixel it
                    // was copied from (plus its own offset in the padding).
                    if tile.pixel(x as i64, y as i64)
                        != tile.pixel(inner_x + gutter, inner_y + gutter)
                    {
                        violations += 1;
                    }
                }
            }
        }
        reports.push(BleedLevelReport {
            level: level.info.level,
            gutter: level.info.gutter,
            checked_pixels: checked,
            violations,
        });
    }
    reports
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureRoute {
    TextureArray,
    PaddedAtlas,
    ClassicAtlas,
}

#[derive(Clone, Copy, Debug)]
pub struct RouteParams {
    pub materials: u32,
    pub base_size: u32,
    pub max_array_layers: u32,
    pub max_texture_dimension_2d: u32,
    pub array_filterable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RouteDecision {
    pub route: TextureRoute,
    pub reason: String,
}

pub fn decide_route(params: RouteParams) -> RouteDecision {
    if params.materials == 0 {
        return RouteDecision {
            route: TextureRoute::ClassicAtlas,
            reason: "no surface materials registered".to_string(),
        };
    }
    let columns = next_power_of_two((params.materials as f32).sqrt().ceil() as u32);
    let cell = params.base_size + BASE_GUTTER * 2;
    let atlas_fits = columns * cell <= params.max_texture_dimension_2d;
    if !params.array_filterable {
        return RouteDecision {
            route: if atlas_fits {
                TextureRoute::PaddedAtlas
            } else {
                TextureRoute::ClassicAtlas
            },
            reason: "surface array format is not filterable on this device".to_string(),
        };
    }
    if params.materials > params.max_array_layers {
        return RouteDecision {
            route: if atlas_fits {
                TextureRoute::PaddedAtlas
            } else {
                TextureRoute::ClassicAtlas
            },
            reason: format!(
                "materials {} > max texture array layers {}",
                params.materials, params.max_array_layers
            ),
        };
    }
    if params.base_size > params.max_texture_dimension_2d {
        return RouteDecision {
            route: TextureRoute::ClassicAtlas,
            reason: format!(
                "base size {} > max texture dimension {}",
                params.base_size, params.max_texture_dimension_2d
            ),
        };
    }
    if !atlas_fits {
        return RouteDecision {
            route: TextureRoute::TextureArray,
            reason: "padded atlas exceeds the 2D limit; arrays carry the full set".to_string(),
        };
    }
    RouteDecision {
        route: TextureRoute::TextureArray,
        reason: "array limits and filterable surface format satisfied".to_string(),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ArrayPlan {
    pub layers: u32,
    pub base_size: u32,
    pub levels: u32,
    pub bytes: u64,
}

pub fn plan_array(chains: &[MipChain]) -> ArrayPlan {
    let mut bytes = 0u64;
    let mut levels = 0u32;
    let mut base_size = 0u32;
    for chain in chains {
        levels = levels.max(chain.levels.len() as u32);
        if let Some(first) = chain.levels.first() {
            base_size = base_size.max(first.width.max(first.height));
        }
        for level in &chain.levels {
            bytes += (level.width * level.height * 4) as u64;
        }
    }
    ArrayPlan {
        layers: chains.len() as u32,
        base_size,
        levels,
        bytes,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AtlasPlan {
    pub columns: u32,
    pub rows: u32,
    pub base_size: u32,
    pub base_gutter: u32,
    pub max_padded_level: u32,
    pub bytes: u64,
    pub levels: Vec<AtlasLevelInfo>,
}

pub fn plan_atlas(atlas: &PaddedAtlas) -> AtlasPlan {
    let mut bytes = 0u64;
    for level in &atlas.levels {
        bytes += (level.info.width * level.info.height * 4) as u64;
    }
    AtlasPlan {
        columns: atlas.columns,
        rows: atlas.rows,
        base_size: atlas.levels[0].info.tile_size,
        base_gutter: atlas.base_gutter,
        max_padded_level: atlas.max_padded_level,
        bytes,
        levels: atlas.levels.iter().map(|level| level.info).collect(),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CoverageStats {
    pub id: u16,
    pub key: &'static str,
    pub levels: Vec<f32>,
    pub worst_step: f32,
    pub halo_delta: f32,
}

pub fn coverage_stats(chains: &[MipChain]) -> Vec<CoverageStats> {
    let mut out = Vec::new();
    for chain in chains {
        let base = threshold_coverage(&chain.levels[0].pixels, ALPHA_CUTOFF);
        if base <= 0.0 || base >= 1.0 {
            continue;
        }
        let evaluated: Vec<&SourceImage> = chain
            .levels
            .iter()
            .filter(|level| level.width.max(level.height) >= CUTOUT_MIN_LEVEL_SIZE)
            .collect();
        let levels: Vec<f32> = evaluated
            .iter()
            .map(|level| threshold_coverage(&level.pixels, ALPHA_CUTOFF))
            .collect();
        let worst_step = levels
            .windows(2)
            .map(|pair| (pair[0] - pair[1]).abs())
            .fold(0.0f32, f32::max);
        // Halo proxy: premultiplied color average vs. naive average at level 1.
        let halo_delta = if chain.levels.len() > 1 {
            let source = &chain.levels[0];
            let naive = downsample(source, ChannelClass::LinearData, chain.edge);
            let actual = &chain.levels[1];
            let mut worst = 0.0f32;
            for (a, b) in actual
                .pixels
                .chunks_exact(4)
                .zip(naive.pixels.chunks_exact(4))
            {
                if b[3] > 0 && b[3] < 255 {
                    let delta = (0..3)
                        .map(|c| (a[c] as f32 - b[c] as f32).abs())
                        .fold(0.0f32, f32::max);
                    worst = worst.max(delta);
                }
            }
            worst / 255.0
        } else {
            0.0
        };
        out.push(CoverageStats {
            id: chain.material.raw(),
            key: material_key(chain.material),
            levels,
            worst_step,
            halo_delta,
        });
    }
    out
}

/// Synthetic check that normal averaging renormalizes and never emits a zero
/// vector. Runs on built-in data so the audit can report it headlessly.
pub fn verify_normal_filter() -> (bool, f32) {
    let mut source = SourceImage::new(2, 2, vec![0; 16]);
    // Encode 0 as 127 and 128 in pairs so the byte rounding bias cancels.
    source.set(0, 0, [255, 128, 128, 255]); // +X
    source.set(1, 0, [0, 127, 128, 255]); // -X
    source.set(0, 1, [128, 255, 128, 255]); // +Y
    source.set(1, 1, [127, 0, 128, 255]); // -Y
    let level = downsample(&source, ChannelClass::Normal, EdgeMode::Clamp);
    let x = level.pixels[0] as f32 / 255.0 * 2.0 - 1.0;
    let y = level.pixels[1] as f32 / 255.0 * 2.0 - 1.0;
    let z = level.pixels[2] as f32 / 255.0 * 2.0 - 1.0;
    let error = ((x * x + y * y + z * z).sqrt() - 1.0).abs();
    (error < 0.02 && z > 0.9, error)
}

/// RGB contact sheet: every padded atlas level stacked, deeper levels nearest-
/// upscaled so all rows share the base width (each level shrinks by exactly 2×
/// while the gutter halves).
pub fn contact_sheet_rgb(atlas: &PaddedAtlas) -> (u32, u32, Vec<u8>) {
    let width = atlas.levels[0].info.width;
    let mut total_height = 0u32;
    for level in &atlas.levels {
        let rows = level.info.height;
        let scale = (width / level.info.width.max(1)).max(1);
        total_height += rows * scale;
    }
    let mut rgb = vec![0u8; (width * total_height * 3) as usize];
    let mut y_offset = 0u32;
    for level in &atlas.levels {
        let scale = (width / level.info.width.max(1)).max(1);
        for y in 0..level.info.height {
            for x in 0..level.info.width {
                let i = (y as usize * level.info.width as usize + x as usize) * 4;
                let pixel = [level.pixels[i], level.pixels[i + 1], level.pixels[i + 2]];
                for dy in 0..scale {
                    for dx in 0..scale {
                        let ox = x * scale + dx;
                        let oy = y_offset + y * scale + dy;
                        let o = (oy as usize * width as usize + ox as usize) * 3;
                        rgb[o..o + 3].copy_from_slice(&pixel);
                    }
                }
            }
        }
        y_offset += level.info.height * scale;
    }
    (width, total_height, rgb)
}

/// Minimal 24-bit BMP writer (dependency-free QA artifact).
pub fn write_bmp(path: &Path, width: u32, height: u32, rgb: &[u8]) -> std::io::Result<()> {
    assert_eq!(rgb.len(), (width * height * 3) as usize);
    let row_bytes = (width * 3).div_ceil(4) * 4;
    let image_bytes = row_bytes * height;
    let file_bytes = 54 + image_bytes;
    let mut out = Vec::with_capacity(file_bytes as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_bytes.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&image_bytes.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..height).rev() {
        let mut written = 0u32;
        for x in 0..width {
            let i = (y as usize * width as usize + x as usize) * 3;
            out.extend_from_slice(&[rgb[i + 2], rgb[i + 1], rgb[i]]);
            written += 3;
        }
        while written < row_bytes {
            out.push(0);
            written += 1;
        }
    }
    std::fs::write(path, out)
}

#[derive(Clone, Debug, Serialize)]
pub struct MaterialMipInfo {
    pub id: u16,
    pub key: &'static str,
    pub class: &'static str,
    pub sizes: Vec<u32>,
    pub bytes: u64,
    pub coverage_error: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct TextureAuditReport {
    pub pipeline_version: u32,
    pub materials: usize,
    pub base_size: u32,
    pub base_gutter: u32,
    pub chain_levels: u32,
    pub max_padded_level: u32,
    pub uv_stable: bool,
    pub bleed: Vec<BleedLevelReport>,
    pub bleed_violations: usize,
    pub array: ArrayPlan,
    pub atlas: AtlasPlan,
    pub classic_bytes: u64,
    pub route: RouteDecision,
    pub coverage: Vec<CoverageStats>,
    pub coverage_failures: usize,
    /// Largest premultiplied-vs-naive color difference at level 1; this is the
    /// dark-halo amount the premultiplied filter removed (higher is better).
    pub max_halo_delta: f32,
    pub normal_filter_ok: bool,
    pub normal_length_error: f32,
    pub compression_hint: Vec<String>,
    pub materials_detail: Vec<MaterialMipInfo>,
    pub notes: Vec<String>,
}

impl TextureAuditReport {
    pub fn failure_count(&self) -> usize {
        self.bleed_violations
            + self.coverage_failures
            + usize::from(!self.uv_stable)
            + usize::from(!self.normal_filter_ok)
    }
}

/// Reference limits when no GPU is probed (headless audit). The runtime route
/// always comes from `RenderCapabilities::texture_route`.
pub fn reference_route_params(low_device: bool) -> RouteParams {
    RouteParams {
        materials: SURFACE_MATERIALS.len() as u32,
        base_size: 64,
        max_array_layers: if low_device { 1 } else { 2048 },
        max_texture_dimension_2d: if low_device { 2048 } else { 16_384 },
        array_filterable: true,
    }
}

/// Build every chain from the live atlas tiles. All surfaces are albedo until
/// B04/B05 declares normal/ORM maps (the classes already exist).
pub fn build_catalog_chains(atlas: &Atlas) -> Result<Vec<MipChain>, String> {
    let mut chains = Vec::with_capacity(SURFACE_MATERIALS.len());
    for material in SURFACE_MATERIALS {
        let tile = atlas
            .tile_material(material.id)
            .ok_or_else(|| format!("{} has no atlas layer", material.key))?;
        chains.push(build_chain(
            material.id,
            ChannelClass::Albedo,
            SourceImage::from_tile(tile),
            EdgeMode::Clamp,
        ));
    }
    Ok(chains)
}

fn produce(low_device: bool) -> Result<(TextureAuditReport, PaddedAtlas), String> {
    let atlas = Atlas::build();
    let chains = build_catalog_chains(&atlas)?;
    let padded = build_padded_atlas(&chains, BASE_GUTTER)?;
    let bleed = verify_bleed(&padded);
    let bleed_violations: usize = bleed.iter().map(|report| report.violations).sum();
    let array = plan_array(&chains);
    let atlas_plan = plan_atlas(&padded);
    let coverage = coverage_stats(&chains);
    let coverage_failures = coverage
        .iter()
        .filter(|stats| stats.worst_step > 0.15)
        .count();
    let max_halo_delta = coverage
        .iter()
        .map(|stats| stats.halo_delta)
        .fold(0.0f32, f32::max);
    let (normal_filter_ok, normal_length_error) = verify_normal_filter();
    let chain_levels = chains
        .iter()
        .map(|chain| chain.levels.len() as u32)
        .max()
        .unwrap_or(0);
    let materials_detail = chains
        .iter()
        .map(|chain| MaterialMipInfo {
            id: chain.material.raw(),
            key: material_key(chain.material),
            class: chain.class.key(),
            sizes: chain.levels.iter().map(|level| level.width).collect(),
            bytes: chain
                .levels
                .iter()
                .map(|level| (level.width * level.height * 4) as u64)
                .sum(),
            coverage_error: chain.coverage_error,
        })
        .collect();
    let report = TextureAuditReport {
        pipeline_version: PIPELINE_VERSION,
        materials: chains.len(),
        base_size: padded.levels[0].info.tile_size,
        base_gutter: padded.base_gutter,
        chain_levels,
        max_padded_level: padded.max_padded_level,
        uv_stable: padded.uv_stable,
        bleed,
        bleed_violations,
        array,
        atlas: atlas_plan,
        classic_bytes: LEGACY_ATLAS_BYTES,
        route: decide_route(reference_route_params(low_device)),
        coverage,
        coverage_failures,
        max_halo_delta,
        normal_filter_ok,
        normal_length_error,
        compression_hint: vec![
            "BC7/BC5 are the PC baseline; probe TEXTURE_COMPRESSION_BC at runtime".to_string(),
            "KTX2/BC artifacts are not shipped until B04/B06 add the offline encoder".to_string(),
        ],
        materials_detail,
        notes: vec![
            "per-tile mips: albedo in linear light with premultiplied alpha, normal vectors renormalized".to_string(),
            "padded atlas keeps the inner UV rect identical across levels; gutter halves exactly per level".to_string(),
            "alpha cutout coverage is preserved per level by a scaled ordered-dither remap (main and shadow share the cutoff)".to_string(),
            "route shown here uses reference limits; the runtime decision reads RenderCapabilities".to_string(),
        ],
    };
    Ok((report, padded))
}

pub fn texture_audit_report(low_device: bool) -> Result<TextureAuditReport, String> {
    produce(low_device).map(|(report, _)| report)
}

/// `--texture-audit` entry: writes the JSON report and a BMP contact sheet,
/// returns the process exit code (2 on mapping/filter failures).
pub fn run_texture_audit(low_device: bool) -> i32 {
    let (report, padded) = match produce(low_device) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("texture-audit: {err}");
            println!("TEXTURE_AUDIT_FAIL reason={err}");
            return 2;
        }
    };
    let out_dir = PathBuf::from("target").join("art-audit");
    if let Err(err) = std::fs::create_dir_all(&out_dir) {
        eprintln!("texture-audit: 无法创建 {}: {err}", out_dir.display());
    }
    let json_path = out_dir.join("texture-pipeline.json");
    match serde_json::to_string_pretty(&report) {
        Ok(json) => {
            if let Err(err) = std::fs::write(&json_path, json) {
                eprintln!("texture-audit: 写入 {} 失败: {err}", json_path.display());
            }
        }
        Err(err) => eprintln!("texture-audit: 序列化报告失败: {err}"),
    }
    let (width, height, rgb) = contact_sheet_rgb(&padded);
    let bmp_path = out_dir.join("texture-contact-sheet.bmp");
    if let Err(err) = write_bmp(&bmp_path, width, height, &rgb) {
        eprintln!("texture-audit: 写入 {} 失败: {err}", bmp_path.display());
    }
    let pbr_failures = crate::art::pbr::write_pbr_audit(&out_dir);
    println!(
        "TEXTURE_AUDIT version={} materials={} base={} levels={} padded_levels={} uv_stable={} bleed={} coverage_failures={} halo_delta={:.3} normal_ok={} route={:?} array_bytes={} atlas_bytes={} classic_bytes={}",
        report.pipeline_version,
        report.materials,
        report.base_size,
        report.chain_levels,
        report.max_padded_level + 1,
        report.uv_stable,
        report.bleed_violations,
        report.coverage_failures,
        report.max_halo_delta,
        report.normal_filter_ok,
        report.route.route,
        report.array.bytes,
        report.atlas.bytes,
        report.classic_bytes,
    );
    for stats in &report.coverage {
        println!(
            "TEXTURE_AUDIT_COVERAGE {} base..min={:.3}..{:.3} worst_step={:.3} halo_delta={:.3}",
            stats.key,
            stats.levels.first().copied().unwrap_or(0.0),
            stats.levels.last().copied().unwrap_or(0.0),
            stats.worst_step,
            stats.halo_delta,
        );
    }
    println!("TEXTURE_AUDIT report={}", json_path.display());
    println!("TEXTURE_AUDIT sheet={}", bmp_path.display());
    let failures = report.failure_count() + pbr_failures;
    if failures == 0 {
        println!("TEXTURE_AUDIT_OK");
        0
    } else {
        println!("TEXTURE_AUDIT_FAIL failures={failures}");
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_by_two(colors: [[u8; 4]; 4]) -> SourceImage {
        let mut pixels = Vec::new();
        for color in colors {
            pixels.extend_from_slice(&color);
        }
        SourceImage::new(2, 2, pixels)
    }

    #[test]
    fn srgb_downsample_averages_in_linear_light() {
        let source = two_by_two([
            [255, 255, 255, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
        ]);
        let level = downsample(&source, ChannelClass::Albedo, EdgeMode::Clamp);
        // 25% white in linear space is 0.25 -> sRGB ~137, not the naive 64.
        let value = level.pixels[0];
        assert!(
            (130..=145).contains(&value),
            "linear-light filter expected ~137, got {value}"
        );
    }

    #[test]
    fn premultiplied_albedo_avoids_dark_halo() {
        let source = two_by_two([
            [255, 255, 255, 255],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
        ]);
        let level = downsample(&source, ChannelClass::Albedo, EdgeMode::Clamp);
        assert!(level.pixels[0] > 240, "opaque color must survive the edge");
        let naive = downsample(&source, ChannelClass::LinearData, EdgeMode::Clamp);
        assert!(naive.pixels[0] < 140, "naive average would darken");
    }

    #[test]
    fn linear_data_does_not_apply_srgb_curve() {
        let source = two_by_two([
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            [0, 0, 0, 255],
            [255, 255, 255, 255],
        ]);
        let level = downsample(&source, ChannelClass::LinearData, EdgeMode::Clamp);
        assert!(
            (125..=130).contains(&level.pixels[0]),
            "ORM channels average linearly, got {}",
            level.pixels[0]
        );
    }

    #[test]
    fn normal_downsample_renormalizes_and_falls_back() {
        let (ok, error) = verify_normal_filter();
        assert!(ok, "normal filter error {error}");
        // A zero-length average must not produce NaN or a zero vector.
        let source = two_by_two([
            [128, 128, 0, 255],
            [128, 128, 0, 255],
            [128, 128, 0, 255],
            [128, 128, 0, 255],
        ]);
        let level = downsample(&source, ChannelClass::Normal, EdgeMode::Clamp);
        assert_eq!(level.pixels[2], 255, "z must stay reconstructed");
        assert!(
            (127..=129).contains(&level.pixels[0]) && (127..=129).contains(&level.pixels[1]),
            "xy must stay near neutral, got {:?}",
            &level.pixels[0..3]
        );
    }

    #[test]
    fn padded_tile_extrudes_its_own_border() {
        let mut source = SourceImage::new(4, 4, vec![0; 64]);
        for y in 0..4 {
            for x in 0..4 {
                source.set(x, y, [(x * 40 + 20) as u8, (y * 40 + 20) as u8, 0, 255]);
            }
        }
        let padded = pad_tile(&source, 2);
        assert_eq!(padded.padded_size(), 8);
        assert_eq!(padded.pixel(0, 0), source.pixel(0, 0, EdgeMode::Clamp));
        assert_eq!(padded.pixel(7, 7), source.pixel(3, 3, EdgeMode::Clamp));
        assert_eq!(padded.pixel(0, 3), source.pixel(0, 1, EdgeMode::Clamp));
    }

    #[test]
    fn chain_reaches_one_pixel_and_alpha_stays_in_range() {
        let tile = crate::textures::Atlas::build();
        let material = crate::art::material_by_key("leaves").unwrap();
        let source = SourceImage::from_tile(tile.tile_material(material.id).unwrap());
        let chain = build_chain(material.id, ChannelClass::Albedo, source, EdgeMode::Clamp);
        assert_eq!(chain.levels.len(), 5);
        assert_eq!(chain.levels.last().unwrap().width, 1);
        for level in &chain.levels {
            assert_eq!(
                level.pixels.len(),
                (level.width * level.height * 4) as usize
            );
        }
    }

    #[test]
    fn cutout_coverage_survives_mips() {
        let atlas = Atlas::build();
        let material = crate::art::material_by_key("leaves").unwrap();
        let source = SourceImage::from_tile(atlas.tile_material(material.id).unwrap());
        let chain = build_chain(material.id, ChannelClass::Albedo, source, EdgeMode::Clamp);
        let base = threshold_coverage(&chain.levels[0].pixels, ALPHA_CUTOFF);
        assert!(base > 0.5 && base < 1.0, "leaves must be a cutout, {base}");
        let stats = coverage_stats(std::slice::from_ref(&chain));
        assert_eq!(stats.len(), 1);
        assert!(
            stats[0].worst_step <= 0.15,
            "coverage drifted across mips: {stats:?}"
        );
    }

    #[test]
    fn alpha_fix_is_a_noop_for_opaque_and_empty_tiles() {
        let opaque = SourceImage::new(2, 2, vec![255; 16]);
        let mut level = SourceImage::new(1, 1, vec![255, 255, 255, 255]);
        assert_eq!(preserve_alpha_coverage(&mut level, 1.0), 0.0);
        assert_eq!(level.pixels[3], 255);
        assert_eq!(threshold_coverage(&opaque.pixels, ALPHA_CUTOFF), 1.0);
        let mut empty = SourceImage::new(1, 1, vec![0, 0, 0, 0]);
        assert_eq!(preserve_alpha_coverage(&mut empty, 0.0), 0.0);
        assert_eq!(empty.pixels[3], 0);
    }

    #[test]
    fn padded_atlas_keeps_uv_stable_and_bleed_zero() {
        let atlas = Atlas::build();
        let chains = build_catalog_chains(&atlas).unwrap();
        let padded = build_padded_atlas(&chains, BASE_GUTTER).unwrap();
        assert!(padded.uv_stable, "inner UV rect moved across levels");
        assert_eq!(padded.columns, 8);
        assert_eq!(padded.rows, 8);
        assert_eq!(padded.max_padded_level, 2);
        let bleed = verify_bleed(&padded);
        assert_eq!(bleed.len(), 3);
        assert!(
            bleed
                .iter()
                .all(|level| level.violations == 0 && level.gutter >= 1)
        );
        assert_eq!(
            (padded.levels[0].info.width, padded.levels[1].info.width),
            (8 * (16 + 8), 8 * (8 + 4))
        );
    }

    #[test]
    fn cross_tile_bilinear_sample_reads_only_its_own_gutter() {
        // Two 2×2 tiles, red and blue, gutter 1. A sample whose center sits on
        // red's right inner edge blends with red's extruded gutter.
        let mut red = SourceImage::new(2, 2, vec![0; 16]);
        let mut blue = SourceImage::new(2, 2, vec![0; 16]);
        for y in 0..2 {
            for x in 0..2 {
                red.set(x, y, [255, 0, 0, 255]);
                blue.set(x, y, [0, 0, 255, 255]);
            }
        }
        let red_padded = pad_tile(&red, 1);
        let blue_padded = pad_tile(&blue, 1);
        for y in 0..4 {
            assert_eq!(
                red_padded.pixel(3, y as i64),
                [255, 0, 0, 255],
                "red gutter must not leak blue"
            );
            assert_eq!(blue_padded.pixel(0, y as i64), [0, 0, 255, 255]);
        }
    }

    #[test]
    fn byte_budget_matches_manual_sum() {
        let atlas = Atlas::build();
        let chains = build_catalog_chains(&atlas).unwrap();
        let array = plan_array(&chains);
        assert_eq!(array.layers as usize, SURFACE_MATERIALS.len());
        assert_eq!(array.levels, 5);
        let manual: u64 = chains
            .iter()
            .flat_map(|chain| chain.levels.iter())
            .map(|level| (level.width * level.height * 4) as u64)
            .sum();
        assert_eq!(array.bytes, manual);
        let padded = build_padded_atlas(&chains, BASE_GUTTER).unwrap();
        let atlas_plan = plan_atlas(&padded);
        let manual_atlas: u64 = padded
            .levels
            .iter()
            .map(|level| (level.info.width * level.info.height * 4) as u64)
            .sum();
        assert_eq!(atlas_plan.bytes, manual_atlas);
        assert!(atlas_plan.bytes > array.bytes);
        assert!(array.bytes > 0 && atlas_plan.bytes < LEGACY_ATLAS_BYTES * 4);
    }

    #[test]
    fn route_falls_back_with_a_reason() {
        let capable = reference_route_params(false);
        assert_eq!(decide_route(capable).route, TextureRoute::TextureArray);
        let low = reference_route_params(true);
        let decision = decide_route(low);
        assert_eq!(decision.route, TextureRoute::PaddedAtlas);
        assert!(decision.reason.contains("array"), "{}", decision.reason);

        let narrow = RouteParams {
            materials: 62,
            base_size: 64,
            max_array_layers: 1,
            max_texture_dimension_2d: 64,
            array_filterable: true,
        };
        assert_eq!(decide_route(narrow).route, TextureRoute::ClassicAtlas);
        let unfilterable = RouteParams {
            array_filterable: false,
            ..capable
        };
        assert_eq!(decide_route(unfilterable).route, TextureRoute::PaddedAtlas);
    }

    #[test]
    fn contact_sheet_and_bmp_are_written() {
        let atlas = Atlas::build();
        let chains = build_catalog_chains(&atlas).unwrap();
        let padded = build_padded_atlas(&chains, BASE_GUTTER).unwrap();
        let (width, height, rgb) = contact_sheet_rgb(&padded);
        assert_eq!(width, padded.levels[0].info.width);
        assert_eq!(
            height,
            padded
                .levels
                .iter()
                .map(|level| level.info.height)
                .sum::<u32>()
                + padded.levels[1].info.height
                + padded.levels[2].info.height * 3
        );
        assert_eq!(rgb.len(), (width * height * 3) as usize);
        let dir = std::env::temp_dir().join(format!("starforge-b03-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("sheet.bmp");
        write_bmp(&path, width, height, &rgb).expect("bmp");
        let bytes = std::fs::read(&path).expect("read bmp");
        assert_eq!(&bytes[0..2], b"BM");
        assert_eq!(
            u32::from_le_bytes(bytes[2..6].try_into().unwrap()) as usize,
            bytes.len()
        );
        assert_eq!(u32::from_le_bytes(bytes[18..22].try_into().unwrap()), width);
        assert_eq!(
            u32::from_le_bytes(bytes[22..26].try_into().unwrap()),
            height
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audit_report_covers_every_catalog_material() {
        let report = texture_audit_report(false).expect("report");
        assert_eq!(report.materials, SURFACE_MATERIALS.len());
        assert_eq!(report.materials_detail.len(), SURFACE_MATERIALS.len());
        assert_eq!(report.failure_count(), 0, "{report:?}");
        assert_eq!(report.route.route, TextureRoute::TextureArray);
        assert!(report.normal_filter_ok);
        assert!(report.uv_stable);
        assert!(report.bleed_violations == 0);
        assert!(!report.coverage.is_empty(), "leaves/plants must be cutouts");
        let json = serde_json::to_string(&report).expect("serialize");
        assert!(json.contains("padded_atlas") || json.contains("texture_array"));
    }
}
