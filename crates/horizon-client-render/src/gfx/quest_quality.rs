//! Deliberate quality tradeoffs for the Quest's vertex/fragment budgets. All opt-in.
use super::{headset, split_solid_gpu};
use std::sync::atomic::{AtomicBool, Ordering};

static NEAREST_LIGHTMAP: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
#[path = "quest_quality_tests.rs"]
mod gpu_tests;

pub fn nearest_lightmap(on: bool) {
    NEAREST_LIGHTMAP.store(on, Ordering::Relaxed);
}

pub fn density(env: Option<&str>, setting: f32) -> u32 {
    env.map_or(Some(setting), |v| v.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .map_or(100, |v| v.clamp(0.0, 100.0).round() as u32)
}

pub fn shader(source: String) -> String {
    lightmap_shader(source, NEAREST_LIGHTMAP.load(Ordering::Relaxed))
}

fn lightmap_shader(source: String, nearest: bool) -> String {
    if !nearest {
        return source;
    }
    let start = source
        .find("fn sample_lightmap(")
        .expect("terrain lightmap function");
    let body = start + source[start..].find('{').unwrap();
    let end = body + source[body..].find('}').unwrap();
    let mut source = source;
    source.replace_range(body + 1..end,
        "\n    return textureLoad(lightmap, vec2<i32>(clamp(round(vec2(block, sky)), vec2(0.0), vec2(15.0))), 0);\n");
    source
}

/// An A/B switch for one of Starling's Adreno paths (`late_sky`, `solid_paths`): the environment
/// variable `env` set to `0` or `1` decides, else `default` (what the phone app does). Read when the
/// renderer is made; on the headset `launch.txt` can set it (`HORIZON_LATE_SKY=0`).
pub(super) fn env_switch(env: &str, default: bool) -> bool {
    match std::env::var(env).as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => default,
    }
}

/// A Quest rendering switch (the VR settings' Experiments page): the environment variable
/// `env` set to `1` or `0` decides, else the VR setting `setting`. Read when the renderer
/// is made, so a change takes a restart.
pub(super) fn quest_switch(env: &str, setting: &str) -> bool {
    match std::env::var(env).as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => headset::installed() && horizon_client_input::headset::settings().bool(setting),
    }
}

/// `HORIZON_QUEST_PLANT_DISTANCE=<blocks>`: on a headset's Adreno, sections whose centre is
/// farther than this leave out their crossed cutout quads (`Quads::CutoutAligned`): short and
/// tall grass, ferns, flowers, saplings. It changes the picture (the plants appear a section
/// at a time as the player nears them), so it is off unless set; the squared distance.
pub(super) fn plant_distance(info: &wgpu::AdapterInfo) -> Option<f64> {
    static BLOCKS: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    let blocks = *BLOCKS.get_or_init(|| {
        match std::env::var("HORIZON_QUEST_PLANT_DISTANCE") {
            Ok(v) => v.parse::<f64>().ok(),
            Err(_) => headset::installed().then(|| {
                f64::from(horizon_client_input::headset::settings().float("horizonPlantDistance"))
            }),
        }
        .filter(|d| d.is_finite() && *d > 0.0)
    });
    blocks
        .filter(|_| headset::installed() && info.vendor == 0x5143)
        .map(|d| d * d)
}

/// Whether section meshes merge their coplanar block faces (`mesher::merge`): on a headset's
/// Adreno, whose frame is bound by how many triangles the terrain has, where solid terrain
/// draws in two passes and no shader pack reads the quads' sources. The picture is the same:
/// only faces whose four corners share one colour and light merge. `HORIZON_QUEST_MERGE_FACES=1`
/// turns it on.
pub(super) fn merge_faces_gpu(info: &wgpu::AdapterInfo, shader_pack: bool) -> bool {
    headset::installed()
        && info.backend == wgpu::Backend::Vulkan
        && info.vendor == 0x5143
        && split_solid_gpu(info)
        && !shader_pack
        && (quest_switch("HORIZON_QUEST_MERGE_FACES", "horizonMergeFaces")
            || quest_switch("HORIZON_QUEST_AGGRESSIVE_MESH", "horizonAggressiveMesh"))
}

/// Whether plants' crossed quads are meshed over the part of their sprite that draws
/// (`mesher::shrink_plants`): on a headset's Adreno, where the pixels the cutout pass throws
/// away cost as much as the ones it keeps. `HORIZON_QUEST_SHRINK_PLANTS=1` turns it on.
pub(super) fn shrink_plants_gpu(info: &wgpu::AdapterInfo, shader_pack: bool) -> bool {
    headset::installed()
        && info.backend == wgpu::Backend::Vulkan
        && info.vendor == 0x5143
        && !shader_pack
        && quest_switch("HORIZON_QUEST_SHRINK_PLANTS", "horizonShrinkPlants")
}

pub(super) fn configure(info: &wgpu::AdapterInfo, shader_pack: bool) {
    if headset::installed()
        && let Some(plan) = horizon_threads::cores::hybrid_plan()
    {
        log::info!(
            "Quest hybrid CPU plan: foreground {:?}, workers {:?}, async {:?}",
            plan.foreground,
            plan.workers,
            plan.efficiency
        );
    }
    let merge_faces = merge_faces_gpu(info, shader_pack);
    crate::mesher::merge_faces(merge_faces);
    let aggressive =
        merge_faces && quest_switch("HORIZON_QUEST_AGGRESSIVE_MESH", "horizonAggressiveMesh");
    crate::mesher::aggressive_merging(aggressive);
    let quest_quality = headset::installed()
        && info.backend == wgpu::Backend::Vulkan
        && info.vendor == 0x5143
        && !shader_pack;
    let plant_percent = if quest_quality {
        density(
            std::env::var("HORIZON_QUEST_PLANT_DENSITY").ok().as_deref(),
            horizon_client_input::headset::settings().float("horizonPlantDensity"),
        )
    } else {
        100
    };
    crate::mesher::plant_density(plant_percent);
    let nearest =
        quest_quality && quest_switch("HORIZON_QUEST_NEAREST_LIGHTMAP", "horizonNearestLightmap");
    nearest_lightmap(nearest);
    let shrink_plants = shrink_plants_gpu(info, shader_pack);
    crate::mesher::shrink_plants(shrink_plants);
    if headset::installed() {
        log::info!(
            "Quest terrain merged block faces: {merge_faces}, aggressive: {aggressive}, shrunk plant quads: {shrink_plants}, plant density: {plant_percent}%, nearest lightmap: {nearest}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_overrides_are_bounded_and_invalid_values_keep_plants() {
        for (text, expected) in [
            ("0", 0),
            ("25", 25),
            ("-1", 0),
            ("150", 100),
            ("NaN", 100),
            ("inf", 100),
            ("bad", 100),
        ] {
            assert_eq!(density(Some(text), 50.0), expected);
        }
        assert_eq!(density(None, 50.0), 50);
        assert_eq!(density(None, f32::NAN), 100);
    }

    #[test]
    fn nearest_and_filtered_lightmaps_validate_in_mono_and_stereo() {
        let source = include_str!("../terrain.wgsl").to_owned();
        for nearest in [false, true] {
            let source = lightmap_shader(source.clone(), nearest);
            for source in [source.clone(), super::super::multiview::shader(&source)] {
                let module = naga::front::wgsl::parse_str(&source).unwrap();
                naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                )
                .validate(&module)
                .unwrap();
            }
        }
    }
}
