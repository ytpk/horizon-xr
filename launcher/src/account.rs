//! Microsoft account sign-in (docs/microsoft-account.md): the OAuth device-code flow, then Xbox
//! Live, XSTS and the Minecraft services, the game ownership check and the profile. Secrets (the
//! Microsoft refresh token, the Minecraft access token, the last ownership answer) live in the OS
//! keychain, or in a file only the player can read where there is none; accounts.json keeps only
//! names and UUIDs. The game gets the session in a file it
//! reads once (`--session-file`), never on its command line. Tokens never reach logs, errors or
//! output.
//!
//! The owner gate ([`launch`]): the launcher starts the game only while an account in it owns
//! Minecraft: Java Edition, as other launchers do, and the game plays as the selected account.
//! The game binary itself, its harnesses and the servers don't go through the launcher's gate.
//!
//! Sign-in configuration is shared with the Quest account crate: an unset client ID selects
//! experimental Live sign-in; a custom `HORIZON_MSA_CLIENT_ID` selects Azure, and empty disables it.
pub mod session;

use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The keychain service name every secret is stored under.
pub const SERVICE: &str = "dev.playhorizon.launcher";

// The sign-in flow, the secret store trait and the secrets file live in `horizon-account`, which
// the Quest uses too; the launcher adds its keychains, accounts.json and the game hand-off below.
pub use horizon_account::*;
#[cfg(test)]
use horizon_account::{file_key as file_key_v1, SEALED_V2 as SEALED};

/// Registers the launcher's user agent and proxy with the shared sign-in code. Call once at start.
pub fn init_transport() {
    set_transport(Transport {
        user_agent: format!("horizon-launcher/{}", crate::backend::VERSION),
        proxy: Some(crate::proxy::agent),
    });
}

/// The launcher's own secrets file: [`SECRETS_FILE`] in its data folder.
pub fn launcher_file_store() -> FileStore {
    FileStore::new(crate::backend::data_dir().join(SECRETS_FILE))
}

/// How long a call to the system keyring may take before the launcher gives up on it (#12179). An
/// unlock prompt the player is typing a password into counts as an answer still coming, so this is
/// longer than a working keyring ever takes, short enough that Play says what is wrong.
pub const KEYCHAIN_WAIT: Duration = Duration::from_secs(20);

/// What the launcher says when the system keyring didn't answer within `wait`.
pub fn keyring_no_answer(wait: Duration) -> String {
    crate::i18n::trf(
        "Your system keyring didn't answer within {seconds} seconds. Make sure it is running and unlocked, then restart the launcher.",
        &[("seconds", &wait.as_secs().max(1).to_string())],
    )
}

/// One call to a [`Bounded`] keyring: its answer, or that the caller gave up on it.
struct Call<T> {
    answer: Option<T>,
    gave_up: bool,
}

/// A keyring whose calls give up after `wait` (#12179): a Secret Service that never answers (a
/// stuck KWallet or GNOME Keyring daemon) otherwise holds the account lock forever, and Play stays
/// on "Starting…" with nothing said. Each call runs on a thread of its own; one given up on keeps
/// running there, and while it does, later calls fail at once instead of piling up behind it.
pub struct Bounded {
    keyring: std::sync::Arc<dyn SecretStore>,
    wait: Duration,
    /// Calls given up on that haven't come back yet.
    stuck: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Bounded {
    pub fn new(keyring: impl SecretStore + 'static, wait: Duration) -> Self {
        Self {
            keyring: std::sync::Arc::new(keyring),
            wait,
            stuck: Default::default(),
        }
    }

    fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&dyn SecretStore) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        use std::sync::atomic::Ordering::SeqCst;
        let no_answer = || {
            let said = keyring_no_answer(self.wait);
            eprintln!("Horizon: {said}");
            Err(said)
        };
        if self.stuck.load(SeqCst) > 0 {
            return no_answer();
        }
        let call = std::sync::Arc::new((
            Mutex::new(Call::<Result<T, String>> {
                answer: None,
                gave_up: false,
            }),
            std::sync::Condvar::new(),
        ));
        let (keyring, stuck, theirs) = (self.keyring.clone(), self.stuck.clone(), call.clone());
        let spawned = std::thread::Builder::new()
            .name("keyring".into())
            .spawn(move || {
                let answer = f(keyring.as_ref());
                let (slot, done) = &*theirs;
                let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
                if slot.gave_up {
                    stuck.fetch_sub(1, SeqCst);
                } else {
                    slot.answer = Some(answer);
                    done.notify_one();
                }
            });
        if let Err(e) = spawned {
            return Err(e.to_string());
        }
        let (slot, done) = &*call;
        let slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        let (mut slot, _) = done
            .wait_timeout_while(slot, self.wait, |call| call.answer.is_none())
            .unwrap_or_else(|e| e.into_inner());
        if let Some(answer) = slot.answer.take() {
            return answer;
        }
        // Under the call's lock: the thread, when it ends, sees this and counts itself back.
        slot.gave_up = true;
        self.stuck.fetch_add(1, SeqCst);
        drop(slot);
        no_answer()
    }
}

impl SecretStore for Bounded {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let key = key.to_string();
        self.call(move |keyring| keyring.get(&key))
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        let (key, secret) = (key.to_string(), secret.to_string());
        self.call(move |keyring| keyring.set(&key, &secret))
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        let key = key.to_string();
        self.call(move |keyring| keyring.delete(&key))
    }
    fn persistent(&self) -> bool {
        self.keyring.persistent()
    }
    fn lock_file(&self) -> Option<PathBuf> {
        self.keyring.lock_file()
    }
    fn operations(&self) -> &Mutex<()> {
        self.keyring.operations()
    }
}

