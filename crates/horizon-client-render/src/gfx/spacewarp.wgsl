// XR_FB_space_warp: forward current-minus-previous motion in NDC, including Z.
// This is deliberately different from FSR's backwards UV motion convention.
struct Params {
    camera_reproject: mat4x4<f32>,
    current: mat4x4<f32>,
    previous: mat4x4<f32>,
    target_size: vec4<f32>,
};
@group(0) @binding(0) var<uniform> params: array<Params, 2>;
var<private> eye: u32;
// Constant indices keep Adreno's uniform loads in registers, as in the world shaders.
fn camera_reproject() -> mat4x4<f32> {
    if eye == 0u { return params[0].camera_reproject; } return params[1].camera_reproject;
}
fn current() -> mat4x4<f32> {
    if eye == 0u { return params[0].current; } return params[1].current;
}
fn previous() -> mat4x4<f32> {
    if eye == 0u { return params[0].previous; } return params[1].previous;
}
fn target_size() -> vec4<f32> {
    if eye == 0u { return params[0].target_size; } return params[1].target_size;
}
@group(0) @binding(1) var depth: texture_depth_2d_array;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;

fn delta(now: vec4<f32>, previous: vec4<f32>) -> vec4<f32> {
    if previous.w <= 0.00001 || now.w <= 0.00001 { return vec4(0.0); }
    return vec4(clamp(now.xyz / now.w - previous.xyz / previous.w, vec3(-2.0), vec3(2.0)), 1.0);
}
fn full_vertex(i: u32) -> vec4<f32> {
    let v = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(v[i], 0.0, 1.0);
}
struct Background {
    @location(0) motion: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};
fn full_pixel(at: vec4<f32>) -> Background {
    let dimensions = vec2<f32>(textureDimensions(depth));
    let pixel = min(vec2<i32>(at.xy / target_size().xy * dimensions), vec2<i32>(dimensions) - 1);
    let z = textureLoad(depth, pixel, i32(eye), 0);
    let uv = (vec2<f32>(pixel) + 0.5) / dimensions;
    let clip = vec4(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), z, 1.0);
    var out: Background;
    out.depth = z;
    out.motion = delta(clip, camera_reproject() * clip);
    if z == 0.0 { out.motion = vec4(0.0); }
    return out;
}
struct Object {
    @builtin(position) clip: vec4<f32>,
    @location(0) current: vec4<f32>,
    @location(1) previous: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) translucent: u32,
};
fn object_vertex(now: vec3<f32>, old: vec3<f32>, uv: vec2<f32>, translucent: u32) -> Object {
    var out: Object;
    out.clip = current() * vec4(now, 1.0);
    out.current = out.clip;
    out.previous = previous() * vec4(old, 1.0);
    out.uv = uv;
    out.translucent = translucent;
    return out;
}
fn object_pixel(in: Object) -> Background {
    let dimensions = vec2<f32>(textureDimensions(depth));
    let pixel = min(vec2<i32>(in.clip.xy / target_size().xy * dimensions), vec2<i32>(dimensions) - 1);
    let stored = textureLoad(depth, pixel, i32(eye), 0);
    let tolerance = stored * 0.002 + 0.0000001;
    let alpha = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0).a;
    if alpha < 0.1 { discard; }
    let overlay = (in.translucent & 2u) != 0u;
    if !overlay {
        if in.clip.z < stored - tolerance { discard; }
        if in.translucent == 0u && abs(in.clip.z - stored) > tolerance { discard; }
    }
    var out: Background;
    out.motion = delta(in.current, in.previous);
    out.depth = select(stored, in.clip.z, overlay);
    return out;
}

@vertex fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return full_vertex(i);
}
@fragment fn fs_full(@builtin(position) at: vec4<f32>) -> Background { return full_pixel(at); }
@vertex fn vs_object(@location(0) now: vec3<f32>, @location(1) old: vec3<f32>,
    @location(2) uv: vec2<f32>, @location(3) translucent: u32) -> Object {
    return object_vertex(now, old, uv, translucent);
}
@fragment fn fs_object(in: Object) -> Background { return object_pixel(in); }

// Multiview entry points (excluded from the module on devices without MULTIVIEW).
@vertex fn vs_full_stereo(@builtin(vertex_index) i: u32, @builtin(view_index) view: u32)
    -> @builtin(position) vec4<f32> { eye = view; return full_vertex(i); }
@fragment fn fs_full_stereo(@builtin(position) at: vec4<f32>, @builtin(view_index) view: u32)
    -> Background { eye = view; return full_pixel(at); }
@vertex fn vs_object_stereo(@location(0) now: vec3<f32>, @location(1) old: vec3<f32>,
    @location(2) uv: vec2<f32>, @location(3) translucent: u32, @builtin(view_index) view: u32)
    -> Object { eye = view; return object_vertex(now, old, uv, translucent); }
@fragment fn fs_object_stereo(in: Object, @builtin(view_index) view: u32)
    -> Background { eye = view; return object_pixel(in); }
