//! Horizon on Meta Quest: the NativeActivity's `android_main`, which points the
//! game at the app's folder, installs the OpenXR headset (horizon-xr) and runs
//! the game as the desktop executable does.
//!
//! The app's folder is its external files directory,
//! `/sdcard/Android/data/<package>/files/`:
//! - `resources/`: the prepared game files (`horizon-prepare` on a computer,
//!   from the player's own Minecraft; pushed with packaging/quest/push_resources.sh);
//! - `home/`: the game's data (options, worlds, logs);
//! - `launch.txt` (optional): more command-line arguments, one per line, such
//!   as `--world` and a name, or `--name` and an offline player name; a line
//!   `NAME=value` sets an environment variable instead (`HORIZON_XR_SCALE=0.7`).
//!
//! Standard output and error go to logcat under the tag `horizon`; the game's
//! log is also in `home/logs/latest.log`.
#![cfg(target_os = "android")]

use std::path::{Path, PathBuf};
use winit::platform::android::activity::AndroidApp;

mod logcat;
mod provision;
mod room;

/// The game's thread: worldgen and parsing recurse deeper than the 2 MiB a
/// Rust thread gets by default. The game must run on `android_main`'s own
/// thread (android-activity polls the activity's looper only there), which
/// android-activity spawns with `std::thread::spawn` before `android_main`
/// runs; so the size goes in `RUST_MIN_STACK` from a library constructor,
/// which runs when the activity loads this library, before any of our threads.
const STACK: &str = "67108864";

#[used]
#[unsafe(link_section = ".init_array")]
static SET_MIN_STACK: extern "C" fn() = {
    extern "C" fn set_min_stack() {
        // Safety: the library is being loaded; none of its threads exist yet.
        unsafe { std::env::set_var("RUST_MIN_STACK", STACK) };
    }
    set_min_stack
};

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    logcat::redirect_stdio();
    logcat::write(logcat::INFO, "Horizon: android_main");
    let Some(files) = app
        .external_data_path()
        .or_else(|| app.internal_data_path())
    else {
        logcat::write(logcat::ERROR, "Horizon: the app has no files directory");
        return;
    };
    let mut args = match prepare(&files, app.internal_data_path().as_deref()) {
        Ok(args) => args,
        Err(e) => {
            logcat::write(logcat::ERROR, &format!("Horizon: {e}"));
            return;
        }
    };
    // A developer's check of unpacking, downloading and preparing on the headset (provision.rs):
    // the game then starts on the resources it made.
    if let (Some(name), Some(internal)) = (
        std::env::var("HORIZON_QUEST_PROVISION_TEST")
            .ok()
            .filter(|n| !n.is_empty() && !n.contains('/')),
        app.internal_data_path(),
    ) {
        match provision::run(&app, &internal, &name) {
            Ok(resources) => {
                if let Some(at) = args.iter().position(|a| a == "--resources") {
                    args[at + 1] = resources.to_string_lossy().into_owned();
                }
            }
            Err(e) => {
                logcat::write(logcat::ERROR, &format!("Horizon provision: failed: {e}"));
                return;
            }
        }
    }
    // The room comes first on every start (docs/quest-launcher-plan.md): sign-in, download and
    // prepare on the TV, then Play hands the game its files. `HORIZON_QUEST_ROOM=0` (a
    // launch.txt line) starts the game straight on the files pushed by push_resources.sh, as
    // before; the provision test above has no room either.
    let provision = std::env::var_os("HORIZON_QUEST_PROVISION_TEST").is_some_and(|n| !n.is_empty());
    if !provision
        && std::env::var("HORIZON_QUEST_ROOM").as_deref() != Ok("0")
        && let Some(internal) = app.internal_data_path()
    {
        match room::start(&app, &internal, &files) {
            Ok(()) => {}
            Err(e) => {
                logcat::write(
                    logcat::ERROR,
                    &format!("Horizon: the room could not start: {e}"),
                );
                return;
            }
        }
    }
    logcat::write(logcat::INFO, &format!("Horizon: arguments {args:?}"));
    use_activity_as_context(&app);
    horizon_client::android::embed::set_args(args);
    horizon_client::android::embed::set_android_app(app);
    // Made when the renderer first asks, after the activity's window exists
    // (gfx::headset::install_lazy says why).
    horizon_client::gfx::headset::install_lazy(Box::new(|| {
        horizon_xr::Quest::new()
            .map(|quest| Box::new(quest) as Box<dyn horizon_client::gfx::headset::Headset>)
            .map_err(|e| format!("OpenXR: {e}"))
    }));
    // On this thread: winit's event loop polls the activity's looper, which
    // android-activity allows only from `android_main`'s thread.
    if std::panic::catch_unwind(|| horizon_client::entry::main_with(Default::default())).is_err() {
        logcat::write(logcat::ERROR, "Horizon: the game panicked");
    }
    // The activity outlives `android_main` otherwise, showing nothing.
    std::process::exit(0);
}

