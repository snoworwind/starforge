// Depth-aware volumetric clouds constrained to a true spherical shell.
#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    mesh_view_bindings::{view, depth_prepass_texture},
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    view_transformations::{position_world_to_clip, position_ndc_to_world},
}

struct CloudShellUniform {
    center_radius: vec4<f32>,
    shell: vec4<f32>,
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
    quality: vec4<f32>,
    wind: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> params: CloudShellUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var density_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var density_sampler: sampler;

const PI: f32 = 3.14159265359;
const INV_FOUR_PI: f32 = 0.07957747155;
const MAX_STEPS: u32 = 64u;
const LIGHT_STEPS: u32 = 4u;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex_in.position, 1.0)
    );
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = normalize(out.world_position.xyz - params.center_radius.xyz);
    return out;
}

fn ray_sphere(origin: vec3<f32>, direction: vec3<f32>, radius: f32) -> vec2<f32> {
    let local_origin = origin - params.center_radius.xyz;
    let b = dot(local_origin, direction);
    let c = dot(local_origin, local_origin) - radius * radius;
    let discriminant = b * b - c;
    if discriminant < 0.0 {
        return vec2<f32>(1e20, -1e20);
    }
    let root = sqrt(discriminant);
    return vec2<f32>(-b - root, -b + root);
}

fn shell_interval(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    let outer = ray_sphere(origin, direction, params.shell.y);
    if outer.y <= 0.0 {
        return vec2<f32>(1.0, 0.0);
    }
    var start = max(outer.x, 0.0);
    var end = outer.y;
    let inner = ray_sphere(origin, direction, params.shell.x);
    let radius = length(origin - params.center_radius.xyz);
    if radius < params.shell.x {
        start = max(start, inner.y);
    } else if radius < params.shell.y {
        start = 0.0;
        if inner.x > 0.0 {
            end = min(end, inner.x);
        }
    } else if inner.x > start && inner.x < end {
        end = inner.x;
    }
    return vec2<f32>(start, end);
}

fn density_at(world_position: vec3<f32>) -> f32 {
    let local = world_position - params.center_radius.xyz;
    let radius = length(local);
    if radius <= params.shell.x || radius >= params.shell.y {
        return 0.0;
    }
    let normal = local / radius;
    // Azimuthal-equidistant coordinates around the local tangent point avoid
    // the longitude singularity directly above the visual planet center.
    let tangent_length = length(normal.xz);
    let tangent_direction = normal.xz / max(tangent_length, 0.00001);
    let surface_angle = acos(clamp(normal.y, -1.0, 1.0));
    let surface = tangent_direction * surface_angle * params.center_radius.w
        + params.center_radius.xz;
    let height = (radius - params.shell.x) / (params.shell.y - params.shell.x);
    let uvw = vec3<f32>(
        surface.x / params.shell.z + params.wind.x,
        height,
        surface.y / params.shell.z + params.wind.y
    );
    let raw = textureSample(density_texture, density_sampler, uvw).r;
    // Coverage shifts the erosion threshold without introducing a hard boundary.
    let threshold = mix(0.62, 0.18, clamp(params.quality.x, 0.0, 1.0));
    return smoothstep(threshold, min(0.98, threshold + 0.22), raw) * params.quality.y;
}

fn henyey_greenstein(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denominator = max(0.001, 1.0 + g2 - 2.0 * g * cos_theta);
    return INV_FOUR_PI * (1.0 - g2) / (denominator * sqrt(denominator));
}

fn light_transmittance(world_position: vec3<f32>) -> f32 {
    let step_length = 55.0;
    var optical_depth = 0.0;
    for (var i = 0u; i < LIGHT_STEPS; i += 1u) {
        let sample_position = world_position + params.sun.xyz * (f32(i) + 0.5) * step_length;
        optical_depth += density_at(sample_position) * params.ambient.w * step_length;
    }
    return exp(-optical_depth);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;
    let camera_radius = length(view.world_position - params.center_radius.xyz);
    let camera_inside_outer = camera_radius < params.shell.y + 16.0;
    if (camera_inside_outer && is_front) || (!camera_inside_outer && !is_front) {
        out.color = vec4<f32>(0.0);
        return out;
    }

    let pixel = vec2<i32>(in.position.xy);
    let dimensions = vec2<f32>(textureDimensions(depth_prepass_texture));
    let uv = in.position.xy / dimensions;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let far_world = position_ndc_to_world(vec3<f32>(ndc, 0.000001));
    let ray_origin = view.world_position;
    let ray_direction = normalize(far_world - ray_origin);
    var interval = shell_interval(ray_origin, ray_direction);

    let scene_depth = textureLoad(depth_prepass_texture, pixel, 0);
    if scene_depth > 0.0 {
        let scene_world = position_ndc_to_world(vec3<f32>(ndc, scene_depth));
        interval.y = min(interval.y, distance(scene_world, ray_origin));
    }
    if interval.y <= interval.x {
        out.color = vec4<f32>(0.0);
        return out;
    }

    let steps = u32(clamp(params.quality.z, 4.0, f32(MAX_STEPS)));
    let step_length = (interval.y - interval.x) / f32(steps);
    let jitter = fract(sin(dot(in.position.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    var distance_along_ray = interval.x + jitter * step_length;
    var transmittance = 1.0;
    var radiance = vec3<f32>(0.0);
    let phase = henyey_greenstein(dot(params.sun.xyz, -ray_direction), params.quality.w);

    for (var i = 0u; i < MAX_STEPS; i += 1u) {
        if i >= steps || distance_along_ray > interval.y || transmittance < 0.01 {
            break;
        }
        let sample_position = ray_origin + ray_direction * distance_along_ray;
        let density = density_at(sample_position);
        if density > 0.0005 {
            let light_trans = light_transmittance(sample_position);
            let direct = params.sun_color.rgb * params.sun.w * phase * light_trans * 4.5;
            let ambient = params.ambient.rgb * params.sun_color.w * (0.45 + 0.55 * light_trans);
            let sample_transmittance = exp(-density * params.ambient.w * step_length);
            let sample_alpha = 1.0 - sample_transmittance;
            radiance += transmittance * (direct + ambient) * sample_alpha;
            transmittance *= sample_transmittance;
        }
        distance_along_ray += step_length;
    }

    out.color = vec4<f32>(radiance, 1.0 - transmittance);
    return out;
}