/// The Secret Service (GNOME Keyring, KWallet) through oo7, as gpui itself uses it.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub struct SystemStore;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl SystemStore {
    fn keyring() -> Result<oo7::Keyring, String> {
        futures_lite::future::block_on(async {
            // `oo7::Keyring::new` takes `GTK_USE_PORTAL=1` (set by many KDE desktops) for a
            // sandbox, and then keeps the secrets in a file under a key the Secret portal gives
            // per app id. Outside a sandbox that id comes from how the launcher was started (the
            // menu or a terminal), so the file opened in one start and not the next (#3249).
            // Flatpak (`/.flatpak-info`) and Snap (`$SNAP`) keep the portal's file.
            let sandboxed =
                Path::new("/.flatpak-info").exists() || std::env::var_os("SNAP").is_some();
            let keyring = if sandboxed {
                oo7::Keyring::new().await.map_err(|e| e.to_string())?
            } else {
                let service = oo7::dbus::Service::new().await.map_err(|e| e.to_string())?;
                oo7::Keyring::DBus(
                    service
                        .default_collection()
                        .await
                        .map_err(|e| e.to_string())?,
                )
            };
            keyring.unlock().await.map_err(|e| e.to_string())?;
            Ok(keyring)
        })
    }
}
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl SecretStore for SystemStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let keyring = Self::keyring()?;
        futures_lite::future::block_on(async {
            let attributes = vec![("service", SERVICE), ("account", key)];
            let items = keyring
                .search_items(&attributes)
                .await
                .map_err(|e| e.to_string())?;
            let Some(item) = items.into_iter().next() else {
                return Ok(None);
            };
            item.unlock().await.map_err(|e| e.to_string())?;
            let secret = item.secret().await.map_err(|e| e.to_string())?;
            Ok(String::from_utf8(secret.to_vec()).ok())
        })
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        let keyring = Self::keyring()?;
        futures_lite::future::block_on(async {
            let attributes = vec![("service", SERVICE), ("account", key)];
            keyring
                .create_item(
                    "Horizon launcher: Minecraft account",
                    &attributes,
                    secret.as_bytes(),
                    true,
                )
                .await
                .map_err(|e| e.to_string())
        })
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        let keyring = Self::keyring()?;
        futures_lite::future::block_on(async {
            let attributes = vec![("service", SERVICE), ("account", key)];
            keyring.delete(&attributes).await.map_err(|e| e.to_string())
        })
    }
    fn persistent(&self) -> bool {
        true
    }
    fn lock_file(&self) -> Option<PathBuf> {
        Some(crate::backend::data_dir().join("account-session.lock"))
    }
}

/// Keychain on macOS, Credential Manager on Windows (the keyring crate). Credential Manager takes at
/// most 2560 bytes of UTF-16 per item and a sign-in's secrets are longer, so on Windows a secret is
/// kept in pieces: the first under the account's key, the rest under `key.1`, `key.2`... The
/// Keychain has no such limit, and every item is a consent prompt for a launcher signature it
/// doesn't know yet — pieces meant several prompts after every self-update (#4912) — so on macOS a
/// secret is one item, and pieces an older launcher left fold into it as they are read.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct SystemStore;

/// Bytes of UTF-8 per keychain item: at most twice as many in UTF-16, well under 2560.
#[cfg(any(target_os = "windows", test))]
const PIECE: usize = 1000;

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn piece_key(key: &str, n: usize) -> String {
    if n == 0 {
        key.to_string()
    } else {
        format!("{key}.{n}")
    }
}

/// `secret` cut into pieces of at most `PIECE` bytes, on character boundaries.
#[cfg(any(target_os = "windows", test))]
fn split_secret(mut secret: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    while secret.len() > PIECE {
        let mut cut = PIECE;
        while !secret.is_char_boundary(cut) {
            cut -= 1;
        }
        let (head, tail) = secret.split_at(cut);
        pieces.push(head);
        secret = tail;
    }
    pieces.push(secret);
    pieces
}

/// One keychain item per key: what a secret's pieces are kept in.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
trait Items {
    fn read(&self, key: &str) -> Result<Option<String>, String>;
    fn write(&self, key: &str, piece: &str) -> Result<(), String>;
    /// Removes one item; false when there was none.
    fn remove(&self, key: &str) -> Result<bool, String>;
}

#[cfg(any(target_os = "windows", test))]
fn get_pieces(items: &dyn Items, key: &str) -> Result<Option<String>, String> {
    let Some(mut secret) = items.read(key)? else {
        return Ok(None);
    };
    for n in 1.. {
        match items.read(&piece_key(key, n))? {
            Some(piece) => secret.push_str(&piece),
            None => break,
        }
    }
    Ok(Some(secret))
}

