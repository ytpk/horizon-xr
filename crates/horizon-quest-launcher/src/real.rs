//! The real account and download backends of the headset's room: `horizon-account` against
//! Microsoft, and `horizon-fetch` plus the bundled runtime for the files. Tested on a computer
//! only where they hold logic (the readiness stamps, the error mapping); the network and the
//! JVM are the headset's.
use crate::machine::{Accounts, Failure, FailureKind, Provisioner, Readiness, Stage};
use horizon_account::{
    Account, Check, DeviceCode, Endpoints, FileStore, MsTokens, Phase, SECRETS_FILE, Secrets,
};
use horizon_fetch::mojang::Stage as FetchStage;
use horizon_fetch::{Error, Progress};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// The secrets file of the room: sealed with a random key in its own file, both mode 0600 in
/// the app's internal folder (`<files>/launcher/`).
pub fn file_store(dir: &Path) -> FileStore {
    FileStore::with_key_file(dir.join(SECRETS_FILE), dir.join("account.key"))
}

/// The game's code for the headset's language (`ru` and `RU` give `ru_ru`); none for English,
/// which is always fetched.
pub fn system_language(language: &str, country: &str) -> Option<String> {
    let language = language.trim().to_lowercase();
    let country = country.trim().to_lowercase();
    if language.len() < 2 || language == "en" || !language.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    Some(if country.is_empty() {
        // A language with no region: the game's usual one for it (`de` is `de_de`).
        format!("{language}_{language}")
    } else {
        format!("{language}_{country}")
    })
}

/// Microsoft's device-code flow with Horizon's client ID.
pub struct QuestAccounts {
    endpoints: Endpoints,
    client_id: Option<String>,
}

impl QuestAccounts {
    pub fn new() -> Self {
        horizon_account::set_transport(horizon_account::Transport {
            user_agent: format!("horizon-quest/{}", env!("CARGO_PKG_VERSION")),
            proxy: None,
        });
        Self {
            endpoints: Endpoints::live(),
            client_id: horizon_account::client_id(),
        }
    }
}

impl Default for QuestAccounts {
    fn default() -> Self {
        Self::new()
    }
}

impl Accounts for QuestAccounts {
    fn configured(&self) -> bool {
        self.client_id.is_some()
    }

    fn begin(&self) -> Result<DeviceCode, String> {
        let id = self
            .client_id
            .as_deref()
            .ok_or(horizon_account::NOT_CONFIGURED)?;
        horizon_account::device_code(&self.endpoints, id)
    }

    fn wait(&self, code: &DeviceCode, cancelled: &dyn Fn() -> bool) -> Result<MsTokens, String> {
        let id = self
            .client_id
            .as_deref()
            .ok_or(horizon_account::NOT_CONFIGURED)?;
        horizon_account::wait_for_sign_in(&self.endpoints, id, code, cancelled)
    }

    fn finish(&self, ms: MsTokens, phase: &dyn Fn(Phase)) -> Result<(Account, Secrets), String> {
        horizon_account::finish_sign_in_with_progress(&self.endpoints, ms, phase)
    }

    fn verify(&self, account: &Account, secrets: Secrets, phase: &dyn Fn(Phase)) -> Check {
        horizon_account::check_with(
            &self.endpoints,
            self.client_id.as_deref(),
            account,
            secrets,
            false,
            phase,
        )
    }
}

/// The folders of the headset's files and the runtime that prepares them.
pub struct QuestProvisioner {
    /// The unpacked exporter bundle (`research-lock.json`, the preparer's scratch in `.local`).
    pub root: PathBuf,
    /// The official-launcher layout Mojang's files are downloaded into.
    pub downloads: PathBuf,
    /// Where the prepared game goes (the app's internal folder).
    pub resources: PathBuf,
    /// A developer's resources pushed to the app's external folder
    /// (`packaging/quest/push_resources.sh`): used when the internal ones are not there.
    pub pushed: Option<PathBuf>,
    /// `libhorizon_jvm.so` in the app's library folder and the unpacked runtime.
    pub launcher: PathBuf,
    pub jre: PathBuf,
    /// Puts the bundle and the runtime in place (from the APK); stamped, so it is quick when
    /// done.
    pub unpack: Box<dyn Fn() -> Result<(), String> + Send + Sync>,
}

const MANIFEST: &str = "horizon-resources.json";

fn has_manifest(dir: &Path) -> bool {
    dir.join(MANIFEST).is_file()
}

impl QuestProvisioner {
    fn languages_present(dir: &Path, languages: &[String]) -> bool {
        languages.iter().filter(|l| l.as_str() != "en_us").all(|l| {
            dir.join(format!("assets/minecraft/lang/{l}.json"))
                .is_file()
        })
    }

    fn stamp(&self, name: &str) -> PathBuf {
        self.downloads.join(name)
    }

    fn core_stamp(&self) -> Option<Vec<String>> {
        let text = fs::read_to_string(self.stamp(".core-complete")).ok()?;
        Some(text.split(',').map(str::to_owned).collect())
    }

