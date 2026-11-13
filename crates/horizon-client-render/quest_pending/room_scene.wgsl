// The headset launcher's room (room_scene.rs): a sky with hills behind the window, then the room's
// static mesh lit by two lanterns, the fire, the window and the TV, then the window's glass.
struct Uniform {
    view_proj: mat4x4<f32>,
    rot0: vec4<f32>,
    rot1: vec4<f32>,
    rot2: vec4<f32>,
    // xyz: the eye in the room; w: seconds.
    eye: vec4<f32>,
    tangents: vec4<f32>,
    // xyz: where the sun is; w: how much day there is (0..1).
    sun: vec4<f32>,
    moon: vec4<f32>,
    // xyz: where; w: how far it reaches.
    lights: array<vec4<f32>, 5>,
    // rgb: colour times strength.
    colors: array<vec4<f32>, 5>,
    // x: the fire's flicker (0.6..1.2), y: twilight.
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniform;
@group(0) @binding(1) var tex: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;

// ---- the sky behind the window ----

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    // One triangle over the whole eye.
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: SkyOut;
    out.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.ndc = p * 2.0 - 1.0;
    return out;
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let t = u.tangents;
    let x = mix(t.x, t.y, in.ndc.x * 0.5 + 0.5);
    let y = mix(t.w, t.z, in.ndc.y * 0.5 + 0.5);
    let local = normalize(vec3<f32>(x, y, -1.0));
    let d = u.rot0.xyz * local.x + u.rot1.xyz * local.y + u.rot2.xyz * local.z;
    let day = u.sun.w;
    let up = clamp(d.y, -1.0, 1.0);
    // Day: pale at the horizon, blue above; night: deep blue; twilight tints the horizon.
    let horizon_day = vec3<f32>(0.78, 0.88, 0.97);
    let zenith_day = vec3<f32>(0.32, 0.55, 0.92);
    let horizon_night = vec3<f32>(0.05, 0.07, 0.15);
    let zenith_night = vec3<f32>(0.01, 0.02, 0.07);
    var sky = mix(mix(horizon_night, zenith_night, pow(max(up, 0.0), 0.5)),
                  mix(horizon_day, zenith_day, pow(max(up, 0.0), 0.5)), day);
    let near_sun = pow(max(dot(d, u.sun.xyz), 0.0), 6.0) * u.params.y;
    sky = mix(sky, vec3<f32>(1.0, 0.55, 0.3), near_sun * 0.6);
    // The sun and the moon, blocky discs.
    let sun_cos = dot(d, u.sun.xyz);
    if (sun_cos > 0.9993) {
        sky = mix(sky, vec3<f32>(1.0, 0.96, 0.8), smoothstep(0.0, 1.0, day + 0.2));
    }
    let moon_cos = dot(d, u.moon.xyz);
    if (moon_cos > 0.9995) {
        sky = mix(sky, vec3<f32>(0.85, 0.88, 0.95) * (0.5 + 0.5 * u.moon.w), 1.0 - day);
    }
    // Stars at night.
    let sp = floor(d.xz / max(abs(d.y) + 0.3, 0.3) * 90.0);
    if (d.y > 0.0 && hash21(sp) > 0.992) {
        sky = mix(sky, vec3<f32>(1.0), (1.0 - day) * 0.9);
    }
    // Hills: two ridges of different distance, from the azimuth.
    let az = atan2(d.x, -d.z);
    let far_ridge = 0.035 + 0.05 * sin(az * 3.0 + 1.0) + 0.03 * sin(az * 7.0 + 0.4);
    let near_ridge = 0.0 + 0.06 * sin(az * 2.0 + 3.0) + 0.04 * sin(az * 5.0 + 2.0);
    let far_col = mix(vec3<f32>(0.02, 0.04, 0.06), vec3<f32>(0.42, 0.58, 0.55), day);
    let near_col = mix(vec3<f32>(0.02, 0.05, 0.03), vec3<f32>(0.22, 0.42, 0.2), day);
    if (d.y < far_ridge) {
        sky = far_col;
    }
    if (d.y < near_ridge) {
        // A touch of banding, like grass.
        let stripe = 0.94 + 0.06 * hash21(floor(vec2<f32>(az * 40.0, d.y * 40.0)));
        sky = near_col * stripe;
    }
    return vec4<f32>(sky, 1.0);
}