#[cfg(any(target_os = "windows", test))]
fn set_pieces(items: &dyn Items, key: &str, secret: &str) -> Result<(), String> {
    let before = get_pieces(items, key).ok().flatten();
    let pieces = split_secret(secret);
    for (n, piece) in pieces.iter().enumerate() {
        if let Err(e) = items.write(&piece_key(key, n), piece) {
            // New pieces before old ones read back as no secret at all (the account would be
            // signed out at the next start): the old secret goes back, which fit before.
            if n > 0 {
                let _ = delete_pieces(items, key);
                if let Some(before) = &before {
                    for (n, piece) in split_secret(before).iter().enumerate() {
                        let _ = items.write(&piece_key(key, n), piece);
                    }
                }
            }
            return Err(e);
        }
    }
    // Pieces left over from a longer secret would be read back as part of this one.
    for n in pieces.len().. {
        match items.remove(&piece_key(key, n)) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                // Some leftovers are already gone, some are still there: the old secret goes back
                // whole, as on a refused write above.
                let _ = delete_pieces(items, key);
                if let Some(before) = &before {
                    for (n, piece) in split_secret(before).iter().enumerate() {
                        let _ = items.write(&piece_key(key, n), piece);
                    }
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn delete_pieces(items: &dyn Items, key: &str) -> Result<(), String> {
    for n in 0.. {
        if !items.remove(&piece_key(key, n))? {
            break;
        }
    }
    Ok(())
}

/// One item per key, the whole secret in it (macOS). A secret an older launcher split over `key.1`,
/// `key.2`, ... reads whole and folds into the one item, so a read touches a single item — and asks
/// once — from then on. A merged item also reads fine as "piece 0" to launchers that still split.
#[cfg(any(target_os = "macos", test))]
fn get_item(items: &dyn Items, key: &str) -> Result<Option<String>, String> {
    let Some(mut secret) = items.read(key)? else {
        return Ok(None);
    };
    let mut split = false;
    for n in 1.. {
        match items.read(&piece_key(key, n))? {
            Some(piece) => {
                secret.push_str(&piece);
                split = true;
            }
            None => break,
        }
    }
    if split {
        let _ = set_item(items, key, &secret);
    }
    Ok(Some(secret))
}

/// Writes `secret` to `key` and removes `key.1`, `key.2`, ... pieces an older, split secret left:
/// they would read back as part of this one. A removal the keychain refuses puts the old items
/// back: the merged secret beside the pieces left would read as a longer, unparseable one, and
/// pieces removed before the failure can't be had again except from here.
#[cfg(any(target_os = "macos", test))]
fn set_item(items: &dyn Items, key: &str, secret: &str) -> Result<(), String> {
    let mut before = Vec::new();
    if let Some(piece) = items.read(key)? {
        before.push((key.to_string(), piece));
        for n in 1.. {
            match items.read(&piece_key(key, n))? {
                Some(piece) => before.push((piece_key(key, n), piece)),
                None => break,
            }
        }
    }
    items.write(key, secret)?;
    for n in 1.. {
        match items.remove(&piece_key(key, n)) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                if before.is_empty() {
                    let _ = items.remove(key);
                } else {
                    for (key, piece) in &before {
                        let _ = items.write(key, piece);
                    }
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

/// Credential Manager's error 8 (`ERROR_NOT_ENOUGH_MEMORY`) on a write: not the computer's memory,
/// the player's credential store is full (an undocumented limit of its total size). The Xbox app
/// and the Minecraft Launcher are known to leave hundreds of items there (#4313, #4366).
#[cfg(any(target_os = "windows", test))]
pub const CREDENTIAL_MANAGER_FULL: &str = "Windows Credential Manager is full, so Horizon can't \
     keep your sign-in (Windows error 8; it isn't your computer's memory). Open Credential Manager \
     from the Start menu, go to Windows Credentials and remove entries you no longer use, then sign \
     in again. The Xbox app and the Minecraft Launcher often leave many XblGrts and Xbl| entries \
     there; removing an entry can sign the app that made it out.";

/// What the launcher says for a Windows error code Credential Manager gave when writing an item,
/// when it says more than the code.
#[cfg(any(target_os = "windows", test))]
fn credential_manager_says(code: u32) -> Option<&'static str> {
    const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
    (code == ERROR_NOT_ENOUGH_MEMORY).then_some(CREDENTIAL_MANAGER_FULL)
}

/// A running executable removed by an older macOS updater cannot create Keychain credentials.
#[cfg(any(target_os = "macos", test))]
pub const MACOS_LAUNCHER_RESTART: &str = "The launcher was updated while it was running. Restart the launcher, then sign in with Microsoft again.";

#[cfg(any(target_os = "macos", test))]
fn macos_keychain_says(code: i32) -> Option<&'static str> {
    // CSSM's Unix ENOENT: Keychain cannot identify an executable removed by an older updater.
    (code == 100002).then_some(MACOS_LAUNCHER_RESTART)
}

/// The keyring crate's error as the player reads it.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn write_error(e: keyring::Error) -> String {
    #[cfg(target_os = "macos")]
    if let keyring::Error::PlatformFailure(inner) = &e {
        if let Some(said) = inner
            .downcast_ref::<security_framework::base::Error>()
            .and_then(|error| macos_keychain_says(error.code()))
        {
            return said.to_string();
        }
    }
    #[cfg(target_os = "windows")]
    if let keyring::Error::PlatformFailure(inner) = &e {
        if let Some(said) = inner
            .downcast_ref::<keyring::windows::Error>()
            .and_then(|code| credential_manager_says(code.0))
        {
            return said.to_string();
        }
    }
    e.to_string()
}

/// The OS keychain's items, under [`SERVICE`].
#[cfg(any(target_os = "macos", target_os = "windows"))]
struct KeyringItems;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Items for KeyringItems {
    fn read(&self, key: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(SERVICE, key).and_then(|e| e.get_password()) {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn write(&self, key: &str, piece: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, key)
            .and_then(|e| e.set_password(piece))
            .map_err(write_error)
    }
    fn remove(&self, key: &str) -> Result<bool, String> {
        match keyring::Entry::new(SERVICE, key).and_then(|e| e.delete_credential()) {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SecretStore for SystemStore {
    #[cfg(target_os = "macos")]
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        get_item(&KeyringItems, key)
    }
    #[cfg(target_os = "windows")]
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        get_pieces(&KeyringItems, key)
    }
    #[cfg(target_os = "macos")]
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        set_item(&KeyringItems, key, secret)
    }
    #[cfg(target_os = "windows")]
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        set_pieces(&KeyringItems, key, secret)
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        delete_pieces(&KeyringItems, key)
    }
    fn persistent(&self) -> bool {
        true
    }
    fn lock_file(&self) -> Option<PathBuf> {
        Some(crate::backend::data_dir().join("account-session.lock"))
    }
}

pub const NO_KEYCHAIN: &str = "No keychain available: you are signed in until the launcher closes, then you will have to sign in again.";

/// What the keychain answered when [`store`] fell back to memory.
static NO_KEYCHAIN_WHY: OnceLock<String> = OnceLock::new();

/// What the window and the CLI say when sign-ins last only this run: [`NO_KEYCHAIN`] and why.
/// None while the keychain keeps them.
pub fn no_keychain() -> Option<String> {
    if store().persistent() {
        return None;
    }
    Some(match NO_KEYCHAIN_WHY.get() {
        Some(why) => format!("{NO_KEYCHAIN} The keychain said: {why}"),
        None => NO_KEYCHAIN.into(),
    })
}

/// What a keychain this run stopped asking answers.
const KEYCHAIN_AWAY: &str =
    "The keychain didn't answer earlier: sign in again to keep this account.";

/// The keychain, with the secrets file for what it can't keep (#5095). Each call asks both, never a
/// choice made once: a launcher that wrote to the file while the keychain was away still finds its
/// secrets after another process handed them to the keychain. Reads take the file's copy first (it
/// is the newer one), writes go to the keychain and to the file only when it refuses.
///
/// The file's lock is never held across a keychain call, which can wait on an unlock prompt: other
/// processes keep reading and writing the file meanwhile. Every write to the keychain holds a lock of
/// its own instead (`account-secrets.keychain.lock`), with what it writes read under it, so a hand-over
/// never puts back a secret another process renewed or signed out since; only keychain writes wait
/// on a prompt, as they would on the keychain itself. A file entry is only removed if it still holds
/// what was read before the keychain call, so another process's newer write stays. And once the
/// keychain refuses or doesn't answer, this run stops asking it (no prompt after a dismissed
/// prompt), until another process hands it a secret this run saw in the file.
pub struct Layered {
    keychain: Box<dyn SecretStore>,
    file: FileStore,
    away: std::sync::atomic::AtomicBool,
    /// What this run saw in the file and didn't hand over itself: put there, read there, or there
    /// when it started. Gone from the file later, the keychain answers again (another process
    /// handed it).
    parked: Mutex<std::collections::HashSet<String>>,
}

impl Layered {
    /// Starts by handing the keychain what the file kept while it was away, one secret at a time,
    /// up to the first it refuses; sign-outs it missed are done there too. Each is read again under
    /// the keychain's lock: one another process renewed or signed out meanwhile isn't handed over.
    pub fn new(keychain: Box<dyn SecretStore>, file: FileStore) -> Self {
        let layered = Self {
            keychain,
            file,
            away: Default::default(),
            parked: Default::default(),
        };
        let keys: Vec<String> = layered
            .file
            .entries()
            .unwrap_or_default()
            .into_keys()
            .collect();
        // Seen before the hand-over starts: whatever another process hands over meanwhile, this
        // run asks the keychain for it again.
        layered.parked.lock().unwrap().extend(keys.iter().cloned());
        for key in keys {
            let handed = layered.keychain_writes(|| {
                let Some(secret) = layered.file.entry(&key)? else {
                    return Ok(());
                };
                layered.ask(|keychain| match secret.as_str() {
                    SIGNED_OUT => keychain.delete(&key),
                    _ => keychain.set(&key, &secret),
                })?;
                layered.unpark(&key, Some(secret))
            });
            if handed.is_err() {
                break;
            }
        }
        layered
    }

    /// Runs `f`, a write to the keychain and what it reads first, holding the keychain's lock.
    fn keychain_writes<T>(&self, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        self.file.holding("keychain.lock", f)
    }

    fn is_away(&self) -> bool {
        self.away.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Asks the keychain unless it was away; an error means it is away for the rest of the run.
    fn ask<T>(
        &self,
        call: impl FnOnce(&dyn SecretStore) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.is_away() {
            return Err(KEYCHAIN_AWAY.into());
        }
        let answer = call(self.keychain.as_ref());
        if answer.is_err() {
            self.away.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        answer
    }

    /// Puts `value` in the file for `key`, the keychain being away.
    fn park(&self, key: &str, value: &str) -> Result<(), String> {
        self.file.put(key, value)?;
        self.parked.lock().unwrap().insert(key.into());
        Ok(())
    }

    /// The keychain took `key`: the file's copy read before goes, unless another process wrote since.
    /// Still in the file, it stays counted as seen there, so its hand-over by another process is
    /// noticed.
    fn unpark(&self, key: &str, before: Option<String>) -> Result<(), String> {
        let still_there = self.file.settle(key, before.as_deref())?;
        let mut parked = self.parked.lock().unwrap();
        if still_there {
            parked.insert(key.into());
        } else {
            parked.remove(key);
        }
        Ok(())
    }
}

impl SecretStore for Layered {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match self.file.entry(key)? {
            Some(secret) if secret == SIGNED_OUT => return Ok(None),
            Some(secret) => {
                self.parked.lock().unwrap().insert(key.into());
                return Ok(Some(secret));
            }
            None => {}
        }
        // Seen in the file and gone from it: another process handed it to the keychain, which
        // answers again.
        if self.is_away() && self.parked.lock().unwrap().remove(key) {
            self.away.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        self.ask(|keychain| keychain.get(key))
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        if self.is_away() {
            return self.park(key, secret);
        }
        self.keychain_writes(|| {
            let before = self.file.entry(key)?;
            match self.ask(|keychain| keychain.set(key, secret)) {
                Ok(()) => self.unpark(key, before),
                Err(_) => self.park(key, secret),
            }
        })
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        // The keychain may hold a copy: the mark hides it until it can go.
        if self.is_away() {
            return self.park(key, SIGNED_OUT);
        }
        self.keychain_writes(|| {
            let before = self.file.entry(key)?;
            match self.ask(|keychain| keychain.delete(key)) {
                Ok(()) => self.unpark(key, before),
                Err(_) => self.park(key, SIGNED_OUT),
            }
        })
    }
    fn persistent(&self) -> bool {
        true
    }
}

/// The OS keychain if it answers, else why not.
fn keychain_or_why() -> Result<Box<dyn SecretStore>, String> {
    // Unit tests never reach the player's keychain.
    #[cfg(all(
        not(test),
        any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "macos",
            target_os = "windows"
        )
    ))]
    {
        if std::env::var_os("HORIZON_NO_KEYCHAIN").is_some() {
            return Err("HORIZON_NO_KEYCHAIN is set.".into());
        }
        let keychain = Bounded::new(SystemStore, KEYCHAIN_WAIT);
        keychain.get("probe").map(|_| Box::new(keychain) as _)
    }
    #[cfg(not(all(
        not(test),
        any(
            target_os = "linux",
            target_os = "freebsd",
            target_os = "macos",
            target_os = "windows"
        )
    )))]
    Err("This system has no keychain.".into())
}

