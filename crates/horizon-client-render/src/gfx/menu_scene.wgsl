// SPDX-License-Identifier: MPL-2.0
// Ports Visor: render/decoration/decorators/mainmenu/VRMenuSky.java (palette, sun, moon, stars, clouds,
// the dotted VISOR sign) and VRMenuFloor.java (the moss floor). Copyright (C) Phoenix-Ra and Visor
// contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//
// The menu room of a headset: one full-screen pass per eye. Each pixel's ray, in the room's frame
// (+Y up, the panel at -Z), meets the floor, the cloud layer below it or the sky.
struct Scene {
    // The eye's turn, columns of the rotation matrix (xyz, w unused).
    rot0: vec4<f32>,
    rot1: vec4<f32>,
    rot2: vec4<f32>,
    // The eye in the room (xyz); w the time in seconds.
    eye: vec4<f32>,
    // Tangents of the lens: left, right, up, down.
    tangents: vec4<f32>,
    // The sun's direction (xyz) and how much of day there is (w, 0 night to 1 day).
    sun: vec4<f32>,
    // The moon's direction (xyz) and how much of it is lit (w).
    moon: vec4<f32>,
    // The play area's half width and depth, the border around it, the twilight (0..1).
    floor: vec4<f32>,
    // The shooting star: spawn direction and progress, travel direction and alpha (0 for none).
    meteor0: vec4<f32>,
    meteor1: vec4<f32>,
    // The UFO: its direction and fade (w 0 for none), the belly light that chases and the number of groups.
    ufo0: vec4<f32>,
    ufo1: vec4<f32>,
    // The headset icon's dots: ten rows of 21 bits, two vec4 and a half.
    icon: array<vec4<u32>, 3>,
}
@group(0) @binding(0) var<uniform> scene: Scene;
@group(0) @binding(1) var floor_texture: texture_2d<f32>;
@group(0) @binding(2) var floor_sampler: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex fn vs_scene(@builtin(vertex_index) index: u32) -> Out {
    let corners = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var out: Out;
    out.clip = vec4(corners[index], 0.0, 1.0);
    out.ndc = corners[index];
    return out;
}

const DAY_ZENITH = vec3<f32>(0.290, 0.455, 0.690);
const DAY_HORIZON = vec3<f32>(0.745, 0.824, 0.902);
const NIGHT_ZENITH = vec3<f32>(0.031, 0.039, 0.110);
const NIGHT_HORIZON = vec3<f32>(0.086, 0.102, 0.204);
const DUSK_HORIZON = vec3<f32>(0.941, 0.549, 0.275);
const DAY_CLOUD = vec3<f32>(0.961, 0.973, 1.0);
const NIGHT_CLOUD = vec3<f32>(0.157, 0.180, 0.275);
const WARM_CLOUD = vec3<f32>(1.0, 0.784, 0.588);
const STAR_HALO = vec3<f32>(0.118, 0.784, 0.961);
const STAR_CORE = vec3<f32>(0.784, 0.980, 1.0);
const CLOUD_Y = -14.0;
const PI = 3.14159265;

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2(123.34, 456.21));
    q += dot(q, q + 45.32);
    return fract(q.x * q.y);
}
fn hash31(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3(443.897, 441.423, 437.195));
    q += dot(q, q.yzx + 19.19);
    return fract((q.x + q.y) * q.z);
}

// 5 x 7 glyphs of VISOR, row by row, 5 bits each (`VISOR_GLYPHS`, '1' lit, left bit first).
fn glyph_row(letter: u32, row: u32) -> u32 {
    var v = array<u32, 7>(17u, 17u, 17u, 17u, 10u, 10u, 4u);
    var i = array<u32, 7>(31u, 4u, 4u, 4u, 4u, 4u, 31u);
    var s = array<u32, 7>(14u, 17u, 16u, 14u, 1u, 17u, 14u);
    var o = array<u32, 7>(14u, 17u, 17u, 17u, 17u, 17u, 14u);
    var r = array<u32, 7>(30u, 17u, 17u, 30u, 20u, 18u, 17u);
    switch letter {
        case 0u: { return v[row]; }
        case 1u: { return i[row]; }
        case 2u: { return s[row]; }
        case 3u: { return o[row]; }
        default: { return r[row]; }
    }
}

