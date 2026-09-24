//! B04 PBR family profiles, per-surface parameters and the procedural
//! normal/ORM/emission atlases.
//!
//! `01-ART-DIRECTION-AND-ASSETS.md` §1.4 fixes a roughness range and metallic
//! policy per material family; this module turns that table into data and
//! derives one [`SurfacePbr`] per [`crate::art::catalog::SurfaceMaterialId`].
//! Maps are generated CPU-side into the same 16×16 grid as the legacy atlas so
//! the existing UV rects keep working:
//!
//! - **normal**: height from albedo luma → Sobel → unit tangent-space vector,
//!   scaled by the family strength (Bevy 0.19 has no normal-strength scalar).
//! - **ORM**: `R = 255` (no baked occlusion on purpose, so vertex AO is not
//!   doubled, R040), `G = roughness`, `B = metallic`.
//! - **emission**: white mask of the bright texels; the actual HDR color and
//!   tier scale stay on `StandardMaterial::emissive` so "ordinary surfaces do
//!   not glow" is a data rule, not an art hope.
//!
//! The shipping terrain material currently consumes only the ORM atlas; normal
//! maps require vertex tangents (C02) and emission wiring is D04/H04, so the
//! other maps are produced and reviewed in the B04 courtyard first.

use bevy::image::{ImageFilterMode, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Serialize;

use super::catalog::{
    MaterialFamily, SURFACE_MATERIALS, SurfaceMaterial, SurfaceMaterialId, material_by_key,
};
use super::style::EMISSION_TIERS;
use super::texture_pipeline::srgb_to_linear;
use crate::textures::{Atlas, Pixel};

pub const PBR_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PbrFamilyProfile {
    pub family: MaterialFamily,
    /// Mid roughness inside the family review range.
    pub roughness_base: f32,
    /// Peak-to-peak roughness variation driven by local albedo detail.
    pub roughness_variance: f32,
    /// Level-0 metallic for the family policy (`High`), 0 for paint/most.
    pub metallic: f32,
    /// Relief amount baked into the normal map (no shader scalar in 0.19).
    pub normal_strength: f32,
    /// Required detail from the 01 §1.4 table, kept next to the numbers.
    pub detail: &'static str,
}

/// Frozen family table. Values stay inside `MaterialFamily::roughness_range()`
/// after variance/clamping (audit enforced).
pub const PBR_FAMILIES: &[PbrFamilyProfile] = &[
    PbrFamilyProfile {
        family: MaterialFamily::Soil,
        roughness_base: 0.88,
        roughness_variance: 0.10,
        metallic: 0.0,
        normal_strength: 0.35,
        detail: "大颗粒层次、低高光；砂坡掠射角不闪烁",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Rock,
        roughness_base: 0.80,
        roughness_variance: 0.14,
        metallic: 0.0,
        normal_strength: 0.55,
        detail: "裂缝、层理、断面；不能全靠 AO 涂黑边",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Grass,
        roughness_base: 0.90,
        roughness_variance: 0.08,
        metallic: 0.0,
        normal_strength: 0.30,
        detail: "顶面与侧面过渡、根部暗部、少量色差",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Wood,
        roughness_base: 0.70,
        roughness_variance: 0.12,
        metallic: 0.0,
        normal_strength: 0.40,
        detail: "顺纹/端纹不同、接缝有方向",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Painted,
        roughness_base: 0.48,
        roughness_variance: 0.12,
        metallic: 0.0,
        normal_strength: 0.35,
        detail: "边缘有限磨损、缝线、标识；不整机金属化",
    },
    PbrFamilyProfile {
        family: MaterialFamily::BareMetal,
        roughness_base: 0.35,
        roughness_variance: 0.10,
        metallic: 0.85,
        normal_strength: 0.25,
        detail: "有环境反射才能审；拉丝方向一致",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Glass,
        roughness_base: 0.10,
        roughness_variance: 0.03,
        metallic: 0.0,
        normal_strength: 0.08,
        detail: "厚度边缘/反射可读，透明与碰撞一致",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Water,
        roughness_base: 0.12,
        roughness_variance: 0.03,
        metallic: 0.0,
        normal_strength: 0.15,
        detail: "Fresnel、吸收、浅水过渡，不用金属度伪造亮度",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Ice,
        roughness_base: 0.28,
        roughness_variance: 0.08,
        metallic: 0.0,
        normal_strength: 0.30,
        detail: "裂纹/气泡深浅层，不能像蓝塑料",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Snow,
        roughness_base: 0.92,
        roughness_variance: 0.05,
        metallic: 0.0,
        normal_strength: 0.20,
        detail: "雪面大形/阴影、抑制微亮点闪烁",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Crystal,
        roughness_base: 0.25,
        roughness_variance: 0.08,
        metallic: 0.0,
        normal_strength: 0.45,
        detail: "刻面法线、内部发光 mask、有限灯光贡献",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Energy,
        roughness_base: 0.40,
        roughness_variance: 0.10,
        metallic: 0.0,
        normal_strength: 0.25,
        detail: "亮区保色、黑壳覆盖、Bloom 不吞轮廓",
    },
    PbrFamilyProfile {
        family: MaterialFamily::Foliage,
        roughness_base: 0.78,
        roughness_variance: 0.10,
        metallic: 0.0,
        normal_strength: 0.40,
        detail: "alpha 轮廓、背光近似、背面 normal",
    },
];

pub fn profile(family: MaterialFamily) -> &'static PbrFamilyProfile {
    PBR_FAMILIES
        .iter()
        .find(|profile| profile.family == family)
        .expect("every MaterialFamily has a PBR profile")
}