/// Microsoft sign-ins. Linux and FreeBSD: the Secret Service with the secrets file beside it
/// ([`Layered`], #5095). macOS and Windows: the keychain if it answers. Memory with
/// `HORIZON_NO_KEYCHAIN` (the launcher's own tests) or no keychain on macOS and Windows (said in the
/// UI with [`no_keychain`]).
pub fn store() -> &'static dyn SecretStore {
    static STORE: OnceLock<Box<dyn SecretStore>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            #[cfg(all(not(test), any(target_os = "linux", target_os = "freebsd")))]
            if std::env::var_os("HORIZON_NO_KEYCHAIN").is_none() {
                return Box::new(Layered::new(
                    Box::new(Bounded::new(SystemStore, KEYCHAIN_WAIT)),
                    launcher_file_store(),
                ));
            }
            keychain_or_why().unwrap_or_else(|why| {
                let _ = NO_KEYCHAIN_WHY.set(why);
                Box::<MemoryStore>::default()
            })
        })
        .as_ref()
}

/// Secrets never written to a file of the launcher's own, the Tailscale access token (it
/// administers the player's whole tailnet): the keychain if it answers, else memory for this run.
/// [`store`] itself on macOS and Windows, where that is what it is.
pub fn keychain() -> &'static dyn SecretStore {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        static KEYCHAIN: OnceLock<Box<dyn SecretStore>> = OnceLock::new();
        KEYCHAIN
            .get_or_init(|| keychain_or_why().unwrap_or_else(|_| Box::<MemoryStore>::default()))
            .as_ref()
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    store()
}

/// Whether sign-ins sit in the secrets file now, the keychain having refused them (#5095): sign-out
/// marks alone, or a file that can't be read, don't count.
pub fn in_secrets_file() -> bool {
    cfg!(any(target_os = "linux", target_os = "freebsd"))
        && store().persistent()
        && launcher_file_store().holds_secrets()
}

