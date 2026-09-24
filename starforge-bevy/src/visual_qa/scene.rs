//! A01 deterministic scene builders: material courtyard (S01), room variants
//! (S02) and streaming route (S06).
//!
//! Every scene is defined by fixed integer coordinates and written through
//! `World::set`, the same path player edits use. Geometry therefore exercises
//! the real chunk meshing, atlas and material code instead of a private
//! rendering copy. Terrain height is seed dependent, so the builders derive
//! their base level from the generated chunks and record it in the manifest.

use bevy::prelude::*;
use serde::Serialize;

use crate::data::{CHUNK, block_by_id, block_by_key, ids};
use crate::world::World;

pub const SCENE_VERSION: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneId {
    S01,
    S02,
    S06,
    /// B01 style/scale calibration rig: terrain, machine, character and
    /// building samples under the same light, plus a 1..5 block scale ladder.
    B01,
    /// D01 exposure/pass matrix: precise gray cards, a small sealed room and
    /// pose variants that toggle one lighting/post step at a time.
    D01,
    /// E01 cloud-shell diagnostics: camera altitudes below/inside/above the
    /// deck plus shader debug views selected per pose.
    E01,
    /// B04 PBR family courtyard: 13 families × 5 primitive shapes under
    /// noon/overcast/sunset/night/interior lighting.
    B04,
}

impl SceneId {
    pub const ALL: [SceneId; 7] = [
        SceneId::S01,
        SceneId::S02,
        SceneId::S06,
        SceneId::B01,
        SceneId::D01,
        SceneId::E01,
        SceneId::B04,
    ];
    /// Default `--visual-qa` run keeps the A01 scene set; B01/D01/E01/B04 are
    /// opt-in so the A01/A02 baselines stay reproducible.
    pub const DEFAULT_RUN: [SceneId; 3] = [SceneId::S01, SceneId::S02, SceneId::S06];

    pub fn key(self) -> &'static str {
        match self {
            SceneId::S01 => "S01",
            SceneId::S02 => "S02",
            SceneId::S06 => "S06",
            SceneId::B01 => "B01",
            SceneId::D01 => "D01",
            SceneId::E01 => "E01",
            SceneId::B04 => "B04",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            SceneId::S01 => "材料庭院",
            SceneId::S02 => "封闭屋与破损墙",
            SceneId::S06 => "地表流式路线",
            SceneId::B01 => "风格/尺度校准架",
            SceneId::D01 => "灰卡曝光与通道矩阵",
            SceneId::E01 => "云壳诊断矩阵",
            SceneId::B04 => "PBR 家族庭院（五光况）",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_uppercase().as_str() {
            "S01" | "COURTYARD" | "MATERIAL" => Some(SceneId::S01),
            "S02" | "ROOM" | "INTERIOR" => Some(SceneId::S02),
            "S06" | "ROUTE" | "STREAMING" => Some(SceneId::S06),
            "B01" | "CALIBRATION" | "SCALE" | "STYLE" => Some(SceneId::B01),
            "D01" | "EXPOSURE" | "GRAYCARD" | "PASSES" => Some(SceneId::D01),
            "E01" | "CLOUD" | "CLOUDDEBUG" | "SHELL" => Some(SceneId::E01),
            "B04" | "PBR" | "FAMILY" | "FAMILIES" => Some(SceneId::B04),
            _ => None,
        }
    }

    /// Frames allowed for dirty chunks to be re-meshed before capture.
    pub fn settle_frames(self) -> u32 {
        match self {
            // The voxel material courtyard rewrites a full stage chunk; give
            // its dirty meshes time to settle before the first wide capture.
            SceneId::S01 => 120,
            SceneId::S02 | SceneId::B01 | SceneId::D01 | SceneId::B04 => 45,
            SceneId::E01 => 60,
            SceneId::S06 => 60,
        }
    }

