//! The pipelines a Vulkan driver compiled, kept on disk between runs.
//!
//! A Vulkan driver turns each pipeline's shaders into the GPU's own code when
//! the pipeline is created; a `VkPipelineCache` made from an earlier run's
//! bytes lets it skip that. Metal keeps its own cache per user, and DX12's
//! compiled shaders are kept by wgpu-hal's `dx12::disk_cache`
//! ([`super::dx12_shader_cache`]), so only Vulkan has a file here.
//!
//! One file per adapter, driver and Horizon build, in the game folder:
//! `cache/wgpu_pipeline_cache_vulkan_{vendor}_{device}-{hash}.bin`, the hash
//! covering the driver's name and version and the build ([`file_name`]). A
//! save replaces the file at once (a temporary file, then a rename) and
//! removes the same adapter's files of other drivers or builds. The file is
//! [`MAGIC`], a checksum, then the bytes `PipelineCache::get_data` gave; wgpu
//! checks its own header in them (the adapter, the driver's cache UUID,
//! wgpu's version) when the cache is created. A file that is missing, short,
//! fails the checksum or wgpu's checks is ignored: the pipelines compile as
//! they would without it, and the next save replaces it.
//! `HORIZON_PIPELINE_CACHE=0` turns the cache off.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

const MAGIC: &[u8; 8] = b"HZVKPC1\n";
/// A cache past this is not saved: Horizon's pipelines take a few MB.
const MAX_BYTES: usize = 256 << 20;
/// When the cache is first saved (the menus' and the world's pipelines are
/// made by then), and how often after that if it grew.
const FIRST_SAVE: Duration = Duration::from_secs(10);
const SAVE_EVERY: Duration = Duration::from_secs(60);

struct Open {
    device: wgpu::Device,
    cache: wgpu::PipelineCache,
    path: PathBuf,
}

static OPEN: OnceLock<Open> = OnceLock::new();

/// The cache to create `device`'s pipelines with, once [`open`] made one.
pub fn get(device: &wgpu::Device) -> Option<&'static wgpu::PipelineCache> {
    OPEN.get()
        .filter(|open| open.device == *device)
        .map(|open| &open.cache)
}

/// Makes the pipeline cache of this run's device from the file an earlier
/// run saved in `home`, and saves it there from a background thread as it
/// grows. Does nothing off Vulkan, when `HORIZON_PIPELINE_CACHE=0`, or after
/// the first call.
pub fn open(adapter: &wgpu::Adapter, device: &wgpu::Device, home: &Path) {
    if OPEN.get().is_some()
        || !device.features().contains(wgpu::Features::PIPELINE_CACHE)
        || !enabled(std::env::var("HORIZON_PIPELINE_CACHE").ok().as_deref())
    {
        return;
    }
    let Some(name) = file_name(&adapter.get_info(), &build()) else {
        return;
    };
    let path = home.join("cache").join(name);
    let data = read(&path);
    let Some(cache) = cache_from(device, data.as_deref()) else {
        return;
    };
    let loaded = cache.get_data().map_or(0, |data| data.len());
    log::info!(
        "Horizon: pipeline cache {} ({})",
        path.display(),
        match &data {
            Some(data) => format!("{} KiB read", data.len() >> 10),
            None => "new".to_owned(),
        }
    );
    if OPEN
        .set(Open {
            device: device.clone(),
            cache,
            path,
        })
        .is_err()
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("Pipeline cache".into())
        .spawn(move || keep_saved(OPEN.get().expect("set above"), loaded));
    if let Err(e) = spawned {
        log::warn!("Horizon: pipeline cache not saved: {e}");
    }
}

/// `HORIZON_PIPELINE_CACHE`: on unless `0`, `off`, `false` or `no`.
fn enabled(env: Option<&str>) -> bool {
    !env.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        )
    })
}

/// The cache file's name for `info`'s adapter and driver and this `build`:
/// wgpu's key (vendor and device), then a hash of the driver's name and
/// version and the build. None off Vulkan.
fn file_name(info: &wgpu::AdapterInfo, build: &str) -> Option<String> {
    let key = wgpu::util::pipeline_cache_key(info)?;
    let mut hash = FNV_OFFSET;
    for part in [info.driver.as_str(), info.driver_info.as_str(), build] {
        hash = fnv1a(hash, part.as_bytes());
        hash = fnv1a(hash, &[0]);
    }
    Some(format!("{key}-{hash:016x}.bin"))
}

/// This build: its version and commit, or for a build from source the
/// executable's time, so a rebuilt game starts a new file.
fn build() -> String {
    let mut build = horizon_core::release::VERSION.to_owned();
    if let Some(nightly) = horizon_core::release::NIGHTLY {
        build += nightly;
    }
    match option_env!("HORIZON_COMMIT") {
        Some(commit) => build += commit,
        None => {
            let built = std::env::current_exe()
                .and_then(|exe| exe.metadata()?.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok());
            if let Some(built) = built {
                build += &format!(" {}", built.as_nanos());
            }
        }
    }
    build
}