/// Alpha handling for the material prototype; `StandardMaterial` still uses
/// `Mask(0.4)` for cutouts so main and shadow share coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PbrAlpha {
    Opaque,
    Cutout,
    Blend,
}

impl PbrAlpha {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Opaque => "opaque",
            Self::Cutout => "cutout",
            Self::Blend => "blend",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SurfacePbr {
    pub id: SurfaceMaterialId,
    pub key: &'static str,
    pub family: MaterialFamily,
    pub roughness: f32,
    pub metallic: f32,
    pub normal_strength: f32,
    pub alpha: PbrAlpha,
    pub emission: Option<&'static str>,
}

/// Per-surface roughness refinements inside the family range (glass/water are
/// smoother than their family midpoint, stone slabs are rougher).
fn roughness_override(key: &str) -> Option<f32> {
    match key {
        "water" => Some(0.10),
        "glass" => Some(0.06),
        "ice" => Some(0.24),
        "crystal" => Some(0.22),
        "lamp_on" => Some(0.38),
        "furnace_on" => Some(0.42),
        "metal" => Some(0.34),
        "metal_dark" => Some(0.42),
        "wind_pole" => Some(0.38),
        "slab" => Some(0.84),
        "concrete" => Some(0.62),
        "rust" => Some(0.85),
        _ => None,
    }
}

fn alpha_for(material: &SurfaceMaterial) -> PbrAlpha {
    match material.key {
        "water" => PbrAlpha::Blend,
        "glass" => PbrAlpha::Cutout,
        _ if material.family == MaterialFamily::Foliage => PbrAlpha::Cutout,
        _ => PbrAlpha::Opaque,
    }
}

pub fn surface_pbr(material: &SurfaceMaterial) -> SurfacePbr {
    let profile = profile(material.family);
    let roughness = roughness_override(material.key).unwrap_or(profile.roughness_base);
    SurfacePbr {
        id: material.id,
        key: material.key,
        family: material.family,
        roughness,
        metallic: profile.metallic,
        normal_strength: profile.normal_strength,
        alpha: alpha_for(material),
        emission: material.emission,
    }
}

fn emission_color(key: &str) -> [f32; 3] {
    match key {
        "lamp_on" => [1.00, 0.85, 0.55],
        "glow_shroom" => [0.35, 1.00, 0.75],
        "crystal" => [0.45, 0.95, 0.95],
        "furnace_on" => [1.00, 0.45, 0.12],
        "murk_top" => [0.30, 0.95, 0.72],
        "uranium_ore" => [0.55, 0.95, 0.35],
        "amber" => [1.00, 0.72, 0.25],
        "barrier" => [0.45, 0.75, 1.00],
        _ => [1.00, 1.00, 1.00],
    }
}

/// HDR emissive tint (color × tier luminance) for a surface with an emission
/// tier. `None` when the catalog does not mark the material as emitting.
pub fn emission_tint(pbr: &SurfacePbr) -> Option<LinearRgba> {
    let tier = pbr.emission?;
    let scale = EMISSION_TIERS
        .iter()
        .find(|tier_entry| tier_entry.key == tier)
        .map(|tier_entry| tier_entry.relative_luminance)
        .unwrap_or(1.0);
    let color = emission_color(pbr.key);
    Some(LinearRgba::rgb(
        color[0] * scale,
        color[1] * scale,
        color[2] * scale,
    ))
}

fn luma(pixel: Pixel) -> f32 {
    let r = srgb_to_linear(pixel[0] as f32 / 255.0);
    let g = srgb_to_linear(pixel[1] as f32 / 255.0);
    let b = srgb_to_linear(pixel[2] as f32 / 255.0);
    r * 0.2126 + g * 0.7152 + b * 0.0722
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0).max(f32::EPSILON)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Tangent-space normal map for one 16×16 tile, derived from albedo relief.
pub fn build_normal_map(tile: &[Pixel; 256], strength: f32) -> Vec<u8> {
    let height = |x: i64, y: i64| -> f32 {
        let x = x.rem_euclid(16) as usize;
        let y = y.rem_euclid(16) as usize;
        luma(tile[y * 16 + x])
    };
    let mut out = vec![0u8; 256 * 4];
    for y in 0..16i64 {
        for x in 0..16i64 {
            let dx = height(x + 1, y) - height(x - 1, y);
            let dy = height(x, y + 1) - height(x, y - 1);
            let scale = strength * 6.0;
            let mut normal = Vec3::new(-dx * scale, -dy * scale, 1.0).normalize_or_zero();
            if normal.length_squared() < 0.5 {
                normal = Vec3::Z;
            }
            let i = (y as usize * 16 + x as usize) * 4;
            out[i] = ((normal.x * 0.5 + 0.5) * 255.0).round() as u8;
            out[i + 1] = ((normal.y * 0.5 + 0.5) * 255.0).round() as u8;
            out[i + 2] = ((normal.z * 0.5 + 0.5) * 255.0).round() as u8;
            out[i + 3] = 255;
        }
    }
    out
}

/// ORM map for one tile: R=occlusion (deliberately 255), G=roughness,
/// B=metallic.
pub fn build_orm_map(tile: &[Pixel; 256], pbr: &SurfacePbr) -> Vec<u8> {
    let profile = profile(pbr.family);
    let mean: f32 = tile.iter().map(|pixel| luma(*pixel)).sum::<f32>() / 256.0;
    let (min, max) = pbr.family.roughness_range();
    let mut out = vec![0u8; 256 * 4];
    for (index, pixel) in tile.iter().enumerate() {
        let detail = (luma(*pixel) - mean) * 2.0;
        let roughness = (pbr.roughness + detail * profile.roughness_variance).clamp(min, max);
        let i = index * 4;
        out[i] = 255;
        out[i + 1] = (roughness * 255.0).round() as u8;
        out[i + 2] = (pbr.metallic * 255.0).round() as u8;
        out[i + 3] = 255;
    }
    out
}

/// Emission mask (white where the surface emits); the HDR tint lives on the
/// material so ordinary tiles cannot accidentally glow.
pub fn build_emission_map(tile: &[Pixel; 256], pbr: &SurfacePbr) -> Vec<u8> {
    let mut out = vec![0u8; 256 * 4];
    if pbr.emission.is_none() {
        return out;
    }
    for (index, pixel) in tile.iter().enumerate() {
        let coverage = smoothstep(0.55, 0.85, luma(*pixel));
        let value = (coverage * 255.0).round() as u8;
        let i = index * 4;
        out[i] = value;
        out[i + 1] = value;
        out[i + 2] = value;
        out[i + 3] = 255;
    }
    out
}

fn atlas_cell(atlas: &Atlas, key: &str) -> Option<usize> {
    atlas.index.get(key).copied()
}

/// Build a 256×256 atlas aligned with the albedo UV rects by reusing each
/// tile's registration cell.
fn build_atlas_with<F>(atlas: &Atlas, mut paint: F) -> Vec<u8>
where
    F: FnMut(&SurfaceMaterial, &[Pixel; 256]) -> Vec<u8>,
{
    let mut out = vec![0u8; 256 * 256 * 4];
    for material in SURFACE_MATERIALS {
        let Some(cell) = atlas_cell(atlas, material.key) else {
            continue;
        };
        let Some(tile) = atlas
            .index
            .get(material.key)
            .map(|index| &atlas.tiles[*index])
        else {
            continue;
        };
        let pixels = paint(material, tile);
        let col = cell % 16;
        let row = cell / 16;
        for y in 0..16 {
            let src = y * 16 * 4;
            let dst = ((row * 16 + y) * 256 + col * 16) * 4;
            out[dst..dst + 16 * 4].copy_from_slice(&pixels[src..src + 16 * 4]);
        }
    }
    out
}

pub fn build_orm_atlas(atlas: &Atlas) -> Vec<u8> {
    build_atlas_with(atlas, |material, tile| {
        build_orm_map(tile, &surface_pbr(material))
    })
}

pub fn build_normal_atlas(atlas: &Atlas) -> Vec<u8> {
    build_atlas_with(atlas, |material, tile| {
        build_normal_map(tile, surface_pbr(material).normal_strength)
    })
}

pub fn build_emission_atlas(atlas: &Atlas) -> Vec<u8> {
    build_atlas_with(atlas, |material, tile| {
        build_emission_map(tile, &surface_pbr(material))
    })
}

/// 16×16 RGBA image from a map tile. ORM/normal data is linear; albedo is sRGB.
pub fn image_from_rgba(size: u32, pixels: Vec<u8>, srgb: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        if srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    let mut sampler = ImageSampler::default();
    let descriptor = sampler.get_or_init_descriptor();
    descriptor.mag_filter = ImageFilterMode::Linear;
    descriptor.min_filter = ImageFilterMode::Linear;
    descriptor.mipmap_filter = ImageFilterMode::Nearest;
    image.sampler = sampler;
    image
}

/// Most characteristic material for a family; used by the courtyard samples
/// and by the family audit.
pub fn family_representative(family: MaterialFamily) -> SurfaceMaterialId {
    let key = match family {
        MaterialFamily::Soil => "dirt",
        MaterialFamily::Rock => "stone",
        MaterialFamily::Grass => "grass_top",
        MaterialFamily::Wood => "planks",
        MaterialFamily::Painted => "vent",
        MaterialFamily::BareMetal => "metal",
        MaterialFamily::Glass => "glass",
        MaterialFamily::Water => "water",
        MaterialFamily::Ice => "ice",
        MaterialFamily::Snow => "snow_top",
        MaterialFamily::Crystal => "crystal",
        MaterialFamily::Energy => "lamp_on",
        MaterialFamily::Foliage => "leaves",
    };
    material_by_key(key)
        .unwrap_or_else(|| panic!("family {family:?} representative {key} missing"))
        .id
}

#[derive(Clone, Debug, Serialize)]
pub struct PbrAuditReport {
    pub schema_version: u32,
    pub families: Vec<PbrFamilyProfile>,
    pub materials: Vec<SurfacePbr>,
    pub family_range_ok: bool,
    pub non_metal_zero: bool,
    pub emission_tier_only: bool,
    pub notes: Vec<String>,
}

impl PbrAuditReport {
    pub fn failure_count(&self) -> usize {
        usize::from(!self.family_range_ok)
            + usize::from(!self.non_metal_zero)
            + usize::from(!self.emission_tier_only)
    }
}

pub fn pbr_audit() -> PbrAuditReport {
    let mut family_range_ok = true;
    let mut non_metal_zero = true;
    let mut emission_tier_only = true;
    let tiers: std::collections::BTreeSet<&str> =
        EMISSION_TIERS.iter().map(|tier| tier.key).collect();
    let materials: Vec<SurfacePbr> = SURFACE_MATERIALS.iter().map(surface_pbr).collect();
    for pbr in &materials {
        let (min, max) = pbr.family.roughness_range();
        if pbr.roughness < min - 1e-4 || pbr.roughness > max + 1e-4 {
            family_range_ok = false;
        }
        if pbr.metallic > 0.0
            && pbr.family.metallic_policy() == super::catalog::MetallicPolicy::Zero
        {
            non_metal_zero = false;
        }
        match pbr.emission {
            Some(tier) if tiers.contains(tier) => {}
            Some(_) => emission_tier_only = false,
            None => {}
        }
    }
    PbrAuditReport {
        schema_version: PBR_SCHEMA_VERSION,
        families: PBR_FAMILIES.to_vec(),
        materials,
        family_range_ok,
        non_metal_zero,
        emission_tier_only,
        notes: vec![
            "occlusion R is 255: vertex AO is the single AO source so it cannot be doubled (R040)"
                .to_string(),
            "normal maps need vertex tangents; the shipping terrain mesh gets them in C02"
                .to_string(),
            "emission mask is generated, but the HDR tint and real lights are wired by D04/H04"
                .to_string(),
        ],
    }
}

/// Write `pbr-families.json` next to the texture audit; returns the failure
/// count so `--texture-audit` can include it in the exit code.
pub fn write_pbr_audit(out_dir: &std::path::Path) -> usize {
    let report = pbr_audit();
    let path = out_dir.join("pbr-families.json");
    match serde_json::to_string_pretty(&report) {
        Ok(json) => {
            if let Err(err) = std::fs::write(&path, json) {
                eprintln!("pbr-audit: 写入 {} 失败: {err}", path.display());
            }
        }
        Err(err) => eprintln!("pbr-audit: 序列化报告失败: {err}"),
    }
    println!(
        "TEXTURE_AUDIT_PBR families={} materials={} family_range_ok={} non_metal_zero={} emission_tier_only={} report={}",
        report.families.len(),
        report.materials.len(),
        report.family_range_ok,
        report.non_metal_zero,
        report.emission_tier_only,
        path.display(),
    );
    report.failure_count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::art::catalog::material_by_id;

    #[test]
    fn every_family_has_exactly_one_profile_inside_its_range() {
        assert_eq!(PBR_FAMILIES.len(), MaterialFamily::ALL.len());
        let mut seen: Vec<MaterialFamily> = Vec::new();
        for entry in PBR_FAMILIES {
            assert!(
                !seen.contains(&entry.family),
                "duplicate {:?}",
                entry.family
            );
            seen.push(entry.family);
            let (min, max) = entry.family.roughness_range();
            assert!(
                (min..=max).contains(&entry.roughness_base),
                "{:?} base {} outside [{min},{max}]",
                entry.family,
                entry.roughness_base
            );
            assert!(
                entry.roughness_base + entry.roughness_variance <= max + 1e-4,
                "{:?} variance escapes the review range",
                entry.family
            );
            assert!(!entry.detail.is_empty());
        }
        for family in MaterialFamily::ALL {
            assert!(seen.contains(&family), "missing {family:?}");
        }
    }

    #[test]
    fn surface_values_stay_inside_family_policy() {
        let report = pbr_audit();
        assert_eq!(report.materials.len(), SURFACE_MATERIALS.len());
        assert_eq!(report.failure_count(), 0, "{report:?}");
        for pbr in &report.materials {
            let (min, max) = pbr.family.roughness_range();
            assert!((min - 1e-4..=max + 1e-4).contains(&pbr.roughness));
            if pbr.family.metallic_policy() == super::super::catalog::MetallicPolicy::Zero {
                assert_eq!(pbr.metallic, 0.0, "{} must stay non-metal", pbr.key);
            }
            if pbr.key == "metal" {
                assert!(pbr.metallic > 0.5, "bare metal must be metallic");
            }
        }
    }

    #[test]
    fn alpha_modes_match_the_legacy_material_split() {
        let water = material_by_key("water").unwrap();
        assert_eq!(surface_pbr(water).alpha, PbrAlpha::Blend);
        let leaves = material_by_key("leaves").unwrap();
        assert_eq!(surface_pbr(leaves).alpha, PbrAlpha::Cutout);
        let grass = material_by_key("grass_top").unwrap();
        assert_eq!(surface_pbr(grass).alpha, PbrAlpha::Opaque);
    }

    #[test]
    fn normal_maps_are_unit_and_neutral_for_flat_tiles() {
        let flat = [[128u8, 128, 128, 255]; 256];
        let map = build_normal_map(&flat, 0.5);
        for pixel in map.chunks_exact(4) {
            let x = pixel[0] as f32 / 255.0 * 2.0 - 1.0;
            let y = pixel[1] as f32 / 255.0 * 2.0 - 1.0;
            let z = pixel[2] as f32 / 255.0 * 2.0 - 1.0;
            let length = (x * x + y * y + z * z).sqrt();
            assert!((length - 1.0).abs() < 0.02, "length {length}");
            assert!(z > 0.9, "flat tile must stay near +Z");
        }
        // A height step produces a real tilt.
        let mut step = [[128u8, 128, 128, 255]; 256];
        for y in 0..16 {
            for x in 8..16 {
                step[y * 16 + x] = [255, 255, 255, 255];
            }
        }
        let tilted = build_normal_map(&step, 0.5);
        let edge = (7 * 16 + 8) * 4;
        assert!(tilted[edge + 2] < 250, "edge texel must tilt away from +Z");
    }

    #[test]
    fn orm_channels_are_occlusion_roughness_metallic() {
        let tile = [[120u8, 120, 120, 255]; 256];
        let material = material_by_key("stone").unwrap();
        let map = build_orm_map(&tile, &surface_pbr(material));
        let (min, max) = MaterialFamily::Rock.roughness_range();
        for pixel in map.chunks_exact(4) {
            assert_eq!(pixel[0], 255, "occlusion must not be baked (R040)");
            assert!(
                (min * 255.0 - 1.0..=max * 255.0 + 1.0).contains(&(pixel[1] as f32)),
                "roughness {} escaped the family range",
                pixel[1]
            );
            assert_eq!(pixel[2], 0, "stone is non-metal");
        }
        let metal = material_by_key("metal").unwrap();
        let metal_map = build_orm_map(&tile, &surface_pbr(metal));
        assert_eq!(metal_map[2], (0.85f32 * 255.0).round() as u8);
    }

    #[test]
    fn only_emission_tier_materials_get_a_mask_and_hdr_tint() {
        let bright = [[255u8, 255, 255, 255]; 256];
        let plain = material_by_key("stone").unwrap();
        assert!(
            build_emission_map(&bright, &surface_pbr(plain))
                .chunks_exact(4)
                .all(|pixel| pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0)
        );
        assert!(emission_tint(&surface_pbr(plain)).is_none());

        let lamp = material_by_key("lamp_on").unwrap();
        let pbr = surface_pbr(lamp);
        assert_eq!(pbr.emission, Some("lamp"));
        let mask = build_emission_map(&bright, &pbr);
        assert_eq!(mask[0], 255);
        let tint = emission_tint(&pbr).expect("lamp tint");
        assert!(tint.red > 1.0, "lamp tier must exceed LDR range");
        let dark = [[0u8, 0, 0, 0]; 256];
        assert!(
            build_emission_map(&dark, &pbr)
                .chunks_exact(4)
                .all(|pixel| pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0),
            "dark texels must not glow"
        );
    }

    #[test]
    fn atlases_align_with_albedo_cells_and_cover_every_material() {
        let atlas = Atlas::build();
        for (name, builder) in [
            ("orm", build_orm_atlas as fn(&Atlas) -> Vec<u8>),
            ("normal", build_normal_atlas),
        ] {
            let bytes = builder(&atlas);
            assert_eq!(bytes.len(), 256 * 256 * 4, "{name}");
            for material in SURFACE_MATERIALS {
                let cell = atlas.index[material.key];
                let col = cell % 16;
                let row = cell / 16;
                let i = (row * 16 * 256 + col * 16) * 4;
                let is_set = bytes[i..i + 4].iter().any(|byte| *byte != 0);
                assert!(is_set, "{name} cell for {} is empty", material.key);
            }
        }
        // The emission atlas only lights emission-tier cells; a specific tile
        // may still be dark (barrier is a dark plate), so the contract is
        // "non-emissive cells are black" plus at least one lit cell overall.
        let emission = build_emission_atlas(&atlas);
        let mut lit_cells = 0usize;
        for material in SURFACE_MATERIALS {
            let cell = atlas.index[material.key];
            let col = cell % 16;
            let row = cell / 16;
            let mut lit = false;
            for y in 0..16 {
                for x in 0..16 {
                    let i = ((row * 16 + y) * 256 + col * 16 + x) * 4;
                    lit |= emission[i..i + 3].iter().any(|byte| *byte != 0);
                }
            }
            if lit {
                lit_cells += 1;
            }
            assert!(
                material.emission.is_some() || !lit,
                "non-emissive {} must not have a mask",
                material.key
            );
        }
        assert!(
            lit_cells > 0,
            "at least one emission tier cell must light up"
        );
    }

    #[test]
    fn family_representatives_exist_and_match_their_family() {
        for family in MaterialFamily::ALL {
            let id = family_representative(family);
            let material = material_by_id(id).expect("representative material");
            assert_eq!(material.family, family, "{}", material.key);
        }
    }
}