fn icon_row(row: u32) -> u32 {
    let v = scene.icon[row / 4u];
    switch row % 4u {
        case 0u: { return v.x; }
        case 1u: { return v.y; }
        case 2u: { return v.z; }
        default: { return v.w; }
    }
}

// The dotted VISOR sign: a star at night, a cloud by day, 1.6 m between dots, 88 m away, up and
// ahead of the panel, with the headset icon of 21 by 10 dots above it (`VRMenuSky.createIconDotsSign`:
// two empty rows between, centred on the letters). Returns the colour and its strength.
fn visor_sign(dir: vec3<f32>, day: f32, time: f32) -> vec4<f32> {
    let centre = normalize(vec3(0.0, 0.5, -1.0));
    let along = dot(dir, centre);
    if along < 0.9 {
        return vec4(0.0);
    }
    let right = normalize(cross(centre, vec3(0.0, 1.0, 0.0)));
    let up = cross(right, centre);
    let p = dir * (88.0 / along) - centre * 88.0;
    let cols = 29.0;
    let x = dot(p, right) / 1.6 + cols * 0.5;
    let y = 3.5 - dot(p, up) / 1.6;
    if x < 0.0 || x >= cols || y < -12.0 || y >= 7.0 {
        return vec4(0.0);
    }
    let col = u32(floor(x));
    var lit = false;
    if y >= 0.0 {
        let row = u32(floor(y));
        let letter = col / 6u;
        let in_letter = col % 6u;
        if in_letter >= 5u {
            return vec4(0.0);
        }
        let bits = glyph_row(letter, row);
        lit = ((bits >> (4u - in_letter)) & 1u) != 0u;
    } else if y < -2.0 && col >= 4u && col < 25u {
        // The icon's row 0 is the top one, 12 dots up.
        let row = u32(floor(y + 12.0));
        lit = ((icon_row(row) >> (col - 4u)) & 1u) != 0u;
    }
    if !lit {
        return vec4(0.0);
    }
    let d = length(vec2(fract(x) - 0.5, fract(y) - 0.5));
    // The traveling highlight and the slow breathing of the dots.
    let gleam = max(0.0, 1.0 - abs(x - (fract(time * 2.5 / 40.0) * 48.0 - 10.0)) / 4.0) * 0.55;
    let pulse = 1.0 + 0.22 * sin(time * 1.1 * 2.0 * PI * 0.25);
    let size = 0.40 * pulse;
    let core = smoothstep(size, size * 0.4, d);
    let halo = smoothstep(0.75, 0.2, d) * 0.5;
    let night_colour = mix(STAR_HALO, STAR_CORE, core);
    let day_colour = mix(vec3(0.82, 0.90, 1.0), vec3(1.0), core);
    let colour = mix(night_colour, day_colour, day) + gleam;
    return vec4(colour, clamp(core + halo, 0.0, 1.0));
}