// ---- the room ----

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) data: u32,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) data: u32,
}

@vertex
fn vs_room(v: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.uv = v.uv;
    out.data = v.data;
    return out;
}

fn normal_of(index: u32) -> vec3<f32> {
    switch index {
        case 0u: { return vec3<f32>(1.0, 0.0, 0.0); }
        case 1u: { return vec3<f32>(-1.0, 0.0, 0.0); }
        case 2u: { return vec3<f32>(0.0, 1.0, 0.0); }
        case 3u: { return vec3<f32>(0.0, -1.0, 0.0); }
        case 4u: { return vec3<f32>(0.0, 0.0, 1.0); }
        default: { return vec3<f32>(0.0, 0.0, -1.0); }
    }
}

// How much of the light a corner of the room keeps: the floor, the ceiling and the walls darken
// where they meet.
fn corner_light(p: vec3<f32>) -> f32 {
    let dx = min(p.x + 3.0, 3.0 - p.x);
    let dz = min(p.z + 3.0, 2.0 - p.z);
    let dy = min(p.y, 3.0 - p.y);
    let near = max(max(1.0 - dx / 0.45, 1.0 - dz / 0.45), 1.0 - dy / 0.45);
    let k = clamp(near, 0.0, 1.0);
    return 1.0 - 0.32 * k * k;
}

fn room_light(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let hemi = mix(0.74, 1.0, n.y * 0.5 + 0.5);
    var acc = (vec3<f32>(0.15, 0.14, 0.16) + vec3<f32>(0.09, 0.1, 0.12) * u.sun.w) * hemi;
    for (var i = 0; i < 5; i++) {
        let to = u.lights[i].xyz - p;
        let dist = max(length(to), 0.05);
        let l = to / dist;
        let reach = clamp(1.0 - dist / u.lights[i].w, 0.0, 1.0);
        let wrap = max(dot(n, l), 0.0) * 0.8 + 0.2;
        acc += u.colors[i].rgb * reach * reach * wrap;
    }
    return acc * corner_light(p);
}

@fragment
fn fs_room(in: VsOut) -> @location(0) vec4<f32> {
    let layer = i32(in.data & 63u);
    let n = normal_of((in.data >> 6u) & 7u);
    var uv = in.uv;
    let flame = (in.data & 2048u) != 0u;
    if (flame) {
        let t = u.eye.w;
        uv.x += 0.05 * sin(t * 11.0 + uv.y * 7.0);
        uv.y += 0.04 * sin(t * 17.0 + uv.x * 5.0);
    }
    var c = textureSample(tex, samp, uv, layer);
    if ((in.data & 1024u) != 0u && c.a < 0.5) {
        discard;
    }
    if ((in.data & 512u) != 0u) {
        // Lit from inside: lantern cores and flames.
        let glow = select(1.15, u.params.x + 0.2, flame);
        return vec4<f32>(c.rgb * glow, 1.0);
    }
    return vec4<f32>(c.rgb * room_light(in.world, n), 1.0);
}

@fragment
fn fs_glass(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv, i32(in.data & 63u));
    return vec4<f32>(c.rgb * (0.6 + 0.5 * u.sun.w), c.a);
}

// The pointer's ray and the controllers: a flat colour (rgb in the low 24 bits of the data).
@fragment
fn fs_flat(in: VsOut) -> @location(0) vec4<f32> {
    let rgb = vec3<f32>(
        f32((in.data >> 16u) & 255u),
        f32((in.data >> 8u) & 255u),
        f32(in.data & 255u),
    ) / 255.0;
    return vec4<f32>(rgb, 1.0);
}