/// Points the process's Android context (`ndk_context`) at the activity.
///
/// android-activity sets it to the `Application`, and the openxr crate passes it
/// to `xrCreateInstance` as `applicationActivity`. Meta's runtime looks up the
/// activity's volumetric window through it; given the `Application` it finds
/// none ("notifyAppStartedRendering: no VW for null"), and the system's launch
/// transition stays drawn over the app's frames for good. The activity is a
/// `Context` as well, so the other users of `ndk_context` (AAudio) are served.
fn use_activity_as_context(app: &AndroidApp) {
    let vm = app.vm_as_ptr();
    // `ANativeActivity::clazz`: a global reference the activity holds while it lives,
    // and this process ends with the activity (`android_main` exits it).
    let activity = app.activity_as_ptr();
    if vm.is_null() || activity.is_null() {
        logcat::write(logcat::ERROR, "Horizon: no activity to give OpenXR");
        return;
    }
    // Safety: android-activity initialised the context before `android_main`, and
    // nothing of ours has read it yet; both pointers live as long as the process.
    unsafe {
        ndk_context::release_android_context();
        ndk_context::initialize_android_context(vm, activity);
    }
}

/// Makes the game's folders and environment under `files` and returns its
/// command line.
fn prepare(files: &Path, internal: Option<&Path>) -> Result<Vec<String>, String> {
    let home = files.join("home");
    let resources = files.join("resources");
    std::fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    // The game keeps caches and servers.dat in its working directory.
    std::env::set_current_dir(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let tmp = internal.map_or_else(|| home.join("tmp"), |dir| dir.join("tmp"));
    let _ = std::fs::create_dir_all(&tmp);
    // Safety: before any other thread of ours starts (the logcat pump reads no
    // environment); nothing else in the process reads it concurrently.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
        std::env::set_var("TMPDIR", &tmp);
    }
    headset_defaults(&home);
    // The headset's own settings before the game starts: the eyes' resolution
    // is fixed when the headset opens, before the game's first frame.
    horizon_client::headset::set_settings(&std::sync::Arc::new(
        horizon_client::headset::VrSettings::load(&home),
    ));
    let text = |p: &PathBuf| p.to_string_lossy().into_owned();
    let mut args = vec![
        "--home".to_owned(),
        text(&home),
        "--resources".to_owned(),
        text(&resources),
        // No window surface: the frames go to the headset (gfx::headset).
        "--offscreen".to_owned(),
    ];
    if let Ok(extra) = std::fs::read_to_string(files.join("launch.txt")) {
        for line in extra.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match environment_line(line) {
                // Safety: as above, before the game's threads start.
                Some((key, value)) => unsafe { std::env::set_var(key, value) },
                None => args.push(line.to_owned()),
            }
        }
    }
    Ok(args)
}

/// Options a headset needs to keep its frame rate (two passes a frame at
/// 72 Hz on a phone GPU): set once, on the first start of this app version's
/// defaults, over vanilla's desktop defaults; the player may raise them again
/// in the game's settings afterwards.
const HEADSET_OPTIONS: &[(&str, &str)] = &[
    ("renderDistance", "8"),
    ("simulationDistance", "6"),
    ("renderClouds", "\"fast\""),
    ("entityShadows", "false"),
    // The headset never bobs the view; the option says so in the settings too.
    ("bobView", "false"),
];
/// Marks `HEADSET_OPTIONS` as applied in the game's home.
const HEADSET_DEFAULTS_MARK: &str = ".headset-defaults-1";

/// Writes `HEADSET_OPTIONS` into `home/options.txt` once (made if missing).
fn headset_defaults(home: &Path) {
    let mark = home.join(HEADSET_DEFAULTS_MARK);
    if mark.exists() {
        return;
    }
    let path = home.join("options.txt");
    // A file without a version would go through every options fixer since 1.8.
    let old = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| format!("version:{}\n", horizon_core::version::DATA_VERSION));
    let mut lines: Vec<String> = old
        .lines()
        .filter(|line| {
            let key = line.split_once(':').map_or(*line, |(k, _)| k);
            !HEADSET_OPTIONS.iter().any(|(k, _)| *k == key)
        })
        .map(str::to_owned)
        .collect();
    lines.extend(HEADSET_OPTIONS.iter().map(|(k, v)| format!("{k}:{v}")));
    let written =
        std::fs::write(&path, lines.join("\n") + "\n").and_then(|()| std::fs::write(&mark, ""));
    if let Err(e) = written {
        logcat::write(
            logcat::WARN,
            &format!("Horizon: headset options in {}: {e}", path.display()),
        );
    }
}

/// A `launch.txt` line `NAME=value` with an upper-case name sets that
/// environment variable (`HORIZON_XR_SCALE=0.7`); any other line is an argument.
fn environment_line(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let name = !key.is_empty()
        && !key.starts_with(|c: char| c.is_ascii_digit())
        && key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    name.then_some((key, value))
}