// The midnight UFO (`VRMenuSky.renderUfo`): 11 by 5 glowing dots 0.9 m apart, 90 m away, a dome of
// hull dots over a belly row whose lights chase. Returns the light to add.
fn ufo(dir: vec3<f32>) -> vec3<f32> {
    let fade = scene.ufo0.w;
    if fade <= 0.0 {
        return vec3(0.0);
    }
    let centre = scene.ufo0.xyz;
    let along = dot(dir, centre);
    if along < 0.95 {
        return vec3(0.0);
    }
    let right = normalize(cross(centre, vec3(0.0, 1.0, 0.0)));
    let up = cross(right, centre);
    let p = dir * (90.0 / along) - centre * 90.0;
    let lx = dot(p, right);
    let ly = dot(p, up);
    let c = floor(5.5 - lx / 0.9);
    let r = floor(2.5 - ly / 0.9);
    if c < 0.0 || c > 10.0 || r < 0.0 || r > 4.0 {
        return vec3(0.0);
    }
    // Rows: "....111....", "...11111...", ".111111111.", "11111111111", ".2..333..4.".
    let ci = u32(c);
    var hull = false;
    var group = -1;
    switch u32(r) {
        case 0u: { hull = ci >= 4u && ci <= 6u; }
        case 1u: { hull = ci >= 3u && ci <= 7u; }
        case 2u: { hull = ci >= 1u && ci <= 9u; }
        case 3u: { hull = true; }
        default: {
            if ci == 1u { group = 0; }
            if ci >= 4u && ci <= 6u { group = 1; }
            if ci == 9u { group = 2; }
        }
    }
    let d = length(vec2(lx - (5.0 - c) * 0.9, ly - (2.0 - r) * 0.9));
    var light = vec3(0.0);
    if group >= 0 {
        let on = f32(group) == scene.ufo1.x;
        let halo_a = select(40.0, 160.0, on) / 255.0 * fade;
        let core_a = select(70.0, 255.0, on) / 255.0 * fade;
        let g1 = clamp(1.0 - d / (0.85 * 0.8 * 0.5), 0.0, 1.0);
        let g2 = clamp(1.0 - d / (0.42 * 0.9 * 0.5), 0.0, 1.0);
        light = vec3(1.0, 0.886, 0.353) * (halo_a * g1 * g1 + core_a * g2 * g2);
    } else if hull {
        let g1 = clamp(1.0 - d / (0.85 * 0.5), 0.0, 1.0);
        let g2 = clamp(1.0 - d / (0.42 * 0.5), 0.0, 1.0);
        light = vec3(0.204, 0.827, 0.600) * (150.0 / 255.0 * fade * g1 * g1)
            + vec3(0.745, 0.961, 0.863) * (235.0 / 255.0 * fade * g2 * g2);
    }
    return light;
}

