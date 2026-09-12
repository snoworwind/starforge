//! A03 renderer capability probe.
//!
//! Capabilities are read from the *locked* Bevy/wgpu device, not guessed from
//! the GPU marketing name (R012): enabled device features/limits decide what
//! the engine can really run on this build, while the adapter report is kept
//! for diagnostics. `--simulate-low-device` lowers the probed limits so the
//! fallback paths can be exercised without owning the target hardware.

use bevy::prelude::*;
use bevy::render::render_resource::{
    TextureFormat, TextureFormatFeatureFlags, TextureUsages, WgpuFeatures,
};
use bevy::render::renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice};
use serde::Serialize;

/// Horizontal/vertical size of the procedural cloud density texture.
pub const CLOUD_DENSITY_SIZE: u32 = 192;
pub const CLOUD_DENSITY_FORMAT: TextureFormat = TextureFormat::R8Unorm;

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct FormatSupport {
    pub render_attachment: bool,
    pub texture_binding: bool,
    pub storage_binding: bool,
    pub sample4: bool,
    pub filterable: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct LimitSet {
    pub max_texture_dimension_2d: u32,
    pub max_texture_dimension_3d: u32,
    pub max_texture_array_layers: u32,
    pub max_bind_groups: u32,
    pub max_sampled_textures_per_shader_stage: u32,
    pub max_storage_textures_per_shader_stage: u32,
    pub max_uniform_buffer_binding_size: u64,
    pub max_buffer_size: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct FeatureSupport {
    pub timestamp_query: bool,
    pub timestamp_inside_encoders: bool,
    pub timestamp_inside_passes: bool,
    pub texture_binding_array: bool,
    pub non_uniform_indexing: bool,
    pub texture_adapter_specific_formats: bool,
    pub multisample_array: bool,
    pub texture_compression_bc: bool,
    pub texture_compression_etc2: bool,
    pub texture_compression_astc: bool,
}

/// Actual capabilities of the locked render build.
#[derive(Resource, Clone, Debug, Default, Serialize)]
pub struct RenderCapabilities {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    /// `Some(label)` when the run was started with a capability simulation.
    pub simulated: Option<String>,
    pub features: FeatureSupport,
    pub limits: LimitSet,
    /// Camera HDR target format (`rgba16float`).
    pub hdr_color: FormatSupport,
    /// Depth prepass/shadow format (`depth32float`).
    pub depth: FormatSupport,
    /// Procedural cloud density texture format (`r8unorm`, dimension 3D).
    pub density_3d: FormatSupport,
    /// Swapchain/target color format (`bgra8unorm-srgb`).
    pub target_color: FormatSupport,
    /// B03 surface color format (`rgba8unorm-srgb`, atlas and texture array).
    pub surface_color: FormatSupport,
    /// B03 compressed surface format (`bc7-rgba-unorm-srgb`) when BC is on.
    pub surface_bc7: FormatSupport,
}

impl RenderCapabilities {
    pub fn hdr_supported(&self) -> bool {
        self.hdr_color.render_attachment && self.hdr_color.filterable
    }

    /// Bevy's AtmospherePlugin refuses to load without `rgba16float`
    /// `STORAGE_BINDING` (it stores its LUTs there), so the game must treat the
    /// sky as optional instead of assuming it rendered.
    pub fn atmosphere_supported(&self) -> bool {
        self.hdr_supported() && self.hdr_color.storage_binding
    }

    pub fn clouds_supported(&self) -> bool {
        self.hdr_supported()
            && self.density_3d.texture_binding
            && self.limits.max_texture_dimension_3d >= CLOUD_DENSITY_SIZE
    }

    pub fn depth_supported(&self) -> bool {
        self.depth.render_attachment
    }

    /// Largest power-of-two shadow map that fits the device and is not larger
    /// than the requested baseline (4096 today).
    pub fn recommended_shadow_map_size(&self, requested: u32) -> u32 {
        let mut size = requested.clamp(256, 4096);
        while size > 256 && size > self.limits.max_texture_dimension_2d {
            size /= 2;
        }
        size
    }

    /// B03 quality route for terrain surface textures: texture array when the
    /// probed device accepts one filterable layer per material, otherwise the
    /// padded atlas, otherwise the legacy classic atlas. The reason string is
    /// part of the report so a silent fallback is visible (R117).
    pub fn texture_route(&self, materials: u32, base_size: u32) -> crate::art::RouteDecision {
        crate::art::decide_route(crate::art::RouteParams {
            materials,
            base_size,
            max_array_layers: self.limits.max_texture_array_layers,
            max_texture_dimension_2d: self.limits.max_texture_dimension_2d,
            array_filterable: self.surface_color.texture_binding && self.surface_color.filterable,
        })
    }

    /// Compression preference for surface textures, derived from probed
    /// features instead of assuming a platform (R012).
    pub fn surface_compression(&self) -> &'static str {
        if self.features.texture_compression_bc && self.surface_bc7.texture_binding {
            "bc7"
        } else if self.features.texture_compression_astc {
            "astc"
        } else if self.features.texture_compression_etc2 {
            "etc2"
        } else {
            "none"
        }
    }
}

/// Debug/testing overrides applied on top of the probed adapter.
#[derive(Resource, Clone, Debug, Default)]
pub struct CapabilitySimulation {
    pub label: String,
    pub max_texture_dimension_2d: Option<u32>,
    pub max_texture_dimension_3d: Option<u32>,
    pub max_texture_array_layers: Option<u32>,
    pub msaa4: Option<bool>,
    pub hdr_storage: Option<bool>,
}

impl CapabilitySimulation {
    /// A 2048px shadow limit (shadows downgrade), no 3D cloud texture and no
    /// storage image (clouds + atmosphere downgrade), matching the Low tier in
    /// `06` without masking the real adapter report.
    pub fn low_device() -> Self {
        Self {
            label: "low-device".to_string(),
            max_texture_dimension_2d: Some(2048),
            max_texture_dimension_3d: Some(128),
            max_texture_array_layers: Some(1),
            msaa4: Some(false),
            hdr_storage: Some(false),
        }
    }
}

/// Apply debug overrides. The simulation may only lower capabilities, so it
/// can never make an unsupported device appear capable in a report.
pub fn apply_simulation(capabilities: &mut RenderCapabilities, simulation: &CapabilitySimulation) {
    capabilities.simulated = Some(simulation.label.clone());
    if let Some(value) = simulation.max_texture_dimension_2d {
        capabilities.limits.max_texture_dimension_2d =
            capabilities.limits.max_texture_dimension_2d.min(value);
    }
    if let Some(value) = simulation.max_texture_dimension_3d {
        capabilities.limits.max_texture_dimension_3d =
            capabilities.limits.max_texture_dimension_3d.min(value);
    }
    if let Some(value) = simulation.max_texture_array_layers {
        capabilities.limits.max_texture_array_layers =
            capabilities.limits.max_texture_array_layers.min(value);
    }
    if let Some(value) = simulation.msaa4 {
        capabilities.hdr_color.sample4 = value;
    }
    if let Some(value) = simulation.hdr_storage {
        capabilities.hdr_color.storage_binding = value;
        capabilities.density_3d.storage_binding = value;
    }
}

fn format_support(adapter: &RenderAdapter, format: TextureFormat) -> FormatSupport {
    let features = adapter.get_texture_format_features(format);
    FormatSupport {
        render_attachment: features
            .allowed_usages
            .contains(TextureUsages::RENDER_ATTACHMENT),
        texture_binding: features
            .allowed_usages
            .contains(TextureUsages::TEXTURE_BINDING),
        storage_binding: features
            .allowed_usages
            .contains(TextureUsages::STORAGE_BINDING),
        sample4: features
            .flags
            .contains(TextureFormatFeatureFlags::MULTISAMPLE_X4),
        filterable: features
            .flags
            .contains(TextureFormatFeatureFlags::FILTERABLE),
    }
}

pub fn collect_render_capabilities(
    device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
    info: Option<Res<RenderAdapterInfo>>,
    simulation: Option<Res<CapabilitySimulation>>,
    mut commands: Commands,
) {
    let features = device.features();
    let limits = device.limits();
    let mut capabilities = RenderCapabilities {
        adapter: info
            .as_ref()
            .map(|info| info.name.clone())
            .unwrap_or_default(),
        backend: info
            .as_ref()
            .map(|info| format!("{:?}", info.backend))
            .unwrap_or_default(),
        device_type: info
            .as_ref()
            .map(|info| format!("{:?}", info.device_type))
            .unwrap_or_default(),
        driver: info
            .as_ref()
            .map(|info| info.driver.clone())
            .unwrap_or_default(),
        driver_info: info
            .as_ref()
            .map(|info| info.driver_info.clone())
            .unwrap_or_default(),
        simulated: None,
        features: FeatureSupport {
            timestamp_query: features.contains(WgpuFeatures::TIMESTAMP_QUERY),
            timestamp_inside_encoders: features
                .contains(WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS),
            timestamp_inside_passes: features.contains(WgpuFeatures::TIMESTAMP_QUERY_INSIDE_PASSES),
            texture_binding_array: features.contains(WgpuFeatures::TEXTURE_BINDING_ARRAY),
            non_uniform_indexing: features.contains(
                WgpuFeatures::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
            ),
            texture_adapter_specific_formats: features
                .contains(WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
            multisample_array: features.contains(WgpuFeatures::MULTISAMPLE_ARRAY),
            texture_compression_bc: features.contains(WgpuFeatures::TEXTURE_COMPRESSION_BC),
            texture_compression_etc2: features.contains(WgpuFeatures::TEXTURE_COMPRESSION_ETC2),
            texture_compression_astc: features.contains(WgpuFeatures::TEXTURE_COMPRESSION_ASTC),
        },
        limits: LimitSet {
            max_texture_dimension_2d: limits.max_texture_dimension_2d,
            max_texture_dimension_3d: limits.max_texture_dimension_3d,
            max_texture_array_layers: limits.max_texture_array_layers,
            max_bind_groups: limits.max_bind_groups,
            max_sampled_textures_per_shader_stage: limits.max_sampled_textures_per_shader_stage,
            max_storage_textures_per_shader_stage: limits.max_storage_textures_per_shader_stage,
            max_uniform_buffer_binding_size: limits.max_uniform_buffer_binding_size,
            max_buffer_size: limits.max_buffer_size,
        },
        hdr_color: format_support(&adapter, TextureFormat::Rgba16Float),
        depth: format_support(&adapter, TextureFormat::Depth32Float),
        density_3d: format_support(&adapter, CLOUD_DENSITY_FORMAT),
        target_color: format_support(&adapter, TextureFormat::Bgra8UnormSrgb),
        surface_color: format_support(&adapter, TextureFormat::Rgba8UnormSrgb),
        surface_bc7: format_support(&adapter, TextureFormat::Bc7RgbaUnormSrgb),
    };

    if let Some(simulation) = simulation.as_deref() {
        apply_simulation(&mut capabilities, simulation);
    }

    let route = capabilities.texture_route(crate::art::MATERIAL_COUNT as u32, 64);
    info!(
        "render capabilities: adapter={} backend={} simulated={:?} max2d={} max3d={} max_array={} hdr={} atmosphere={} clouds={} timestamps={} surface_route={:?} compression={}",
        capabilities.adapter,
        capabilities.backend,
        capabilities.simulated,
        capabilities.limits.max_texture_dimension_2d,
        capabilities.limits.max_texture_dimension_3d,
        capabilities.limits.max_texture_array_layers,
        capabilities.hdr_supported(),
        capabilities.atmosphere_supported(),
        capabilities.clouds_supported(),
        capabilities.features.timestamp_query,
        route.route,
        capabilities.surface_compression(),
    );
    commands.insert_resource(capabilities);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capable() -> RenderCapabilities {
        RenderCapabilities {
            hdr_color: FormatSupport {
                render_attachment: true,
                texture_binding: true,
                storage_binding: true,
                sample4: true,
                filterable: true,
            },
            depth: FormatSupport {
                render_attachment: true,
                texture_binding: true,
                ..Default::default()
            },
            density_3d: FormatSupport {
                texture_binding: true,
                ..Default::default()
            },
            surface_color: FormatSupport {
                texture_binding: true,
                filterable: true,
                ..Default::default()
            },
            surface_bc7: FormatSupport {
                texture_binding: true,
                ..Default::default()
            },
            features: FeatureSupport {
                texture_compression_bc: true,
                ..Default::default()
            },
            limits: LimitSet {
                max_texture_dimension_2d: 16_384,
                max_texture_dimension_3d: 2_048,
                max_texture_array_layers: 2_048,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn capable_device_supports_all_baseline_features() {
        let caps = capable();
        assert!(caps.hdr_supported());
        assert!(caps.atmosphere_supported());
        assert!(caps.clouds_supported());
        assert_eq!(caps.recommended_shadow_map_size(4096), 4096);
    }

    #[test]
    fn low_device_falls_back_instead_of_claiming_support() {
        let mut caps = capable();
        caps.limits.max_texture_dimension_2d = 2_048;
        caps.limits.max_texture_dimension_3d = 128;
        caps.hdr_color.storage_binding = false;
        assert!(!caps.atmosphere_supported());
        assert!(!caps.clouds_supported());
        assert_eq!(caps.recommended_shadow_map_size(4096), 2048);
    }

    #[test]
    fn simulation_only_lowers_probed_capabilities() {
        let mut caps = capable();
        apply_simulation(&mut caps, &CapabilitySimulation::low_device());
        assert_eq!(caps.simulated.as_deref(), Some("low-device"));
        assert_eq!(caps.limits.max_texture_dimension_2d, 2048);
        assert_eq!(caps.limits.max_texture_dimension_3d, 128);
        assert_eq!(caps.limits.max_texture_array_layers, 1);
        assert!(!caps.hdr_color.sample4);
        assert!(!caps.atmosphere_supported());
        assert!(!caps.clouds_supported());

        // A generous simulation must not inflate a weaker real device.
        let mut weak = capable();
        weak.limits.max_texture_dimension_2d = 1024;
        apply_simulation(
            &mut weak,
            &CapabilitySimulation {
                label: "generous".into(),
                max_texture_dimension_2d: Some(8192),
                max_texture_array_layers: Some(4096),
                ..Default::default()
            },
        );
        assert_eq!(weak.limits.max_texture_dimension_2d, 1024);
        assert_eq!(weak.limits.max_texture_array_layers, 2048);
    }

    #[test]
    fn b03_texture_route_reads_probed_caps() {
        use crate::art::TextureRoute;
        let caps = capable();
        assert_eq!(
            caps.texture_route(crate::art::MATERIAL_COUNT as u32, 64)
                .route,
            TextureRoute::TextureArray
        );
        assert_eq!(caps.surface_compression(), "bc7");

        let mut low = capable();
        apply_simulation(&mut low, &CapabilitySimulation::low_device());
        let decision = low.texture_route(crate::art::MATERIAL_COUNT as u32, 64);
        assert_eq!(decision.route, TextureRoute::PaddedAtlas);
        assert!(decision.reason.contains("array"));

        let mut unfilterable = capable();
        unfilterable.surface_color.filterable = false;
        assert_eq!(
            unfilterable
                .texture_route(crate::art::MATERIAL_COUNT as u32, 64)
                .route,
            TextureRoute::PaddedAtlas
        );
        assert_eq!(unfilterable.surface_compression(), "bc7");
        let mut no_compression = capable();
        no_compression.features.texture_compression_bc = false;
        assert_eq!(no_compression.surface_compression(), "none");
    }
}
