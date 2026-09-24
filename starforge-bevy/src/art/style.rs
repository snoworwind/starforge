//! B01 style reference: global base palette, emission tiers and scale anchors.
//!
//! These constants are the single source the later B/C/F/G/H/I streams read
//! instead of hard-coding a second set of colors or sizes. Biome-specific
//! palettes are F01's deliverable; this file only freezes the global baseline
//! from `docs/art-overhaul/01-ART-DIRECTION-AND-ASSETS.md` §1.1 and the real
//! size anchors from the gameplay code.
//!
//! See `art/style-guide.md` for the human-readable rules that consume these
//! values.

use serde::Serialize;

/// Increments when a change alters the meaning of an art ID, palette slot or
/// manifest field. Consumers compare it before mixing cached artifacts.
pub const ART_STYLE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PaletteSwatch {
    pub key: &'static str,
    pub srgb: [u8; 3],
    /// Where the color is allowed to appear. "Danger only" is a hard rule:
    /// the rest of the world must not use hazard hues for decoration.
    pub usage: &'static str,
}

/// Global baseline. Cyan is reserved for interaction, amber for energy and
/// guidance, danger hues only for hazards.
pub const BASE_PALETTE: &[PaletteSwatch] = &[
    PaletteSwatch {
        key: "metal_deep_blue_gray",
        srgb: [0x3a, 0x47, 0x58],
        usage: "machine hulls, structural metal, data panels",
    },
    PaletteSwatch {
        key: "rock_warm_gray",
        srgb: [0x8a, 0x81, 0x78],
        usage: "rock, basalt, concrete and stone families",
    },
    PaletteSwatch {
        key: "wood_warm",
        srgb: [0xa8, 0x7c, 0x4f],
        usage: "planks, structural wood, crates",
    },
    PaletteSwatch {
        key: "interact_cyan",
        srgb: [0x35, 0xe0, 0xe8],
        usage: "interaction highlights, scan glyphs, friendly UI accents",
    },
    PaletteSwatch {
        key: "energy_amber",
        srgb: [0xff, 0xb3, 0x47],
        usage: "energy sources, guided objectives, warm interior light",
    },
    PaletteSwatch {
        key: "danger_red",
        srgb: [0xff, 0x6a, 0x5e],
        usage: "hazards, damage and destructive warnings only",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct EmissionTier {
    pub key: &'static str,
    /// Relative emitted luminance in the scene-linear range; the exposure
    /// baseline lives in `daynight.rs`, not here.
    pub relative_luminance: f32,
    /// Whether the tier is expected to contribute to Bloom prefilter.
    pub bloom: bool,
    /// Whether the tier should register a real punctual light (budgeted by
    /// D04), instead of relying on emissive surface shading.
    pub allocates_light: bool,
}

/// Emission levels from 01 §1.1.5. Ordinary white walls must never glow.
pub const EMISSION_TIERS: &[EmissionTier] = &[
    EmissionTier {
        key: "marker",
        relative_luminance: 0.6,
        bloom: false,
        allocates_light: false,
    },
    EmissionTier {
        key: "lamp",
        relative_luminance: 1.6,
        bloom: true,
        allocates_light: false,
    },
    EmissionTier {
        key: "source",
        relative_luminance: 4.0,
        bloom: true,
        allocates_light: true,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ScaleReference {
    pub key: &'static str,
    pub meters: f32,
    /// File and symbol the number was read from, so the guide cannot silently
    /// drift from the implementation.
    pub source: &'static str,
}

/// Real size anchors read from gameplay code. New assets must position pivots,
/// feet and connection sockets against these, not against an artist's guess.
pub const SCALE_REFERENCES: &[ScaleReference] = &[
    ScaleReference {
        key: "voxel_block",
        meters: 1.0,
        source: "data.rs grid (1 block = 1 world meter)",
    },
    ScaleReference {
        key: "player_eye",
        meters: 1.62,
        source: "player.rs EYE",
    },
    ScaleReference {
        key: "humanoid_height",
        meters: 1.9,
        source: "char.rs npc_scale target",
    },
    ScaleReference {
        key: "door_clearance",
        meters: 2.0,
        source: "world.rs stamp_hut door cells f..=f+1",
    },
    ScaleReference {
        key: "hut_wall_height",
        meters: 3.0,
        source: "world.rs stamp_hut walls f..=f+2",
    },
    ScaleReference {
        key: "ship_parked_envelope_y",
        meters: 1.25,
        source: "space.rs SHIP_BOX[1]",
    },
    ScaleReference {
        key: "ship_collision_radius",
        meters: 3.0,
        source: "space.rs SHIP_R",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn palette_keys_and_values_are_unique_and_opaque() {
        let mut keys = HashSet::new();
        for swatch in BASE_PALETTE {
            assert!(
                keys.insert(swatch.key),
                "duplicate palette key {}",
                swatch.key
            );
            assert_eq!(swatch.srgb.len(), 3);
            assert!(!swatch.usage.is_empty());
        }
    }

    #[test]
    fn emission_tiers_are_ordered_and_non_overlapping() {
        let mut previous = 0.0;
        for tier in EMISSION_TIERS {
            assert!(tier.relative_luminance > previous, "tier order");
            assert!(!tier.key.is_empty());
            previous = tier.relative_luminance;
        }
    }

    #[test]
    fn scale_anchors_are_positive_and_sourced() {
        let mut keys = HashSet::new();
        for reference in SCALE_REFERENCES {
            assert!(reference.meters > 0.0, "{}", reference.key);
            assert!(!reference.source.is_empty(), "{}", reference.key);
            assert!(keys.insert(reference.key), "duplicate {}", reference.key);
        }
    }
}
