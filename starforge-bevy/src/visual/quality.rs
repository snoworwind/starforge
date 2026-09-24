//! A03 quality resolution: turns the persisted user request plus the probed
//! renderer capabilities into the settings the engine will actually use.
//!
//! The rule everywhere is "no silent no-op": a requested feature that the
//! device cannot run resolves to `effective = false` with a human-readable
//! reason, and the UI reads this resource instead of pretending the request
//! took effect.

use bevy::prelude::*;
use serde::Serialize;

use super::capabilities::RenderCapabilities;
use crate::save::Settings;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityTier {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualFeature {
    Clouds,
    Weather,
    Ssao,
    ContactShadows,
    Bloom,
    Atmosphere,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FeatureResolution {
    /// Whether the device/build can run the feature at all.
    pub supported: bool,
    /// What the user asked for (persisted settings).
    pub requested: bool,
    /// What the renderer will run this frame.
    pub effective: bool,
    /// Why a requested feature was disabled; `None` when no downgrade happened.
    pub reason: Option<String>,
}

/// Requested quality derived from persisted settings. The tier is a temporary
/// heuristic until the settings UI exposes an explicit tier (J05); the
/// resolver never invents values that are not applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QualityProfile {
    pub tier: QualityTier,
    pub requested_shadow_map_size: u32,
    pub clouds: bool,
    pub weather: bool,
    pub ssao: bool,
    pub contact_shadows: bool,
    pub bloom: bool,
    pub atmosphere: bool,
}

impl QualityProfile {
    pub fn from_settings(settings: &Settings) -> Self {
        let tier = match settings.view_dist {
            14.. => QualityTier::High,
            8..=13 => QualityTier::Medium,
            _ => QualityTier::Low,
        };
        Self {
            tier,
            requested_shadow_map_size: 4096,
            clouds: settings.clouds,
            weather: settings.weather,
            ssao: true,
            contact_shadows: true,
            bloom: true,
            atmosphere: true,
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Default)]
pub struct ResolvedQuality {
    pub tier: QualityTier,
    pub hdr: bool,
    pub msaa4_supported: bool,
    /// Shadow map edge in texels after clamping to device limits.
    pub shadow_map_size: u32,
    pub clouds: FeatureResolution,
    pub weather: FeatureResolution,
    pub ssao: FeatureResolution,
    pub contact_shadows: FeatureResolution,
    pub bloom: FeatureResolution,
    pub atmosphere: FeatureResolution,
    /// One line per requested-but-unavailable feature, for diagnostics/QA.
    pub downgrades: Vec<String>,
}

impl ResolvedQuality {
    pub fn feature(&self, feature: VisualFeature) -> &FeatureResolution {
        match feature {
            VisualFeature::Clouds => &self.clouds,
            VisualFeature::Weather => &self.weather,
            VisualFeature::Ssao => &self.ssao,
            VisualFeature::ContactShadows => &self.contact_shadows,
            VisualFeature::Bloom => &self.bloom,
            VisualFeature::Atmosphere => &self.atmosphere,
        }
    }

    pub fn has_downgrades(&self) -> bool {
        !self.downgrades.is_empty()
    }
}

fn resolve_feature(
    label: &str,
    requested: bool,
    supported: bool,
    reason: Option<String>,
    downgrades: &mut Vec<String>,
) -> FeatureResolution {
    let effective = requested && supported;
    if requested && !supported {
        let detail = reason.unwrap_or_else(|| "设备不支持".to_string());
        downgrades.push(format!("{label}：{detail}，已关闭"));
        FeatureResolution {
            supported,
            requested,
            effective: false,
            reason: Some(detail),
        }
    } else {
        FeatureResolution {
            supported,
            requested,
            effective,
            reason: None,
        }
    }
}

pub fn resolve_quality(profile: &QualityProfile, caps: &RenderCapabilities) -> ResolvedQuality {
    let mut downgrades = Vec::new();

    let hdr = caps.hdr_supported();
    if !hdr {
        downgrades.push("HDR：rgba16float 目标不可用，改用 LDR 合成".to_string());
    }
    let atmosphere_supported = caps.atmosphere_supported();
    let clouds_supported = caps.clouds_supported();
    let depth_ok = caps.depth_supported();

    let shadow_map_size = caps.recommended_shadow_map_size(profile.requested_shadow_map_size);
    if shadow_map_size < profile.requested_shadow_map_size {
        downgrades.push(format!(
            "阴影贴图：{}→{}（max_texture_dimension_2d={}）",
            profile.requested_shadow_map_size,
            shadow_map_size,
            caps.limits.max_texture_dimension_2d
        ));
    }

    let clouds_reason = if !caps.hdr_supported() {
        Some("HDR 目标不可用".to_string())
    } else if !caps.density_3d.texture_binding {
        Some("r8unorm 不支持纹理绑定".to_string())
    } else {
        Some(format!(
            "三维纹理上限 {} < 云密度纹理 {}",
            caps.limits.max_texture_dimension_3d,
            super::capabilities::CLOUD_DENSITY_SIZE
        ))
    };
    let clouds = resolve_feature(
        "体积云",
        profile.clouds,
        clouds_supported,
        clouds_reason,
        &mut downgrades,
    );
    let weather = resolve_feature("生态天气", profile.weather, true, None, &mut downgrades);
    let ssao = resolve_feature(
        "屏幕空间环境光遮蔽",
        profile.ssao,
        depth_ok,
        Some("depth32float 不能作为渲染目标".to_string()),
        &mut downgrades,
    );
    let contact_shadows = resolve_feature(
        "接触阴影",
        profile.contact_shadows,
        depth_ok,
        Some("depth32float 不能作为渲染目标".to_string()),
        &mut downgrades,
    );
    let bloom = resolve_feature(
        "泛光",
        profile.bloom,
        hdr,
        Some("HDR 目标不可用".to_string()),
        &mut downgrades,
    );
    let atmosphere = resolve_feature(
        "大气天空",
        profile.atmosphere,
        atmosphere_supported,
        Some("rgba16float 不支持 STORAGE_BINDING（Bevy AtmospherePlugin 不会加载）".to_string()),
        &mut downgrades,
    );

    ResolvedQuality {
        tier: if caps.simulated.is_some() {
            QualityTier::Low
        } else {
            profile.tier
        },
        hdr,
        msaa4_supported: caps.hdr_color.sample4,
        shadow_map_size,
        clouds,
        weather,
        ssao,
        contact_shadows,
        bloom,
        atmosphere,
        downgrades,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visual::capabilities::{FormatSupport, LimitSet};

    fn capable_caps() -> RenderCapabilities {
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
            limits: LimitSet {
                max_texture_dimension_2d: 16_384,
                max_texture_dimension_3d: 2_048,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn requested_all() -> QualityProfile {
        QualityProfile {
            tier: QualityTier::High,
            requested_shadow_map_size: 4096,
            clouds: true,
            weather: true,
            ssao: true,
            contact_shadows: true,
            bloom: true,
            atmosphere: true,
        }
    }

    #[test]
    fn capable_device_keeps_every_requested_feature() {
        let resolved = resolve_quality(&requested_all(), &capable_caps());
        assert!(resolved.clouds.effective);
        assert!(resolved.weather.effective);
        assert!(resolved.atmosphere.effective);
        assert!(resolved.bloom.effective);
        assert!(resolved.hdr);
        assert_eq!(resolved.shadow_map_size, 4096);
        assert!(!resolved.has_downgrades());
    }

    #[test]
    fn low_device_disables_features_with_reasons() {
        let mut caps = capable_caps();
        caps.limits.max_texture_dimension_2d = 2_048;
        caps.limits.max_texture_dimension_3d = 128;
        caps.hdr_color.storage_binding = false;
        caps.simulated = Some("low-device".to_string());
        let resolved = resolve_quality(&requested_all(), &caps);
        assert!(!resolved.clouds.effective);
        assert!(resolved.clouds.reason.is_some());
        assert!(!resolved.atmosphere.effective);
        assert!(resolved.atmosphere.reason.is_some());
        assert_eq!(resolved.shadow_map_size, 2_048);
        assert_eq!(resolved.tier, QualityTier::Low);
        assert!(resolved.has_downgrades());
    }

    #[test]
    fn user_disabled_feature_is_not_reported_as_downgrade() {
        let mut profile = requested_all();
        profile.clouds = false;
        let resolved = resolve_quality(&profile, &capable_caps());
        assert!(!resolved.clouds.effective);
        assert!(resolved.clouds.supported);
        assert!(resolved.clouds.reason.is_none());
        assert!(!resolved.has_downgrades());
    }

    #[test]
    fn missing_hdr_disables_bloom_without_touching_weather() {
        let mut caps = capable_caps();
        caps.hdr_color.render_attachment = false;
        let resolved = resolve_quality(&requested_all(), &caps);
        assert!(!resolved.hdr);
        assert!(!resolved.bloom.effective);
        assert!(resolved.weather.effective);
        assert!(!resolved.clouds.effective);
    }
}