    /// The version in `research-lock.json`, for the TV.
    fn lock_version(&self) -> Option<String> {
        let lock: serde_json::Value =
            serde_json::from_slice(&fs::read(self.root.join("research-lock.json")).ok()?).ok()?;
        lock["minecraft"]["version"].as_str().map(str::to_owned)
    }
}

/// How a download's error reads on the TV.
pub fn classify_fetch(e: &Error) -> Failure {
    let kind = match e {
        Error::Cancelled => FailureKind::Cancelled,
        Error::NoSpace { .. } => FailureKind::NoSpace,
        Error::Status { .. } | Error::Network { .. } => FailureKind::Network,
        Error::BadHash { .. } | Error::BadSize { .. } => FailureKind::Checksum,
        Error::Io(m) | Error::Other(m) if m.contains("No space left") => FailureKind::NoSpace,
        Error::Io(_) | Error::Other(_) => FailureKind::Other,
    };
    Failure::new(kind, e.to_string())
}

/// How the preparer's text reads on the TV.
pub fn classify_prepare(message: &str) -> Failure {
    let lower = message.to_lowercase();
    let kind = if lower.contains("no space left") {
        FailureKind::NoSpace
    } else if lower.contains("jvm") || lower.contains("java") || lower.contains("exporting") {
        FailureKind::JvmFailed
    } else {
        FailureKind::Other
    };
    Failure::new(kind, message)
}

impl Provisioner for QuestProvisioner {
    fn version(&self) -> String {
        self.lock_version().unwrap_or_default()
    }

    fn resources(&self) -> PathBuf {
        if has_manifest(&self.resources) {
            return self.resources.clone();
        }
        match &self.pushed {
            Some(pushed) if has_manifest(pushed) => pushed.clone(),
            _ => self.resources.clone(),
        }
    }

    fn readiness(&self, languages: &[String]) -> Readiness {
        let dir = self.resources();
        let wanted: Vec<&str> = languages
            .iter()
            .map(String::as_str)
            .filter(|l| *l != "en_us")
            .collect();
        Readiness {
            // A pushed folder is the developer's own: its languages are what they are.
            game: has_manifest(&dir)
                && (dir != self.resources || Self::languages_present(&dir, languages)),
            sounds: dir.join("assets/minecraft/sounds.json").is_file(),
            core_downloaded: self
                .core_stamp()
                .is_some_and(|have| wanted.iter().all(|l| have.iter().any(|h| h == l))),
            sounds_downloaded: self.stamp(".sounds-complete").is_file(),
        }
    }

    fn download(
        &self,
        stage: Stage,
        languages: &[String],
        progress: &Progress,
        cancel: &AtomicBool,
        log: &dyn Fn(String),
    ) -> Result<(), Failure> {
        (self.unpack)().map_err(|e| Failure::new(FailureKind::Other, e))?;
        let extra: Vec<&str> = languages
            .iter()
            .map(String::as_str)
            .filter(|l| *l != "en_us")
            .collect();
        crate::download(
            &crate::Download {
                root: &self.root,
                into: &self.downloads,
                languages: &extra,
                stage: match stage {
                    Stage::Core => FetchStage::Core,
                    Stage::Sounds => FetchStage::Sounds,
                },
                // Room to prepare: the preparer's scratch, the prepared game, the runtime.
                reserve: 3 << 30,
            },
            progress,
            cancel,
            log,
        )
        .map_err(|e| classify_fetch(&e))?;
        let (name, text) = match stage {
            Stage::Core => (".core-complete", languages.join(",")),
            Stage::Sounds => (".sounds-complete", String::from("1")),
        };
        fs::write(self.stamp(name), text)
            .map_err(|e| Failure::new(FailureKind::Other, e.to_string()))
    }

    fn prepare(
        &self,
        stage: Stage,
        languages: &[String],
        log: &dyn Fn(&str),
        progress: &dyn Fn(f32),
    ) -> Result<(), Failure> {
        (self.unpack)().map_err(|e| Failure::new(FailureKind::Other, e))?;
        let result = match stage {
            Stage::Sounds => {
                crate::import_assets("sounds", &self.root, &self.downloads, &self.resources, log)
                    .map(|()| progress(1.0))
            }
            // Prepared already, a language added: only the language files are copied.
            Stage::Core if has_manifest(&self.resources) => {
                debug_assert!(!languages.is_empty());
                crate::import_assets(
                    "languages",
                    &self.root,
                    &self.downloads,
                    &self.resources,
                    log,
                )
                .map(|()| progress(1.0))
            }
            Stage::Core => crate::prepare(
                &self.root,
                &self.downloads,
                &self.resources,
                &crate::Runtime {
                    launcher: &self.launcher,
                    jre: &self.jre,
                },
                log,
                progress,
            )
            .map(drop),
        };
        result.map_err(|e| classify_prepare(&e))
    }

