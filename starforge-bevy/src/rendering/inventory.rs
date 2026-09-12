//! D01 pass/format inventory.
//!
//! The inventory is collected once in `PostStartup`, after every startup camera
//! exists, and serialized by the visual QA runner as `<run>-rendering.json`.
//! It records the real components present on each camera plus the pass order
//! and color chain this build performs. Timing is explicitly out of scope:
//! per-pass GPU timestamps need the device feature and are deferred to K03.
//!
//! Values that can only be observed in the render world (the concrete
//! `ViewTarget` texture format) are derived from the camera's `Hdr` component
//! and the `RenderCapabilities` probe instead of guessing a GPU name.

use bevy::camera::{Exposure, Hdr, RenderTarget};
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{AtmosphereEnvironmentMapLight, DirectionalLightShadowMap};
use bevy::pbr::{
    AtmosphereMode, AtmosphereSettings, ContactShadows, DistanceFog, ScreenSpaceAmbientOcclusion,
};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use serde::Serialize;

use crate::visual::RenderCapabilities;

pub const PASS_INVENTORY_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize)]
pub struct TargetRecord {
    pub kind: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BloomRecord {
    pub intensity: f32,
    pub threshold: f32,
    pub threshold_softness: f32,
    pub low_frequency_boost: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct CameraRecord {
    pub kind: &'static str,
    pub order: isize,
    pub hdr: bool,
    pub msaa: String,
    pub tonemapping: String,
    pub exposure_ev100: Option<f32>,
    pub clear_color: String,
    pub depth_prepass: bool,
    pub bloom: Option<BloomRecord>,
    pub ssao: bool,
    pub contact_shadow_length: Option<f32>,
    pub atmosphere: Option<String>,
    pub atmosphere_fill: Option<f32>,
    pub distance_fog: Option<String>,
    pub target: TargetRecord,
    pub viewport: [u32; 2],
}

/// One stage of the 3D frame. The list is a declaration of what the locked
/// build performs and where D01 inserted the probe; `reads`/`writes` are the
/// data hand-offs the composition workstreams depend on.
#[derive(Clone, Debug, Serialize)]
pub struct PassStage {
    pub name: &'static str,
    pub schedule: &'static str,
    pub reads: &'static str,
    pub writes: &'static str,
    pub fallback: &'static str,
}

pub const PASS_ORDER: &[PassStage] = &[
    PassStage {
        name: "depth_prepass",
        schedule: "Core3dSystems::Prepass",
        reads: "scene geometry",
        writes: "depth + normals (DepthPrepass component present)",
        fallback: "contact shadows and SSAO degrade to global ambient when removed",
    },
    PassStage {
        name: "shadow_maps",
        schedule: "light render before main opaque",
        reads: "shadow-casting geometry",
        writes: "4096px directional cascade",
        fallback: "DirectionalLightShadowMap size lowered to the probed 2D texture limit (A03)",
    },
    PassStage {
        name: "main_opaque",
        schedule: "Core3dSystems::MainPass",
        reads: "terrain/structures/actors",
        writes: "HDR color + depth in the view target",
        fallback: "camera target without Hdr falls back to the LDR surface format",
    },
    PassStage {
        name: "sky_atmosphere",
        schedule: "Core3d MainPass (raymarched sky)",
        reads: "sun direction, scattering medium, camera height",
        writes: "HDR sky color",
        fallback: "ClearColor + DistanceFog when AtmosphereSettings is removed (A03)",
    },
    PassStage {
        name: "cloud_shell",
        schedule: "Core3d MainPass + transparent pass (CloudShellMaterial)",
        reads: "depth, sun direction, weather density",
        writes: "premultiplied (L, 1-T) transparent color",
        fallback: "cloud entity removed when `clouds_supported()` is false (A03)",
    },
    PassStage {
        name: "main_transparent",
        schedule: "Core3dSystems::MainPass (transparent)",
        reads: "water/glass/particles",
        writes: "blended HDR color",
        fallback: "translucent surfaces sink to the opaque fallback look if depth is unavailable",
    },
    PassStage {
        name: "bloom",
        schedule: "Core3d PostProcess (before tonemapping)",
        reads: "HDR color above the prefilter threshold",
        writes: "HDR color with bloom added",
        fallback: "Bloom component removed with LDR targets (A03)",
    },
    PassStage {
        name: "exposure_probe_d01",
        schedule: "Core3dSystems::PostProcess, before tonemapping",
        reads: "HDR view target (source half of post_process_write)",
        writes: "HDR view target with the diagnostic overlay",
        fallback: "no ExposureProbe component means the pass is skipped; shader load failure disables the request (R011)",
    },
    PassStage {
        name: "tonemapping",
        schedule: "Core3dSystems::PostProcess (after bloom/probe)",
        reads: "scene-linear HDR",
        writes: "display-referred ACES fitted (Tonemapping::AcesFitted)",
        fallback: "no Tonemapping component leaves HDR values clipped by the target",
    },
    PassStage {
        name: "upscaling",
        schedule: "Core3d after tonemapping",
        reads: "camera render target",
        writes: "window surface (or the 640x360 pixel target's upscale node)",
        fallback: "pixel mode keeps the nearest-neighbor ImageNode upscale (app::startup)",
    },
    PassStage {
        name: "product_ui",
        schedule: "bevy_egui manual pass after 3D",
        reads: "native-resolution context",
        writes: "window surface, never tonemapped twice",
        fallback: "UI draws at native resolution in both pixel and modern modes (04/4.8)",
    },
];

/// One conversion in the albedo-to-display chain. The chain is asserted to
/// convert exactly once; sources already linear must not be re-encoded.
#[derive(Clone, Debug, Serialize)]
pub struct ColorStage {
    pub name: &'static str,
    pub space: &'static str,
    pub operation: &'static str,
}

pub const COLOR_CHAIN: &[ColorStage] = &[
    ColorStage {
        name: "albedo_texture",
        space: "sRGB",
        operation: "hardware sRGB decode on sample into linear (no manual pow in WGSL)",
    },
    ColorStage {
        name: "material_and_vertex_color",
        space: "linear",
        operation: "atlas mip sampling; corner AO and face shade multiply linear values",
    },
    ColorStage {
        name: "lighting",
        space: "scene-linear HDR",
        operation: "physical sun illuminance/exposure; clouds output premultiplied linear (L, 1-T)",
    },
    ColorStage {
        name: "exposure_probe",
        space: "scene-linear HDR",
        operation: "diagnostic overlay in/out of the same HDR range; disabled probe returns the source",
    },
    ColorStage {
        name: "tonemapping",
        space: "display-referred",
        operation: "ACES fitted applied once after Bloom and before the target encode",
    },
    ColorStage {
        name: "presentation",
        space: "bgra8unorm-srgb",
        operation: "single sRGB encode on write; egui UI composited after at native resolution",
    },
];

/// What anti-aliasing paths the locked build can run and which blockers apply.
#[derive(Clone, Debug, Serialize)]
pub struct AaMatrix {
    pub msaa_current: String,
    pub msaa_x4_supported: bool,
    pub hdr_target: bool,
    pub depth_prepass_active: bool,
    pub motion_vectors_active: bool,
    pub fxaa_available: bool,
    pub smaa_available: bool,
    pub taa_available: bool,
    pub notes: Vec<String>,
}

#[derive(Resource, Clone, Debug, Serialize)]
pub struct PassInventory {
    pub schema_version: u32,
    pub shadow_map_size: usize,
    pub global_clear_color: String,
    pub cameras: Vec<CameraRecord>,
    pub pass_order: Vec<PassStage>,
    pub color_chain: Vec<ColorStage>,
    pub aa: AaMatrix,
    pub notes: Vec<String>,
}

fn target_record(
    target: &RenderTarget,
    viewport: UVec2,
    hdr: bool,
    images: &Assets<Image>,
) -> TargetRecord {
    match target {
        RenderTarget::Window(_) => TargetRecord {
            kind: "window".to_string(),
            width: viewport.x,
            height: viewport.y,
            format: if hdr {
                "Rgba16Float (Hdr camera)".to_string()
            } else {
                "surface (bgra8unorm-srgb fallback)".to_string()
            },
        },
        RenderTarget::Image(image_target) => {
            let image = images.get(&image_target.handle);
            TargetRecord {
                kind: "image".to_string(),
                width: image
                    .map(|image| image.texture_descriptor.size.width)
                    .unwrap_or(viewport.x),
                height: image
                    .map(|image| image.texture_descriptor.size.height)
                    .unwrap_or(viewport.y),
                format: image
                    .map(|image| format!("{:?}", image.texture_descriptor.format))
                    .unwrap_or_else(|| "unresolved image handle".to_string()),
            }
        }
        other => TargetRecord {
            kind: "other".to_string(),
            width: viewport.x,
            height: viewport.y,
            format: format!("{other:?}"),
        },
    }
}

fn camera_kind(is_3d: bool, is_2d: bool) -> &'static str {
    match (is_3d, is_2d) {
        (true, _) => "primary_3d",
        (_, true) => "overlay_2d",
        _ => "camera",
    }
}

/// Derive the AA matrix from the probed capabilities and the primary camera.
/// Pure so the unit tests can cover supported/unsupported and pixel/quality
/// paths without a GPU.
pub fn derive_aa_matrix(
    capabilities: &RenderCapabilities,
    primary: Option<&CameraRecord>,
) -> AaMatrix {
    let msaa_current = primary
        .map(|record| record.msaa.clone())
        .unwrap_or_else(|| "absent".to_string());
    let depth_prepass_active = primary.is_some_and(|record| record.depth_prepass);
    let mut notes = vec![
        "MSAA and post-process AA cannot be combined blindly; D06 selects the shipping path (R045)"
            .to_string(),
        "FXAA/SMAA/TAA components are compiled in the locked bevy_core_pipeline build".to_string(),
    ];
    if !depth_prepass_active {
        notes.push(
            "no DepthPrepass on the primary camera: contact shadows/SSAO would be inert".into(),
        );
    }
    if !capabilities.hdr_color.sample4 {
        notes.push("rgba16float MULTISAMPLE_X4 is not reported by the adapter".into());
    }
    AaMatrix {
        msaa_current,
        msaa_x4_supported: capabilities.hdr_color.sample4,
        hdr_target: capabilities.hdr_supported(),
        depth_prepass_active,
        motion_vectors_active: false,
        fxaa_available: true,
        smaa_available: true,
        taa_available: true,
        notes,
    }
}

type CameraQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Camera,
        &'static RenderTarget,
        Option<&'static Camera3d>,
        Option<&'static Camera2d>,
        Option<&'static Hdr>,
        Option<&'static Msaa>,
        Option<&'static Exposure>,
        Option<&'static Tonemapping>,
        Option<&'static Bloom>,
        Option<&'static ScreenSpaceAmbientOcclusion>,
        Option<&'static ContactShadows>,
        Option<&'static AtmosphereSettings>,
        Option<&'static AtmosphereEnvironmentMapLight>,
        Option<&'static DistanceFog>,
        Option<&'static DepthPrepass>,
    ),
