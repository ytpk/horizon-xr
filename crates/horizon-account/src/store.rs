use super::*;

/// Where secrets are kept: the OS keychain, a file of the launcher's own when there is none, or
/// memory in tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
    /// False for memory: secrets are gone when the launcher closes.
    fn persistent(&self) -> bool;
    /// A shared lock for read/renew/write operations; memory stores need only the process lock.
    fn lock_file(&self) -> Option<PathBuf> {
        None
    }
    /// The in-process half of that lock. The system keychain is one per computer, so every
    /// handle to it shares the process-wide lock; a memory store's secrets are its own, and so
    /// is its lock: two of them (one per unit test) never wait for each other.
    fn operations(&self) -> &Mutex<()> {
        crate::process_lock()
    }
}

#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>, Mutex<()>);
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(key.into(), secret.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
    fn persistent(&self) -> bool {
        false
    }
    fn operations(&self) -> &Mutex<()> {
        &self.1
    }
}

/// The secrets file in the launcher's data folder, when no keychain answers (#5095): many Linux
/// systems run no Secret Service, and sign-ins would otherwise last one run. One JSON object, key
/// to secret, encrypted with a key of this computer's ([`seal`]), readable and writable by the
/// player only (0600 on Unix), replaced whole on every write. A lock file beside it keeps each read-modify-replace whole across every launcher and CLI
/// process. The keychain takes its secrets back at the first start it answers ([`Layered`]).
pub struct FileStore {
    path: PathBuf,
    key: KeySource,
}

/// Where a [`FileStore`] gets the key it seals the file with.
enum KeySource {
    /// This computer's identity (the desktop launcher).
    Machine,
    /// A random key in its own file, made on first use with mode 0600 (the Quest has no
    /// `/etc/machine-id`): a copy of the secrets file alone is useless without it.
    File(PathBuf),
}

