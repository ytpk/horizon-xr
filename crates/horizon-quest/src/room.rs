//! The headset launcher's room on the Quest (docs/quest-launcher-plan.md, phase 6): where its
//! files live, how the bundled runtime is unpacked from the APK, and the bridge that gives the
//! game's TV (`horizon_client::room`) the launcher's state machine.
//!
//! Everything but the game's own data is in the app's internal folder: `horizon/launcher/` (the
//! state, the news, the sealed secrets and their key), `horizon/downloads/` (Mojang's files),
//! `horizon/resources/` (the prepared game), `horizon/jre/` and `horizon/root/` (the runtime and
//! the exporters, from the APK). A developer's `files/resources` (push_resources.sh) still counts
//! as the game's files when the internal ones are not there.
use crate::logcat;
use horizon_client::room::{
    Code, Failure, Handoff, News, Page, Progress, RoomAction, RoomHost, RoomModel, Session,
};
use horizon_quest_launcher as launcher;
use launcher::{FailureKind, Launcher, Press, Stage, Step};
use std::ffi::CString;
use std::path::Path;
use std::sync::{Arc, Mutex};
use winit::platform::android::activity::AndroidApp;

const USER_AGENT: &str = concat!("horizon-quest/", env!("CARGO_PKG_VERSION"));

/// Puts an APK asset (a zip) where `dest` is, unless that is already done.
fn unpack(app: &AndroidApp, asset: &str, dest: &Path) -> Result<(), String> {
    let name = CString::new(asset).map_err(|e| e.to_string())?;
    let file = app
        .asset_manager()
        .open(&name)
        .ok_or_else(|| format!("{asset} is not in the APK (packaging/quest/build_apk.sh)"))?;
    let bytes = launcher::read_all(file)?;
    let started = std::time::Instant::now();
    let done = launcher::unpack_zip(&bytes, dest)?;
    if done {
        logcat::write(
            logcat::INFO,
            &format!(
                "Horizon room: {asset} unpacked ({} MB, {:.1} s)",
                bytes.len() >> 20,
                started.elapsed().as_secs_f32()
            ),
        );
    }
    Ok(())
}

/// Makes the room's launcher and hands it to the game.
pub fn start(app: &AndroidApp, internal: &Path, files: &Path) -> Result<(), String> {
    let base = internal.join("horizon");
    let dir = base.join("launcher");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    {
        // The state and the sealed secrets are this app's only.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let library = launcher::own_library_dir().ok_or("the app's library folder is unknown")?;
    let (jre, root) = (base.join("jre"), base.join("root"));
    let unpacked = Arc::new(Mutex::new(false));
    let provisioner = launcher::QuestProvisioner {
        root: root.clone(),
        downloads: base.join("downloads"),
        resources: base.join("resources"),
        pushed: Some(files.join("resources")),
        launcher: library.join("libhorizon_jvm.so"),
        jre: jre.clone(),
        unpack: {
            let app = app.clone();
            Box::new(move || {
                let mut done = unpacked.lock().unwrap_or_else(|e| e.into_inner());
                if !*done {
                    unpack(&app, "horizon-jre.zip", &jre)?;
                    unpack(&app, "horizon-bundle.zip", &root)?;
                    *done = true;
                }
                Ok(())
            })
        },
    };
    let config = {
        let c = app.config();
        launcher::Config {
            dir: dir.clone(),
            system_language: launcher::system_language(
                &c.language().unwrap_or_default(),
                &c.country().unwrap_or_default(),
            ),
            skip_gate: cfg!(feature = "dev-no-gate")
                && std::env::var("HORIZON_QUEST_NO_GATE").as_deref() == Ok("1"),
        }
    };
    if config.skip_gate {
        logcat::write(
            logcat::WARN,
            "Horizon room: DEVELOPER BUILD, the ownership gate is skipped (HORIZON_QUEST_NO_GATE=1)",
        );
    }
    logcat::write(
        logcat::INFO,
        &format!(
            "Horizon room: files in {}, language {:?}, client id {}",
            base.display(),
            config.system_language,
            if horizon_account::client_id().is_some() {
                "set"
            } else {
                "missing"
            }
        ),
    );
    let store: Arc<dyn horizon_account::SecretStore> = Arc::new(launcher::file_store(&dir));
    let launcher = Launcher::new(
        config,
        Arc::new(launcher::QuestAccounts::new()),
        Arc::new(provisioner),
        store,
        Arc::new(launcher::news::Web {
            user_agent: USER_AGENT.to_owned(),
        }),
    );
    horizon_client::room::set_host(Box::new(Bridge {
        launcher,
        head: Arc::default(),
    }));
    Ok(())
}

/// The face of the account's skin, fetched once its address is known.
type Head = Arc<Mutex<Option<(String, Option<Vec<u8>>)>>>;

struct Bridge {
    launcher: Launcher,
    head: Head,
}

fn kind(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::Network => "network",
        FailureKind::NotOwner => "not_owner",
        FailureKind::NoProfile => "no_profile",
        FailureKind::NotApproved => "not_approved",
        FailureKind::NotConfigured => "not_configured",
        FailureKind::XboxNoProfile => "xbox_no_profile",
        FailureKind::XboxRegion => "xbox_region",
        FailureKind::XboxAdult => "xbox_adult",
        FailureKind::XboxChild => "xbox_child",
        FailureKind::CodeExpired => "code_expired",
        FailureKind::Declined => "declined",
        FailureKind::SignInExpired => "sign_in_expired",
        FailureKind::NoSpace => "no_space",
        FailureKind::Checksum => "checksum",
        FailureKind::Cancelled => "cancelled",
        FailureKind::JvmFailed => "jvm_failed",
        FailureKind::Other => "other",
    }
}