// ----- Accounts ------------------------------------------------------------------------------------

/// accounts.json: the signed-in accounts and the one that plays. Never a token.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Accounts {
    pub accounts: Vec<Account>,
    /// The UUID of the account the game starts with; none: the first one.
    pub selected: Option<String>,
    /// Offline names an older launcher kept (`offline`), read only so that the next save also
    /// deletes their skins and capes (`skins/<name>.png`, `skins/<name>.cape.png`). Its
    /// `offline_selected` and `looks` are dropped on read.
    #[serde(default, rename = "offline", skip_serializing)]
    old_offline: Vec<String>,
}

/// The secrets an account keeps in `store`, if it still has them there.
fn secrets_in(store: &dyn SecretStore, uuid: &str) -> Option<Secrets> {
    store
        .get(uuid)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
}

impl Accounts {
    pub(crate) fn path() -> PathBuf {
        crate::backend::data_dir().join("accounts.json")
    }
    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }
    pub(crate) fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub(crate) fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| e.to_string())?;
        // An older launcher's offline names: their skins and capes go too, missing files are fine.
        if let Some(skins) = path.parent().map(|dir| dir.join("skins")) {
            for name in &self.old_offline {
                let key = name.to_ascii_lowercase();
                for file in [format!("{key}.png"), format!("{key}.cape.png")] {
                    let _ = std::fs::remove_file(skins.join(file));
                }
            }
            let _ = std::fs::remove_dir(&skins);
        }
        Ok(())
    }
    pub fn selected(&self) -> Option<&Account> {
        let uuid = self.selected.as_ref()?;
        self.accounts.iter().find(|a| &a.uuid == uuid)
    }
    /// Find an account by name (any case) or UUID (with or without dashes).
    pub fn find(&self, who: &str) -> Option<&Account> {
        let undashed = who.replace('-', "").to_lowercase();
        self.accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(who) || a.uuid == undashed)
    }
    /// Keep a new or renewed account, its secrets in the store, and select it.
    pub fn add(
        &mut self,
        store: &dyn SecretStore,
        account: Account,
        secrets: &Secrets,
    ) -> Result<(), String> {
        let _operation = session::lock(store)?;
        self.keep_sign_in(store, account, secrets)
    }

    fn keep_sign_in(
        &mut self,
        store: &dyn SecretStore,
        account: Account,
        secrets: &Secrets,
    ) -> Result<(), String> {
        store.set(
            &account.uuid,
            &serde_json::to_string(secrets).map_err(|e| e.to_string())?,
        )?;
        self.selected = Some(account.uuid.clone());
        match self.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            Some(a) => *a = account,
            None => self.accounts.push(account),
        }
        Ok(())
    }
    /// Whether an account here owned the game at its last check (the keychain's answer, no
    /// request): what Play needs.
    pub fn has_owner(&self, store: &dyn SecretStore) -> bool {
        // The selected account first: it owning the game ends the search after one account's
        // item — every account's item read can ask for the keychain (macOS, #4912).
        self.selected()
            .into_iter()
            .chain(self.accounts.iter().filter(|a| Some(*a) != self.selected()))
            .any(|a| secrets_in(store, &a.uuid).is_some_and(|s| s.owns))
    }

    /// Plays as the signed-in account `uuid`.
    pub fn play_as(&mut self, uuid: &str) -> Result<(), String> {
        if !self.accounts.iter().any(|a| a.uuid == uuid) {
            return Err("That account isn't signed in.".into());
        }
        self.selected = Some(uuid.to_string());
        Ok(())
    }

    /// A renewed profile (its name or skin changed) in place of the old one; who plays stays.
    fn update(&mut self, account: Account) {
        if let Some(a) = self.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            *a = account;
        }
    }

    /// Sign out: the secrets leave the keychain, the account the list.
    pub fn remove(&mut self, store: &dyn SecretStore, uuid: &str) -> Result<(), String> {
        let _operation = session::lock(store)?;
        store.delete(uuid)?;
        self.accounts.retain(|a| a.uuid != uuid);
        if self.selected.as_deref() == Some(uuid) {
            self.selected = None;
        }
        Ok(())
    }
}

/// No account in the launcher owns Minecraft: Java Edition: the game must not start.
#[derive(Debug, Default, PartialEq)]
pub struct NoOwner {
    /// Why a sign-in couldn't be read, when the keyring didn't give it (#12179), or why the
    /// account that plays can't: what is said instead of the refusal, which would tell a
    /// signed-in player to sign in.
    pub keyring: Option<String>,
}
impl std::fmt::Display for NoOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.keyring {
            Some(why) => write!(f, "{why}"),
            None => write!(f, "{NEEDS_OWNER}."),
        }
    }
}

impl NoOwner {
    /// The same refusal for the window, which has Settings rather than a command to name.
    pub fn in_window(&self) -> String {
        use crate::i18n::tr;
        if let Some(why) = &self.keyring {
            return why.clone();
        }
        format!(
            "{}. {}",
            tr(NEEDS_OWNER),
            tr("Open Settings and sign in under Account.")
        )
    }
}

/// The owner gate's pass for one launch.
#[derive(Debug, Default)]
pub struct Launch {
    /// The selected account and its renewed session; none when its sign-in couldn't be renewed
    /// now (`offline` says why).
    pub session: Option<(Account, Secrets)>,
    /// Why the selected account plays without its session this time.
    pub offline: Option<String>,
    /// An account the services said owns the game just now, the selected one first: who the
    /// launcher may download the game for.
    pub owner: Option<(Account, Secrets)>,
    /// A renewed sign-in the keychain refused to keep: it plays now, and may have to sign in
    /// again next time.
    pub unsaved: Option<String>,
}

/// The last [`Launch::unsaved`], for the window or the terminal to show once.
static UNSAVED: Mutex<Option<String>> = Mutex::new(None);

/// What the last launch couldn't save in the keychain, once.
pub fn take_unsaved() -> Option<String> {
    UNSAVED.lock().ok()?.take()
}

