//! The player's own download of the pinned version from Mojang, for a signed-in account that
//! owns Minecraft: Java Edition (#434), as other launchers get it: the version JSON, the client
//! jar, this system's libraries and the assets (the sounds), each checked against its sha1,
//! in the official launcher's layout in the launcher's data folder. sources.rs then finds them
//! there like any launcher's. Nothing of Mojang's is part of Horizon.
//!
//! Each file is checked against the size and sha1 Mojang lists, and written whole. A download
//! stopped half way (cancelled, no network) keeps the files it finished: the next one fetches
//! only the rest.

use crate::sources::{fetch_checked, Fetch, Pins};
use horizon_fetch::mojang::Selection;
pub(crate) use horizon_fetch::mojang::{native_classifier, relative};
use horizon_fetch::{Bodies, Item as File, Options, Progress};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// The index of Mojang's Java runtimes, the one the official launcher reads.
const JAVA_RUNTIMES: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// Set to stop the download under way (the window's Cancel); cleared when a preparation starts.
pub static CANCEL: AtomicBool = AtomicBool::new(false);
/// The download under way (`horizon-fetch`'s counters).
static PROGRESS: Progress = Progress::new();

/// How far the download under way is, files done of how many; none when none runs.
pub fn progress() -> Option<(usize, usize)> {
    PROGRESS.snapshot().map(|s| (s.files_done, s.files_total))
}

fn options(jobs: usize) -> Options {
    Options {
        jobs,
        ..Options::default()
    }
}

/// Downloads the pinned version into `dir`, `jobs` files at a time; files already there and
/// whole are kept. An asset that fails is a line in the log (the game is then partly silent),
/// anything else an error. `cancel` stops it between files. The work is `horizon-fetch`'s.
pub fn download(
    pins: &Pins,
    dir: &Path,
    os: &str,
    jobs: usize,
    fetch: Fetch,
    cancel: &AtomicBool,
    log: &dyn Fn(String),
) -> Result<(), String> {
    horizon_fetch::mojang::download_with(
        pins,
        dir,
        &Selection::all(os),
        &options(jobs),
        fetch,
        &PROGRESS,
        cancel,
        log,
    )
    .map_err(String::from)
}

/// Mojang's name for this system in its Java runtime index.
pub(crate) fn java_platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", "x86_64") => "mac-os",
        ("linux", "x86_64") => "linux",
        ("linux", "x86") => "linux-i386",
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86") => "windows-x86",
        _ => return None,
    })
}

/// Downloads Mojang's Java runtime `component` (the version JSON's `javaVersion.component`, e.g.
/// `java-runtime-epsilon`) into the game directory's `runtime` folder, as the official launcher
/// lays it out: `<component>/<platform>/<component>/`. Each file is checked against its sha1;
/// files already there and whole are kept. These also include javac for older release preparers;
/// current releases can use an already installed runtime without it.
pub fn java_runtime(
    component: &str,
    runtime: &Path,
    jobs: usize,
    fetch: Fetch,
    cancel: &AtomicBool,
    log: &dyn Fn(String),
) -> Result<PathBuf, String> {
    let platform = java_platform().ok_or("Mojang has no Java for this system")?;
    let index: Value = serde_json::from_slice(&fetch(JAVA_RUNTIMES)?)
        .map_err(|e| format!("Mojang's Java index: {e}"))?;
    let entry = &index[platform][component][0];
    if entry.is_null() {
        return Err(format!("Mojang has no {component} for {platform}"));
    }
    let manifest = &entry["manifest"];
    let manifest: Value = serde_json::from_slice(&fetch_checked(
        fetch,
        &text(manifest, "url")?,
        &text(manifest, "sha1")?,
    )?)
    .map_err(|e| format!("Mojang's {component} manifest: {e}"))?;
    let files = manifest["files"]
        .as_object()
        .ok_or_else(|| format!("Mojang's {component} manifest lists no files"))?;
    let home = runtime.join(component).join(platform).join(component);
    let mut downloads = Vec::new();
    let mut executables = Vec::new();
    let mut links = Vec::new();
    for (name, file) in files {
        let path = home.join(relative(name)?);
        match file["type"].as_str() {
            Some("file") => {
                let raw = &file["downloads"]["raw"];
                if file["executable"] == true {
                    executables.push(path.clone());
                }
                downloads.push(File {
                    url: text(raw, "url")?,
                    sha1: text(raw, "sha1")?,
                    size: raw["size"].as_u64(),
                    path,
                });
            }
            Some("link") => links.push((path, text(file, "target")?)),
            _ => {}
        }
    }
    let name = entry["version"]["name"].as_str().unwrap_or(component);
    log(format!(
        "Downloading Minecraft's Java {name} from Mojang ({} files)",
        downloads.len()
    ));
    horizon_fetch::fetch_all(
        &downloads,
        &options(jobs),
        &Bodies(fetch),
        &PROGRESS,
        cancel,
        log,
    )
    .map_err(String::from)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in &executables {
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        for (path, target) in &links {
            if !link_stays_inside(name_in(&home, path), target) {
                return Err(format!(
                    "Unsafe link {target:?} in Mojang's {component} manifest"
                ));
            }
            let _ = fs::remove_file(path);
            fs::create_dir_all(path.parent().unwrap())
                .and_then(|_| std::os::unix::fs::symlink(target, path))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (executables, links);
    Ok(home)
}

// Used by the unix link step below; the path-traversal check itself is tested on every
// platform, so these stay compiled and are allowed to look unused elsewhere.
#[cfg_attr(not(unix), allow(dead_code))]
fn name_in<'a>(home: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(home).unwrap_or(path)
}

/// Whether a link at `link` (relative to the runtime's home) pointing at `target` stays in the
/// home.
#[cfg_attr(not(unix), allow(dead_code))]
fn link_stays_inside(link: &Path, target: &str) -> bool {
    let mut depth = link.components().count().saturating_sub(1);
    for c in Path::new(target).components() {
        match c {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    true
}

fn text(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .map(String::from)
        .ok_or_else(|| format!("The version file has no {key} in {v}"))
}

#[cfg(test)]
#[path = "mojang_tests.rs"]
mod tests;
