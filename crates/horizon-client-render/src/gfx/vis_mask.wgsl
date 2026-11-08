// The headset's hidden-area mesh: depth only, at the nearest depth (1.0, the renderer's depth is
// reversed), so the eye's pass rejects every fragment behind it before shading it.
@vertex
fn vs(@location(0) ndc: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4<f32>(ndc, 1.0, 1.0);
}

@fragment
fn fs() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