/// The owner gate: the game starts only while an account in the launcher owns Minecraft: Java
/// Edition. The accounts are asked again, the selected one first, until one owns it; each keeps
/// the answer in the keychain. An account that gets no answer (no network) keeps its last one, so
/// being offline never blocks a player who passed before, while "doesn't own" blocks at once.
/// The caller saves `accounts` (renewed names and skins).
pub fn launch(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
) -> Result<Launch, NoOwner> {
    let selected = accounts.selected.clone();
    let mut order = accounts.accounts.clone();
    order.sort_by_key(|a| Some(&a.uuid) != selected.as_ref());
    let mut pass = Launch::default();
    let mut owned = false;
    let mut unreadable = None;
    for account in order {
        if owned {
            break;
        }
        let plays = Some(&account.uuid) == selected.as_ref();
        let operation = session::lock(store);
        let read = match store.get(&account.uuid) {
            Ok(text) => text.and_then(|s| serde_json::from_str::<Secrets>(&s).ok()),
            Err(why) => {
                // Not "signed out": the keyring didn't say (#12179). With no owner, the refusal
                // says this instead.
                let said = crate::i18n::trf(
                    "Can't read {name}'s sign-in: {why}",
                    &[("name", &account.name), ("why", &why)],
                );
                eprintln!("Horizon: {said}");
                if plays {
                    pass.offline = Some(said.clone());
                }
                unreadable = Some(said);
                continue;
            }
        };
        let Some(secrets) = read else {
            if plays {
                pass.offline = Some(format!(
                    "{} is no longer signed in on this computer. Sign in again.",
                    account.name
                ));
            }
            continue;
        };
        let before = serde_json::to_string(&secrets).ok();
        let checked = match &operation {
            Ok(_) => check(ep, client_id, &account, secrets),
            Err(e) => Check::NoAnswer(e.clone(), secrets),
        };
        let (said, secrets) = match checked {
            Check::Owns(renewed, secrets) => {
                accounts.update(renewed.clone());
                if plays {
                    pass.session = Some((renewed.clone(), secrets.clone()));
                }
                pass.owner = Some((renewed, secrets.clone()));
                (None, secrets)
            }
            Check::DoesNot(secrets) => (Some(DOES_NOT_OWN.to_string()), secrets),
            Check::NoAnswer(e, secrets) => (Some(e), secrets),
        };
        owned = secrets.owns;
        if plays {
            pass.offline = said;
        }
        let after = serde_json::to_string(&secrets).ok();
        if after != before {
            if let Some(text) = after {
                // The game still starts; the player hears why the next launch may ask again.
                if let Err(e) = store.set(&account.uuid, &text) {
                    let said = format!(
                        "{}'s renewed sign-in couldn't be saved in the keychain ({e}): you may \
                         have to sign in again next time.",
                        account.name
                    );
                    if let Ok(mut last) = UNSAVED.lock() {
                        *last = Some(said.clone());
                    }
                    pass.unsaved = Some(said);
                }
            }
        }
    }
    if owned {
        Ok(pass)
    } else {
        Err(NoOwner {
            keyring: unreadable,
        })
    }
}

/// The owner gate before the game starts: [`launch`], where the account that plays (the selected
/// one, else the first) must own the game itself, by the services' answer or, when they can't be
/// asked, its last one. Never another account's ownership for a name of its own.
pub fn launch_for_game(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
) -> Result<Launch, NoOwner> {
    if accounts.selected().is_none() {
        accounts.selected = accounts.accounts.first().map(|a| a.uuid.clone());
    }
    let pass = launch(ep, client_id, store, accounts)?;
    let owns = accounts
        .selected()
        .and_then(|a| secrets_in(store, &a.uuid))
        .is_some_and(|s| s.owns);
    if owns {
        Ok(pass)
    } else {
        // Why that account can't play (it doesn't own the game, signed out, an unread keyring).
        Err(NoOwner {
            keyring: pass.offline,
        })
    }
}

/// Write the session for the game, readable by this user only, in a fresh file it deletes
/// when it reads it.
pub fn write_session_file(
    dir: &Path,
    account: &Account,
    secrets: &Secrets,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // The folder too is this user's only (the files already are).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    // Leftovers of games that never started (refused by the game after two minutes anyway).
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|m| m.elapsed().is_ok_and(|age| age > Duration::from_secs(120)));
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let mut name = [0u8; 12];
    getrandom::fill(&mut name).map_err(|e| e.to_string())?;
    let path = dir.join(format!(
        "session-{}.json",
        name.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    let body = json!({
        "name": account.name,
        "uuid": account.uuid,
        "access_token": secrets.minecraft_token,
        "user_type": "msa",
        "xuid": secrets.xuid,
    });
    std::io::Write::write_all(&mut file, body.to_string().as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}

/// How long a game takes a session file: it refuses one older than this (the game's
/// `MAX_SESSION_FILE_AGE`), so the launcher deletes it then if it is still there.
const SESSION_FILE_LIFE: Duration = Duration::from_secs(120);

/// The session files this launcher wrote for games it starts, until they are gone: a game
/// deletes each as it reads it; the rest go when the game would refuse them anyway
/// ([`SESSION_FILE_LIFE`], a game that never started or crashed first) or when the launch failed
/// ([`discard_unread_sessions`]). Nothing with a token stays on disk.
static UNREAD: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// [`write_session_file`] for a game this launcher is about to start (see [`UNREAD`]).
fn write_launch_session_file(
    dir: &Path,
    account: &Account,
    secrets: &Secrets,
) -> Result<PathBuf, String> {
    let path = write_session_file(dir, account, secrets)?;
    let Ok(mut unread) = UNREAD.lock() else {
        return Ok(path);
    };
    if unread.is_empty() {
        // One reaper while files wait; it ends with the list (under its lock, so a file added
        // as it ends starts the next).
        let _ = std::thread::Builder::new()
            .name("Session files".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(5));
                let Ok(mut unread) = UNREAD.lock() else {
                    return;
                };
                reap(&mut unread, SESSION_FILE_LIFE);
                if unread.is_empty() {
                    return;
                }
            });
    }
    unread.push(path.clone());
    Ok(path)
}

/// Deletes the files of `unread` older than `life` (the game would refuse them) and forgets
/// those gone (read).
fn reap(unread: &mut Vec<PathBuf>, life: Duration) {
    unread.retain(|path| {
        let waiting = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .is_ok_and(|m| m.elapsed().is_ok_and(|age| age <= life));
        if !waiting {
            let _ = std::fs::remove_file(path);
        }
        waiting
    });
}

/// Deletes the session files written for a launch whose game didn't start, or has ended: any it
/// didn't read is left over (a game deletes those it reads).
pub fn discard_unread_sessions() {
    let files = UNREAD
        .lock()
        .map(|mut unread| std::mem::take(&mut *unread))
        .unwrap_or_default();
    for path in files {
        let _ = std::fs::remove_file(path);
    }
}