    /// Frames skipped at the start of the measured window so shader/pipeline
    /// compilation and first-streaming stalls do not pollute percentiles.
    pub fn measure_skip_frames(self) -> u32 {
        match self {
            SceneId::S01 | SceneId::S02 | SceneId::B01 => 20,
            SceneId::S06 => 60,
            // Variant switches between poses should not count as steady state.
            SceneId::D01 => 30,
            SceneId::E01 => 30,
            SceneId::B04 => 30,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ScenePose {
    pub name: String,
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
    /// D01 pass-matrix variant applied while this pose is framed; `None` for
    /// scenes that only change the camera.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// B04: per-pose day clock override (noon/overcast/sunset/night). `None`
    /// keeps the run-level `--visual-qa-day`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_time: Option<f32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SampleRef {
    pub key: String,
    pub block_id: u8,
    pub pos: [i32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub enum SceneProp {
    /// An original voxel NPC with its role-specific outfit and rig.
    Humanoid {
        role: crate::char::NpcRole,
        pos: [f32; 3],
        yaw: f32,
    },
    /// A machine visual spawned for a block already written by the builder
    /// (`factory::spawn_machine`).
    Machine {
        key: &'static str,
        pos: [i32; 3],
        dir: u8,
    },
    /// D01 precise PBR card; `material` indexes the runner's card palette.
    Card {
        material: u8,
        pos: [f32; 3],
        yaw: f32,
    },
    /// B04 material sample: `family` indexes `MaterialFamily::ALL`, `shape`
    /// indexes the runner's primitive mesh set (sphere/cube/ramp/sheet/prop).
    FamilySample {
        family: u8,
        shape: u8,
        pos: [f32; 3],
        yaw: f32,
    },
}

#[derive(Clone, Debug, Default)]
pub struct BuiltScene {
    pub anchor: [f32; 3],
    pub poses: Vec<ScenePose>,
    pub samples: Vec<SampleRef>,
    pub modified_voxels: u32,
    /// Arc-length waypoints for S06 (world space). Empty for staged scenes.
    pub route: Vec<Vec3>,
    /// Non-voxel presentation entities the runner must spawn (humanoids,
    /// machine visuals) so the calibration rig compares real assets.
    pub props: Vec<SceneProp>,
}

pub fn build(scene: SceneId, world: &mut World, _seed: u32) -> BuiltScene {
    match scene {
        SceneId::S01 => build_courtyard(world),
        SceneId::S02 => build_room(world),
        SceneId::S06 => build_route(world),
        SceneId::B01 => build_calibration(world),
        SceneId::D01 => build_exposure_matrix(world),
        SceneId::E01 => build_cloud_diagnostics(world),
        SceneId::B04 => build_pbr_courtyard(world),
    }
}

/// Convert an eye/target pair into the yaw/pitch convention used by
/// `player::camera_system` (`Quat::from_rotation_y(yaw) * from_rotation_x(pitch)`).
pub fn pose_from_look(name: &str, eye: Vec3, target: Vec3, fov: f32) -> ScenePose {
    let direction = (target - eye).normalize_or_zero();
    let pitch = direction.y.clamp(-1.0, 1.0).asin();
    let yaw = (-direction.x).atan2(-direction.z);
    ScenePose {
        name: name.to_string(),
        eye: eye.to_array(),
        yaw,
        pitch,
        fov,
        variant: None,
        day_time: None,
    }
}

/// Position along an arc-length parameterised route plus a heading toward the
/// next waypoint. `t` is clamped to [0, 1]; the last waypoint keeps its
/// previous segment direction.
pub fn route_pose(route: &[Vec3], t: f32, fov: f32) -> Option<ScenePose> {
    if route.len() < 2 {
        return None;
    }
    let lengths: Vec<f32> = route
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .collect();
    let total: f32 = lengths.iter().sum();
    if !total.is_finite() || total <= f32::EPSILON {
        return None;
    }
    let target = t.clamp(0.0, 1.0) * total;
    let mut travelled = 0.0;
    let mut index = 0;
    for (segment, length) in lengths.iter().enumerate() {
        if travelled + length >= target || segment + 1 == lengths.len() {
            index = segment;
            break;
        }
        travelled += length;
    }
    let length = lengths[index].max(f32::EPSILON);
    let local = ((target - travelled) / length).clamp(0.0, 1.0);
    let start = route[index];
    let end = route[index + 1];
    let position = start.lerp(end, local);
    let mut direction = (end - start).normalize_or_zero();
    if direction.length_squared() <= f32::EPSILON {
        direction = Vec3::NEG_Z;
    }
    let pitch = direction.y.clamp(-1.0, 1.0).asin() - 0.04;
    let yaw = (-direction.x).atan2(-direction.z);
    Some(ScenePose {
        name: format!("route_{:03}", (t.clamp(0.0, 1.0) * 100.0).round() as u32),
        eye: position.to_array(),
        yaw,
        pitch,
        fov,
        variant: None,
        day_time: None,
    })
}

fn prepare_stage(world: &mut World, cx: i32, cz: i32, radius: i32, floor: u8) -> (i32, u32) {
    let mut modified = 0u32;
    for dz in -radius - 1..=radius + 1 {
        for dx in -radius - 1..=radius + 1 {
            let (x, z) = (cx + dx, cz + dz);
            world.ensure_chunk(x.div_euclid(CHUNK), z.div_euclid(CHUNK));
        }
    }
    let mut top = 0;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            top = top.max(world.top_at(cx + dx, cz + dz));
        }
    }
    let base = top + 1;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let (x, z) = (cx + dx, cz + dz);
            // Clear enough headroom that trees inside the stage footprint do
            // not hang into every capture; the courtyard tree is planted
            // afterwards in `build_courtyard`.
            for y in base..base + 10 {
                if world.get(x, y, z) != ids::AIR {
                    world.set(x, y, z, ids::AIR);
                    modified += 1;
                }
            }
            for y in (base - 3).max(0)..base {
                let current = block_by_id(world.get(x, y, z));
                if !current.solid || current.liquid {
                    world.set(x, y, z, floor);
                    modified += 1;
                }
            }
            if world.get(x, base, z) != floor {
                world.set(x, base, z, floor);
                modified += 1;
            }
        }
    }
    (base, modified)
}

fn place_sample(world: &mut World, x: i32, y: i32, z: i32, key: &str) -> Option<SampleRef> {
    let def = block_by_key(key);
    if def.id == ids::AIR {
        return None;
    }
    world.set(x, y, z, def.id);
    Some(SampleRef {
        key: key.to_string(),
        block_id: def.id,
        pos: [x, y, z],
    })
}

fn build_sphere(world: &mut World, center: (i32, i32, i32), radius: i32, id: u8) -> u32 {
    let mut writes = 0;
    for dy in -radius..=radius {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy + dz * dz <= radius * radius + radius / 2 {
                    world.set(center.0 + dx, center.1 + dy, center.2 + dz, id);
                    writes += 1;
                }
            }
        }
    }
    writes
}

