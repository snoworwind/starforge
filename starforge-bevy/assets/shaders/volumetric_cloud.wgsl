// STARFORGE volumetric cloud material.
//
// The coverage map describes where cloud columns exist. The 3D texture carries
// Worley-FBM detail after a CPU-generated curl warp. This pass ray-marches the
// AABB, evaluates single scattering toward the sun, then adds an inexpensive
// multi-scattering approximation for the soft silver lining and dark bases.

#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
}

struct CloudUniform {
    bounds_min: vec4<f32>,
    bounds_max: vec4<f32>,
    wind_time: vec4<f32>,
    shape: vec4<f32>,
    scattering: vec4<f32>,
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> params: CloudUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var coverage_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var coverage_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3)
var detail_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4)
var detail_sampler: sampler;

const PI: f32 = 3.14159265359;
const INV_FOUR_PI: f32 = 0.07957747155;
// Keep the primary march detailed enough for soft silhouettes, but cap the
// nested light march aggressively: this shader runs for every cloud pixel.
const RAY_STEPS: u32 = 32u;
const LIGHT_STEPS: u32 = 3u;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex_in.position, 1.0)
    );
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = vec3<f32>(0.0, 1.0, 0.0);
    return out;
}

fn ray_box(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    var safe_direction = direction;
    if abs(safe_direction.x) < 0.00001 { safe_direction.x = 0.00001; }
    if abs(safe_direction.y) < 0.00001 { safe_direction.y = 0.00001; }
    if abs(safe_direction.z) < 0.00001 { safe_direction.z = 0.00001; }
    let t0 = (params.bounds_min.xyz - origin) / safe_direction;
    let t1 = (params.bounds_max.xyz - origin) / safe_direction;
    let near = min(t0, t1);
    let far = max(t0, t1);
    return vec2<f32>(
        max(max(near.x, near.y), near.z),
        min(min(far.x, far.y), far.z)
    );
}

fn height_profile(world_position: vec3<f32>, base_noise: f32, coarse_noise: f32) -> f32 {
    let height = (world_position.y - params.shape.x) / (params.shape.y - params.shape.x);
    // Use a low-frequency horizontal field for the actual cloud-base height.
    // This creates broad hanging lobes and hollows instead of merely changing
    // the softness of one global horizontal edge.
    let base_lobes = smoothstep(0.18, 0.82, base_noise);
    let bottom_start = 0.018 + base_lobes * 0.20 + (coarse_noise - 0.5) * 0.045;
    let bottom_thickness = mix(0.10, 0.25, base_lobes);
    let bottom_end = min(0.42, bottom_start + bottom_thickness);
    let bottom = smoothstep(bottom_start, bottom_end, height);
    let top_shift = (coarse_noise - 0.5) * 0.08;
    let top = 1.0 - smoothstep(0.62 + top_shift, 1.0 + top_shift, height);
    // A rounded, locally varying vertical body reads as a cloud bank instead
    // of a flat slab.
    return pow(bottom * top, 0.78);
}

fn density_at(world_position: vec3<f32>, detailed: bool) -> f32 {
    let volume_uv = (world_position - params.bounds_min.xyz) /
        (params.bounds_max.xyz - params.bounds_min.xyz);
    // The AABB is only a streaming volume. Fade its horizontal border so its
    // rectangular shape can never become visible in the sky.
    let edge_x = smoothstep(0.0, 0.08, volume_uv.x) * (1.0 - smoothstep(0.92, 1.0, volume_uv.x));
    let edge_z = smoothstep(0.0, 0.08, volume_uv.z) * (1.0 - smoothstep(0.92, 1.0, volume_uv.z));
    let edge_fade = edge_x * edge_z;

    // Coarse detail also acts as a domain warp, breaking straight coverage
    // contours before the higher-frequency erosion is applied.
    // Keep this sample horizontal so it can also drive the uneven cloud base.
    // The lighting march reuses it, avoiding a separate warp lookup.
    let base_uv = fract(vec3<f32>(
        world_position.x * params.shape.w * 0.24 + params.wind_time.x * params.wind_time.z * 0.0005,
        0.47,
        world_position.z * params.shape.w * 0.24 + params.wind_time.y * params.wind_time.z * 0.0005
    ));
    let base_noise = textureSample(detail_texture, detail_sampler, base_uv).r;
    let warp = base_noise * 2.0 - 1.0;
    let coverage_uv = fract(
        world_position.xz * params.shape.z +
        params.wind_time.xz * params.wind_time.z * 0.0008 +
        vec2<f32>(warp * 0.055, -warp * 0.035)
    );
    let coverage_sample = textureSample(coverage_texture, coverage_sampler, coverage_uv);
    let medium_shape = coverage_sample.b;
    // Large cells remain dominant, while the medium mask can create smaller
    // isolated clouds outside the large systems.
    let broad_shape = max(
        smoothstep(0.16, 0.72, coverage_sample.r) * 0.92,
        medium_shape * 0.52
    );

    let coarse_detail_uv = fract(
        world_position * vec3<f32>(params.shape.w * 0.42, params.shape.w * 0.58, params.shape.w * 0.42) +
        vec3<f32>(0.31, 0.07, 0.67)
    );
    let coarse_detail = textureSample(detail_texture, detail_sampler, coarse_detail_uv).r;
    var detail = coarse_detail;
    if detailed {
        let detail_uv = fract(
            world_position * vec3<f32>(params.shape.w, params.shape.w * 1.25, params.shape.w) +
            vec3<f32>(params.wind_time.x, 0.07, params.wind_time.y) * params.wind_time.z * 0.0015
        );
        let fine_detail = textureSample(detail_texture, detail_sampler, detail_uv).r;
        detail = coarse_detail * 0.68 + fine_detail * 0.32;
    }
    // Erode the coverage edge with two scales of detail. The coarse term makes
    // large cauliflower lobes; the fine term adds wisps without square holes.
    let eroded = max(0.0, broad_shape - (1.0 - detail) * params.scattering.w * 0.62);
    let puffy = mix(0.58, 1.0, smoothstep(0.22, 0.76, coarse_detail));
    return eroded * puffy * height_profile(world_position, base_noise, coarse_detail) * edge_fade;
}