/// Called under the account-operation lock, after the final profile is known. Rejected or
/// cancelled sign-ins must not save a credential, add another identity, or change selection.
fn accept_sign_in(
    accounts: &mut Accounts,
    store: &dyn SecretStore,
    expected: Option<&Account>,
    account: Account,
    secrets: &Secrets,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(session::Phase),
) -> Result<(), String> {
    if cancelled() {
        return Err("Sign-in cancelled.".into());
    }
    if let Some(expected) = expected {
        if expected.uuid != account.uuid {
            return Err(format!(
                "Signed in as {} instead of {}. Use the Microsoft account for {} and try again.",
                account.name, expected.name, expected.name
            ));
        }
        if accounts.find(&expected.uuid).is_none() {
            return Err("That account was signed out while sign-in was in progress. Add it again to continue.".into());
        }
    }
    let selected = accounts.selected.clone();
    progress(session::Phase::Saving);
    accounts.keep_sign_in(store, account, secrets)?;
    if expected.is_some() {
        accounts.selected = selected;
    }
    Ok(())
}

/// Where a background sign-in is (the window and the MCP server show it).
#[derive(Clone, Debug, PartialEq)]
pub enum SignInState {
    Starting,
    /// Waiting for the player at `verification_uri` with `user_code`.
    Code(DeviceCode),
    Working(session::Phase),
    Done(Result<Account, String>),
}

