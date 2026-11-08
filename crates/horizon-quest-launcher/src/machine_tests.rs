//! The state machine on mock account and download backends: transitions, persistence, a kill at
//! every step, errors with Retry, and the ownership gate.
use crate::machine::*;
use crate::news::NewsSource;
use horizon_account::{
    Account, Check, DOES_NOT_OWN, DeviceCode, MemoryStore, MsTokens, Phase, SecretStore, Secrets,
};
use horizon_fetch::Progress;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SLOW: Duration = Duration::from_secs(10);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quest-machine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What the mocks did, in order, across both backends.
type Calls = Arc<Mutex<Vec<String>>>;

fn secrets(owns: bool) -> Secrets {
    Secrets {
        refresh_token: "refresh".into(),
        sign_in_required: false,
        minecraft_token: "mc-token".into(),
        minecraft_expires_at: horizon_account::now() + 86_400,
        xuid: None,
        owns,
    }
}

fn account() -> Account {
    Account {
        uuid: "069a79f444e94726a5befca90e38aaf5".into(),
        name: "Notch".into(),
        skin: None,
        cape: None,
    }
}

/// What the mock account does at each call.
#[derive(Clone)]
enum Verify {
    Owns,
    DoesNot,
    Offline(bool),
    SignInRequired,
}

struct MockAccounts {
    calls: Calls,
    /// What `finish` answers: Ok, or the services' message.
    finish: Mutex<Result<(), String>>,
    verify: Mutex<Verify>,
    /// `begin` waits for this to be set (a slow first lookup).
    hold_begin: AtomicBool,
    /// `wait` returns once this is set; the machine's own cancel also ends it.
    approve: AtomicBool,
    codes: AtomicUsize,
}

impl MockAccounts {
    fn new(calls: &Calls) -> Arc<Self> {
        Arc::new(Self {
            calls: calls.clone(),
            finish: Mutex::new(Ok(())),
            verify: Mutex::new(Verify::Owns),
            hold_begin: AtomicBool::new(false),
            approve: AtomicBool::new(true),
            codes: AtomicUsize::new(0),
        })
    }
    fn note(&self, what: &str) {
        self.calls.lock().unwrap().push(what.to_owned());
    }
}