fn light_transmittance(world_position: vec3<f32>) -> f32 {
    let light_step = 26.0;
    var transmittance = 1.0;
    for (var i: u32 = 0u; i < LIGHT_STEPS; i += 1u) {
        let p = world_position + params.sun.xyz * (f32(i) + 0.5) * light_step;
        // Fine erosion is visually important along the camera ray, but is
        // too expensive to repeat for every shadow sample.
        let density = density_at(p, false);
        transmittance *= exp(-density * params.scattering.x * light_step * 1.35);
    }
    return transmittance;
}

fn henyey_greenstein(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denominator = max(0.001, 1.0 + g2 - 2.0 * g * cos_theta);
    return INV_FOUR_PI * (1.0 - g2) / (denominator * sqrt(denominator));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;
    let camera_inside = all(view.world_position >= params.bounds_min.xyz) &&
        all(view.world_position <= params.bounds_max.xyz);
    // Outside the volume, march from the front surface. Inside the volume,
    // only the back-facing entry surface is useful. Keeping this explicit
    // avoids rendering the same ray twice now that culling is disabled.
    if camera_inside {
        if is_front {
            out.color = vec4<f32>(0.0);
            return out;
        }
    } else if !is_front {
        out.color = vec4<f32>(0.0);
        return out;
    }
    let ray_origin = view.world_position;
    let ray_direction = normalize(in.world_position.xyz - ray_origin);
    let interval = ray_box(ray_origin, ray_direction);
    let start = max(interval.x, 0.0);
    let end = interval.y;
    if end <= start {
        out.color = vec4<f32>(0.0);
        return out;
    }

    let step_size = (end - start) / f32(RAY_STEPS);
    // A stable per-pixel jitter hides the fixed-step bands at low resolution.
    let jitter = fract(sin(dot(in.position.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    var distance = start + jitter * step_size;
    var transmittance = 1.0;
    var radiance = vec3<f32>(0.0);

    for (var i: u32 = 0u; i < RAY_STEPS; i += 1u) {
        let sample_position = ray_origin + ray_direction * distance;
        let density = density_at(sample_position, true);
        if density > 0.001 {
            let light_trans = light_transmittance(sample_position);
            let view_cos = dot(params.sun.xyz, -ray_direction);
            let phase = henyey_greenstein(view_cos, params.scattering.y);
            let powder = 1.0 - exp(-density * 7.0);

            // Single scattering: direct sun attenuated through the cloud and
            // shaped by the forward HG phase function.
            let single = params.sun_color.rgb * params.sun.w * phase * light_trans * 6.0;
            // Multiple scattering approximation: diffuse sky plus forward
            // energy recovered from the light lost inside the cloud.
            let multi = params.ambient.rgb * params.ambient.w +
                params.sun_color.rgb * params.sun.w * params.scattering.z *
                (1.0 - light_trans) * (0.35 + powder * 0.65);

            let optical_depth = density * params.scattering.x * step_size;
            let step_trans = exp(-optical_depth);
            let step_alpha = 1.0 - step_trans;
            radiance += transmittance * (single + multi) * step_alpha;
            transmittance *= step_trans;
            if transmittance < 0.015 {
                break;
            }
        }
        distance += step_size;
        if distance > end {
            break;
        }
    }

    let alpha = 1.0 - transmittance;
    if alpha <= 0.001 {
        out.color = vec4<f32>(0.0);
        return out;
    }
    // The integral is premultiplied by alpha; convert it to the source color
    // expected by Bevy's non-premultiplied alpha blend state.
    out.color = vec4<f32>(radiance / alpha, alpha);
    return out;
}