    fn cleanup(&self) {
        // The exporters' scratch (about 409 MB): everything the game reads is in `resources`.
        let scratch = self.root.join(".local");
        if let Err(e) = fs::remove_dir_all(&scratch)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("Horizon room: {}: {e}", scratch.display());
        }
    }

    fn forget_sounds_download(&self) {
        let stamp = self.stamp(".sounds-complete");
        if let Err(e) = fs::remove_file(&stamp)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("Horizon room: {}: {e}", stamp.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_headsets_language_becomes_a_game_language_code() {
        assert_eq!(system_language("ru", "RU"), Some("ru_ru".into()));
        assert_eq!(system_language("pt", "BR"), Some("pt_br".into()));
        assert_eq!(system_language("de", ""), Some("de_de".into()));
        assert_eq!(system_language("en", "GB"), None);
        assert_eq!(system_language("", ""), None);
        assert_eq!(system_language("../", "x"), None);
    }

    #[test]
    fn download_errors_are_told_apart_for_the_tv() {
        assert_eq!(
            classify_fetch(&Error::Cancelled).kind,
            FailureKind::Cancelled
        );
        let net = Error::Network {
            url: "https://x".into(),
            message: "timed out".into(),
        };
        assert_eq!(classify_fetch(&net).kind, FailureKind::Network);
        let bad = Error::BadHash {
            url: "u".into(),
            actual: "0".into(),
        };
        assert_eq!(classify_fetch(&bad).kind, FailureKind::Checksum);
        assert_eq!(
            classify_fetch(&Error::NoSpace { need: 5, free: 1 }).kind,
            FailureKind::NoSpace
        );
        assert_eq!(
            classify_prepare("Exported the thing: the JVM exited 1").kind,
            FailureKind::JvmFailed
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quest-real-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn provisioner(dir: &Path) -> QuestProvisioner {
        QuestProvisioner {
            root: dir.join("root"),
            downloads: dir.join("downloads"),
            resources: dir.join("resources"),
            pushed: Some(dir.join("pushed")),
            launcher: dir.join("jvm"),
            jre: dir.join("jre"),
            unpack: Box::new(|| Ok(())),
        }
    }

    #[test]
    fn readiness_follows_the_files_the_stamps_and_the_chosen_languages() {
        let dir = scratch("ready");
        let p = provisioner(&dir);
        fs::create_dir_all(&p.downloads).unwrap();
        let en = vec!["en_us".to_owned()];
        let both = vec!["en_us".to_owned(), "ru_ru".to_owned()];
        assert_eq!(p.readiness(&en), Readiness::default());
        // Downloaded for English only: a second language needs another download.
        fs::write(p.downloads.join(".core-complete"), "en_us").unwrap();
        assert!(p.readiness(&en).core_downloaded);
        assert!(!p.readiness(&both).core_downloaded);
        // Prepared: the manifest and the language file.
        fs::create_dir_all(p.resources.join("assets/minecraft/lang")).unwrap();
        fs::write(p.resources.join(MANIFEST), "{}").unwrap();
        assert!(p.readiness(&en).game);
        assert!(!p.readiness(&both).game, "the Russian file is missing");
        fs::write(p.resources.join("assets/minecraft/lang/ru_ru.json"), "{}").unwrap();
        assert!(p.readiness(&both).game);
        assert!(!p.readiness(&both).sounds);
        fs::write(p.resources.join("assets/minecraft/sounds.json"), "{}").unwrap();
        assert!(p.readiness(&both).sounds);
        fs::write(p.downloads.join(".sounds-complete"), "1").unwrap();
        assert!(p.readiness(&both).sounds_downloaded);
        p.forget_sounds_download();
        assert!(!p.readiness(&both).sounds_downloaded);
        p.forget_sounds_download(); // Already gone: nothing to do.
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pushed_resources_count_as_ready_until_the_internal_ones_exist() {
        let dir = scratch("pushed");
        let p = provisioner(&dir);
        fs::create_dir_all(dir.join("pushed")).unwrap();
        fs::write(dir.join("pushed").join(MANIFEST), "{}").unwrap();
        let both = vec!["en_us".to_owned(), "ru_ru".to_owned()];
        assert!(p.readiness(&both).game);
        assert_eq!(p.resources(), dir.join("pushed"));
        fs::create_dir_all(&p.resources).unwrap();
        fs::write(p.resources.join(MANIFEST), "{}").unwrap();
        assert_eq!(p.resources(), dir.join("resources"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_scratch_goes_after_a_prepare_and_the_resources_stay() {
        let dir = scratch("cleanup");
        let p = provisioner(&dir);
        fs::create_dir_all(p.root.join(".local/horizon-client")).unwrap();
        fs::write(p.root.join(".local/big"), vec![0u8; 1024]).unwrap();
        fs::write(p.root.join("research-lock.json"), "{}").unwrap();
        fs::create_dir_all(&p.resources).unwrap();
        fs::write(p.resources.join(MANIFEST), "{}").unwrap();
        p.cleanup();
        assert!(!p.root.join(".local").exists());
        assert!(p.root.join("research-lock.json").exists());
        assert!(p.resources.join(MANIFEST).exists());
        p.cleanup(); // again: nothing to remove is fine
        fs::remove_dir_all(dir).unwrap();
    }
}