>;

#[allow(clippy::too_many_arguments)]
pub fn collect_pass_inventory(
    mut commands: Commands,
    capabilities: Res<RenderCapabilities>,
    shadow_map: Res<DirectionalLightShadowMap>,
    clear: Res<ClearColor>,
    images: Res<Assets<Image>>,
    cameras: CameraQuery,
) {
    let mut records: Vec<(isize, CameraRecord)> = Vec::new();
    for (
        camera,
        target,
        is_3d,
        is_2d,
        hdr,
        msaa,
        exposure,
        tonemapping,
        bloom,
        ssao,
        contact,
        atmosphere,
        atmosphere_fill,
        fog,
        depth_prepass,
    ) in &cameras
    {
        let hdr_active = hdr.is_some();
        let viewport = camera.physical_viewport_size().unwrap_or(UVec2::ZERO);
        let record = CameraRecord {
            kind: camera_kind(is_3d.is_some(), is_2d.is_some()),
            order: camera.order,
            hdr: hdr_active,
            msaa: msaa
                .map(|msaa| format!("{msaa:?}"))
                .unwrap_or_else(|| "absent".to_string()),
            tonemapping: tonemapping
                .map(|tonemapping| format!("{tonemapping:?}"))
                .unwrap_or_else(|| "absent".to_string()),
            exposure_ev100: exposure.map(|exposure| exposure.ev100),
            clear_color: format!("{:?}", camera.clear_color),
            depth_prepass: depth_prepass.is_some(),
            bloom: bloom.map(|bloom| BloomRecord {
                intensity: bloom.intensity,
                threshold: bloom.prefilter.threshold,
                threshold_softness: bloom.prefilter.threshold_softness,
                low_frequency_boost: bloom.low_frequency_boost,
            }),
            ssao: ssao.is_some(),
            contact_shadow_length: contact.map(|contact| contact.length),
            atmosphere: atmosphere.map(|settings| {
                match settings.rendering_method {
                    AtmosphereMode::Raymarched => "Raymarched",
                    AtmosphereMode::LookupTexture => "LookupTexture",
                }
                .to_string()
            }),
            atmosphere_fill: atmosphere_fill.map(|fill| fill.intensity),
            distance_fog: fog.map(|fog| format!("{:?}", fog.falloff)),
            target: target_record(target, viewport, hdr_active, &images),
            viewport: [viewport.x, viewport.y],
        };
        records.push((camera.order, record));
    }
    records.sort_by_key(|(order, _)| *order);
    let cameras: Vec<CameraRecord> = records.into_iter().map(|(_, record)| record).collect();
    let primary = cameras
        .iter()
        .find(|record| record.kind == "primary_3d" && record.hdr);
    let aa = derive_aa_matrix(&capabilities, primary);

    let inventory = PassInventory {
        schema_version: PASS_INVENTORY_SCHEMA_VERSION,
        shadow_map_size: shadow_map.size,
        global_clear_color: format!("{:?}", clear.0),
        cameras,
        pass_order: PASS_ORDER.to_vec(),
        color_chain: COLOR_CHAIN.to_vec(),
        aa,
        notes: vec![
            "collected in PostStartup from the locked component state; per-pass GPU timing is deferred to K03"
                .to_string(),
            "the probe insertion point is Core3dSystems::PostProcess before tonemapping (validated by capture)"
                .to_string(),
            "only the primary HDR camera receives D01 probes; overlay/photo cameras are recorded for composition"
                .to_string(),
        ],
    };
    info!(
        "render pass inventory: cameras={} hdr_targets={} shadow_map={} msaa={}",
        inventory.cameras.len(),
        inventory.cameras.iter().filter(|camera| camera.hdr).count(),
        inventory.shadow_map_size,
        inventory.aa.msaa_current,
    );
    commands.insert_resource(inventory);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visual::{FormatSupport, RenderCapabilities};

    fn record(kind: &'static str, msaa: &str, hdr: bool, prepass: bool) -> CameraRecord {
        CameraRecord {
            kind,
            order: 0,
            hdr,
            msaa: msaa.to_string(),
            tonemapping: "AcesFitted".to_string(),
            exposure_ev100: Some(13.0),
            clear_color: "Global".to_string(),
            depth_prepass: prepass,
            bloom: None,
            ssao: true,
            contact_shadow_length: Some(2.5),
            atmosphere: Some("Raymarched".to_string()),
            atmosphere_fill: Some(0.75),
            distance_fog: Some("Linear".to_string()),
            target: TargetRecord {
                kind: "window".to_string(),
                width: 1280,
                height: 720,
                format: "Rgba16Float (Hdr camera)".to_string(),
            },
            viewport: [1280, 720],
        }
    }

    #[test]
    fn pass_order_has_unique_named_stages_and_documents_the_probe() {
        let names: Vec<&str> = PASS_ORDER.iter().map(|stage| stage.name).collect();
        for (index, name) in names.iter().enumerate() {
            assert!(!name.is_empty());
            assert!(
                !names[index + 1..].contains(name),
                "duplicate pass stage {name}"
            );
        }
        let probe = PASS_ORDER
            .iter()
            .find(|stage| stage.name == "exposure_probe_d01")
            .expect("D01 probe stage is documented");
        assert!(probe.schedule.contains("PostProcess"));
        assert!(probe.reads.contains("HDR"));
        assert!(probe.fallback.contains("skipped"));
        let tonemap = PASS_ORDER
            .iter()
            .position(|stage| stage.name == "tonemapping")
            .expect("tonemapping documented");
        let probe_index = PASS_ORDER
            .iter()
            .position(|stage| stage.name == "exposure_probe_d01")
            .expect("probe documented");
        assert!(probe_index < tonemap, "probe runs before tonemapping");
    }

    #[test]
    fn color_chain_starts_at_srgb_and_ends_at_presentation() {
        let first = COLOR_CHAIN.first().expect("chain not empty");
        let last = COLOR_CHAIN.last().expect("chain not empty");
        assert_eq!(first.space, "sRGB");
        assert_eq!(last.space, "bgra8unorm-srgb");
        let tonemaps = COLOR_CHAIN
            .iter()
            .filter(|stage| stage.operation.contains("ACES fitted applied once"))
            .count();
        assert_eq!(tonemaps, 1, "display transform must be documented once");
    }

    #[test]
    fn aa_matrix_reports_current_camera_and_device_support() {
        let capabilities = RenderCapabilities {
            hdr_color: FormatSupport {
                render_attachment: true,
                texture_binding: true,
                storage_binding: true,
                sample4: true,
                filterable: true,
            },
            ..Default::default()
        };

        let primary = record("primary_3d", "Off", true, true);
        let matrix = derive_aa_matrix(&capabilities, Some(&primary));
        assert_eq!(matrix.msaa_current, "Off");
        assert!(matrix.msaa_x4_supported);
        assert!(matrix.hdr_target);
        assert!(matrix.depth_prepass_active);
        assert!(!matrix.motion_vectors_active);

        let weak = RenderCapabilities {
            hdr_color: FormatSupport {
                sample4: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let matrix = derive_aa_matrix(&weak, None);
        assert!(!matrix.msaa_x4_supported);
        assert_eq!(matrix.msaa_current, "absent");
        assert!(!matrix.depth_prepass_active);
        assert!(
            matrix
                .notes
                .iter()
                .any(|note| note.contains("MULTISAMPLE_X4")),
            "unsupported MSAA must be reported, not silently assumed"
        );
    }

    #[test]
    fn camera_kind_distinguishes_3d_primary_and_overlay() {
        assert_eq!(camera_kind(true, false), "primary_3d");
        assert_eq!(camera_kind(false, true), "overlay_2d");
        assert_eq!(camera_kind(false, false), "camera");
    }
}