/// The cache made from `data`, or if wgpu refuses `data` an empty one; None
/// (logged) if the device makes neither.
fn cache_from(device: &wgpu::Device, data: Option<&[u8]>) -> Option<wgpu::PipelineCache> {
    create(device, data).or_else(|| data.and_then(|_| create(device, None)))
}

/// The cache, or None (logged) if the device refused to make it: with
/// `fallback`, wgpu still refuses data saved for another adapter.
fn create(device: &wgpu::Device, data: Option<&[u8]>) -> Option<wgpu::PipelineCache> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    // SAFETY: `data` is what `get_data` gave for this adapter's cache in an
    // earlier run, behind a checksum ([`read`]); wgpu checks its header against
    // this device and driver, and with `fallback` it makes an empty cache from
    // data it cannot use.
    let cache = unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("Horizon pipeline cache"),
            data,
            fallback: true,
        })
    };
    let error = [internal, memory, validation]
        .into_iter()
        .find_map(|scope| pollster::block_on(scope.pop()));
    match error {
        Some(e) => {
            log::warn!(
                "Horizon: pipeline cache {}: {e}",
                if data.is_some() {
                    "file not used"
                } else {
                    "off"
                }
            );
            None
        }
        None => Some(cache),
    }
}

/// Saves `open`'s cache whenever it grew past `saved` bytes.
fn keep_saved(open: &Open, mut saved: usize) {
    std::thread::sleep(FIRST_SAVE);
    loop {
        if let Some(data) = open.cache.get_data()
            && data.len() > saved
            && data.len() <= MAX_BYTES
        {
            match save(&open.path, &data) {
                Ok(()) => saved = data.len(),
                Err(e) => log::warn!("Horizon: pipeline cache {}: {e}", open.path.display()),
            }
        }
        std::thread::sleep(SAVE_EVERY);
    }
}

/// The cache bytes saved at `path`, or None if there are none or they are
/// not whole.
fn read(path: &Path) -> Option<Vec<u8>> {
    let mut bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!("Horizon: pipeline cache {} not read: {e}", path.display());
            return None;
        }
    };
    let whole = bytes.strip_prefix(MAGIC.as_slice()).and_then(|rest| {
        let (check, data) = rest.split_first_chunk::<8>()?;
        (u64::from_le_bytes(*check) == fnv1a(FNV_OFFSET, data)).then_some(())
    });
    if whole.is_none() {
        log::warn!(
            "Horizon: pipeline cache {} ignored: not a whole cache",
            path.display()
        );
        return None;
    }
    bytes.drain(..MAGIC.len() + 8);
    Some(bytes)
}