/// A sign-in on its own thread, from the code to the saved account.
pub struct SignIn {
    state: std::sync::Arc<Mutex<SignInState>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl SignIn {
    pub fn start() -> Self {
        Self::start_expected(None)
    }

    /// Restore this exact Minecraft identity without changing who currently plays.
    pub fn start_for(account: Account) -> Self {
        Self::start_expected(Some(account))
    }

    fn start_expected(expected: Option<Account>) -> Self {
        use std::sync::atomic::Ordering;
        let state = std::sync::Arc::new(Mutex::new(SignInState::Starting));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (thread_state, thread_cancel) = (state.clone(), cancel.clone());
        std::thread::spawn(move || {
            let set = |s| *thread_state.lock().unwrap() = s;
            let result = (|| {
                let ep = Endpoints::live();
                let client_id = client_id().ok_or(NOT_CONFIGURED)?;
                let code = device_code(&ep, &client_id)?;
                set(SignInState::Code(code.clone()));
                let ms = wait_for_sign_in(&ep, &client_id, &code, &|| {
                    thread_cancel.load(Ordering::Relaxed)
                })?;
                let progress = |phase| set(SignInState::Working(phase));
                let (account, secrets) = finish_sign_in_with_progress(&ep, ms, &progress)?;
                progress(session::Phase::Queued);
                let store = store();
                let _operation = session::lock(store)?;
                let mut accounts = Accounts::load();
                accept_sign_in(
                    &mut accounts,
                    store,
                    expected.as_ref(),
                    account.clone(),
                    &secrets,
                    &|| thread_cancel.load(Ordering::Relaxed),
                    &progress,
                )?;
                progress(session::Phase::Saving);
                accounts.save()?;
                Ok(account)
            })();
            set(SignInState::Done(result));
        });
        Self { state, cancel }
    }
    pub fn state(&self) -> SignInState {
        self.state.lock().unwrap().clone()
    }
    pub fn cancel(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What the game about to start gets from the accounts.
#[derive(Debug, Default)]
pub struct GameSession {
    /// The playing account's session (`--session-file`); none: offline.
    pub file: Option<PathBuf>,
    /// Why the account plays offline this time, if it does.
    pub offline: Option<String>,
    /// The other accounts' sessions, for splitscreen players (`account_sessions`).
    pub env: Vec<(&'static str, String)>,
}

/// The owner gate for the game about to start, then the selected account's session in a file
/// for it, and the other accounts' for its splitscreen players.
pub fn session_for_game(game_home: &Path) -> Result<GameSession, NoOwner> {
    let mut accounts = Accounts::load();
    let pass = launch_for_game(
        &Endpoints::live(),
        client_id().as_deref(),
        store(),
        &mut accounts,
    )?;
    let env = split_sessions(
        game_home,
        &mut accounts,
        pass.session.as_ref().map(|(a, _)| a.uuid.as_str()),
    );
    if let Err(e) = accounts.save() {
        eprintln!("Horizon: the accounts are not saved: {e}");
    }
    Ok(session_file(game_home, pass, env))
}

/// [`session_for_game`] for one launch as `who` (`play --account`, a signed-in account): who
/// plays changes for this launch only, so several games can play as several accounts at once
/// (#4952). What the services renewed (tokens, names, skins) is still kept; which account
/// `account select` picked is not touched.
pub fn session_for_game_as(game_home: &Path, who: &str) -> Result<GameSession, String> {
    let mut accounts = Accounts::load();
    let pass = launch_as(
        &Endpoints::live(),
        client_id().as_deref(),
        store(),
        &mut accounts,
        who,
    );
    if let Err(e) = accounts.save() {
        eprintln!("Horizon: the accounts are not saved: {e}");
    }
    let pass = pass?;
    let env = split_sessions(
        game_home,
        &mut accounts,
        pass.session.as_ref().map(|(a, _)| a.uuid.as_str()),
    );
    if let Err(e) = accounts.save() {
        eprintln!("Horizon: the accounts are not saved: {e}");
    }
    Ok(session_file(game_home, pass, env))
}

/// The owner gate as `who`, on `accounts`, which keeps its choice of player.
fn launch_as(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
    who: &str,
) -> Result<Launch, String> {
    let chosen = accounts.selected.clone();
    let pass = accounts
        .play_as(who)
        .and_then(|()| launch_for_game(ep, client_id, store, accounts).map_err(|e| e.to_string()));
    accounts.selected = chosen;
    pass
}

/// The game's session files for a launch that passed the gate.
fn session_file(game_home: &Path, pass: Launch, env: Vec<(&'static str, String)>) -> GameSession {
    let mut out = GameSession {
        env,
        offline: pass.offline,
        ..GameSession::default()
    };
    if let Some((account, secrets)) = pass.session {
        match write_launch_session_file(&game_home.join("sessions"), &account, &secrets) {
            Ok(file) => out.file = Some(file),
            Err(e) => out.offline = Some(e),
        }
    }
    out
}

fn split_sessions(
    game_home: &Path,
    accounts: &mut Accounts,
    playing: Option<&str>,
) -> Vec<(&'static str, String)> {
    if splitscreen_on(game_home) {
        // The launch may override the saved player.
        let selected = std::mem::replace(&mut accounts.selected, playing.map(str::to_owned));
        let env = account_sessions(
            &Endpoints::live(),
            client_id().as_deref(),
            store(),
            &game_home.join("sessions"),
            accounts,
        );
        accounts.selected = selected;
        env
    } else {
        Vec::new()
    }
}

/// Whether the game's Splitscreen setting is on (`horizon-extras.txt`, `splitscreen:true`): only
/// then do the other accounts' sessions go to it.
fn splitscreen_on(game_home: &Path) -> bool {
    std::fs::read_to_string(game_home.join("horizon-extras.txt"))
        .is_ok_and(|text| text.lines().any(|l| l.trim() == "splitscreen:true"))
}

/// The signed-in accounts but the one that plays, each its session in a file of its own, for
/// the game's splitscreen players to sign in with (`HORIZON_ACCOUNT_SESSIONS`: name, uuid and
/// file each). A token known to own the game and not due goes as it is; otherwise it is renewed
/// and asked about (`check`), and an account that can't be renewed, doesn't own the game or
/// whose token names another profile is left out: the game starts all the same. Nothing with one
/// account.
/// How long Play spends renewing the other accounts' sessions, in all: past it, the rest are
/// offered only with a token that still holds (no request starts; one under way finishes).
const OTHERS_WAIT: Duration = Duration::from_secs(10);

fn account_sessions(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    dir: &Path,
    accounts: &mut Accounts,
) -> Vec<(&'static str, String)> {
    account_sessions_until(
        ep,
        client_id,
        store,
        dir,
        accounts,
        Instant::now() + OTHERS_WAIT,
    )
}

fn account_sessions_until(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    dir: &Path,
    accounts: &mut Accounts,
    deadline: Instant,
) -> Vec<(&'static str, String)> {
    let others: Vec<Account> = accounts
        .accounts
        .iter()
        .filter(|a| Some(&a.uuid) != accounts.selected.as_ref())
        .cloned()
        .collect();
    let mut list = Vec::new();
    for account in others {
        let skipped =
            |reason: &str| json!({ "name": account.name, "uuid": account.uuid, "skipped": reason });
        // Under the account lock, as every renewal is: a refresh token Microsoft rotates here is
        // not lost to another launcher renewing the same account at once. Still busy at the
        // deadline: no request, the token while it holds.
        let operation = session::lock_within(
            store,
            Some(deadline.saturating_duration_since(Instant::now())),
        );
        let Some(secrets) = secrets_in(store, &account.uuid) else {
            list.push(skipped("Sign in again in the launcher."));
            continue;
        };
        let fallback = skipped("Sign in again in the launcher.");
        let before = serde_json::to_string(&secrets).ok();
        let uuid = account.uuid.clone();
        // An account the services sent back to sign-in (a refused refresh, a token now for
        // another profile) is never handed over, whatever its token's time says.
        let holds = |s: &Secrets, margin: u64| {
            s.owns && !s.sign_in_required && s.minecraft_expires_at > now() + margin
        };
        let known = holds(&secrets, RENEW_BEFORE);
        let late = operation.is_err() || Instant::now() >= deadline;
        let (account, secrets) = match known {
            true => (Some(account), secrets),
            // Out of time: its token while it holds, else Sign in again.
            false if late => (holds(&secrets, 60).then_some(account), secrets),
            false => match check(ep, client_id, &account, secrets) {
                Check::Owns(renewed, secrets) => {
                    accounts.update(renewed.clone());
                    (Some(renewed), secrets)
                }
                Check::DoesNot(secrets) => (None, secrets),
                Check::NoAnswer(e, secrets) => {
                    eprintln!("{}'s session for splitscreen: {e}", account.name);
                    // Its token may still hold (no network): offered only while it does. A token
                    // for another profile is marked sign-in required, so it never is.
                    (holds(&secrets, 60).then_some(account), secrets)
                }
            },
        };
        // A renewed token, or the services' answer, kept for the next launch.
        let after = serde_json::to_string(&secrets).ok();
        if let (true, Some(text)) = (after != before, after) {
            let _ = store.set(&uuid, &text);
        }
        drop(operation);
        let Some(account) = account else {
            list.push(if secrets.owns { fallback } else {
                json!({ "name": fallback["name"], "uuid": uuid, "skipped": "This account needs Minecraft: Java Edition ownership confirmed in the launcher." })
            });
            continue;
        };
        match write_launch_session_file(dir, &account, &secrets) {
            Ok(file) => {
                list.push(json!({ "name": account.name, "uuid": account.uuid, "file": file }))
            }
            Err(_) => list.push(fallback),
        }
    }
    if list.is_empty() {
        return Vec::new();
    }
    vec![(
        "HORIZON_ACCOUNT_SESSIONS",
        serde_json::Value::from(list).to_string(),
    )]
}

/// Who plays the real Minecraft: Java Edition (a Java instance), past the owner gate itself
/// ([`launch`]): the selected account when it owns the game, else one that does. With no answer
/// from the services, an account that owned it plays with its last token. Mojang's game takes the
/// token on its command line, as Mojang's launcher gives it; that command is never logged.
pub fn java_player() -> Result<crate::java_launch::Player, String> {
    let mut accounts = Accounts::load();
    let player = java_player_with(
        &Endpoints::live(),
        client_id().as_deref(),
        store(),
        &mut accounts,
    );
    if let Err(e) = accounts.save() {
        eprintln!("Horizon: the accounts are not saved: {e}");
    }
    player
}

fn java_player_with(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
) -> Result<crate::java_launch::Player, String> {
    let pass = launch(ep, client_id, store, accounts).map_err(|e| e.to_string())?;
    let kept = || {
        let order = accounts.selected().into_iter().chain(&accounts.accounts);
        order
            .into_iter()
            .find_map(|a| Some((a.clone(), secrets_in(store, &a.uuid).filter(|s| s.owns)?)))
    };
    let (account, secrets) = pass
        .session
        .or(pass.owner)
        .or_else(kept)
        .ok_or_else(|| NoOwner::default().to_string())?;
    Ok(crate::java_launch::Player {
        name: account.name,
        uuid: account.uuid,
        token: secrets.minecraft_token,
        xuid: secrets.xuid.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests;