fn phase(phase: horizon_account::Phase) -> Option<&'static str> {
    use horizon_account::Phase::*;
    match phase {
        Microsoft => Some("microsoft"),
        Xbox => Some("xbox"),
        Minecraft => Some("minecraft"),
        Ownership => Some("ownership"),
        Profile => Some("profile"),
        Queued | Keychain | Saving | SigningOut => None,
    }
}

impl Bridge {
    /// The account's face: asked for once per skin address, on a thread of its own.
    fn head(&self, skin: Option<&str>) -> Option<Vec<u8>> {
        let skin = skin?;
        let mut slot = self.head.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some((url, face)) if url == skin => face.clone(),
            _ => {
                *slot = Some((skin.to_owned(), None));
                let (url, head) = (skin.to_owned(), self.head.clone());
                std::thread::spawn(move || {
                    let face = launcher::skin::fetch_face(&url, USER_AGENT).ok();
                    let mut slot = head.lock().unwrap_or_else(|e| e.into_inner());
                    if slot.as_ref().is_some_and(|(u, _)| *u == url) {
                        *slot = Some((url, face));
                    }
                });
                None
            }
        }
    }
}

impl RoomHost for Bridge {
    fn model(&mut self) -> RoomModel {
        let v = self.launcher.view();
        let sounds_stage = v.stage == Stage::Sounds;
        RoomModel {
            prismarine_live: horizon_account::client_id().as_deref()
                == Some(horizon_account::PRISMARINE_CLIENT_ID),
            page: match v.step {
                Step::NoAccount => Page::NoAccount,
                Step::LoginCode => Page::SignIn,
                Step::CheckOwnership => Page::Checking,
                Step::Download => Page::Download,
                Step::Prepare => Page::Prepare,
                Step::RoomIdle | Step::Play => Page::Idle,
            },
            sounds_stage,
            head: self.head(
                v.account
                    .as_ref()
                    .and_then(|a| a.skin.as_ref())
                    .map(|s| s.0.as_str()),
            ),
            account: v.account.map(|a| a.name),
            version: v.version,
            code: v.code.map(|c| Code {
                user_code: c.user_code,
                url: c.url,
                qr_url: c.qr_url,
                expires_in: c.expires_in,
            }),
            step: v.phase.and_then(phase).map(str::to_owned),
            connecting: v.connecting,
            download: v.download.map(|d| Progress {
                files_done: d.files_done,
                files_total: d.files_total,
                bytes_done: d.bytes_done,
                bytes_total: d.bytes_total,
                bytes_per_sec: d.bytes_per_sec,
                eta_secs: d.eta_secs,
            }),
            prepare: v.prepare.map(|p| (p.line, p.fraction)),
            error: v.error.map(|e| Failure {
                kind: kind(e.kind).to_owned(),
                message: e.message,
            }),
            news: v.news.map(|n| News {
                title: n.title,
                date: n.date,
                lines: n.lines,
            }),
            sounds: v.settings.sounds,
            system_language: v.system_language,
            use_system_language: v.settings.system_language,
            can_play: v.can_play,
            verifying: v.verifying,
            offline: v.offline_answer,
            resources: v.resources,
        }
    }

    fn act(&mut self, action: RoomAction) {
        self.launcher.press(match action {
            RoomAction::SignIn => Press::SignIn,
            RoomAction::CancelSignIn => Press::CancelSignIn,
            RoomAction::Retry => Press::Retry,
            RoomAction::CancelDownload => Press::CancelDownload,
            RoomAction::SignOut => Press::SignOut,
            RoomAction::Play => Press::Play,
            RoomAction::SetSounds(on) => Press::SetSounds(on),
            RoomAction::SetSystemLanguage(on) => Press::SetSystemLanguage(on),
        });
    }

    fn take_play(&mut self) -> Option<Handoff> {
        let play = self.launcher.take_play()?;
        Some(Handoff {
            resources: play.resources,
            session: play.session.map(|s| Session {
                name: s.name,
                uuid: s.uuid,
                token: s.token,
                xuid: s.xuid,
            }),
        })
    }
}