// Blocky box clouds under the player (`VRMenuSky.renderClouds`): clumps of 5 m tiles, each a box
// 4 m thick around -14 m (the height of a clump's 25 m cell moves up or down by up to 4 m), drifting.
// The ray walks the 5 m grid through the band the boxes fill and meets the first box: its top, or a
// wall shaded as Visor shades them (up 1, north-south 0.85, west-east 0.72).
fn tile_present(tile: vec2<f32>) -> bool {
    // Clumps: a coarse cell decides how full it is, the tile inside it whether it is cloud.
    let coarse = floor(tile / 5.0);
    let fill = 0.35 + 0.5 * hash21(coarse);
    return hash21(tile + 17.0) < fill;
}
fn tile_y(tile: vec2<f32>) -> f32 {
    return CLOUD_Y + (hash21(floor(tile / 5.0) + 7.0) * 2.0 - 1.0) * 4.0;
}
fn clouds(dir: vec3<f32>, eye: vec3<f32>, day: f32, twilight: f32, time: f32, horizon: vec3<f32>) -> vec4<f32> {
    if dir.y > -0.002 {
        return vec4(0.0);
    }
    let top = CLOUD_Y + 6.0;
    let bottom = CLOUD_Y - 6.0;
    let t0 = max((top - eye.y) / dir.y, 0.0);
    let t1 = (bottom - eye.y) / dir.y;
    if t1 <= t0 {
        return vec4(0.0);
    }
    let drift = vec2(time * 0.3, time * 0.12);
    let origin = eye.xz + drift;
    let sx = select(-1.0, 1.0, dir.x > 0.0);
    let sz = select(-1.0, 1.0, dir.z > 0.0);
    let inv = 1.0 / (abs(dir.xz) + vec2(1e-6));
    var cell = floor((origin + dir.xz * t0) / 5.0);
    var next = vec2(
        (((cell.x + select(0.0, 1.0, sx > 0.0)) * 5.0 - origin.x) / (dir.x + select(-1e-6, 1e-6, dir.x >= 0.0))),
        (((cell.y + select(0.0, 1.0, sz > 0.0)) * 5.0 - origin.y) / (dir.z + select(-1e-6, 1e-6, dir.z >= 0.0))),
    );
    let lit = mix(NIGHT_CLOUD, DAY_CLOUD, day);
    let tinted = mix(lit, WARM_CLOUD, twilight * 0.55 * day + twilight * 0.25);
    for (var i = 0; i < 30; i++) {
        if min(next.x, next.y) > t1 + 6.0 && i > 0 {
            break;
        }
        if tile_present(cell) {
            let yc = tile_y(cell);
            let lo = vec3(cell.x * 5.0 - origin.x + eye.x, yc - 2.0, cell.y * 5.0 - origin.y + eye.z);
            let hi = lo + vec3(5.0, 4.0, 5.0);
            let id = 1.0 / (dir + vec3(select(-1e-6, 1e-6, dir.x >= 0.0), 0.0, select(-1e-6, 1e-6, dir.z >= 0.0)));
            let ta = (lo - eye) * id;
            let tb = (hi - eye) * id;
            let tmin = min(ta, tb);
            let tmax = max(ta, tb);
            let enter = max(tmin.x, max(tmin.y, tmin.z));
            let exit = min(tmax.x, min(tmax.y, tmax.z));
            if enter <= exit && exit > 0.0 && enter > 0.0 {
                var shade = 1.0;
                if enter == tmin.x {
                    shade = 0.72;
                } else if enter == tmin.z {
                    shade = 0.85;
                }
                var colour = tinted * shade;
                // Each box a little different.
                colour *= 0.94 + 0.06 * hash21(cell + 3.0);
                // The farther, the more of the horizon's colour (`AERIAL_START`).
                colour = mix(colour, horizon, smoothstep(38.0, 100.0, enter) * 0.5);
                return vec4(colour, smoothstep(130.0, 60.0, enter));
            }
        }
        if next.x < next.y {
            cell.x += sx;
            next.x += 5.0 * inv.x;
        } else {
            cell.y += sz;
            next.y += 5.0 * inv.y;
        }
    }
    return vec4(0.0);
}

