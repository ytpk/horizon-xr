//! The headset launcher's work, apart from drawing it (docs/quest-launcher-plan.md): put the
//! bundled Java runtime and exporter classes where the preparer finds them ([`unpack_zip`]),
//! download the pinned game from Mojang ([`download`]) and prepare it ([`prepare`]). Builds and
//! tests on a computer; the Quest app calls it.
mod machine;
#[cfg(test)]
mod machine_tests;
pub mod news;
mod real;
pub mod skin;

pub use machine::{
    Accounts, CodeView, Config, Failure, FailureKind, Launcher, Persisted, PlayRequest,
    PrepareView, Press, Provisioner, Readiness, Session, Settings, Stage, Step, View,
    classify_account,
};
pub use real::{QuestAccounts, QuestProvisioner, file_store, system_language};

use horizon_fetch::mojang::{self, Selection, Stage as FetchStage};
use horizon_fetch::{Options as FetchOptions, Pins, Progress, Ureq};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// The folder of this program's own library: on Android, the app's native library folder, where
/// `horizon-jvm` (shipped as `libhorizon_jvm.so`) can be run from.
#[cfg(unix)]
pub fn own_library_dir() -> Option<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    let here = own_library_dir as *const ();
    // SAFETY: dladdr fills `info` for an address of this library.
    if unsafe { libc::dladdr(here.cast(), &mut info) } == 0 || info.dli_fname.is_null() {
        return None;
    }
    let path = unsafe { CStr::from_ptr(info.dli_fname) };
    Path::new(OsStr::from_bytes(path.to_bytes()))
        .parent()
        .map(Path::to_path_buf)
}

/// Unpacks `zip` into `dest`, unless `dest` already holds this very archive (its stamp). Refuses
/// names that leave `dest`. Replaces `dest` whole: a half-unpacked folder never stays (the stamp
/// is written last). Returns whether it unpacked.
pub fn unpack_zip(zip: &[u8], dest: &Path) -> Result<bool, String> {
    let stamp = format!("{:08x}-{}", crc32fast::hash(zip), zip.len());
    let stamp_file = dest.join(".horizon-unpacked");
    if fs::read_to_string(&stamp_file).is_ok_and(|s| s == stamp) {
        return Ok(false);
    }
    let error = |what: &Path, e: &dyn std::fmt::Display| format!("{}: {e}", what.display());
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| error(dest, &e))?;
    let partial = dest.with_file_name(format!(
        ".{}.unpacking",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|e| error(&partial, &e))?;
    for n in 0..archive.len() {
        let mut entry = archive.by_index(n).map_err(|e| error(dest, &e))?;
        let name = entry.name().map_err(|e| error(dest, &e))?.into_owned();
        let safe = Path::new(&name)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !safe {
            return Err(format!("{name:?} in the archive leaves its folder"));
        }
        let target = partial.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| error(&target, &e))?;
            continue;
        }
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| error(&target, &e))?;
        let mut out = fs::File::create(&target).map_err(|e| error(&target, &e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| error(&target, &e))?;
    }
    fs::write(partial.join(".horizon-unpacked"), &stamp).map_err(|e| error(&partial, &e))?;
    let _ = fs::remove_dir_all(dest);
    fs::rename(&partial, dest).map_err(|e| error(dest, &e))?;
    Ok(true)
}

/// What to fetch from Mojang and where it goes.
pub struct Download<'a> {
    /// The folder with `research-lock.json` (the unpacked bundle).
    pub root: &'a Path,
    /// Official-launcher layout: `versions/`, `libraries/`, `assets/`.
    pub into: &'a Path,
    /// Language codes besides what the game needs to start (`en_us` is always fetched).
    pub languages: &'a [&'a str],
    pub stage: FetchStage,
    /// Free bytes that must remain after downloading (room to prepare).
    pub reserve: u64,
}

/// Downloads the pinned version (client jar, libraries, server jar, assets of the chosen
/// languages, or the sounds) from Mojang's public servers; the caller has asked the Minecraft
/// services whether the account owns the game (docs/microsoft-account.md). The path of the
/// server jar is `versions/<v>/<v>-server.jar`.
pub fn download(
    what: &Download,
    progress: &Progress,
    cancel: &AtomicBool,
    log: &dyn Fn(String),
) -> Result<(), horizon_fetch::Error> {
    use horizon_fetch::Error;
    let lock_path = what.root.join("research-lock.json");
    let lock: serde_json::Value = serde_json::from_slice(
        &fs::read(&lock_path).map_err(|e| Error::Io(format!("{}: {e}", lock_path.display())))?,
    )
    .map_err(|e| Error::Other(format!("{}: {e}", lock_path.display())))?;
    let pins = Pins::read(&lock)
        .ok_or_else(|| Error::Other("research-lock.json pins no version".into()))?;
    let server_sha1 = lock["minecraft"]["server_sha1"].as_str().map(str::to_owned);
    let mut languages: Vec<String> = vec!["en_us".into()];
    languages.extend(what.languages.iter().map(|l| l.to_lowercase()));
    languages.dedup();
    let selection = Selection {
        languages: Some(languages),
        stage: what.stage,
        strict_assets: true,
        server_sha1: server_sha1.as_deref(),
        ..Selection::all("linux")
    };
    let options = FetchOptions {
        jobs: 6,
        reserve: what.reserve,
        ..FetchOptions::default()
    };
    let transport = Ureq::new(format!("horizon-quest/{}", env!("CARGO_PKG_VERSION")));
    mojang::download(
        &pins, what.into, &selection, &options, &transport, progress, cancel, log,
    )
}

