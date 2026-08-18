// STARFORGE terrain fragment shader — PBR + glow blocks + curvature edge fade + scan pulse.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

struct CurveUniform {
    center: vec2<f32>,
    amt: f32,
    grow: f32,
    wave_time: f32,
    wave_on: f32,
    water_on: f32,
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

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // curvature edge fade + global fade + far hole (applied before alpha discard)
    let dx = in.world_position.x - curve.center.x;
    let dz = in.world_position.z - curve.center.y;
    let r2 = dx * dx + dz * dz;
    let edge_fade = smoothstep(0.0, 3600.0, curve.edge_r * curve.edge_r - r2);
    var far_a = 1.0;
    if curve.far_hole_on > 0.5 {
        // 远景挖空环（JS farHoleU 同口径：smoothstep(r0², r1², d²)），替代旧版 CPU 逐帧改写顶点 alpha
        far_a = smoothstep(curve.far_hole_r0 * curve.far_hole_r0, curve.far_hole_r1 * curve.far_hole_r1, r2);
    }
    let fade = curve.fade * edge_fade * far_a;
    let bc = pbr_input.material.base_color;
    pbr_input.material.base_color = vec4<f32>(bc.rgb, bc.a * fade);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef VERTEX_COLORS
    // glow blocks: vertex color > 1.0 encodes additive emissive
    let emissive_add = max(vec3<f32>(0.0), in.color.rgb - vec3<f32>(1.0));
#endif

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

#ifdef VERTEX_COLORS
    let lit = out.color;
    out.color = vec4<f32>(lit.rgb + emissive_add * 0.9 * fade, lit.a);
#endif

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);

    // NMS-style scan pulse
    let sd = length(in.world_position.xz - vec2<f32>(curve.scan_cx, curve.scan_cz)) - curve.scan_r;
    let bk = -sd;
    let trail = smoothstep(0.0, 6.0, bk) * (1.0 - smoothstep(10.0, 55.0, bk));
    let gv = abs(fract(in.world_position.xz * 0.125) - vec2<f32>(0.5));
    let grid = smoothstep(0.40, 0.5, max(gv.x, gv.y));
    let scan_add = vec3<f32>(0.13, 0.86, 0.9) * (exp(-sd * sd * 0.018) + grid * trail * 0.5) * curve.scan_a;
    let pre_scan = out.color;
    out.color = vec4<f32>(pre_scan.rgb + scan_add, pre_scan.a);

    // Animated sky/light sheen for water. This stays stable across streamed
    // voxel chunks while preventing the surface from reading as flat glass.
    if curve.water_on > 0.5 {
        let ripple_a = 0.5 + 0.5 * sin(in.world_position.x * 0.17 + curve.wave_time * 1.8);
        let ripple_b = 0.5 + 0.5 * cos(in.world_position.z * 0.13 - curve.wave_time * 1.25);
        let sheen = ripple_a * ripple_b * 0.20;
        out.color = vec4<f32>(out.color.rgb + vec3<f32>(0.12, 0.27, 0.42) * sheen, out.color.a);
    }

    return out;
}
