// D01 exposure/clipping probe. Runs in Core3dSystems::PostProcess before
// tonemapping, so the sampled `scene` is scene-linear HDR (Rgba16Float on HDR
// cameras) and the output feeds Bloom/tonemapping like any main-pass result.
//
// modes: 1 false-color EV bands, 2 clipping zebra, 3 log-luminance view.
// Mode 0 (or no ExposureProbe component) returns the source unchanged.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct ExposureProbeSettings {
    mode: u32,
    middle_gray: f32,
    clip_stops: f32,
    shadow_stops: f32,
    exposure_ev100: f32,
    strength: f32,
};

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: ExposureProbeSettings;

// The shipping camera uses Exposure { ev100: 13.0 }; the probe rescales to the
// live value so exposure experiments keep one reference for middle gray.
const BASE_EXPOSURE_EV: f32 = 13.0;
const MIN_LINEAR: f32 = 1e-6;

fn scene_luminance(color: vec3<f32>) -> f32 {
    return dot(max(color, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn exposure_stops(color: vec3<f32>) -> f32 {
    let exposure_scale = exp2(BASE_EXPOSURE_EV - settings.exposure_ev100);
    let luma = max(scene_luminance(color) * exposure_scale, MIN_LINEAR);
    return log2(luma / max(settings.middle_gray, 1e-4));
}

fn band_color(stops: f32) -> vec3<f32> {
    if (stops < -6.0) {
        return vec3<f32>(0.03, 0.03, 0.35);
    }
    if (stops < -4.0) {
        return vec3<f32>(0.12, 0.25, 0.85);
    }
    if (stops < -2.0) {
        return vec3<f32>(0.10, 0.65, 0.55);
    }
    if (stops < 0.0) {
        return vec3<f32>(0.25, 0.55, 0.15);
    }
    if (stops < 2.0) {
        return vec3<f32>(0.85, 0.85, 0.20);
    }
    if (stops < 4.0) {
        return vec3<f32>(0.95, 0.55, 0.10);
    }
    return vec3<f32>(0.95, 0.10, 0.10);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(screen_texture, texture_sampler, in.uv);
    if (settings.mode == 0u) {
        return scene;
    }
    let stops = exposure_stops(scene.rgb);
    var overlay = scene.rgb;
    if (settings.mode == 1u) {
        overlay = band_color(stops);
    } else if (settings.mode == 2u) {
        if (stops >= settings.clip_stops) {
            overlay = vec3<f32>(1.0, 0.0, 0.85);
        } else if (stops <= settings.shadow_stops) {
            overlay = vec3<f32>(0.05, 0.35, 1.0);
        }
    } else {
        let normalized = clamp((stops + 6.0) / 12.0, 0.0, 1.0);
        let checker = 0.04 * f32((u32(in.position.x) + u32(in.position.y)) & 8u) / 8.0;
        overlay = vec3<f32>(normalized + checker);
    }
    let mixed = mix(scene.rgb, overlay, clamp(settings.strength, 0.0, 1.0));
    return vec4<f32>(mixed, scene.a);
}