fn build_tree(world: &mut World, x: i32, ground: i32, z: i32) -> u32 {
    let mut writes = 0;
    for y in 0..3 {
        world.set(x, ground + y, z, ids::LOG);
        writes += 1;
    }
    for dz in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            world.set(x + dx, ground + 3, z + dz, ids::LEAVES);
            writes += 1;
        }
    }
    for dz in -1..=1 {
        for dx in -1..=1 {
            world.set(x + dx, ground + 4, z + dz, ids::LEAVES);
            writes += 1;
        }
    }
    world.set(x, ground + 5, z, ids::LEAVES);
    writes += 1;
    writes
}

fn build_courtyard(world: &mut World) -> BuiltScene {
    let (cx, cz) = (96, 96);
    let (base, mut modified) = prepare_stage(world, cx, cz, 12, ids::CONCRETE);

    let rows: [&[&str]; 4] = [
        &["grass", "dirt", "sand", "stone", "basalt", "snow", "ice"],
        &[
            "planks", "log", "leaves", "glass", "metal", "concrete", "slab",
        ],
        &[
            "coal_ore",
            "iron_ore",
            "copper_ore",
            "gold_ore",
            "titanium_ore",
            "uranium_ore",
            "amber",
        ],
        &[
            "lamp",
            "crystal",
            "solar",
            "belt",
            "sodium_plant",
            "oxygen_plant",
            "fern",
        ],
    ];
    let mut samples = Vec::new();
    for (row, keys) in rows.iter().enumerate() {
        let dz = -9 + row as i32 * 3;
        for (column, key) in keys.iter().enumerate() {
            let dx = -9 + column as i32 * 3;
            if let Some(sample) = place_sample(world, cx + dx, base + 1, cz + dz, key) {
                samples.push(sample);
                modified += 1;
            }
        }
    }

    modified += build_sphere(world, (cx, base + 4, cz + 7), 3, ids::METAL);
    modified += build_tree(world, cx - 10, base + 1, cz + 2);

    for step in 0..4 {
        world.set(cx + 6 + step, base + 1 + step, cz + 7, ids::SLAB);
        modified += 1;
    }
    // Give water a broad, sunlit patch away from the metal sphere's shadow.
    for dz in 0..4 {
        for dx in 0..4 {
            let (x, z) = (cx - 10 + dx, cz + 6 + dz);
            world.set(x, base, z, ids::WATER);
            world.set(x, base - 1, z, ids::WATER);
            modified += 2;
        }
    }

    let f = |x: i32, y: i32, z: i32| Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
    let poses = vec![
        pose_from_look("wide", f(cx, base + 16, cz - 24), f(cx, base + 2, cz), 60.0),
        pose_from_look(
            "samples",
            f(cx, base + 10, cz - 18),
            f(cx, base + 1, cz - 3),
            60.0,
        ),
        pose_from_look(
            "sphere",
            f(cx, base + 8, cz - 6),
            f(cx, base + 4, cz + 7),
            50.0,
        ),
        pose_from_look(
            "water",
            f(cx - 8, base + 8, cz + 16),
            f(cx - 8, base, cz + 7),
            55.0,
        ),
    ];

    BuiltScene {
        anchor: [cx as f32 + 0.5, base as f32, cz as f32 + 0.5],
        poses,
        samples,
        modified_voxels: modified,
        route: Vec::new(),
        props: Vec::new(),
    }
}

/// B01 calibration rig. The acceptance needs terrain, machine, character and
/// building samples in one frame under one light, plus a scale ladder so later
/// streams can read sizes from pixels instead of guessing.
fn build_calibration(world: &mut World) -> BuiltScene {
    let (cx, cz) = (96, 288);
    let (base, mut modified) = prepare_stage(world, cx, cz, 14, ids::CONCRETE);

    // Terrain family row (z = cz - 10).
    let terrain = ["grass", "dirt", "sand", "stone", "basalt", "snow", "ice"];
    let mut samples = Vec::new();
    for (index, key) in terrain.iter().enumerate() {
        let x = cx - 9 + index as i32 * 3;
        if let Some(sample) = place_sample(world, x, base + 1, cz - 10, key) {
            samples.push(sample);
            modified += 1;
        }
    }

    // Machine family row (z = cz - 5): voxel block + matching machine visual.
    let machines = [
        "furnace",
        "miner",
        "assembler",
        "reactor",
        "solar",
        "battery",
    ];
    let mut props = Vec::new();
    for (index, key) in machines.iter().enumerate() {
        let x = cx - 8 + index as i32 * 3;
        let z = cz - 5;
        if place_sample(world, x, base + 1, z, key).is_some() {
            modified += 1;
            props.push(SceneProp::Machine {
                key,
                pos: [x, base + 1, z],
                dir: 0,
            });
        }
    }

    // Building sample: 7×7 plank/log hut with a 2-high door, windows and lamp.
    modified += build_calibration_hut(world, cx + 11, base, cz + 5);

    // Scale ladder: 1..=5 block pillars two cells apart (z = cz + 10).
    for step in 0..5i32 {
        let x = cx - 11 + step * 2;
        let z = cz + 10;
        for y in 1..=step + 1 {
            world.set(x, base + y, z, ids::CONCRETE);
            modified += 1;
        }
    }

    // Original voxel character family: fixed role lineup, feet on the platform.
    let feet = base as f32 + 1.0;
    for (index, role) in crate::char::NpcRole::SHOWCASE.into_iter().enumerate() {
        props.push(SceneProp::Humanoid {
            role,
            pos: [cx as f32 - 4.0 + index as f32 * 2.0, feet, cz as f32 - 0.5],
            yaw: 0.0,
        });
    }

    let f = |x: f32, y: f32, z: f32| Vec3::new(x + 0.5, y, z + 0.5);
    let poses = vec![
        pose_from_look(
            "overview",
            f(cx as f32 - 18.0, base as f32 + 14.0, cz as f32 - 18.0),
            f(cx as f32 + 2.0, base as f32 + 2.0, cz as f32),
            60.0,
        ),
        pose_from_look(
            "terrain",
            f(cx as f32 - 10.0, base as f32 + 4.0, cz as f32 - 16.0),
            f(cx as f32, base as f32 + 1.0, cz as f32 - 10.0),
            55.0,
        ),
        pose_from_look(
            "machines",
            f(cx as f32 - 10.0, base as f32 + 4.0, cz as f32 - 1.0),
            f(cx as f32, base as f32 + 2.0, cz as f32 - 5.0),
            55.0,
        ),
        pose_from_look(
            "characters",
            f(cx as f32 - 0.5, base as f32 + 3.2, cz as f32 + 7.0),
            f(cx as f32 - 0.5, base as f32 + 1.6, cz as f32 - 0.5),
            45.0,
        ),
        pose_from_look(
            "building",
            f(cx as f32 + 11.0, base as f32 + 6.0, cz as f32 + 22.0),
            f(cx as f32 + 11.0, base as f32 + 1.5, cz as f32 + 5.0),
            50.0,
        ),
    ];

    BuiltScene {
        anchor: [cx as f32 + 0.5, base as f32, cz as f32 + 0.5],
        poses,
        samples,
        modified_voxels: modified,
        route: Vec::new(),
        props,
    }
}

