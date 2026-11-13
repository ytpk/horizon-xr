//! The headset launcher's state machine (docs/quest-launcher-plan.md, phase 4).
//!
//! ```text
//! NoAccount -> LoginCode -> CheckOwnership -> Download(core) -> Prepare -> Download(sounds)
//!                                                -> Prepare(sounds) -> RoomIdle -> Play
//! ```
//!
//! Every step is persisted in `state.json` before it starts, so a killed app resumes where it
//! was: a step that was running runs again (downloads continue from their partial files, the
//! preparer skips what is made), a step that failed shows its error again with Retry. The
//! ownership gate comes first: nothing downloads until the Minecraft services said, in this very
//! run, that the account owns the game (a stored answer is enough only for what needs no network).
//!
//! The work runs on threads. The screen reads [`Launcher::view`], a snapshot, and presses buttons
//! with [`Launcher::press`]; it never waits. The account and the downloads are traits, so the
//! tests run the whole machine on mocks.
use crate::news::{self, News, NewsSource};
use horizon_account::{
    Account, Check, DOES_NOT_OWN, DeviceCode, MsTokens, NO_PROFILE, NOT_CONFIGURED, Phase,
    SIGN_IN_AGAIN, SecretStore, Secrets,
};
use horizon_fetch::{Progress, Snapshot};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Where the machine is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    NoAccount,
    /// The device code is on the TV and the player approves it on a phone.
    LoginCode,
    /// The Xbox, Minecraft and ownership checks (after a sign-in, and at every start).
    CheckOwnership,
    Download,
    Prepare,
    RoomIdle,
    /// The player pressed Play: the room hands over to the game (never written to disk).
    Play,
}

/// The two parts of the game's files: everything the game needs to start, then the sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    Core,
    Sounds,
}

/// What went wrong, for the headline the TV picks; the words are the services' own
/// (`Failure::message`), shown beneath it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureKind {
    Network,
    NotOwner,
    NoProfile,
    NotApproved,
    NotConfigured,
    XboxNoProfile,
    XboxRegion,
    XboxAdult,
    XboxChild,
    CodeExpired,
    Declined,
    SignInExpired,
    NoSpace,
    Checksum,
    Cancelled,
    JvmFailed,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
}

impl Failure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// The account services' English messages, told apart (the strings are `horizon-account`'s).
pub fn classify_account(message: &str) -> Failure {
    let kind = match message {
        m if m == DOES_NOT_OWN => FailureKind::NotOwner,
        m if m == NO_PROFILE => FailureKind::NoProfile,
        m if m == SIGN_IN_AGAIN => FailureKind::SignInExpired,
        m if m == NOT_CONFIGURED => FailureKind::NotConfigured,
        m if m.starts_with("Could not reach") || m.starts_with("No connection") => {
            FailureKind::Network
        }
        m if m.contains("not approved by Mojang") => FailureKind::NotApproved,
        m if m.contains("no Xbox profile") => FailureKind::XboxNoProfile,
        m if m.contains("country or region") => FailureKind::XboxRegion,
        m if m.contains("adult verification") => FailureKind::XboxAdult,
        m if m.contains("child account") => FailureKind::XboxChild,
        m if m.contains("code expired") => FailureKind::CodeExpired,
        m if m.contains("declined") => FailureKind::Declined,
        _ => FailureKind::Other,
    };
    Failure::new(kind, message)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Download the sounds (359 MB) after the game's files.
    pub sounds: bool,
    /// Download the headset's own language too (English is always there).
    pub system_language: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sounds: true,
            system_language: true,
        }
    }
}

/// What `state.json` holds: no secret, only where the machine is and who is signed in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Persisted {
    pub version: u32,
    pub step: Step,
    pub stage: Stage,
    pub account: Option<Account>,
    pub error: Option<Failure>,
    pub settings: Settings,
}

const STATE_VERSION: u32 = 1;

impl Default for Persisted {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            step: Step::NoAccount,
            stage: Stage::Core,
            account: None,
            error: None,
            settings: Settings::default(),
        }
    }
}

