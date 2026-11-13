//! A developer's end-to-end run of the on-headset launcher's first part, started by the
//! `launch.txt` line `HORIZON_QUEST_PROVISION_TEST=<name>`: unpack the bundled Java runtime and
//! exporter classes from the APK, download the pinned game from Mojang's public servers (this
//! needs no sign-in; the room asks the Minecraft services first, docs/quest-launcher-plan.md),
//! prepare it, and start the game on what was made. Everything goes under the app's internal
//! folder, `provision-test/<name>/`, apart from the real game files.
use crate::logcat;
use horizon_fetch::Progress;
use horizon_fetch::mojang::Stage;
use horizon_quest_launcher as launcher;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use winit::platform::android::activity::AndroidApp;

fn say(line: &str) {
    logcat::write(logcat::INFO, &format!("Horizon provision: {line}"));
}

fn unpack(app: &AndroidApp, asset: &str, dest: &Path) -> Result<(), String> {
    let name = CString::new(asset).map_err(|e| e.to_string())?;
    let file = app
        .asset_manager()
        .open(&name)
        .ok_or_else(|| format!("{asset} is not in the APK (packaging/quest/build_apk.sh)"))?;
    let bytes = launcher::read_all(file)?;
    let started = Instant::now();
    let done = launcher::unpack_zip(&bytes, dest)?;
    say(&format!(
        "{asset}: {} ({} MB, {:.1} s)",
        if done { "unpacked" } else { "already unpacked" },
        bytes.len() >> 20,
        started.elapsed().as_secs_f32()
    ));
    Ok(())
}

/// Runs the whole chain; the prepared resources' folder on success.
pub fn run(app: &AndroidApp, internal: &Path, name: &str) -> Result<PathBuf, String> {
    let base = internal.join("provision-test").join(name);
    let (jre, root) = (base.join("jre"), base.join("root"));
    let (downloads, resources) = (base.join("downloads"), base.join("resources"));
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    unpack(app, "horizon-jre.zip", &jre)?;
    unpack(app, "horizon-bundle.zip", &root)?;
    let library_dir = launcher::own_library_dir().ok_or("the app's library folder is unknown")?;
    let jvm = library_dir.join("libhorizon_jvm.so");
    say(&format!(
        "runtime launcher {} ({})",
        jvm.display(),
        jvm.is_file()
    ));

    let total = Instant::now();
    let progress = Progress::new();
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    launcher::download(
        &launcher::Download {
            root: &root,
            into: &downloads,
            languages: &["ru_ru"],
            stage: Stage::Core,
            reserve: 3 << 30,
        },
        &progress,
        &cancel,
        &|line| say(&line),
    )?;
    say(&format!(
        "downloaded in {:.1} s",
        started.elapsed().as_secs_f32()
    ));

    let started = Instant::now();
    let output = launcher::prepare(
        &root,
        &downloads,
        &resources,
        &launcher::Runtime {
            launcher: &jvm,
            jre: &jre,
        },
        &|line| say(line),
        &|fraction| say(&format!("prepare {:.0}%", fraction * 100.0)),
    )?;
    say(&format!(
        "prepared in {:.1} s (total {:.1} s): {}",
        started.elapsed().as_secs_f32(),
        total.elapsed().as_secs_f32(),
        output.display()
    ));
    Ok(output)
}