/// 7×7 hut used by the B01 building sample. Mirrors the gameplay hut grammar
/// (log corners, 2-block door, glass windows, plank roof) but stays local so
/// the calibration rig cannot be broken by worldgen changes.
fn build_calibration_hut(world: &mut World, hx: i32, base: i32, hz: i32) -> u32 {
    let mut writes = 0;
    let s = 3;
    for dx in -s..=s {
        for dz in -s..=s {
            world.set(hx + dx, base, hz + dz, ids::CONCRETE);
            writes += 1;
            if dx.abs() == s || dz.abs() == s {
                let corner = dx.abs() == s && dz.abs() == s;
                for y in 1..=3 {
                    let id = if corner { ids::LOG } else { ids::PLANKS };
                    world.set(hx + dx, base + y, hz + dz, id);
                    writes += 1;
                }
            }
            world.set(hx + dx, base + 4, hz + dz, ids::PLANKS);
            writes += 1;
        }
    }
    // 2-high door on the +z wall.
    for y in 1..=2 {
        world.set(hx, base + y, hz + s, ids::AIR);
        writes += 1;
    }
    // Windows on the -z wall and the +x wall.
    for dx in -1..=1 {
        world.set(hx + dx, base + 2, hz - s, ids::GLASS);
        writes += 1;
    }
    world.set(hx + s, base + 2, hz, ids::GLASS);
    writes += 1;
    // Interior lamp so the same rig shows emissive + local-light response.
    world.set(hx, base + 3, hz, ids::LAMP);
    writes += 1;
    writes
}

/// D01 exposure/pass matrix. Uses `StandardMaterial` cards with exact
/// scene-linear albedo instead of atlas blocks so the exposure readout is not
/// polluted by texture authoring, plus a small sealed room so indoor ambient
/// and atmosphere behavior can be compared at the same fixed exposure.
fn build_exposure_matrix(world: &mut World) -> BuiltScene {
    let (cx, cz) = (176, 288);
    let (base, mut modified) = prepare_stage(world, cx, cz, 12, ids::CONCRETE);

    // Seven cards, left to right: black, 18% gray, 50% gray, 90% white,
    // emissive, metal and a saturated color patch. `material` indexes the
    // runner palette in `visual_qa::mod`.
    let mut props = Vec::new();
    for (index, material) in [0u8, 1, 2, 3, 4, 5, 6].iter().enumerate() {
        let x = cx - 9 + index as i32 * 3;
        props.push(SceneProp::Card {
            material: *material,
            pos: [x as f32 + 0.5, base as f32 + 2.1, cz as f32 - 4.5],
            yaw: 0.0,
        });
    }

    // Sealed 7x7 room with a 2-high doorway on the -z wall and one window.
    let (rx, rz) = (cx + 8, cz + 8);
    let s = 3;
    for dx in -s..=s {
        for dz in -s..=s {
            world.set(rx + dx, base, rz + dz, ids::CONCRETE);
            modified += 1;
            if dx.abs() == s || dz.abs() == s {
                let corner = dx.abs() == s && dz.abs() == s;
                let id = if corner { ids::METAL } else { ids::CONCRETE };
                for y in 1..=3 {
                    world.set(rx + dx, base + y, rz + dz, id);
                    modified += 1;
                }
            }
            world.set(rx + dx, base + 4, rz + dz, ids::CONCRETE);
            modified += 1;
        }
    }
    for y in 1..=2 {
        world.set(rx, base + y, rz - s, ids::AIR);
        modified += 1;
    }
    for dz in -1..=1 {
        world.set(rx - s, base + 2, rz + dz, ids::GLASS);
        modified += 1;
    }
    world.set(rx, base + 3, rz, ids::LAMP);
    modified += 1;
    // Indoor 18% card: read the doorway contrast against the outdoor cards.
    props.push(SceneProp::Card {
        material: 1,
        pos: [rx as f32 + 0.5, base as f32 + 2.1, rz as f32 + 1.5],
        yaw: 0.0,
    });

    let f = |x: i32, y: i32, z: i32| Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
    let cards_pose = |name: &str, variant: &str| {
        let mut pose = pose_from_look(
            &format!("cards_{name}"),
            f(cx, base + 3, cz - 16),
            f(cx, base + 2, cz - 4),
            45.0,
        );
        pose.variant = Some(variant.to_string());
        pose
    };
    let room_pose = |name: &str, variant: &str| {
        let mut pose = pose_from_look(
            &format!("room_{name}"),
            f(rx, base + 2, rz - 8),
            f(rx, base + 2, rz - s),
            50.0,
        );
        pose.variant = Some(variant.to_string());
        pose
    };
    let mut inside = pose_from_look(
        "room_inside_full",
        f(rx, base + 2, rz - 1),
        f(rx, base + 2, rz - s),
        60.0,
    );
    inside.variant = Some("full".to_string());

    let poses = vec![
        cards_pose("full", "full"),
        cards_pose("no_post", "no_post"),
        cards_pose("sun_only", "sun_only"),
        cards_pose("ambient_only", "ambient_only"),
        cards_pose("probe_stops", "probe_stops"),
        cards_pose("probe_zebra", "probe_zebra"),
        room_pose("full", "full"),
        room_pose("no_post", "no_post"),
        room_pose("probe_stops", "probe_stops"),
        inside,
    ];

    BuiltScene {
        anchor: [cx as f32 + 0.5, base as f32, cz as f32 + 0.5],
        poses,
        samples: Vec::new(),
        modified_voxels: modified,
        route: Vec::new(),
        props,
    }
}