/// The secrets file's name in the launcher's data folder.
pub const SECRETS_FILE: &str = "account-secrets.json";

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            key: KeySource::Machine,
        }
    }

    /// A store sealed with the random key kept in `key_file` (made if missing).
    pub fn with_key_file(path: PathBuf, key_file: PathBuf) -> Self {
        Self {
            path,
            key: KeySource::File(key_file),
        }
    }

    fn sealing_key(&self) -> Result<chacha20poly1305::Key, String> {
        match &self.key {
            KeySource::Machine => Ok(file_key()),
            KeySource::File(path) => key_from_file(path),
        }
    }

    pub fn shown(&self, e: std::io::Error) -> String {
        format!("{}: {e}", self.path.display())
    }

    /// Runs `f` holding the file's lock (`account-secrets.lock`): other processes wait.
    pub fn locked<T>(&self, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        self.holding("lock", f)
    }

    /// Runs `f` holding the lock file `account-secrets.<name>`, made if need be.
    pub fn holding<T>(
        &self,
        name: &str,
        f: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| self.shown(e))?;
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.path.with_extension(name))
            .map_err(|e| self.shown(e))?;
        lock.lock().map_err(|e| self.shown(e))?;
        // Closing the file releases the lock, after `f` whatever it answers.
        f()
    }

    /// Whether the file holds a sign-in: not only sign-out marks, and readable.
    pub fn holds_secrets(&self) -> bool {
        self.entries()
            .is_ok_and(|secrets| secrets.values().any(|secret| secret != SIGNED_OUT))
    }

    pub fn read(&self) -> Result<std::collections::BTreeMap<String, String>, String> {
        match std::fs::read(&self.path) {
            // Only where the file breaks: serde's own message can quote what the file holds.
            Ok(bytes) => {
                let (json, old) = self.unseal(&bytes)?.ok_or_else(|| {
                    format!(
                        "{} can't be read on this computer: it was written on another one, or changed. \
                         Remove it and sign in again.",
                        self.path.display()
                    )
                })?;
                let secrets = serde_json::from_slice(&json).map_err(|e| {
                    format!(
                        "{} can't be read ({:?} error at line {}, column {}).",
                        self.path.display(),
                        e.classify(),
                        e.line(),
                        e.column()
                    )
                })?;
                // Sealed under the old key: sealed again under this install's. Every read runs
                // under the lock; a write that fails leaves the old file, read again next time.
                if old {
                    let _ = self.write(&secrets);
                }
                Ok(secrets)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
            Err(e) => Err(self.shown(e)),
        }
    }

    /// Replaces the file with `secrets` through a temporary file of this write's own beside it, so
    /// a crash leaves the old file or the new one; removes it when there are none left.
    pub fn write(
        &self,
        secrets: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), String> {
        if secrets.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(self.shown(e)),
                _ => Ok(()),
            };
        }
        static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = self
            .path
            .with_extension(format!("json.{}.{n}.tmp", std::process::id()));
        let json = serde_json::to_vec(secrets).map_err(|e| e.to_string())?;
        let sealed = self.seal(&json)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let written = options.open(&tmp).and_then(|mut file| {
            std::io::Write::write_all(&mut file, &sealed)?;
            file.sync_all()
        });
        if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, &self.path)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(self.shown(e));
        }
        Ok(())
    }

    /// The file's entry for `key`, if the file is there.
    pub fn entry(&self, key: &str) -> Result<Option<String>, String> {
        if !self.path.exists() {
            return Ok(None);
        }
        self.locked(|| Ok(self.read()?.remove(key)))
    }

    /// Everything the file holds, if it is there.
    pub fn entries(&self) -> Result<std::collections::BTreeMap<String, String>, String> {
        if !self.path.exists() {
            return Ok(Default::default());
        }
        self.locked(|| self.read())
    }

    pub fn put(&self, key: &str, value: &str) -> Result<(), String> {
        self.locked(|| {
            let mut secrets = self.read()?;
            secrets.insert(key.into(), value.into());
            self.write(&secrets)
        })
    }

    /// The keychain took `key`: removes its entry if it still holds `before`, what was read before
    /// the keychain call. Whether the file still has an entry for it: one another process wrote
    /// since stays, and is read first.
    pub fn settle(&self, key: &str, before: Option<&str>) -> Result<bool, String> {
        if !self.path.exists() {
            return Ok(false);
        }
        self.locked(|| {
            let mut secrets = self.read()?;
            if before.is_some() && secrets.get(key).map(String::as_str) == before {
                secrets.remove(key);
                self.write(&secrets)?;
                return Ok(false);
            }
            Ok(secrets.contains_key(key))
        })
    }

    fn install_secret(&self, make: bool) -> Result<Option<[u8; 32]>, String> {
        let path = self.path.with_extension("key");
        match std::fs::read(&path) {
            Ok(bytes) => <[u8; 32]>::try_from(bytes).map(Some).map_err(|_| {
                format!(
                    "{} is damaged. Remove it and {} and sign in again.",
                    path.display(),
                    self.path.display()
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && make => {
                let mut secret = [0u8; 32];
                getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
                // Whole or not at all, as the secrets file: a temporary file, then a rename.
                let tmp = path.with_extension(format!("key.{}.tmp", std::process::id()));
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
                let written = options.open(&tmp).and_then(|mut file| {
                    std::io::Write::write_all(&mut file, &secret)?;
                    file.sync_all()
                });
                if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, &path)) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(format!("{}: {e}", path.display()));
                }
                Ok(Some(secret))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn seal(&self, json: &[u8]) -> Result<Vec<u8>, String> {
        if let KeySource::File(_) = &self.key {
            return seal_with(&self.sealing_key()?, json);
        }
        use chacha20poly1305::{aead::Aead, KeyInit};
        let key = file_key_v2(&self.install_secret(true)?.expect("made"));
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let sealed = chacha20poly1305::ChaCha20Poly1305::new(&key)
            .encrypt(&nonce.into(), json)
            .map_err(|_| "the secrets couldn't be sealed".to_string())?;
        Ok([SEALED_V2, &nonce, &sealed].concat())
    }

    fn unseal(&self, bytes: &[u8]) -> Result<Option<(Vec<u8>, bool)>, String> {
        if let KeySource::File(_) = &self.key {
            return Ok(unseal_with(&self.sealing_key()?, bytes).map(|json| (json, false)));
        }
        use chacha20poly1305::{aead::Aead, KeyInit};
        let (key, old, rest) = if let Some(rest) = bytes.strip_prefix(SEALED_V2) {
            let Some(secret) = self.install_secret(false)? else {
                return Ok(None);
            };
            (file_key_v2(&secret), false, rest)
        } else if let Some(rest) = bytes.strip_prefix(SEALED) {
            (file_key(), true, rest)
        } else {
            return Ok(None);
        };
        let Some((nonce, sealed)) = rest.split_at_checked(12) else {
            return Ok(None);
        };
        let nonce: [u8; 12] = nonce.try_into().expect("12 bytes");
        Ok(chacha20poly1305::ChaCha20Poly1305::new(&key)
            .decrypt(&nonce.into(), sealed)
            .ok()
            .map(|json| (json, old)))
    }
}

/// The secrets file's first bytes: format 1, the JSON sealed with ChaCha20-Poly1305 under
/// [`file_key`], then a random 12-byte nonce and the ciphertext with its tag.
pub const SEALED: &[u8] = b"HZSECv1\n";

/// The secrets file's key: this computer's identity (`/etc/machine-id` on Linux, `/etc/hostid` on
/// FreeBSD) hashed with a constant of the launcher's. The tokens are never on disk in plain text,
/// and a copy of the file taken off the computer (a backup, a synced home folder) can't be read
/// elsewhere. On the computer itself, the file's mode (0600) is what keeps other users out.
pub fn file_key() -> chacha20poly1305::Key {
    use sha2::Digest;
    let mut hash = sha2::Sha256::new();
    hash.update(b"horizon-launcher account-secrets v1");
    if let Some(id) = ["/etc/machine-id", "/var/lib/dbus/machine-id", "/etc/hostid"]
        .iter()
        .find_map(|path| std::fs::read(path).ok())
    {
        hash.update(id.trim_ascii());
    }
    hash.finalize()
}

/// The key in `path`, 32 random bytes made on first use (0600). A damaged file is an error: a new
/// key would make every sealed secret unreadable without saying why.
fn key_from_file(path: &std::path::Path) -> Result<chacha20poly1305::Key, String> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() == 32 => Ok(*chacha20poly1305::Key::from_slice(&bytes)),
        Ok(_) => Err(format!(
            "{} is damaged. Remove it and sign in again.",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut key = [0u8; 32];
            getrandom::fill(&mut key).map_err(|e| e.to_string())?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            match options.open(path) {
                Ok(mut file) => {
                    std::io::Write::write_all(&mut file, &key)
                        .and_then(|()| file.sync_all())
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    Ok(key.into())
                }
                // Another process made it first: use theirs.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => key_from_file(path),
                Err(e) => Err(format!("{}: {e}", path.display())),
            }
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// `json` sealed for the secrets file ([`SEALED`]) with this computer's key.
pub fn seal(json: &[u8]) -> Result<Vec<u8>, String> {
    seal_with(&file_key(), json)
}

pub fn seal_with(key: &chacha20poly1305::Key, json: &[u8]) -> Result<Vec<u8>, String> {
    use chacha20poly1305::{aead::Aead, KeyInit};
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let sealed = chacha20poly1305::ChaCha20Poly1305::new(key)
        .encrypt(&nonce.into(), json)
        .map_err(|_| "the secrets couldn't be sealed".to_string())?;
    Ok([SEALED, &nonce, &sealed].concat())
}

/// What [`seal`] sealed, or `None` for a file in another format, changed, or sealed on another
/// computer.
pub fn unseal(bytes: &[u8]) -> Option<Vec<u8>> {
    unseal_with(&file_key(), bytes)
}

pub fn unseal_with(key: &chacha20poly1305::Key, bytes: &[u8]) -> Option<Vec<u8>> {
    use chacha20poly1305::{aead::Aead, KeyInit};
    let rest = bytes.strip_prefix(SEALED)?;
    let (nonce, sealed) = rest.split_at_checked(12)?;
    let nonce: [u8; 12] = nonce.try_into().ok()?;
    chacha20poly1305::ChaCha20Poly1305::new(key)
        .decrypt(&nonce.into(), sealed)
        .ok()
}

/// A file entry for an account signed out while the keychain was away: the keychain's copy, if any,
/// counts as gone, and goes at the first start it answers.
pub const SIGNED_OUT: &str = "";

impl SecretStore for FileStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.locked(|| Ok(self.read()?.remove(key)))
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        self.locked(|| {
            let mut secrets = self.read()?;
            secrets.insert(key.into(), secret.into());
            self.write(&secrets)
        })
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.locked(|| {
            let mut secrets = self.read()?;
            if secrets.remove(key).is_none() {
                return Ok(());
            }
            self.write(&secrets)
        })
    }
    fn persistent(&self) -> bool {
        true
    }
}

pub const SEALED_V2: &[u8] = b"HZSECv2\n";
fn machine_id() -> Option<Vec<u8>> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id", "/etc/hostid"]
        .iter()
        .find_map(|path| std::fs::read(path).ok())
        .map(|id| id.trim_ascii().to_vec())
}

/// The secrets file's key: a constant of the launcher's, this computer's identity and the install's
/// own random secret (`account-secrets.key`, 0600). The tokens are never on disk in plain text,
/// and the key is never something every local user can work out (the machine id is readable by
/// all, and some systems have none): it takes the install secret, which only the player can read.
/// The file's mode (0600) is still what keeps other users out; the secret means a copy of the file
/// alone, or one left readable by others while the secret wasn't, can't be read either.
fn file_key_v2(install_secret: &[u8; 32]) -> chacha20poly1305::Key {
    use sha2::Digest;
    let mut hash = sha2::Sha256::new();
    hash.update(b"horizon-launcher account-secrets v2");
    let id = machine_id().unwrap_or_default();
    hash.update((id.len() as u64).to_le_bytes());
    hash.update(&id);
    hash.update(install_secret);
    hash.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-account-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_key_file_store_keeps_secrets_that_a_copy_of_the_file_alone_cannot_read() {
        let dir = scratch("keyfile");
        let store = FileStore::with_key_file(dir.join("secrets.json"), dir.join("secrets.key"));
        store.set("uuid", "refresh-token-1").unwrap();
        // A second handle (a restart) reads it back through the same key file.
        let again = FileStore::with_key_file(dir.join("secrets.json"), dir.join("secrets.key"));
        assert_eq!(
            again.get("uuid").unwrap().as_deref(),
            Some("refresh-token-1")
        );
        let on_disk = std::fs::read(dir.join("secrets.json")).unwrap();
        assert!(!String::from_utf8_lossy(&on_disk).contains("refresh-token-1"));
        // The same bytes beside another key file (a copied data folder) are refused.
        std::fs::copy(dir.join("secrets.json"), dir.join("copy.json")).unwrap();
        let stolen = FileStore::with_key_file(dir.join("copy.json"), dir.join("other.key"));
        assert!(stolen.get("uuid").unwrap_err().contains("can't be read"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in ["secrets.json", "secrets.key"] {
                let mode = std::fs::metadata(dir.join(file))
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o077, 0, "{file} is open to other users");
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn desktop_format_one_is_upgraded_without_losing_the_session() {
        let dir = scratch("desktop-upgrade");
        let path = dir.join("secrets.json");
        std::fs::write(&path, seal(br#"{"uuid":"kept-session"}"#).unwrap()).unwrap();
        let store = FileStore::new(path.clone());
        assert_eq!(store.get("uuid").unwrap().as_deref(), Some("kept-session"));
        assert!(std::fs::read(&path).unwrap().starts_with(SEALED_V2));
        assert_eq!(std::fs::read(path.with_extension("key")).unwrap().len(), 32);
        assert_eq!(
            FileStore::new(path).get("uuid").unwrap().as_deref(),
            Some("kept-session")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn desktop_read_without_its_install_secret_never_creates_a_replacement() {
        let dir = scratch("desktop-missing-key");
        let path = dir.join("secrets.json");
        let store = FileStore::new(path.clone());
        store.set("uuid", "kept-session").unwrap();
        let sealed = std::fs::read(&path).unwrap();
        std::fs::remove_file(path.with_extension("key")).unwrap();
        assert!(store.get("uuid").is_err());
        assert!(!path.with_extension("key").exists());
        assert_eq!(std::fs::read(&path).unwrap(), sealed);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_damaged_key_file_is_reported_and_not_replaced() {
        let dir = scratch("damaged");
        std::fs::write(dir.join("secrets.key"), b"short").unwrap();
        let store = FileStore::with_key_file(dir.join("secrets.json"), dir.join("secrets.key"));
        assert!(store.set("uuid", "x").unwrap_err().contains("damaged"));
        assert_eq!(std::fs::read(dir.join("secrets.key")).unwrap(), b"short");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