/// What is on disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Readiness {
    /// The prepared game files are there (`horizon-resources.json`) with the chosen languages.
    pub game: bool,
    /// The sounds are prepared.
    pub sounds: bool,
    /// The core files are downloaded (a preparer can start without the network).
    pub core_downloaded: bool,
    pub sounds_downloaded: bool,
}

/// The downloads and the preparer, behind a trait (`real::QuestProvisioner` on the headset).
pub trait Provisioner: Send + Sync {
    /// The game version shown on the TV.
    fn version(&self) -> String;
    fn readiness(&self, languages: &[String]) -> Readiness;
    /// Where the game reads its prepared files.
    fn resources(&self) -> PathBuf;
    fn download(
        &self,
        stage: Stage,
        languages: &[String],
        progress: &Progress,
        cancel: &AtomicBool,
        log: &dyn Fn(String),
    ) -> Result<(), Failure>;
    fn prepare(
        &self,
        stage: Stage,
        languages: &[String],
        log: &dyn Fn(&str),
        progress: &dyn Fn(f32),
    ) -> Result<(), Failure>;
    /// After a good prepare of the game: deletes the preparer's scratch (about 409 MB).
    fn cleanup(&self);
    /// Forgets that the sounds were downloaded, so the next step checks them and fetches what
    /// is missing.
    fn forget_sounds_download(&self);
}

/// The Microsoft account flow, behind a trait (`real::QuestAccounts`).
pub trait Accounts: Send + Sync {
    /// Whether this build has a client ID.
    fn configured(&self) -> bool;
    fn begin(&self) -> Result<DeviceCode, String>;
    /// Waits for the player to approve `code` in a browser; `cancelled` says stop.
    fn wait(&self, code: &DeviceCode, cancelled: &dyn Fn() -> bool) -> Result<MsTokens, String>;
    /// Xbox, XSTS, Minecraft, ownership and profile; an account that does not own the game is
    /// refused with `DOES_NOT_OWN`.
    fn finish(&self, ms: MsTokens, phase: &dyn Fn(Phase)) -> Result<(Account, Secrets), String>;
    /// Asks again whether `account` owns the game, renewing its tokens when they are due.
    fn verify(&self, account: &Account, secrets: Secrets, phase: &dyn Fn(Phase)) -> Check;
}

/// The signed-in account's session, for the game's online play.
#[derive(Clone, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub uuid: String,
    pub token: String,
    pub xuid: Option<String>,
    pub expires_at: u64,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session({}, <hidden>)", self.name)
    }
}

/// What Play hands to the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayRequest {
    pub resources: PathBuf,
    /// None offline, or when the stored session ran out and could not be renewed.
    pub session: Option<Session>,
}

/// What the player can press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    SignIn,
    CancelSignIn,
    /// Run the failed step again.
    Retry,
    /// Stops a download; the sounds' stage is skipped (and turned off), the game's is an error
    /// with Retry.
    CancelDownload,
    SignOut,
    Play,
    SetSounds(bool),
    SetSystemLanguage(bool),
}

/// Where the machine keeps its files and what it was told about the headset.
#[derive(Clone, Debug)]
pub struct Config {
    /// `<files>/launcher`: `state.json`, `news.json`.
    pub dir: PathBuf,
    /// The headset's language as a game code (`ru_ru`), if known.
    pub system_language: Option<String>,
    /// Developer builds only (feature `dev-no-gate`): skip the ownership gate.
    pub skip_gate: bool,
}

/// What the Minecraft services last said about the account in this run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    /// Not asked yet.
    Unknown,
    /// Owns the game, said in this run: downloads may start.
    Passed,
    /// No answer (offline): the last answer, kept with the tokens, stands for what needs no
    /// network.
    LastAnswer,
    /// Said it does not.
    Denied,
}

#[derive(Default)]
struct Live {
    code: Option<DeviceCode>,
    phase: Option<Phase>,
    /// A request is out and has not answered yet (the first lookup of an Azure host takes 5 s).
    connecting: bool,
    preparing: bool,
    prepare_line: String,
    prepare_fraction: f32,
    session: Option<Session>,
    news: Option<News>,
    play: Option<PlayRequest>,
}