/// B04 PBR family courtyard. One column per `MaterialFamily` and five rows of
/// primitive samples (sphere/cube/ramp/sheet/prop) let every family be reviewed
/// under noon/overcast/sunset/night light; a small interior room adds a real
/// lamp sample so "ordinary surfaces do not glow" can be judged next to the
/// emissive families.
fn build_pbr_courtyard(world: &mut World) -> BuiltScene {
    use crate::art::catalog::MaterialFamily;
    let (cx, cz) = (256, 96);
    let (base, mut modified) = prepare_stage(world, cx, cz, 22, ids::CONCRETE);

    let families = MaterialFamily::ALL.len() as i32;
    let family_x = |index: i32| cx - (families / 2) * 3 + index * 3;
    let mut props = Vec::new();
    for family in 0..families {
        for shape in 0..5u8 {
            props.push(SceneProp::FamilySample {
                family: family as u8,
                shape,
                pos: [
                    family_x(family) as f32 + 0.5,
                    base as f32 + 1.0,
                    (cz - 6 + shape as i32 * 3) as f32 + 0.5,
                ],
                yaw: 0.35,
            });
        }
    }

    // Open-front interior cutaway: log corners, plank walls/roof, one lamp;
    // two samples inside remain visible to the diagnostic camera.
    let (rx, rz) = (cx + 15, cz + 12);
    let s = 2;
    for dx in -s..=s {
        for dz in -s..=s {
            world.set(rx + dx, base, rz + dz, ids::CONCRETE);
            modified += 1;
            // The camera looks along +Z, so the near (-Z) side stays open.
            if dx.abs() == s || dz == s {
                let corner = dx.abs() == s && dz.abs() == s;
                for y in 1..=3 {
                    world.set(
                        rx + dx,
                        base + y,
                        rz + dz,
                        if corner { ids::LOG } else { ids::PLANKS },
                    );
                    modified += 1;
                }
            }
            world.set(rx + dx, base + 4, rz + dz, ids::PLANKS);
            modified += 1;
        }
    }
    world.set(rx, base + 3, rz, ids::LAMP);
    modified += 1;
    let painted = MaterialFamily::ALL
        .iter()
        .position(|family| *family == MaterialFamily::Painted)
        .unwrap_or(0) as u8;
    let energy = MaterialFamily::ALL
        .iter()
        .position(|family| *family == MaterialFamily::Energy)
        .unwrap_or(0) as u8;
    props.push(SceneProp::FamilySample {
        family: painted,
        shape: 4,
        pos: [rx as f32 - 0.7, base as f32 + 1.0, rz as f32 + 0.5],
        yaw: 0.4,
    });
    props.push(SceneProp::FamilySample {
        family: energy,
        shape: 1,
        pos: [rx as f32 + 0.9, base as f32 + 1.0, rz as f32 + 0.5],
        yaw: 0.0,
    });

    let index_of = |family: MaterialFamily| {
        MaterialFamily::ALL
            .iter()
            .position(|entry| *entry == family)
            .unwrap_or(0) as i32
    };
    let closeup = |name: &str, family: MaterialFamily, day: f32| {
        let x = family_x(index_of(family)) as f32 + 0.5;
        let eye = Vec3::new(x, base as f32 + 3.3, cz as f32 - 7.5);
        let target = Vec3::new(x, base as f32 + 1.5, cz as f32 + 0.5);
        let mut pose = pose_from_look(name, eye, target, 48.0);
        pose.variant = Some("full".to_string());
        pose.day_time = Some(day);
        pose
    };

    let grid = |name: &str, variant: &str, day: f32| {
        let mut pose = pose_from_look(
            name,
            Vec3::new(cx as f32 + 0.5, base as f32 + 22.0, cz as f32 - 34.0),
            Vec3::new(cx as f32 + 0.5, base as f32 + 2.0, cz as f32),
            58.0,
        );
        pose.variant = Some(variant.to_string());
        pose.day_time = Some(day);
        pose
    };
    let mut interior = pose_from_look(
        "interior",
        Vec3::new(rx as f32 + 0.5, base as f32 + 1.75, rz as f32 - 5.0),
        Vec3::new(rx as f32 + 0.5, base as f32 + 1.5, rz as f32 + 0.5),
        58.0,
    );
    interior.variant = Some("full".to_string());
    interior.day_time = Some(0.94);

    let poses = vec![
        grid("grid_noon", "full", 0.50),
        grid("grid_overcast", "overcast", 0.50),
        grid("grid_sunset", "full", 0.78),
        grid("grid_night", "full", 0.94),
        interior,
        closeup("metal_closeup", MaterialFamily::BareMetal, 0.50),
        closeup("rock_closeup", MaterialFamily::Rock, 0.50),
        closeup("wood_closeup", MaterialFamily::Wood, 0.50),
        closeup("emissive_closeup", MaterialFamily::Energy, 0.94),
    ];

    BuiltScene {
        anchor: [cx as f32 + 0.5, base as f32, cz as f32 + 0.5],
        poses,
        samples: Vec::new(),
        modified_voxels: modified,
        route: Vec::new(),
        props,
    }
}