/// Writes `data` to `path` at once (a temporary file renamed over it), then
/// removes the files of the same adapter's other drivers and builds.
fn save(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mut bytes = Vec::with_capacity(MAGIC.len() + 8 + data.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&fnv1a(FNV_OFFSET, data).to_le_bytes());
    bytes.extend_from_slice(data);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("cache");
    let temp = dir.join(format!("{name}.{}.tmp", std::process::id()));
    fs::write(&temp, &bytes)?;
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    // `{key}-{hash}.bin`: the other hashes of this key are stale.
    if let Some((key, _)) = name.rsplit_once('-')
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let other = entry.file_name();
            let Some(other) = other.to_str() else {
                continue;
            };
            if other != name
                && other.ends_with(".bin")
                && other.rsplit_once('-').is_some_and(|(k, _)| k == key)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vulkan(vendor: u32, device: u32, driver_info: &str) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "GPU".into(),
            vendor,
            device,
            device_type: wgpu::DeviceType::DiscreteGpu,
            driver: "NVIDIA".into(),
            driver_info: driver_info.into(),
            backend: wgpu::Backend::Vulkan,
            ..wgpu::AdapterInfo::new(Default::default(), wgpu::Backend::Vulkan)
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-pipeline-cache-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_file_is_named_by_adapter_driver_and_build() {
        let nvidia = vulkan(0x10de, 0x2684, "580.95");
        let name = file_name(&nvidia, "0.4.1").unwrap();
        assert!(
            name.starts_with("wgpu_pipeline_cache_vulkan_4318_9860-") && name.ends_with(".bin"),
            "{name}"
        );
        assert_eq!(file_name(&nvidia, "0.4.1").unwrap(), name);
        let others = [
            file_name(&vulkan(0x10de, 0x2684, "581.08"), "0.4.1").unwrap(),
            file_name(&nvidia, "0.4.2").unwrap(),
            file_name(&vulkan(0x10de, 0x2704, "580.95"), "0.4.1").unwrap(),
            file_name(&vulkan(0x1002, 0x2684, "580.95"), "0.4.1").unwrap(),
        ];
        for other in &others {
            assert_ne!(other, &name);
        }
        // A new driver or build keeps the adapter's part, so its save
        // replaces the old file.
        for other in &others[..2] {
            assert_eq!(
                other.rsplit_once('-').unwrap().0,
                name.rsplit_once('-').unwrap().0
            );
        }
        for backend in [wgpu::Backend::Metal, wgpu::Backend::Dx12, wgpu::Backend::Gl] {
            let info = wgpu::AdapterInfo {
                backend,
                ..nvidia.clone()
            };
            assert_eq!(file_name(&info, "0.4.1"), None, "{backend:?}");
        }
    }

    #[test]
    fn the_cache_turns_off_with_its_variable() {
        for off in ["0", "off", "OFF", "false", "no", " 0 "] {
            assert!(!enabled(Some(off)), "{off}");
        }
        for on in [None, Some("1"), Some(""), Some("on")] {
            assert!(enabled(on), "{on:?}");
        }
    }

    #[test]
    fn a_saved_cache_reads_back_and_a_damaged_one_is_ignored() {
        let dir = scratch("damaged");
        let path = dir.join("wgpu_pipeline_cache_vulkan_1_2-00.bin");
        assert_eq!(read(&path), None, "no file yet");
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        save(&path, &data).unwrap();
        assert_eq!(read(&path).as_deref(), Some(data.as_slice()));
        assert!(
            fs::read_dir(&dir).unwrap().count() == 1,
            "no temporary file left"
        );
        let whole = fs::read(&path).unwrap();
        let mut flipped = whole.clone();
        flipped[MAGIC.len() + 8 + 1234] ^= 0x10;
        let mut other_magic = whole.clone();
        other_magic[0] = b'X';
        for damaged in [
            whole[..whole.len() - 1].to_vec(),
            whole[..MAGIC.len() + 4].to_vec(),
            Vec::new(),
            flipped,
            other_magic,
        ] {
            fs::write(&path, &damaged).unwrap();
            assert_eq!(read(&path), None, "{} bytes", damaged.len());
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_save_removes_the_adapters_files_of_other_builds() {
        let dir = scratch("stale");
        let old = dir.join("wgpu_pipeline_cache_vulkan_4318_9860-0000000000000001.bin");
        let other_gpu = dir.join("wgpu_pipeline_cache_vulkan_4098_9860-0000000000000001.bin");
        let new = dir.join("wgpu_pipeline_cache_vulkan_4318_9860-0000000000000002.bin");
        save(&old, b"old").unwrap();
        save(&other_gpu, b"other").unwrap();
        fs::write(dir.join("dx12-shaders.bin"), b"dx12").unwrap();
        save(&new, b"new").unwrap();
        assert!(!old.exists());
        assert_eq!(read(&new).as_deref(), Some(b"new".as_slice()));
        assert_eq!(read(&other_gpu).as_deref(), Some(b"other".as_slice()));
        assert!(dir.join("dx12-shaders.bin").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// On a Vulkan device (lavapipe on CI's Linux runners): a cache saved
    /// from one device makes the next one's, and bytes wgpu rejects make an
    /// empty cache instead of an error. Skipped without Vulkan.
    #[test]
    fn a_vulkan_cache_round_trips_and_bad_bytes_fall_back() {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(descriptor);
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        if !adapter.features().contains(wgpu::Features::PIPELINE_CACHE) {
            return;
        }
        let device = || {
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::PIPELINE_CACHE,
                ..Default::default()
            }))
            .unwrap()
            .0
        };
        let first = device();
        let cache = create(&first, None).unwrap();
        let shader = first.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
        });
        first.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: None,
            compilation_options: Default::default(),
            cache: Some(&cache),
        });
        let data = cache.get_data().unwrap();
        let dir = scratch("vulkan");
        let path = dir.join(file_name(&adapter.get_info(), "test").unwrap());
        save(&path, &data).unwrap();

        let second = device();
        let read_back = read(&path).unwrap();
        assert_eq!(read_back, data);
        let cache = create(&second, Some(&read_back)).expect("the saved cache is used");
        assert!(cache.get_data().unwrap().len() >= data.len() / 2);
        // wgpu's header: the adapter's key at 17..32 (a refusal even with
        // `fallback`), the driver's validation key at 32..48 (outdated), then
        // the data's length (truncated).
        let mut other_gpu = read_back.clone();
        other_gpu[20] ^= 0x5a;
        assert!(create(&second, Some(&other_gpu)).is_none());
        let mut other_driver = read_back.clone();
        other_driver[40] ^= 0x5a;
        for bad in [
            &other_gpu[..],
            &other_driver[..],
            &read_back[..read_back.len() / 2],
            b"junk",
        ] {
            let cache = cache_from(&second, Some(bad)).expect("an empty cache instead");
            assert!(
                cache.get_data().unwrap().len() < data.len(),
                "{} bytes",
                bad.len()
            );
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