struct Core {
    p: Persisted,
    live: Live,
    gate: Gate,
}

struct Shared {
    cfg: Config,
    accounts: Arc<dyn Accounts>,
    prov: Arc<dyn Provisioner>,
    store: Arc<dyn SecretStore>,
    core: Mutex<Core>,
    changed: Condvar,
    /// Counts the starts of a step: a worker whose number is out of date was cancelled or
    /// replaced and drops its result.
    generation: AtomicU64,
    dead: AtomicBool,
    progress: Progress,
    cancel_download: AtomicBool,
}

/// The machine. Dropping it stops its workers from changing anything.
pub struct Launcher {
    shared: Arc<Shared>,
}

impl Drop for Launcher {
    fn drop(&mut self) {
        self.shared.dead.store(true, SeqCst);
        self.shared.cancel_download.store(true, SeqCst);
    }
}

/// What the screen draws, a snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub step: Step,
    pub stage: Stage,
    pub version: String,
    pub account: Option<Account>,
    pub error: Option<Failure>,
    pub code: Option<CodeView>,
    pub phase: Option<Phase>,
    /// A request is out: "connecting", not a hang.
    pub connecting: bool,
    pub download: Option<Snapshot>,
    pub prepare: Option<PrepareView>,
    pub news: Option<News>,
    pub settings: Settings,
    /// The headset's language (`ru_ru`), for the Settings page.
    pub system_language: Option<String>,
    pub can_play: bool,
    /// The account is being asked again (at the start of the room).
    pub verifying: bool,
    /// The last answer was kept, not asked: the headset is offline.
    pub offline_answer: bool,
    /// Where the game's prepared files are, once they are (the room's textures come from them).
    pub resources: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeView {
    pub user_code: String,
    pub url: String,
    /// The address the QR code carries: the page with the code filled in.
    pub qr_url: String,
    pub expires_in: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrepareView {
    /// The preparer's last line.
    pub line: String,
    pub fraction: f32,
}

fn secret_key(account: &Account) -> String {
    format!("quest-account/{}", account.uuid)
}

impl Core {
    fn languages(&self, cfg: &Config) -> Vec<String> {
        let mut languages = vec!["en_us".to_owned()];
        if self.p.settings.system_language
            && let Some(code) = &cfg.system_language
            && !languages.contains(code)
        {
            languages.push(code.clone());
        }
        languages
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Core> {
        self.core.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn gate_skipped(&self) -> bool {
        cfg!(feature = "dev-no-gate") && self.cfg.skip_gate
    }

    fn state_path(&self) -> PathBuf {
        self.cfg.dir.join("state.json")
    }

    /// Writes `state.json` whole (a crash leaves the old file or the new one). Play is never
    /// written: after a game the room comes back idle.
    fn save(&self, core: &Core) {
        let mut p = core.p.clone();
        if p.step == Step::Play {
            p.step = Step::RoomIdle;
        }
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(&self.cfg.dir)?;
            let tmp = self.cfg.dir.join("state.json.tmp");
            std::fs::write(
                &tmp,
                serde_json::to_vec_pretty(&p).map_err(std::io::Error::other)?,
            )?;
            std::fs::rename(&tmp, self.state_path())
        };
        if let Err(e) = write() {
            log::warn!("Horizon room: state.json: {e}");
        }
    }

    /// Runs `f` on the machine if the worker `epoch` is still the current one.
    fn with<R>(&self, epoch: u64, f: impl FnOnce(&mut Core) -> R) -> Option<R> {
        let mut core = self.lock();
        if self.dead.load(SeqCst) || self.generation.load(SeqCst) != epoch {
            return None;
        }
        let r = f(&mut core);
        self.changed.notify_all();
        Some(r)
    }

    fn current(&self, epoch: u64) -> bool {
        !self.dead.load(SeqCst) && self.generation.load(SeqCst) == epoch
    }

    fn languages(&self, core: &Core) -> Vec<String> {
        core.languages(&self.cfg)
    }

    /// Where the files stand: the next step after the account is checked.
    fn plan(&self, core: &Core) -> (Step, Stage) {
        let ready = self.prov.readiness(&self.languages(core));
        if !ready.game {
            return if ready.core_downloaded {
                (Step::Prepare, Stage::Core)
            } else {
                (Step::Download, Stage::Core)
            };
        }
        if core.p.settings.sounds && !ready.sounds {
            return if ready.sounds_downloaded {
                (Step::Prepare, Stage::Sounds)
            } else {
                (Step::Download, Stage::Sounds)
            };
        }
        (Step::RoomIdle, Stage::Core)
    }

    /// Moves to `step` and starts its worker. The lock is held.
    fn advance(self: &Arc<Self>, core: &mut Core, step: Step, stage: Stage) {
        log::info!("Horizon room: {step:?} ({stage:?})");
        core.p.step = step;
        core.p.stage = stage;
        core.p.error = None;
        core.live.phase = None;
        core.live.connecting = false;
        core.live.preparing = false;
        self.save(core);
        self.start(core);
    }

    /// Takes the plan's next step, unless it downloads and the gate has not passed in this run:
    /// then the account is checked first.
    fn advance_planned(self: &Arc<Self>, core: &mut Core) {
        let (step, stage) = self.plan(core);
        let needs_network = step == Step::Download;
        if needs_network && core.gate != Gate::Passed && !self.gate_skipped() {
            return self.advance(core, Step::CheckOwnership, Stage::Core);
        }
        self.advance(core, step, stage);
    }

    fn fail(self: &Arc<Self>, core: &mut Core, step: Step, failure: Failure) {
        self.generation.fetch_add(1, SeqCst);
        // The services' own words: never a token or a code (docs/microsoft-account.md).
        log::warn!(
            "Horizon room: {step:?} failed: {:?}: {}",
            failure.kind,
            failure.message
        );
        core.p.step = step;
        core.p.error = Some(failure);
        core.live.phase = None;
        core.live.connecting = false;
        core.live.preparing = false;
        self.save(core);
    }

    /// Bumps the generation and starts the worker of the current step. The lock is held.
    fn start(self: &Arc<Self>, core: &mut Core) {
        let epoch = self.generation.fetch_add(1, SeqCst) + 1;
        let me = self.clone();
        match core.p.step {
            Step::NoAccount | Step::RoomIdle | Step::Play => {}
            Step::LoginCode => {
                core.live.code = None;
                core.live.connecting = true;
                spawn("room sign-in", move || me.sign_in(epoch));
            }
            Step::CheckOwnership => {
                core.live.connecting = true;
                spawn("room verify", move || me.verify(epoch));
            }
            Step::Download => {
                // The gate: no download unless the services said yes in this run.
                if core.gate != Gate::Passed && !self.gate_skipped() {
                    core.p.step = Step::CheckOwnership;
                    self.save(core);
                    return self.start(core);
                }
                core.live.connecting = true;
                self.cancel_download.store(false, SeqCst);
                self.progress.end();
                let stage = core.p.stage;
                spawn("room download", move || me.download(epoch, stage));
            }
            Step::Prepare => {
                if !matches!(core.gate, Gate::Passed | Gate::LastAnswer) && !self.gate_skipped() {
                    core.p.step = Step::CheckOwnership;
                    self.save(core);
                    return self.start(core);
                }
                core.live.preparing = true;
                core.live.prepare_line.clear();
                core.live.prepare_fraction = 0.0;
                let stage = core.p.stage;
                spawn("room prepare", move || me.prepare(epoch, stage));
            }
        }
    }

    // ----- Workers -------------------------------------------------------------------------

    fn sign_in(self: &Arc<Self>, epoch: u64) {
        if !self.accounts.configured() {
            self.with(epoch, |core| {
                let failure = classify_account(NOT_CONFIGURED);
                self.fail(core, Step::LoginCode, failure);
            });
            return;
        }
        let code = match self.accounts.begin() {
            Ok(code) => code,
            Err(e) => {
                self.with(epoch, |core| {
                    self.fail(core, Step::LoginCode, classify_account(&e))
                });
                return;
            }
        };
        if self
            .with(epoch, |core| {
                core.live.code = Some(code.clone());
                core.live.connecting = false;
            })
            .is_none()
        {
            return;
        }
        let ms = match self.accounts.wait(&code, &|| !self.current(epoch)) {
            Ok(ms) => ms,
            Err(e) => {
                self.with(epoch, |core| {
                    self.fail(core, Step::LoginCode, classify_account(&e))
                });
                return;
            }
        };
        // Approved: the same worker goes on through the checks.
        if self
            .with(epoch, |core| {
                core.p.step = Step::CheckOwnership;
                core.live.code = None;
                core.live.connecting = true;
                self.save(core);
            })
            .is_none()
        {
            return;
        }
        let phase = |p: Phase| {
            self.with(epoch, |core| {
                core.live.phase = Some(p);
                core.live.connecting = false;
            });
        };
        match self.accounts.finish(ms, &phase) {
            Ok((account, secrets)) => {
                if let Err(e) = self.keep(&account, &secrets) {
                    self.with(epoch, |core| {
                        self.fail(
                            core,
                            Step::CheckOwnership,
                            Failure::new(FailureKind::Other, e),
                        )
                    });
                    return;
                }
                self.with(epoch, |core| {
                    core.gate = Gate::Passed;
                    core.live.session = session_of(&account, &secrets);
                    core.p.account = Some(account);
                    self.advance_planned(core);
                });
            }
            Err(e) => {
                self.with(epoch, |core| {
                    core.gate = Gate::Denied;
                    self.fail(core, Step::CheckOwnership, classify_account(&e))
                });
            }
        }
    }

    fn keep(&self, account: &Account, secrets: &Secrets) -> Result<(), String> {
        let json = serde_json::to_string(secrets).map_err(|e| e.to_string())?;
        self.store.set(&secret_key(account), &json)
    }

    fn secrets_of(&self, account: &Account) -> Option<Secrets> {
        let json = self.store.get(&secret_key(account)).ok()??;
        serde_json::from_str(&json).ok()
    }

    /// The account is asked again (after a sign-in's tokens are already checked, at every start):
    /// renewed tokens, ownership, profile.
    fn verify(self: &Arc<Self>, epoch: u64) {
        let account = self.lock().p.account.clone();
        let Some(account) = account else {
            // No account to ask about: a developer build skips the gate, any other starts the
            // sign-in (a state file that says "download" without an account is not believed).
            self.with(epoch, |core| {
                if self.gate_skipped() {
                    core.gate = Gate::Passed;
                    self.advance_planned(core);
                } else {
                    self.advance(core, Step::LoginCode, Stage::Core);
                }
            });
            return;
        };
        let Some(secrets) = self.secrets_of(&account) else {
            self.with(epoch, |core| {
                core.p.account = None;
                core.gate = Gate::Unknown;
                self.fail(core, Step::NoAccount, classify_account(SIGN_IN_AGAIN));
            });
            return;
        };
        let phase = |p: Phase| {
            self.with(epoch, |core| {
                core.live.phase = Some(p);
                core.live.connecting = false;
            });
        };
        match self.accounts.verify(&account, secrets, &phase) {
            Check::Owns(profile, secrets) => {
                let _ = self.keep(&profile, &secrets);
                self.with(epoch, |core| {
                    core.gate = Gate::Passed;
                    core.live.session = session_of(&profile, &secrets);
                    core.p.account = Some(profile);
                    self.advance_planned(core);
                });
            }
            Check::DoesNot(secrets) => {
                let _ = self.keep(&account, &secrets);
                self.with(epoch, |core| {
                    core.gate = Gate::Denied;
                    core.live.session = None;
                    self.fail(core, Step::CheckOwnership, classify_account(DOES_NOT_OWN));
                });
            }
            Check::NoAnswer(message, secrets) => {
                let _ = self.keep(&account, &secrets);
                self.with(epoch, |core| {
                    if secrets.sign_in_required {
                        let _ = self.store.delete(&secret_key(&account));
                        core.p.account = None;
                        core.gate = Gate::Unknown;
                        core.live.session = None;
                        self.fail(core, Step::NoAccount, classify_account(&message));
                        return;
                    }
                    if !secrets.owns {
                        core.gate = Gate::Denied;
                        self.fail(core, Step::CheckOwnership, classify_account(DOES_NOT_OWN));
                        return;
                    }
                    // Offline, or the services did not answer: the last answer stands for what
                    // needs no network. A download waits for a real answer.
                    core.gate = Gate::LastAnswer;
                    core.live.session = session_of(&account, &secrets);
                    let (step, stage) = self.plan(core);
                    if step == Step::Download {
                        self.fail(core, Step::CheckOwnership, classify_account(&message));
                    } else {
                        self.advance(core, step, stage);
                    }
                });
            }
        }
    }

    fn download(self: &Arc<Self>, epoch: u64, stage: Stage) {
        let languages = self.with(epoch, |core| self.languages(core));
        let Some(languages) = languages else { return };
        // The downloader's "N of M files" every second would fill the log: one line in ten.
        let lines = std::sync::atomic::AtomicUsize::new(0);
        let log = |line: String| {
            if !line.contains(" of ") || lines.fetch_add(1, SeqCst).is_multiple_of(10) {
                log::info!("Horizon room: {line}");
            }
        };
        let result = self.prov.download(
            stage,
            &languages,
            &self.progress,
            &self.cancel_download,
            &log,
        );
        self.progress.end();
        match result {
            Ok(()) => {
                self.with(epoch, |core| {
                    self.advance(core, Step::Prepare, stage);
                });
            }
            Err(f) if f.kind == FailureKind::Cancelled && stage == Stage::Sounds => {
                // Skipping the sounds: the game is ready without them.
                self.with(epoch, |core| {
                    core.p.settings.sounds = false;
                    self.advance_planned(core);
                });
            }
            Err(f) => {
                self.with(epoch, |core| self.fail(core, Step::Download, f));
            }
        }
    }

    fn prepare(self: &Arc<Self>, epoch: u64, stage: Stage) {
        let languages = self.with(epoch, |core| self.languages(core));
        let Some(languages) = languages else { return };
        let line = |text: &str| {
            self.with(epoch, |core| core.live.prepare_line = text.to_owned());
        };
        let fraction = |f: f32| {
            self.with(epoch, |core| core.live.prepare_fraction = f);
        };
        let result = self.prov.prepare(stage, &languages, &line, &fraction);
        match result {
            Ok(()) => {
                if stage == Stage::Core {
                    self.prov.cleanup();
                }
                self.with(epoch, |core| self.advance_planned(core));
            }
            Err(f) => {
                self.with(epoch, |core| self.fail(core, Step::Prepare, f));
            }
        }
    }

    fn press(self: &Arc<Self>, press: Press) {
        let mut core = self.lock();
        if self.dead.load(SeqCst) {
            return;
        }
        let step = core.p.step;
        match press {
            Press::SignIn if step == Step::NoAccount => {
                self.advance(&mut core, Step::LoginCode, Stage::Core)
            }
            Press::CancelSignIn if matches!(step, Step::LoginCode) => {
                self.advance(&mut core, Step::NoAccount, Stage::Core)
            }
            Press::Retry if core.p.error.is_some() => {
                let (retry_step, stage) = (step, core.p.stage);
                match retry_step {
                    // The code ran out or the sign-in failed after it: a new code.
                    Step::NoAccount | Step::LoginCode => {
                        self.advance(&mut core, Step::LoginCode, Stage::Core)
                    }
                    Step::CheckOwnership if core.p.account.is_none() && !self.gate_skipped() => {
                        self.advance(&mut core, Step::LoginCode, Stage::Core)
                    }
                    // Adding the sounds found one missing: copying again cannot bring it back.
                    Step::Prepare if stage == Stage::Sounds => {
                        self.prov.forget_sounds_download();
                        self.advance_planned(&mut core);
                    }
                    other => self.advance(&mut core, other, stage),
                }
            }
            Press::CancelDownload if step == Step::Download && core.p.error.is_none() => {
                self.cancel_download.store(true, SeqCst);
            }
            Press::SignOut => {
                if let Some(account) = core.p.account.take() {
                    let _ = self.store.delete(&secret_key(&account));
                }
                core.gate = Gate::Unknown;
                core.live.session = None;
                self.advance(&mut core, Step::NoAccount, Stage::Core);
            }
            Press::Play if step == Step::RoomIdle && self.can_play(&core) => {
                core.live.play = Some(PlayRequest {
                    resources: self.prov.resources(),
                    session: core.live.session.clone(),
                });
                core.p.step = Step::Play;
                self.generation.fetch_add(1, SeqCst);
                self.save(&core);
            }
            Press::SetSounds(on) => {
                core.p.settings.sounds = on;
                self.save(&core);
                // The game is ready without the sounds, so turning them off leaves their error.
                let leaves_sounds_error = !on
                    && core.p.error.is_some()
                    && core.p.stage == Stage::Sounds
                    && matches!(step, Step::Download | Step::Prepare);
                if (step == Step::RoomIdle && core.p.error.is_none()) || leaves_sounds_error {
                    self.advance_planned(&mut core);
                }
            }
            Press::SetSystemLanguage(on) => {
                core.p.settings.system_language = on;
                self.save(&core);
                if step == Step::RoomIdle && core.p.error.is_none() {
                    self.advance_planned(&mut core);
                }
            }
            _ => {}
        }
        self.changed.notify_all();
    }

    fn can_play(&self, core: &Core) -> bool {
        let gate = self.gate_skipped() || matches!(core.gate, Gate::Passed | Gate::LastAnswer);
        core.p.step == Step::RoomIdle
            && core.p.error.is_none()
            && gate
            && self.prov.readiness(&self.languages(core)).game
    }

    fn view(&self) -> View {
        let core = self.lock();
        let step = core.p.step;
        let code = core.live.code.as_ref().map(|c| CodeView {
            user_code: c.user_code.clone(),
            url: c.verification_uri.clone(),
            qr_url: format!(
                "{}{}otc={}",
                c.verification_uri,
                if c.verification_uri.contains('?') {
                    '&'
                } else {
                    '?'
                },
                c.user_code
            ),
            expires_in: c.expires_at.saturating_sub(horizon_account::now()),
        });
        let download = self.progress.snapshot();
        let verifying = step == Step::CheckOwnership && core.p.error.is_none();
        View {
            step,
            stage: core.p.stage,
            version: self.prov.version(),
            account: core.p.account.clone(),
            error: core.p.error.clone(),
            code,
            phase: core.live.phase,
            connecting: core.live.connecting && download.is_none() && core.live.phase.is_none(),
            download: download.filter(|_| step == Step::Download),
            prepare: (step == Step::Prepare && core.live.preparing).then(|| PrepareView {
                line: core.live.prepare_line.clone(),
                fraction: core.live.prepare_fraction,
            }),
            news: core.live.news.clone(),
            settings: core.p.settings,
            system_language: self.cfg.system_language.clone(),
            can_play: self.can_play(&core),
            verifying,
            offline_answer: core.gate == Gate::LastAnswer,
            resources: self
                .prov
                .readiness(&self.languages(&core))
                .game
                .then(|| self.prov.resources()),
        }
    }
}

fn session_of(account: &Account, secrets: &Secrets) -> Option<Session> {
    (secrets.owns
        && !secrets.sign_in_required
        && secrets.minecraft_expires_at > horizon_account::now() + 60)
        .then(|| Session {
            name: account.name.clone(),
            uuid: account.uuid.clone(),
            token: secrets.minecraft_token.clone(),
            xuid: secrets.xuid.clone(),
            expires_at: secrets.minecraft_expires_at,
        })
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
                log::error!("Horizon room: a worker panicked");
            }
        });
    if let Err(e) = spawned {
        log::error!("Horizon room: could not start a worker: {e}");
    }
}