/// E01 cloud-shell diagnostic scene. The camera is teleported to altitudes
/// below/inside/above the deck; each pose selects one shader debug view through
/// `variant` so a single run produces paired full/debug captures. No voxels are
/// written: the real terrain, atmosphere and cloud shell stay in play.
fn build_cloud_diagnostics(_world: &World) -> BuiltScene {
    let (cx, cz) = (288.0f32, 64.0f32);
    let eye = |altitude: f32| Vec3::new(cx + 0.5, crate::data::SEA_Y + altitude, cz + 0.5);
    let look = |altitude: f32, dx: f32| Vec3::new(cx + dx, crate::data::SEA_Y + altitude, cz + 0.5);
    let pose =
        |name: &str, altitude: f32, target_altitude: f32, dx: f32, fov: f32, variant: &str| {
            let mut pose = pose_from_look(name, eye(altitude), look(target_altitude, dx), fov);
            pose.variant = Some(variant.to_string());
            pose
        };
    let poses = vec![
        pose("below_full", 180.0, 520.0, 600.0, 60.0, "full"),
        pose("below_faces", 180.0, 520.0, 600.0, 60.0, "debug_faces"),
        pose("bottom_density", 415.0, 430.0, 900.0, 60.0, "debug_density"),
        pose(
            "inside_interval",
            600.0,
            600.0,
            900.0,
            60.0,
            "debug_interval",
        ),
        pose(
            "inside_transmittance",
            600.0,
            600.0,
            900.0,
            60.0,
            "debug_transmittance",
        ),
        pose(
            "inside_scattering",
            600.0,
            600.0,
            900.0,
            60.0,
            "debug_scattering",
        ),
        pose("inside_light", 600.0, 600.0, 900.0, 60.0, "debug_light"),
        pose("above_full", 1050.0, 700.0, 900.0, 60.0, "full"),
        pose("above_steps", 1050.0, 700.0, 900.0, 60.0, "debug_steps"),
        pose("above_depth", 1050.0, 700.0, 900.0, 60.0, "debug_depth"),
        pose("away_full", 600.0, 1180.0, 60.0, 70.0, "full"),
        pose("tangent_full", 900.0, 350.0, 4000.0, 60.0, "full"),
    ];
    BuiltScene {
        anchor: [cx + 0.5, crate::data::SEA_Y, cz + 0.5],
        poses,
        samples: Vec::new(),
        modified_voxels: 0,
        route: Vec::new(),
        props: Vec::new(),
    }
}