/// Where the preparer finds the runtime: `launcher` is the program `horizon-jvm` (a library file
/// of the app, so it may be run), `jre` the unpacked runtime.
pub struct Runtime<'a> {
    pub launcher: &'a Path,
    pub jre: &'a Path,
}

/// Prepares the downloaded files into `output` with the bundled exporters and runtime.
pub fn prepare(
    root: &Path,
    downloads: &Path,
    output: &Path,
    runtime: &Runtime,
    log: horizon_prepare::Log,
    progress: horizon_prepare::Progress,
) -> Result<PathBuf, String> {
    let lock: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("research-lock.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let version = lock["minecraft"]["version"]
        .as_str()
        .ok_or("research-lock.json names no version")?;
    // The preparer starts its JVMs from these, in this process's environment.
    // SAFETY: set before the preparer's threads start; nothing else reads them concurrently.
    unsafe {
        std::env::set_var("HORIZON_JAVA_EXE", runtime.launcher);
        std::env::set_var("HORIZON_JVM_HOME", runtime.jre);
    }
    let options = horizon_prepare::Options {
        root: root.to_path_buf(),
        import: true,
        minecraft_dir: None,
        server_jar: Some(downloads.join(format!("versions/{version}/{version}-server.jar"))),
        java_home: None,
        inputs: inputs(downloads, version),
        output: Some(output.to_path_buf()),
    };
    horizon_prepare::prepare_with_progress(&options, log, progress)
}

/// The downloaded files, named one by one for the preparer.
fn inputs(downloads: &Path, version: &str) -> horizon_prepare::artifacts::Inputs {
    horizon_prepare::artifacts::Inputs {
        version_json: Some(downloads.join(format!("versions/{version}/{version}.json"))),
        client_jar: Some(downloads.join(format!("versions/{version}/{version}.jar"))),
        libraries_dir: Some(downloads.join("libraries")),
        assets_dir: Some(downloads.join("assets")),
        prism_dir: None,
    }
}

/// Copies the sounds (`kind` "sounds") or the languages (`kind` "languages") of the downloaded
/// asset store into already prepared resources: no JVM, a few seconds.
pub fn import_assets(
    kind: &str,
    root: &Path,
    downloads: &Path,
    output: &Path,
    log: horizon_prepare::Log,
) -> Result<(), String> {
    let lock: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("research-lock.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let version = lock["minecraft"]["version"]
        .as_str()
        .ok_or("research-lock.json names no version")?;
    horizon_prepare::add_assets(
        kind,
        root,
        Some(output),
        None,
        &inputs(downloads, version),
        log,
    )
    .map(drop)
}

/// Reads a whole file for the unpacker (an APK asset is already in memory on the Quest).
pub fn read_all(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for (name, data) in files {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quest-launcher-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_archive_unpacks_once_and_again_only_when_it_changes() {
        let dir = scratch("unpack");
        let dest = dir.join("jre");
        let one = zip_of(&[("lib/modules", b"jimage"), ("release", b"25")]);
        assert!(unpack_zip(&one, &dest).unwrap());
        assert_eq!(fs::read(dest.join("lib/modules")).unwrap(), b"jimage");
        assert!(
            !unpack_zip(&one, &dest).unwrap(),
            "the same archive is left alone"
        );
        // A newer archive replaces the folder whole: nothing of the old one stays.
        let two = zip_of(&[("lib/modules", b"jimage2")]);
        assert!(unpack_zip(&two, &dest).unwrap());
        assert_eq!(fs::read(dest.join("lib/modules")).unwrap(), b"jimage2");
        assert!(!dest.join("release").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_name_that_leaves_the_folder_is_refused_and_nothing_is_left_half_unpacked() {
        let dir = scratch("slip");
        let dest = dir.join("jre");
        let evil = zip_of(&[("ok", b"1"), ("../escaped", b"2")]);
        let error = unpack_zip(&evil, &dest).unwrap_err();
        assert!(error.contains("leaves its folder"), "{error}");
        assert!(!dir.join("escaped").exists());
        assert!(!dest.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
