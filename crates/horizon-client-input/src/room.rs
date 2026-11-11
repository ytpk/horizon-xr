//! What the headset launcher's room (the TV in `game/quest_room.rs`) shows and what it can be
//! told, as plain data, so that the game needs nothing of the launcher: `horizon-quest` makes a
//! [`RoomHost`] out of its state machine and gives it to the game before it starts
//! (`embed_room`), and a developer's fixed host (`HORIZON_ROOM_STATE`) stands in on a computer.
use std::path::PathBuf;
use std::sync::Mutex;

/// Which page the TV is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    NoAccount,
    SignIn,
    /// The account is being asked (after a sign-in, and at every start).
    Checking,
    Download,
    Prepare,
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub user_code: String,
    pub url: String,
    /// The address the QR code carries.
    pub qr_url: String,
    pub expires_in: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub bytes_per_sec: f64,
    pub eta_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// `network`, `not_owner`, ...: the suffix of the headline's lang key.
    pub kind: String,
    /// The services' own words, English, shown beneath.
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct News {
    pub title: String,
    pub date: String,
    pub lines: Vec<String>,
}

/// What the TV draws.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomModel {
    pub page: Page,
    /// The browser identifies the default Live provider as Minecraft for Nintendo Switch.
    pub prismarine_live: bool,
    /// The second stage: the sounds.
    pub sounds_stage: bool,
    pub account: Option<String>,
    /// The face of the account's skin, 8 by 8 RGBA, when it is known.
    pub head: Option<Vec<u8>>,
    pub version: String,
    pub code: Option<Code>,
    /// The `step.*` key of the check that runs: `microsoft`, `xbox`, `minecraft`, `ownership`, `profile`.
    pub step: Option<String>,
    /// A request is out: "connecting", not a hang.
    pub connecting: bool,
    pub download: Option<Progress>,
    /// The preparer's last line and how far it is, 0..1.
    pub prepare: Option<(String, f32)>,
    pub error: Option<Failure>,
    pub news: Option<News>,
    pub sounds: bool,
    /// The headset's language code (`ru_ru`) and whether it is downloaded too.
    pub system_language: Option<String>,
    pub use_system_language: bool,
    pub can_play: bool,
    pub verifying: bool,
    pub offline: bool,
    /// Where the game's prepared files are, when they are: the room's textures come from them.
    pub resources: Option<PathBuf>,
}

impl Default for RoomModel {
    fn default() -> Self {
        Self {
            page: Page::NoAccount,
            prismarine_live: false,
            sounds_stage: false,
            account: None,
            head: None,
            version: String::new(),
            code: None,
            step: None,
            connecting: false,
            download: None,
            prepare: None,
            error: None,
            news: None,
            sounds: true,
            system_language: None,
            use_system_language: true,
            can_play: false,
            verifying: false,
            offline: false,
            resources: None,
        }
    }
}

/// What a press on the TV asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomAction {
    SignIn,
    CancelSignIn,
    Retry,
    CancelDownload,
    SignOut,
    Play,
    SetSounds(bool),
    SetSystemLanguage(bool),
}

/// The signed-in account's session for online play.
#[derive(Clone, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub uuid: String,
    pub token: String,
    pub xuid: Option<String>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session({}, <hidden>)", self.name)
    }
}

/// What Play hands over to the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    pub resources: PathBuf,
    /// None when the account has none now (offline, a token that ran out): the game is offline.
    pub session: Option<Session>,
}

/// The launcher behind the TV.
pub trait RoomHost: Send {
    fn model(&mut self) -> RoomModel;
    fn act(&mut self, action: RoomAction);
    /// The hand-off after Play, once.
    fn take_play(&mut self) -> Option<Handoff>;
}

static HOST: Mutex<Option<Box<dyn RoomHost>>> = Mutex::new(None);

/// The room's launcher; the game shows the room when it has one (and a headset).
pub fn set_host(host: Box<dyn RoomHost>) {
    *HOST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(host);
}

pub fn take_host() -> Option<Box<dyn RoomHost>> {
    HOST.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}