fn build_room(world: &mut World) -> BuiltScene {
    let (ax, az) = (176, 96);
    let (base_a, mut modified) = prepare_stage(world, ax, az, 6, ids::CONCRETE);

    for dx in -4i32..=4 {
        for dz in -4i32..=4 {
            if dx.abs() != 4 && dz.abs() != 4 {
                continue;
            }
            let corner = dx.abs() == 4 && dz.abs() == 4;
            for y in 1..=3 {
                let id = if corner {
                    ids::METAL
                } else if y == 1 {
                    ids::PLANKS
                } else {
                    ids::CONCRETE
                };
                world.set(ax + dx, base_a + y, az + dz, id);
                modified += 1;
            }
        }
    }
    for dx in -4i32..=4 {
        for dz in -4i32..=4 {
            world.set(ax + dx, base_a + 4, az + dz, ids::CONCRETE);
            modified += 1;
        }
    }
    // A warmer floor and a glazed skylight give the room material contrast
    // while keeping its shell closed for the lighting check.
    for dx in -3i32..=3 {
        for dz in -3i32..=3 {
            world.set(ax + dx, base_a, az + dz, ids::PLANKS);
            modified += 1;
        }
    }
    for dx in -1i32..=0 {
        for dz in -1i32..=0 {
            world.set(ax + dx, base_a + 4, az + dz, ids::GLASS);
            modified += 1;
        }
    }
    // Door (open), window and a two-block breach on the back wall.
    for y in 1..=2 {
        for dx in -1..=0 {
            world.set(ax + dx, base_a + y, az + 4, ids::AIR);
            modified += 1;
        }
    }
    for y in 1..=2 {
        for dx in 1..=2 {
            world.set(ax + dx, base_a + y, az - 4, ids::AIR);
            modified += 1;
        }
    }
    for dz in -1..=1 {
        world.set(ax + 4, base_a + 2, az + dz, ids::GLASS);
        modified += 1;
    }
    for (dx, dz) in [(-3, -2), (-3, 2)] {
        world.set(ax + dx, base_a + 3, az + dz, ids::LAMP);
        modified += 1;
    }

    let (bx, bz) = (176, 110);
    let (base_b, mut modified_b) = prepare_stage(world, bx, bz, 4, ids::METAL);
    for dx in -2i32..=2 {
        for dz in -2i32..=2 {
            if dx.abs() != 2 && dz.abs() != 2 {
                continue;
            }
            for y in 1..=3 {
                world.set(bx + dx, base_b + y, bz + dz, ids::METAL);
                modified_b += 1;
            }
        }
    }
    for dx in -2i32..=2 {
        for dz in -2i32..=2 {
            world.set(bx + dx, base_b + 4, bz + dz, ids::METAL);
            modified_b += 1;
        }
    }
    modified += modified_b;

    let f = |x: i32, y: i32, z: i32| Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
    let poses = vec![
        pose_from_look(
            "outside",
            f(ax, base_a + 7, az + 21),
            f(ax, base_a + 1, az),
            55.0,
        ),
        pose_from_look(
            "window",
            f(ax - 2, base_a + 2, az),
            f(ax + 4, base_a + 2, az),
            60.0,
        ),
        pose_from_look(
            "door",
            f(ax, base_a + 2, az + 9),
            f(ax, base_a + 2, az - 2),
            60.0,
        ),
        pose_from_look(
            "sealed",
            f(bx, base_b + 2, bz),
            f(bx - 2, base_b + 2, bz),
            55.0,
        ),
    ];

    BuiltScene {
        anchor: [ax as f32 + 0.5, base_a as f32, az as f32 + 0.5],
        poses,
        samples: Vec::new(),
        modified_voxels: modified,
        route: Vec::new(),
        props: Vec::new(),
    }
}