impl Accounts for MockAccounts {
    fn configured(&self) -> bool {
        true
    }
    fn begin(&self) -> Result<DeviceCode, String> {
        self.note("begin");
        while self.hold_begin.load(SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let n = self.codes.fetch_add(1, SeqCst) + 1;
        Ok(DeviceCode {
            user_code: format!("CODE{n}"),
            verification_uri: "https://www.microsoft.com/link".into(),
            interval: 1,
            expires_at: horizon_account::now() + 900,
            device_code: "secret-device-code".into(),
        })
    }
    fn wait(&self, _: &DeviceCode, cancelled: &dyn Fn() -> bool) -> Result<MsTokens, String> {
        self.note("wait");
        loop {
            if cancelled() {
                return Err("Sign-in cancelled.".into());
            }
            if self.approve.load(SeqCst) {
                return Ok(MsTokens {
                    access: "a".into(),
                    refresh: "r".into(),
                });
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn finish(&self, _: MsTokens, phase: &dyn Fn(Phase)) -> Result<(Account, Secrets), String> {
        self.note("finish");
        phase(Phase::Xbox);
        phase(Phase::Ownership);
        self.finish
            .lock()
            .unwrap()
            .clone()
            .map(|()| (account(), secrets(true)))
    }
    fn verify(&self, _: &Account, s: Secrets, phase: &dyn Fn(Phase)) -> Check {
        self.note("verify");
        phase(Phase::Ownership);
        match self.verify.lock().unwrap().clone() {
            Verify::Owns => Check::Owns(account(), secrets(true)),
            Verify::DoesNot => Check::DoesNot(Secrets { owns: false, ..s }),
            Verify::Offline(owns) => Check::NoAnswer(
                "Could not reach the Minecraft services. Check your internet connection.".into(),
                Secrets { owns, ..s },
            ),
            Verify::SignInRequired => Check::NoAnswer(
                horizon_account::SIGN_IN_AGAIN.into(),
                Secrets {
                    sign_in_required: true,
                    ..s
                },
            ),
        }
    }
}

/// A folder of "files": the stamps are booleans the mock sets as downloads and prepares finish.
struct MockProvisioner {
    calls: Calls,
    core_downloaded: AtomicBool,
    game: AtomicBool,
    sounds_downloaded: AtomicBool,
    sounds: AtomicBool,
    /// The next downloads fail with this (once each).
    fail_download: Mutex<Option<Failure>>,
    /// The next prepare fails with this (once).
    fail_prepare: Mutex<Option<Failure>>,
    /// A download of this stage stops here until released (cancel or release).
    block_download: Mutex<Option<Stage>>,
    release: AtomicBool,
    cleaned: AtomicUsize,
}

impl MockProvisioner {
    fn new(calls: &Calls) -> Arc<Self> {
        Arc::new(Self {
            calls: calls.clone(),
            core_downloaded: AtomicBool::new(false),
            game: AtomicBool::new(false),
            sounds_downloaded: AtomicBool::new(false),
            sounds: AtomicBool::new(false),
            fail_download: Mutex::new(None),
            fail_prepare: Mutex::new(None),
            block_download: Mutex::new(None),
            release: AtomicBool::new(false),
            cleaned: AtomicUsize::new(0),
        })
    }
    fn note(&self, what: &str) {
        self.calls.lock().unwrap().push(what.to_owned());
    }
}

impl Provisioner for MockProvisioner {
    fn version(&self) -> String {
        "26.3".into()
    }
    fn readiness(&self, _: &[String]) -> Readiness {
        Readiness {
            game: self.game.load(SeqCst),
            sounds: self.sounds.load(SeqCst),
            core_downloaded: self.core_downloaded.load(SeqCst),
            sounds_downloaded: self.sounds_downloaded.load(SeqCst),
        }
    }
    fn resources(&self) -> PathBuf {
        PathBuf::from("/resources")
    }
    fn download(
        &self,
        stage: Stage,
        _: &[String],
        progress: &Progress,
        cancel: &AtomicBool,
        _: &dyn Fn(String),
    ) -> Result<(), Failure> {
        self.note(&format!("download {stage:?}"));
        progress.begin(10, 1000);
        if let Some(f) = self.fail_download.lock().unwrap().take() {
            return Err(f);
        }
        if *self.block_download.lock().unwrap() == Some(stage) {
            while !self.release.load(SeqCst) {
                if cancel.load(SeqCst) {
                    return Err(Failure::new(FailureKind::Cancelled, "cancelled"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        match stage {
            Stage::Core => self.core_downloaded.store(true, SeqCst),
            Stage::Sounds => self.sounds_downloaded.store(true, SeqCst),
        }
        Ok(())
    }
    fn prepare(
        &self,
        stage: Stage,
        _: &[String],
        log: &dyn Fn(&str),
        progress: &dyn Fn(f32),
    ) -> Result<(), Failure> {
        self.note(&format!("prepare {stage:?}"));
        if let Some(f) = self.fail_prepare.lock().unwrap().take() {
            return Err(f);
        }
        log("Exporting server data");
        progress(0.5);
        match stage {
            Stage::Core => self.game.store(true, SeqCst),
            Stage::Sounds => self.sounds.store(true, SeqCst),
        }
        Ok(())
    }
    fn cleanup(&self) {
        self.note("cleanup");
        self.cleaned.fetch_add(1, SeqCst);
    }
    fn forget_sounds_download(&self) {
        self.sounds_downloaded.store(false, SeqCst);
    }
}

struct NoNews;
impl NewsSource for NoNews {
    fn releases(&self) -> Result<Value, String> {
        Err("No connection".into())
    }
}

struct Rig {
    dir: PathBuf,
    calls: Calls,
    accounts: Arc<MockAccounts>,
    prov: Arc<MockProvisioner>,
    store: Arc<MemoryStore>,
}

impl Rig {
    fn new(name: &str) -> Self {
        let calls = Calls::default();
        Self {
            dir: scratch(name),
            accounts: MockAccounts::new(&calls),
            prov: MockProvisioner::new(&calls),
            store: Arc::new(MemoryStore::default()),
            calls,
        }
    }
    fn launcher(&self) -> Launcher {
        Launcher::new(
            Config {
                dir: self.dir.clone(),
                system_language: Some("ru_ru".into()),
                skip_gate: false,
            },
            self.accounts.clone(),
            self.prov.clone(),
            self.store.clone(),
            Arc::new(NoNews),
        )
    }
    /// A signed-in account's files, as an earlier run left them.
    fn signed_in(&self, step: Step) {
        self.store
            .set(
                &format!("quest-account/{}", account().uuid),
                &serde_json::to_string(&secrets(true)).unwrap(),
            )
            .unwrap();
        let p = Persisted {
            step,
            account: Some(account()),
            ..Persisted::default()
        };
        std::fs::write(self.dir.join("state.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn state(&self) -> Persisted {
        serde_json::from_slice(&std::fs::read(self.dir.join("state.json")).unwrap()).unwrap()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn idle(v: &View) -> bool {
    v.step == Step::RoomIdle && v.can_play
}

#[test]
fn a_fresh_start_waits_for_the_player_and_the_whole_chain_runs_in_order() {
    let rig = Rig::new("chain");
    let launcher = rig.launcher();
    let view = launcher.view();
    assert_eq!(view.step, Step::NoAccount);
    assert!(
        rig.calls().is_empty(),
        "nothing runs before the player presses"
    );

    launcher.press(Press::SignIn);
    let view = launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
    assert!(view.can_play, "{view:?}");
    assert_eq!(view.account, Some(account()));
    assert_eq!(view.version, "26.3");
    assert_eq!(
        rig.calls(),
        [
            "begin",
            "wait",
            "finish",
            "download Core",
            "prepare Core",
            "cleanup",
            "download Sounds",
            "prepare Sounds"
        ]
    );
    // What is written is the state, not a secret.
    let written = std::fs::read_to_string(rig.dir.join("state.json")).unwrap();
    assert!(!written.contains("mc-token") && !written.contains("refresh"));
    assert_eq!(rig.state().step, Step::RoomIdle);
}

#[test]
fn the_code_is_on_the_view_with_its_qr_address_while_waiting() {
    let rig = Rig::new("code");
    rig.accounts.approve.store(false, SeqCst);
    let launcher = rig.launcher();
    launcher.press(Press::SignIn);
    let view = launcher.wait(|v| v.code.is_some(), SLOW);
    let code = view.code.unwrap();
    assert_eq!(view.step, Step::LoginCode);
    assert_eq!(code.user_code, "CODE1");
    assert_eq!(code.url, "https://www.microsoft.com/link");
    assert_eq!(code.qr_url, "https://www.microsoft.com/link?otc=CODE1");
    assert!(code.expires_in > 800);
    // Cancel goes back; nothing was downloaded; the device code is never in a file.
    launcher.press(Press::CancelSignIn);
    assert_eq!(launcher.view().step, Step::NoAccount);
    assert!(!rig.calls().iter().any(|c| c.starts_with("download")));
    let written = std::fs::read_to_string(rig.dir.join("state.json")).unwrap();
    assert!(!written.contains("secret-device-code"));
}

#[test]
fn a_slow_first_lookup_reads_as_connecting_not_as_a_hang() {
    let rig = Rig::new("connecting");
    rig.accounts.hold_begin.store(true, SeqCst);
    rig.accounts.approve.store(false, SeqCst);
    let launcher = rig.launcher();
    launcher.press(Press::SignIn);
    let view = launcher.view();
    assert_eq!(view.step, Step::LoginCode);
    assert!(view.connecting && view.code.is_none(), "{view:?}");
    rig.accounts.hold_begin.store(false, SeqCst);
    let view = launcher.wait(|v| v.code.is_some(), SLOW);
    assert!(!view.connecting);
}

#[test]
fn an_account_that_does_not_own_the_game_is_refused_before_any_download() {
    let rig = Rig::new("refused");
    *rig.accounts.finish.lock().unwrap() = Err(DOES_NOT_OWN.into());
    let launcher = rig.launcher();
    launcher.press(Press::SignIn);
    let view = launcher.wait(|v| v.error.is_some(), SLOW);
    assert_eq!(view.step, Step::CheckOwnership);
    assert_eq!(view.error.unwrap().kind, FailureKind::NotOwner);
    assert!(!view.can_play);
    assert!(
        !rig.calls().iter().any(|c| c.starts_with("download")),
        "{:?}",
        rig.calls()
    );

    // A known account whose ownership the services now deny: the same, at the next start.
    let rig = Rig::new("refused2");
    rig.signed_in(Step::RoomIdle);
    *rig.accounts.verify.lock().unwrap() = Verify::DoesNot;
    let launcher = rig.launcher();
    let view = launcher.wait(|v| v.error.is_some(), SLOW);
    assert_eq!(view.error.unwrap().kind, FailureKind::NotOwner);
    assert!(!view.can_play);
    launcher.press(Press::Play);
    assert_eq!(launcher.take_play(), None, "no Play without an owner");
    assert!(!rig.calls().iter().any(|c| c.starts_with("download")));
}

#[test]
fn a_state_file_that_says_download_without_an_account_is_not_believed() {
    let rig = Rig::new("forged");
    let p = Persisted {
        step: Step::Download,
        ..Persisted::default()
    };
    std::fs::write(rig.dir.join("state.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    let launcher = rig.launcher();
    assert_eq!(launcher.view().step, Step::NoAccount);
    std::thread::sleep(Duration::from_millis(100));
    assert!(rig.calls().is_empty(), "{:?}", rig.calls());
}

#[test]
fn a_download_waits_for_an_answer_from_the_services_in_this_run() {
    // Offline at the start with an account that owned the game: files that are missing cannot
    // be fetched on the old answer.
    let rig = Rig::new("gate-offline");
    rig.signed_in(Step::Download);
    *rig.accounts.verify.lock().unwrap() = Verify::Offline(true);
    let launcher = rig.launcher();
    let view = launcher.wait(|v| v.error.is_some(), SLOW);
    assert_eq!(
        view.error.as_ref().unwrap().kind,
        FailureKind::Network,
        "{view:?}"
    );
    assert_eq!(view.step, Step::CheckOwnership);
    assert!(!rig.calls().iter().any(|c| c.starts_with("download")));
    // Back online: Retry asks again, then downloads.
    *rig.accounts.verify.lock().unwrap() = Verify::Owns;
    launcher.press(Press::Retry);
    let view = launcher.wait(idle, SLOW);
    assert_eq!(view.step, Step::RoomIdle);
    let calls = rig.calls();
    let first_download = calls
        .iter()
        .position(|c| c.starts_with("download"))
        .unwrap();
    let last_verify = calls.iter().rposition(|c| c == "verify").unwrap();
    assert!(last_verify < first_download, "{calls:?}");
}

#[test]
fn the_last_answer_lets_a_ready_headset_play_offline() {
    let rig = Rig::new("offline-play");
    rig.prov.game.store(true, SeqCst);
    rig.prov.sounds.store(true, SeqCst);
    rig.signed_in(Step::RoomIdle);
    *rig.accounts.verify.lock().unwrap() = Verify::Offline(true);
    let launcher = rig.launcher();
    let view = launcher.wait(idle, SLOW);
    assert!(view.offline_answer);
    launcher.press(Press::Play);
    let play = launcher.take_play().expect("play was pressed");
    assert_eq!(play.resources, PathBuf::from("/resources"));
    assert_eq!(launcher.view().step, Step::Play);
    assert_eq!(launcher.take_play(), None, "handed over once");
    // The file still says idle: after the game the room is idle again.
    assert_eq!(rig.state().step, Step::RoomIdle);
}

#[test]
fn play_hands_over_the_session_of_an_owner() {
    let rig = Rig::new("session");
    rig.prov.game.store(true, SeqCst);
    rig.prov.sounds.store(true, SeqCst);
    rig.signed_in(Step::RoomIdle);
    let launcher = rig.launcher();
    let view = launcher.wait(idle, SLOW);
    assert!(!view.offline_answer);
    launcher.press(Press::Play);
    let session = launcher.take_play().unwrap().session.unwrap();
    assert_eq!(
        (session.name.as_str(), session.token.as_str()),
        ("Notch", "mc-token")
    );
    assert!(
        !format!("{session:?}").contains("mc-token"),
        "the token is never printed"
    );
}

#[test]
fn an_expired_sign_in_returns_to_no_account_and_forgets_the_tokens() {
    let rig = Rig::new("expired");
    rig.prov.game.store(true, SeqCst);
    rig.signed_in(Step::RoomIdle);
    *rig.accounts.verify.lock().unwrap() = Verify::SignInRequired;
    let launcher = rig.launcher();
    let view = launcher.wait(|v| v.step == Step::NoAccount && v.error.is_some(), SLOW);
    assert_eq!(view.error.unwrap().kind, FailureKind::SignInExpired);
    assert_eq!(view.account, None);
    assert_eq!(
        rig.store
            .get(&format!("quest-account/{}", account().uuid))
            .unwrap(),
        None
    );
    // Pressing Sign in starts a new code.
    launcher.press(Press::Retry);
    let view = launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
    assert!(view.can_play);
}

#[test]
fn a_failed_download_shows_its_error_with_retry_and_retry_goes_on() {
    let rig = Rig::new("retry");
    *rig.prov.fail_download.lock().unwrap() = Some(Failure::new(
        FailureKind::Network,
        "piston-data.mojang.com timed out",
    ));
    let launcher = rig.launcher();
    launcher.press(Press::SignIn);
    let view = launcher.wait(|v| v.error.is_some(), SLOW);
    assert_eq!(view.step, Step::Download);
    assert_eq!(view.stage, Stage::Core);
    let error = view.error.unwrap();
    assert_eq!(error.kind, FailureKind::Network);
    assert!(error.message.contains("timed out"));
    assert!(!view.can_play);

    launcher.press(Press::Retry);
    let view = launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
    assert!(view.can_play && view.error.is_none());
    // The failed step was entered again, not the whole chain from the sign-in.
    assert_eq!(rig.calls().iter().filter(|c| *c == "begin").count(), 1);
    assert_eq!(
        rig.calls().iter().filter(|c| *c == "download Core").count(),
        2
    );
}

/// Signed in with the game ready and the sounds downloaded, stopped on a missing sound object.
fn stuck_adding_the_sounds(name: &str) -> Rig {
    let rig = Rig::new(name);
    rig.prov.game.store(true, SeqCst);
    rig.prov.sounds_downloaded.store(true, SeqCst);
    *rig.prov.fail_prepare.lock().unwrap() =
        Some(Failure::new(FailureKind::Other, "Missing asset object"));
    rig.signed_in(Step::RoomIdle);
    rig
}

#[test]
fn retry_after_a_missing_sound_downloads_the_sounds_again() {
    let rig = stuck_adding_the_sounds("sounds-retry");
    let launcher = rig.launcher();
    let view = launcher.wait(|v| v.error.is_some(), SLOW);
    assert_eq!((view.step, view.stage), (Step::Prepare, Stage::Sounds));

    launcher.press(Press::Retry);
    let view = launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
    assert!(view.can_play && view.error.is_none());
    let calls = rig.calls();
    let download = calls.iter().position(|c| c == "download Sounds");
    let last_prepare = calls.iter().rposition(|c| c == "prepare Sounds");
    assert!(download.is_some() && download < last_prepare, "{calls:?}");
    assert!(rig.prov.sounds.load(SeqCst));
}

#[test]
fn turning_the_sounds_off_leaves_their_error_for_the_room() {
    let rig = stuck_adding_the_sounds("sounds-off");
    let launcher = rig.launcher();
    launcher.wait(|v| v.error.is_some(), SLOW);

    launcher.press(Press::SetSounds(false));
    let view = launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
    assert!(view.can_play && view.error.is_none());
    assert!(!rig.state().settings.sounds);
    assert!(!rig.calls().iter().any(|c| c == "download Sounds"));
}

#[test]
fn turning_the_sounds_off_does_not_leave_an_error_of_the_game_files() {
    let rig = Rig::new("core-error-stays");
    *rig.prov.fail_download.lock().unwrap() = Some(Failure::new(FailureKind::Network, "down"));
    let launcher = rig.launcher();
    launcher.press(Press::SignIn);
    launcher.wait(|v| v.error.is_some(), SLOW);
    launcher.press(Press::SetSounds(false));
    let view = launcher.view();
    assert_eq!(view.step, Step::Download);
    assert!(view.error.is_some());
}

#[test]
fn a_failure_is_shown_again_after_a_restart_and_retry_resumes() {
    let rig = Rig::new("error-restart");
    *rig.prov.fail_download.lock().unwrap() = Some(Failure::new(FailureKind::NoSpace, "full"));
    {
        let launcher = rig.launcher();
        launcher.press(Press::SignIn);
        launcher.wait(|v| v.error.is_some(), SLOW);
    }
    let launcher = rig.launcher();
    let view = launcher.view();
    assert_eq!(view.step, Step::Download);
    assert_eq!(view.error.as_ref().unwrap().kind, FailureKind::NoSpace);
    let before = rig.calls().len();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        rig.calls().len(),
        before,
        "nothing runs by itself behind an error"
    );
    launcher.press(Press::Retry);
    assert!(launcher.wait(|v| v.step == Step::RoomIdle, SLOW).can_play);
}

/// The persisted step at which the process died, and what a restart must end in.
#[test]
fn a_kill_at_every_step_resumes_and_ends_idle() {
    for step in [
        Step::NoAccount,
        Step::LoginCode,
        Step::CheckOwnership,
        Step::Download,
        Step::Prepare,
        Step::RoomIdle,
    ] {
        let rig = Rig::new(&format!("kill-{step:?}"));
        if step == Step::Prepare {
            // The files were downloaded before the kill: the preparer needs no network.
            rig.prov.core_downloaded.store(true, SeqCst);
        }
        if step == Step::NoAccount || step == Step::LoginCode {
            // The device code was never written: the player signs in again.
            let p = Persisted {
                step,
                ..Persisted::default()
            };
            std::fs::write(rig.dir.join("state.json"), serde_json::to_vec(&p).unwrap()).unwrap();
        } else {
            rig.signed_in(step);
        }
        let launcher = rig.launcher();
        if step == Step::NoAccount {
            launcher.press(Press::SignIn);
        }
        let view = launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
        assert_eq!(view.step, Step::RoomIdle, "{step:?}: {view:?}");
        assert!(view.can_play, "{step:?}");
        assert!(view.error.is_none(), "{step:?}");
        let calls = rig.calls();
        if step == Step::Prepare {
            assert!(!calls.contains(&"download Core".to_owned()), "{calls:?}");
            assert!(calls.contains(&"prepare Core".to_owned()));
        } else if matches!(step, Step::Download | Step::CheckOwnership) {
            assert!(
                calls.contains(&"download Core".to_owned()),
                "{step:?} {calls:?}"
            );
        }
        // Whatever the step, the account was asked before the first download.
        if let Some(d) = calls.iter().position(|c| c.starts_with("download")) {
            assert!(
                calls[..d].iter().any(|c| c == "verify" || c == "finish"),
                "{step:?}: {calls:?}"
            );
        }
    }
}

#[test]
fn a_kill_in_the_middle_of_a_download_leaves_the_new_run_in_charge() {
    let rig = Rig::new("kill-running");
    *rig.prov.block_download.lock().unwrap() = Some(Stage::Core);
    let first = rig.launcher();
    first.press(Press::SignIn);
    first.wait(|v| v.step == Step::Download && v.download.is_some(), SLOW);
    assert_eq!(rig.state().step, Step::Download);
    drop(first); // the process dies; its worker is still stuck in the mock
    *rig.prov.block_download.lock().unwrap() = None;
    let second = rig.launcher();
    let view = second.wait(|v| v.step == Step::RoomIdle, SLOW);
    assert!(view.can_play, "{view:?}");
    // The first run's worker, freed now, changes nothing.
    rig.prov.release.store(true, SeqCst);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(second.view().step, Step::RoomIdle);
    assert_eq!(rig.state().step, Step::RoomIdle);
}

#[test]
fn the_preparers_scratch_is_deleted_after_a_good_prepare_of_the_game_only() {
    let rig = Rig::new("cleanup");
    let launcher = rig.launcher();
    launcher.press(Press::SignIn);
    launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
    // Once, after the game's prepare; the sounds' import has no scratch.
    assert_eq!(rig.prov.cleaned.load(SeqCst), 1);
    let calls = rig.calls();
    let cleanup = calls.iter().position(|c| c == "cleanup").unwrap();
    let prepare = calls.iter().position(|c| c == "prepare Core").unwrap();
    assert!(prepare < cleanup);
}

#[test]
fn the_sounds_follow_the_core_files_by_themselves_and_can_be_skipped_with_cancel() {
    let rig = Rig::new("sounds-skip");
    *rig.prov.block_download.lock().unwrap() = Some(Stage::Sounds);
    let launcher = rig.launcher();
    launcher.press(Press::SignIn);
    let view = launcher.wait(
        |v| v.step == Step::Download && v.stage == Stage::Sounds,
        SLOW,
    );
    assert!(view.download.is_some() || view.connecting);
    launcher.press(Press::CancelDownload);
    let view = launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
    // The game plays without sounds; the choice is remembered and the setting is off.
    assert!(view.can_play);
    assert!(!view.settings.sounds);
    assert!(!rig.state().settings.sounds);
}

#[test]
fn cancelling_the_game_download_is_an_error_with_retry() {
    let rig = Rig::new("core-cancel");
    *rig.prov.block_download.lock().unwrap() = Some(Stage::Core);
    let launcher = rig.launcher();
    launcher.press(Press::SignIn);
    launcher.wait(|v| v.step == Step::Download, SLOW);
    launcher.press(Press::CancelDownload);
    let view = launcher.wait(|v| v.error.is_some(), SLOW);
    assert_eq!(view.error.unwrap().kind, FailureKind::Cancelled);
    *rig.prov.block_download.lock().unwrap() = None;
    launcher.press(Press::Retry);
    assert!(launcher.wait(|v| v.step == Step::RoomIdle, SLOW).can_play);
}

#[test]
fn turning_the_sounds_on_later_downloads_them_and_off_does_nothing() {
    let rig = Rig::new("sounds-later");
    rig.prov.game.store(true, SeqCst);
    rig.signed_in(Step::RoomIdle);
    let mut p = rig.state();
    p.settings.sounds = false;
    std::fs::write(rig.dir.join("state.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    let launcher = rig.launcher();
    launcher.wait(idle, SLOW);
    assert!(!rig.calls().iter().any(|c| c.starts_with("download")));
    launcher.press(Press::SetSounds(true));
    launcher.wait(
        |v| v.step == Step::RoomIdle && v.settings.sounds && v.can_play,
        SLOW,
    );
    assert!(
        rig.calls().contains(&"download Sounds".to_owned()),
        "{:?}",
        rig.calls()
    );
    assert!(rig.prov.sounds.load(SeqCst));
    launcher.press(Press::SetSounds(false));
    assert_eq!(launcher.view().step, Step::RoomIdle);
    assert!(!rig.state().settings.sounds);
}

#[test]
fn sign_out_forgets_the_tokens_and_the_account_and_goes_to_no_account() {
    let rig = Rig::new("sign-out");
    rig.prov.game.store(true, SeqCst);
    rig.prov.sounds.store(true, SeqCst);
    rig.signed_in(Step::RoomIdle);
    let launcher = rig.launcher();
    launcher.wait(idle, SLOW);
    launcher.press(Press::SignOut);
    let view = launcher.view();
    assert_eq!(view.step, Step::NoAccount);
    assert_eq!(view.account, None);
    assert!(!view.can_play, "no Play without an owner");
    assert_eq!(
        rig.store
            .get(&format!("quest-account/{}", account().uuid))
            .unwrap(),
        None
    );
    assert_eq!(rig.state().account, None);
    assert_eq!(rig.state().step, Step::NoAccount);
    // Restarting stays signed out.
    drop(launcher);
    assert_eq!(rig.launcher().view().step, Step::NoAccount);
}

#[test]
fn a_build_without_a_client_id_says_so() {
    struct Unconfigured;
    impl Accounts for Unconfigured {
        fn configured(&self) -> bool {
            false
        }
        fn begin(&self) -> Result<DeviceCode, String> {
            unreachable!()
        }
        fn wait(&self, _: &DeviceCode, _: &dyn Fn() -> bool) -> Result<MsTokens, String> {
            unreachable!()
        }
        fn finish(&self, _: MsTokens, _: &dyn Fn(Phase)) -> Result<(Account, Secrets), String> {
            unreachable!()
        }
        fn verify(&self, _: &Account, _: Secrets, _: &dyn Fn(Phase)) -> Check {
            unreachable!()
        }
    }
    let rig = Rig::new("unconfigured");
    let launcher = Launcher::new(
        Config {
            dir: rig.dir.clone(),
            system_language: None,
            skip_gate: false,
        },
        Arc::new(Unconfigured),
        rig.prov.clone(),
        rig.store.clone(),
        Arc::new(NoNews),
    );
    launcher.press(Press::SignIn);
    let view = launcher.wait(|v| v.error.is_some(), SLOW);
    assert_eq!(view.error.unwrap().kind, FailureKind::NotConfigured);
}

#[cfg(not(feature = "dev-no-gate"))]
#[test]
fn a_normal_build_ignores_the_developers_skip_gate_switch() {
    let rig = Rig::new("no-gate-off");
    let launcher = Launcher::new(
        Config {
            dir: rig.dir.clone(),
            system_language: None,
            skip_gate: true,
        },
        rig.accounts.clone(),
        rig.prov.clone(),
        rig.store.clone(),
        Arc::new(NoNews),
    );
    assert_eq!(launcher.view().step, Step::NoAccount);
    std::thread::sleep(Duration::from_millis(100));
    assert!(rig.calls().is_empty());
}

#[cfg(feature = "dev-no-gate")]
#[test]
fn the_developers_switch_provisions_with_no_account() {
    let rig = Rig::new("no-gate-on");
    let launcher = Launcher::new(
        Config {
            dir: rig.dir.clone(),
            system_language: None,
            skip_gate: true,
        },
        rig.accounts.clone(),
        rig.prov.clone(),
        rig.store.clone(),
        Arc::new(NoNews),
    );
    let view = launcher.wait(|v| v.step == Step::RoomIdle, SLOW);
    assert!(view.can_play && view.account.is_none());
    assert!(!rig.calls().iter().any(|c| c == "begin" || c == "verify"));
}
