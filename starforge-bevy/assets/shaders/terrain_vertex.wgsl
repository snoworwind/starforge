// STARFORGE terrain vertex shader — planet curvature + water waves + scan varyings.
// Replaces the StandardMaterial vertex shader (static meshes only: no skinning/morph).
#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

struct CurveUniform {
    center: vec2<f32>,
    amt: f32,
    grow: f32,
    wave_time: f32,
    wave_on: f32,
    fade: f32,
    edge_r: f32,
    pad: f32,
    scan_r: f32,
    scan_cx: f32,
    scan_cz: f32,
    scan_a: f32,
    far_hole_on: f32,
    far_hole_r0: f32,
    far_hole_r1: f32,
    far_hole_cx: f32,
    far_hole_cz: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> curve: CurveUniform;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex_in.normal,
        vertex_in.instance_index
    );
#endif

#ifdef VERTEX_POSITIONS
    var wp = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex_in.position, 1.0)
    ).xyz;
    // planet curvature (anchor SEA_Y = 28)
    let anchor = 28.0;
    wp.y = anchor + (wp.y - anchor) * curve.grow;
    let dx = wp.x - curve.center.x;
    let dz = wp.z - curve.center.y;
    wp.y -= curve.amt * (dx * dx + dz * dz) * 0.002;
#ifdef VERTEX_NORMALS
    if curve.wave_on > 0.5 && vertex_in.normal.y > 0.5 {
        wp.y += sin(wp.x * 0.85 + curve.wave_time * 2.2) * 0.035
            + cos(wp.z * 0.70 + curve.wave_time * 1.6) * 0.035;
    }
#endif
    out.world_position = vec4<f32>(wp, 1.0);
    out.position = position_world_to_clip(wp);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex_in.uv;
#endif
#ifdef VERTEX_COLORS
    out.color = vertex_in.color;
#endif
    return out;
}
