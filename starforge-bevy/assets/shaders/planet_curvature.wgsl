// Shared continuous planet curvature for distant terrain.
#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

struct PlanetCurveUniform {
    center_radius: vec4<f32>,
    profile: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> curve: PlanetCurveUniform;

fn curve_position(flat: vec3<f32>) -> vec3<f32> {
    let d = flat.xz - curve.center_radius.xz;
    let distance = length(d);
    let direction = d / max(distance, 0.0001);
    let angle = min(distance / curve.center_radius.w, 1.45);
    let radial = vec3<f32>(direction.x * sin(angle), cos(angle), direction.y * sin(angle));
    let height = flat.y - curve.profile.x;
    let sphere = curve.center_radius.xyz + radial * (curve.center_radius.w + height);
    let blend = smoothstep(curve.profile.y, curve.profile.z, distance);
    return mix(flat, sphere, blend);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let flat = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex_in.position, 1.0)
    ).xyz;
    let curved = curve_position(flat);

#ifdef VERTEX_NORMALS
    let flat_normal = mesh_functions::mesh_normal_local_to_world(
        vertex_in.normal,
        vertex_in.instance_index
    );
    let ny = max(abs(flat_normal.y), 0.05) * sign(flat_normal.y);
    let slope_x = -flat_normal.x / ny;
    let slope_z = -flat_normal.z / ny;
    let tangent_x = curve_position(flat + vec3<f32>(1.0, slope_x, 0.0)) - curved;
    let tangent_z = curve_position(flat + vec3<f32>(0.0, slope_z, 1.0)) - curved;
    out.world_normal = normalize(cross(tangent_z, tangent_x));
#endif
#ifdef VERTEX_POSITIONS
    out.world_position = vec4<f32>(curved, 1.0);
    out.position = position_world_to_clip(curved);
#endif
#ifdef VERTEX_UVS_A
    out.uv = vertex_in.uv;
#endif
#ifdef VERTEX_COLORS
    out.color = vertex_in.color;
#endif
    return out;
}