impl Launcher {
    /// Reads `state.json` and resumes: a step that was running runs again, a failure shows
    /// again, and an account is asked about again before anything else.
    pub fn new(
        cfg: Config,
        accounts: Arc<dyn Accounts>,
        prov: Arc<dyn Provisioner>,
        store: Arc<dyn SecretStore>,
        source: Arc<dyn NewsSource>,
    ) -> Self {
        let mut p: Persisted = std::fs::read(cfg.dir.join("state.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .filter(|p: &Persisted| p.version == STATE_VERSION)
            .unwrap_or_default();
        // A state file that says "download" with no account is not believed (only a developer
        // build without the gate may go on without one).
        let skip = cfg!(feature = "dev-no-gate") && cfg.skip_gate;
        let resume = |step: Step| {
            matches!(
                step,
                Step::CheckOwnership | Step::Download | Step::Prepare | Step::RoomIdle | Step::Play
            )
        };
        if p.step == Step::Play {
            p.step = Step::RoomIdle;
        }
        if p.account.is_none() && resume(p.step) && !skip {
            p.step = Step::NoAccount;
            p.error = None;
        }
        if p.account.is_some() && resume(p.step) && p.error.is_none() {
            // The account is asked again at every start, whatever the file says.
            p.step = Step::CheckOwnership;
        }
        if skip
            && p.account.is_none()
            && matches!(p.step, Step::NoAccount | Step::LoginCode)
            && p.error.is_none()
        {
            p.step = Step::CheckOwnership;
        }
        if p.step == Step::LoginCode && p.error.is_none() {
            // The device code was never written down: a new one.
        }
        let shared = Arc::new(Shared {
            cfg,
            accounts,
            prov,
            store,
            core: Mutex::new(Core {
                p,
                live: Live::default(),
                gate: Gate::Unknown,
            }),
            changed: Condvar::new(),
            generation: AtomicU64::new(0),
            dead: AtomicBool::new(false),
            progress: Progress::new(),
            cancel_download: AtomicBool::new(false),
        });
        {
            let mut core = shared.lock();
            // A stored answer is the gate's start for an account that owned the game.
            if let Some(account) = core.p.account.clone()
                && shared.secrets_of(&account).is_some_and(|s| s.owns)
            {
                core.gate = Gate::LastAnswer;
            }
            if core.p.error.is_none() {
                shared.save(&core);
                shared.start(&mut core);
            } else {
                // A failure shown again, with its Retry.
                shared.generation.fetch_add(1, SeqCst);
            }
        }
        let launcher = Launcher { shared };
        launcher.load_news(source);
        launcher
    }

    fn load_news(&self, source: Arc<dyn NewsSource>) {
        let shared = self.shared.clone();
        spawn("room news", move || {
            let cache = shared.cfg.dir.join("news.json");
            let _ = std::fs::create_dir_all(&shared.cfg.dir);
            if let Ok(news) = news::load(source.as_ref(), &cache)
                && !shared.dead.load(SeqCst)
            {
                shared.lock().live.news = Some(news);
                shared.changed.notify_all();
            }
        });
    }

    pub fn view(&self) -> View {
        self.shared.view()
    }

    pub fn press(&self, press: Press) {
        self.shared.press(press);
    }

    /// The hand-off to the game, once, after Play.
    pub fn take_play(&self) -> Option<PlayRequest> {
        self.shared.lock().live.play.take()
    }

    /// Blocks until `done(view)` or `timeout` (for tests and tools); the last view either way.
    pub fn wait(&self, done: impl Fn(&View) -> bool, timeout: Duration) -> View {
        let until = Instant::now() + timeout;
        loop {
            let view = self.view();
            if done(&view) || Instant::now() >= until {
                return view;
            }
            let core = self.shared.lock();
            let _ = self
                .shared
                .changed
                .wait_timeout(core, Duration::from_millis(25))
                .map(|(guard, _)| drop(guard));
        }
    }

    /// The folder `state.json` is in.
    pub fn dir(&self) -> &Path {
        &self.shared.cfg.dir
    }
}