fn build_route(world: &World) -> BuiltScene {
    let mut route = Vec::new();
    for step in 0..=24 {
        let x = 96 + step * 12;
        let z = 192;
        // This route crosses sea-level terrain and dense forest. Keep each
        // waypoint above both the water surface and nearby generated canopy so
        // captures cannot begin underwater or inside a trunk.
        let mut top = world.g.sea();
        for dz in -6..=6 {
            for dx in -6..=6 {
                let wx = x + dx;
                let wz = z + dz;
                let ground = world.g.height_at(wx as f32 + 0.5, wz as f32 + 0.5);
                top = top.max(ground);
                let tree_mul = world
                    .g
                    .sub_at(wx as f32 + 0.5, wz as f32 + 0.5)
                    .map(|sub| sub.1)
                    .unwrap_or(1.0);
                if let Some((tree_ground, trunk_height, _)) = world.g.tree_at(wx, wz, tree_mul) {
                    top = top.max(tree_ground + trunk_height + 2);
                }
            }
        }
        // Leave room for the view frustum below the eye. A 2.5-block gap
        // still put the nearest crowns across the bottom of each capture.
        let y = top as f32 + 8.0;
        route.push(Vec3::new(x as f32 + 0.5, y, z as f32 + 0.5));
    }
    let anchor = route.first().copied().unwrap_or(Vec3::ZERO);
    BuiltScene {
        anchor: anchor.to_array(),
        poses: Vec::new(),
        samples: Vec::new(),
        modified_voxels: 0,
        route,
        props: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_keys_parse_and_roundtrip() {
        for id in SceneId::ALL {
            assert_eq!(SceneId::from_key(id.key()), Some(id));
        }
        assert_eq!(SceneId::from_key(" route "), Some(SceneId::S06));
        assert_eq!(SceneId::from_key("nope"), None);
    }

    #[test]
    fn pose_from_look_matches_player_convention() {
        let eye = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(0.0, 0.0, -5.0);
        let pose = pose_from_look("north", eye, target, 60.0);
        let rotation = Quat::from_rotation_y(pose.yaw) * Quat::from_rotation_x(pose.pitch);
        let forward = rotation * Vec3::NEG_Z;
        assert!(forward.distance(Vec3::new(0.0, 0.0, -1.0)) < 1e-4);

        let target = Vec3::new(5.0, 0.0, 0.0);
        let pose = pose_from_look("east", eye, target, 60.0);
        let rotation = Quat::from_rotation_y(pose.yaw) * Quat::from_rotation_x(pose.pitch);
        let forward = rotation * Vec3::NEG_Z;
        assert!(forward.distance(Vec3::new(1.0, 0.0, 0.0)) < 1e-4);
    }

    #[test]
    fn calibration_scene_covers_all_sample_families() {
        let mut world = World::new(11, "lush", 3);
        let built = build(SceneId::B01, &mut world, 11);
        assert_eq!(built.poses.len(), 5, "overview + four close poses");
        assert!(built.samples.len() >= 7, "terrain row");
        let machines = built
            .props
            .iter()
            .filter(|prop| matches!(prop, SceneProp::Machine { .. }))
            .count();
        let humanoids = built
            .props
            .iter()
            .filter(|prop| matches!(prop, SceneProp::Humanoid { .. }))
            .count();
        assert!(machines >= 5, "machine family row");
        assert_eq!(humanoids, 3, "character lineup");
        assert!(built.modified_voxels > 0);
        assert!(built.route.is_empty());
    }

    #[test]
    fn exposure_matrix_covers_cards_room_and_pass_variants() {
        let mut world = World::new(11, "lush", 3);
        let built = build(SceneId::D01, &mut world, 11);
        assert_eq!(
            built.poses.len(),
            10,
            "six card variants + three room + one inside"
        );
        assert!(
            built
                .props
                .iter()
                .all(|prop| matches!(prop, SceneProp::Card { .. })),
            "D01 props are precise PBR cards"
        );
        assert_eq!(
            built.props.len(),
            8,
            "seven outdoor cards + one indoor card"
        );
        let variants: Vec<&str> = built
            .poses
            .iter()
            .filter_map(|pose| pose.variant.as_deref())
            .collect();
        for expected in [
            "full",
            "no_post",
            "sun_only",
            "ambient_only",
            "probe_stops",
            "probe_zebra",
        ] {
            assert!(variants.contains(&expected), "missing variant {expected}");
        }
        assert!(built.modified_voxels > 0);
        assert!(built.route.is_empty());
    }

    #[test]
    fn pbr_courtyard_covers_families_shapes_and_light_conditions() {
        use crate::art::catalog::MaterialFamily;
        let mut world = World::new(11, "lush", 3);
        let built = build(SceneId::B04, &mut world, 11);
        assert_eq!(built.poses.len(), 9, "four grid + interior + four closeups");
        assert!(built.modified_voxels > 0, "courtyard floor and room");
        assert!(built.route.is_empty());
        let samples: Vec<(u8, u8)> = built
            .props
            .iter()
            .filter_map(|prop| match prop {
                SceneProp::FamilySample { family, shape, .. } => Some((*family, *shape)),
                _ => None,
            })
            .collect();
        assert_eq!(
            samples.len(),
            MaterialFamily::ALL.len() * 5 + 2,
            "13 families × 5 shapes + 2 interior samples"
        );
        let mut families: Vec<u8> = samples.iter().map(|(family, _)| *family).collect();
        families.sort_unstable();
        families.dedup();
        assert_eq!(families.len(), MaterialFamily::ALL.len());
        let mut shapes: Vec<u8> = samples.iter().map(|(_, shape)| *shape).collect();
        shapes.sort_unstable();
        shapes.dedup();
        assert_eq!(shapes, vec![0, 1, 2, 3, 4]);
        let days: Vec<f32> = built
            .poses
            .iter()
            .filter_map(|pose| pose.day_time)
            .collect();
        assert!(days.iter().any(|day| *day < 0.6), "noon");
        assert!(days.iter().any(|day| (0.7..0.85).contains(day)), "sunset");
        assert!(days.iter().any(|day| *day > 0.9), "night");
        let variants: Vec<&str> = built
            .poses
            .iter()
            .filter_map(|pose| pose.variant.as_deref())
            .collect();
        assert!(variants.contains(&"overcast"), "one overcast pose");
        assert!(variants.contains(&"full"), "shipping light poses");
    }

    #[test]
    fn cloud_diagnostic_scene_covers_regions_and_debug_views() {
        let mut world = World::new(11, "lush", 3);
        let built = build(SceneId::E01, &mut world, 11);
        assert_eq!(built.poses.len(), 12);
        assert_eq!(built.modified_voxels, 0, "E01 must not edit terrain");
        assert!(built.props.is_empty());
        assert!(built.route.is_empty());
        let variants: Vec<&str> = built
            .poses
            .iter()
            .filter_map(|pose| pose.variant.as_deref())
            .collect();
        for expected in [
            "full",
            "debug_faces",
            "debug_density",
            "debug_interval",
            "debug_transmittance",
            "debug_scattering",
            "debug_light",
            "debug_steps",
            "debug_depth",
        ] {
            assert!(variants.contains(&expected), "missing variant {expected}");
        }
        assert_eq!(
            variants
                .iter()
                .filter(|variant| **variant == "full")
                .count(),
            4,
            "below/above/away/tangent shipping captures"
        );
        // The altitude spread must cross both shell boundaries.
        let altitudes: Vec<f32> = built.poses.iter().map(|pose| pose.eye[1]).collect();
        let sea = crate::data::SEA_Y;
        assert!(altitudes.iter().any(|y| y - sea < 420.0));
        assert!(
            altitudes
                .iter()
                .any(|y| (420.0..780.0).contains(&(y - sea)))
        );
        assert!(altitudes.iter().any(|y| y - sea > 780.0));
    }

    #[test]
    fn route_pose_walks_the_whole_polyline() {
        let route = vec![
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(10.0, 1.0, 0.0),
            Vec3::new(10.0, 1.0, 10.0),
        ];
        let start = route_pose(&route, 0.0, 60.0).unwrap();
        assert!(Vec3::from_array(start.eye).distance(route[0]) < 1e-4);
        let midpoint = route_pose(&route, 0.5, 60.0).unwrap();
        assert!(Vec3::from_array(midpoint.eye).distance(Vec3::new(10.0, 1.0, 0.0)) < 1e-3);
        let middle = route_pose(&route, 0.75, 60.0).unwrap();
        assert!(Vec3::from_array(middle.eye).distance(Vec3::new(10.0, 1.0, 5.0)) < 1e-3);
        let end = route_pose(&route, 1.0, 60.0).unwrap();
        assert!(Vec3::from_array(end.eye).distance(route[2]) < 1e-4);
        assert!(route_pose(&route[..1], 0.5, 60.0).is_none());
    }
}