@fragment fn fs_scene(in: Out) -> @location(0) vec4<f32> {
    let tan = scene.tangents;
    let view = vec3(
        mix(tan.x, tan.y, in.ndc.x * 0.5 + 0.5),
        mix(tan.w, tan.z, in.ndc.y * 0.5 + 0.5),
        -1.0,
    );
    let rot = mat3x3<f32>(scene.rot0.xyz, scene.rot1.xyz, scene.rot2.xyz);
    let dir = normalize(rot * view);
    let eye = scene.eye.xyz;
    let time = scene.eye.w;
    let day = scene.sun.w;
    let twilight = scene.floor.w;

    // The sky: a gradient from the horizon to the zenith, warm toward the sun at dusk and dawn.
    let zenith = mix(NIGHT_ZENITH, DAY_ZENITH, day);
    var horizon = mix(NIGHT_HORIZON, DAY_HORIZON, day);
    let toward_sun = max(0.0, dot(normalize(vec3(scene.sun.x, 0.0, scene.sun.z) + vec3(1e-4, 0.0, 0.0)), normalize(vec3(dir.x, 0.0, dir.z) + vec3(1e-4, 0.0, 0.0))));
    horizon = mix(horizon, DUSK_HORIZON, twilight * 0.85 * pow(toward_sun, 2.0));
    var colour: vec3<f32>;
    if dir.y >= 0.0 {
        colour = mix(horizon, zenith, pow(clamp(dir.y, 0.0, 1.0), 0.55));
    } else {
        // Under the horizon the haze deepens.
        colour = mix(horizon, horizon * 0.55, clamp(-dir.y * 3.0, 0.0, 1.0));
    }
    // Stars (about 500 in Visor), twinkling, only at night and above the horizon.
    if dir.y > -0.05 {
        let p = dir * 55.0;
        let cell = floor(p);
        let r = hash31(cell);
        if r > 0.985 {
            let jitter = vec3(hash31(cell + 1.7), hash31(cell + 5.3), hash31(cell + 9.1));
            let d = length(p - (cell + 0.2 + 0.6 * jitter));
            let star = smoothstep(0.16, 0.0, d);
            let twinkle = 0.65 + 0.35 * sin(time * 2.0 + r * 400.0);
            colour += vec3(0.95) * star * twinkle * (1.0 - day);
        }
    }
    // The shooting star (`emitShootingStar`): a quad along a great circle from a tail that fades to nothing
    // to a bright head, 0.38 radians crossed in 0.65 s, its trail 0.11 long, 0.22 wide at the head
    // and 15 % of that at the tail, 95 blocks out.
    if scene.meteor1.w > 0.0 {
        let spawn = scene.meteor0.xyz;
        let travel = scene.meteor1.xyz;
        let head = scene.meteor0.w * 0.38;
        let tail = head - 0.11;
        let along = atan2(dot(dir, travel), dot(dir, spawn));
        let u = (along - tail) / (head - tail);
        if u >= 0.0 && u <= 1.0 && dot(dir, spawn) > 0.0 {
            let half_width = mix(0.22 * 0.15, 0.22, u) / 95.0;
            let side = abs(dot(dir, normalize(cross(spawn, travel))));
            let cover = 1.0 - smoothstep(half_width * 0.7, half_width, side);
            colour = mix(colour, vec3(1.0, 0.988, 0.961), u * scene.meteor1.w * cover);
        }
    }
    // The sun and the moon, discs of 13 and 10 blocks at 92 and 90.
    let sun_cos = dot(dir, scene.sun.xyz);
    let sun_edge = cos(atan(6.5 / 92.0));
    if sun_cos > sun_edge {
        colour = mix(colour, mix(vec3(1.0, 0.59, 0.27), vec3(1.0), day), smoothstep(sun_edge, sun_edge + 0.0004, sun_cos));
    }
    let moon_cos = dot(dir, scene.moon.xyz);
    let moon_edge = cos(atan(5.0 / 90.0));
    if moon_cos > moon_edge {
        // Lit from the sun's side: the terminator moves with the phase.
        let across = dot(dir - scene.moon.xyz * moon_cos, normalize(cross(scene.moon.xyz, vec3(0.0, 1.0, 0.0))));
        let lit = smoothstep(-0.02, 0.02, across * 40.0 * (scene.moon.w * 2.0 - 1.0) + (scene.moon.w - 0.5) * 2.0);
        let moon = vec3(0.80, 0.84, 1.0) * mix(0.12, 1.0, lit);
        colour = mix(colour, moon, smoothstep(moon_edge, moon_edge + 0.0004, moon_cos) * 0.95);
    }
    // The midnight UFO, added to the sky.
    colour += ufo(dir);
    // The sign in the sky.
    let sign = visor_sign(dir, day, time);
    colour = mix(colour, sign.rgb, sign.a * mix(1.0, 0.85, day));
    // Clouds under the floor.
    let cloud = clouds(dir, eye, day, twilight, time, horizon);
    colour = mix(colour, cloud.rgb, cloud.a);
    // The floor: the play area and a 2 m border (`VRMenuFloor`), the texture repeating four times
    // a metre, at half strength.
    if dir.y < -0.0005 {
        let t = (0.0 - eye.y) / dir.y;
        if t > 0.0 {
            let hit = eye + dir * t;
            let inner = abs(hit.x) <= scene.floor.x && abs(hit.z) <= scene.floor.y;
            let outer = abs(hit.x) <= scene.floor.x + scene.floor.z && abs(hit.z) <= scene.floor.y + scene.floor.z;
            if outer {
                let texel = textureSample(floor_texture, floor_sampler, hit.xz * 4.0).rgb;
                var floor_colour = texel * 0.5;
                if !inner {
                    floor_colour *= 0.85;
                }
                colour = floor_colour;
            }
        }
    }
    return vec4(colour, 1.0);
}
