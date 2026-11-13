//! The game's official files, fetched from Mojang's servers, each checked against the hash its
//! manifest gives, as the official launcher does. Starling pins the version JSON itself (`research-lock.json`), so the
//! chain of hashes starts from Starling's own files. Nothing of the game ships with Starling:
//! the files land on the player's device, in the official launcher's layout
//! (`versions/<v>/<v>.jar`, `libraries/…`, `assets/indexes|objects`) that horizon-prepare reads.
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// What `research-lock.json` pins about the game files.
#[derive(Clone, Debug)]
pub struct Pins {
    pub version: String,
    /// The version JSON's sha1, then the earlier ones Mojang republished.
    pub manifests: Vec<String>,
    pub client_sha1: String,
    pub server_sha1: String,
}

impl Pins {
    /// Starling's own pins, built in.
    pub fn starling() -> Pins {
        let lock: Value = serde_json::from_str(include_str!("../../../research-lock.json"))
            .expect("research-lock.json is JSON");
        Pins::read(&lock).expect("research-lock.json pins the game")
    }
    pub fn read(lock: &Value) -> Option<Pins> {
        let game = &lock["minecraft"];
        let text = |key: &str| game[key].as_str().map(String::from);
        let mut manifests = vec![text("manifest_sha1")?];
        if let Some(list) = game["accepted_manifest_sha1"].as_array() {
            manifests.extend(list.iter().filter_map(Value::as_str).map(String::from));
        }
        Some(Pins {
            version: text("version")?,
            manifests,
            client_sha1: text("client_sha1")?,
            server_sha1: text("server_sha1")?,
        })
    }
}

/// Mojang's servers, or a mock in tests.
#[derive(Clone, Debug)]
pub struct Sources {
    /// piston-meta: the version JSONs by their sha1.
    pub meta: String,
    /// The asset objects by hash.
    pub resources: String,
}

impl Sources {
    pub fn live() -> Self {
        Self {
            meta: "https://piston-meta.mojang.com".into(),
            resources: "https://resources.download.minecraft.net".into(),
        }
    }
}

/// Which files to fetch beyond the version JSON and the client jar.
#[derive(Clone, Copy, Debug)]
pub struct Wanted {
    pub server: bool,
    pub libraries: bool,
    pub assets: bool,
}

/// How far a download is: what the progress bar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub files: usize,
    pub files_total: usize,
    pub bytes: u64,
    pub bytes_total: u64,
}

/// Where the fetched files are.
#[derive(Clone, Debug)]
pub struct Installed {
    /// The launcher-layout root (holding `versions`, `libraries`, `assets`).
    pub root: PathBuf,
    pub client_jar: PathBuf,
    pub server_jar: Option<PathBuf>,
}

/// One agent for every download of the run: its connections stay open and are used again, so
/// the thousands of small asset files go over the download workers' few connections instead of
/// a connection and a TLS handshake each, which a phone's network (and its router) does not
/// keep up with for long.
pub(crate) fn agent() -> ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT
        .get_or_init(|| {
            ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(20))
                .timeout_read(Duration::from_secs(60))
                // A connection kept for each download worker, and some to spare.
                .max_idle_connections_per_host(8)
                .user_agent(&format!("starling/{}", env!("CARGO_PKG_VERSION")))
                .build()
        })
        .clone()
}

pub(crate) fn sha1_hex(data: &[u8]) -> String {
    let digest = Sha1::digest(data);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// How a try at a download failed: worth another (the network dropped for a moment, as a
/// phone's does when it changes networks, or the server was busy), or not.
pub(crate) enum Failure {
    Again(String),
    Final(String),
}

/// Tries before a download's error is the player's.
const TRIES: u32 = 5;
/// The wait before the second try; it doubles before each one after.
const FIRST_WAIT: Duration = Duration::from_secs(1);

/// Whether a server's refusal may pass with time: its own trouble, or its limit on this address.
/// A mobile network shares one address among many phones, and GitHub answers those with 403 or 429.
pub(crate) fn may_pass(code: u16) -> bool {
    code >= 500 || matches!(code, 403 | 408 | 429)
}

/// `attempt` until it succeeds, a final failure, or [`TRIES`] tries.
pub(crate) fn retrying<T>(mut attempt: impl FnMut() -> Result<T, Failure>) -> Result<T, String> {
    let mut wait = FIRST_WAIT;
    for _ in 1..TRIES {
        match attempt() {
            Ok(done) => return Ok(done),
            Err(Failure::Final(e)) => return Err(e),
            Err(Failure::Again(_)) => {
                std::thread::sleep(wait);
                wait *= 2;
            }
        }
    }
    attempt().map_err(|(Failure::Again(e) | Failure::Final(e))| e)
}

/// The name a message gives a file: the last part of its URL, never more of it.
fn file_name(url: &str) -> &str {
    url.split(['?', '#'])
        .next()
        .and_then(|u| u.rsplit('/').next())
        .unwrap_or("a file")
}

/// A fetch failure in the words the player reads.
fn explain(e: horizon_fetch::Error) -> String {
    use horizon_fetch::Error;
    match e {
        Error::Cancelled => "The download was cancelled.".into(),
        Error::Status { url, status } if may_pass(status) => format!(
            "Mojang's server refused {} ({status}). Try again in a few minutes.",
            file_name(&url)
        ),
        Error::Status { url, status } => {
            format!("Mojang's server refused {} ({status}).", file_name(&url))
        }
        Error::BadHash { url, .. } | Error::BadSize { url, .. } => format!(
            "{} is not the file Mojang lists (its hash differs).",
            file_name(&url)
        ),
        Error::Network { url, .. } => format!(
            "Could not download {}. Check your internet connection.",
            file_name(&url)
        ),
        other => other.to_string(),
    }
}

/// Fetch the pinned version's files into `root`, in the official launcher's layout: what is
/// there already with the right hash is kept, and a half-fetched file continues, so an
/// interrupted download resumes. The version JSON must be one Starling pins; every other file is
/// checked against the hash it lists. The planning and the transfers are horizon-fetch's, which
/// the headset's launcher shares; this adds Starling's pins, the retry policy of a phone's
/// network and its messages.
pub fn download(
    sources: &Sources,
    pins: &Pins,
    root: &Path,
    wanted: Wanted,
    progress: &(dyn Fn(Progress) + Sync),
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Installed, String> {
    use horizon_fetch::mojang::Selection;
    use horizon_fetch::{Item, Options, fetch_all, mojang};
    let v = &pins.version;
    let version_dir = root.join("versions").join(v);
    let manifest_path = version_dir.join(format!("{v}.json"));
    let fetch_pins = horizon_fetch::Pins {
        version: v.clone(),
        client_sha1: pins.client_sha1.clone(),
        manifests: pins.manifests.clone(),
    };
    let transport = horizon_fetch::Ureq::new(format!("starling/{}", env!("CARGO_PKG_VERSION")));
    let options = Options {
        jobs: 4,
        retries: 4,
        wait: FIRST_WAIT,
        growing: true,
        transient: may_pass,
        ..Options::default()
    };
    let stop = AtomicBool::new(false);
    let log = |_: String| {};
    let whole = |url: &str| -> Result<Vec<u8>, String> {
        use horizon_fetch::Transport;
        let mut body = Vec::new();
        transport
            .open(url, 0)
            .map_err(explain)?
            .reader
            .read_to_end(&mut body)
            .map_err(|_| format!("The download of {} was cut off.", file_name(url)))?;
        Ok(body)
    };
    // The version file, from piston-meta by the sha1 Starling pins, unless it is there.
    if !fetch_pins.manifest_accepted(&manifest_path) {
        let sha1 = &pins.manifests[0];
        let url = format!("{}/v1/packages/{sha1}/{v}.json", sources.meta);
        let data = retrying(|| {
            whole(&url).map_err(|e| {
                if e.contains("refused") && !e.contains("Try again") {
                    Failure::Final(e)
                } else {
                    Failure::Again(e)
                }
            })
        })?;
        if &sha1_hex(&data) != sha1 {
            return Err(format!(
                "{} is not the file Mojang lists (its hash differs).",
                file_name(&url)
            ));
        }
        horizon_fetch::write_file(&manifest_path, &data)?;
    }
    let json: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|e| e.to_string())?)
            .map_err(|_| format!("The {v} version file is not valid JSON."))?;
    let selection = Selection {
        server_sha1: wanted.server.then_some(pins.server_sha1.as_str()),
        strict_assets: true,
        objects_base: sources.resources.clone(),
        ..Selection::all("linux")
    };
    let (mut items, _) = mojang::game_files(&fetch_pins, &json, root, &selection)?;
    if !wanted.libraries {
        items.retain(|i| !i.path.starts_with(root.join("libraries")));
    }
    let listed = &json["downloads"]["server"]["sha1"];
    if wanted.server && listed.as_str() != Some(pins.server_sha1.as_str()) {
        return Err(format!(
            "The {v} version file lists another server jar than Starling pins."
        ));
    }
    if json["downloads"]["client"]["sha1"].as_str() != Some(pins.client_sha1.as_str()) {
        return Err(format!(
            "The {v} version file lists another client jar than Starling pins."
        ));
    }
    let hooks = |progress_state: &horizon_fetch::Progress,
                 items: &[Item],
                 f: &mut dyn FnMut() -> Result<(), horizon_fetch::Error>| {
        let total = items.len();
        let bytes_total: u64 = items.iter().filter_map(|i| i.size).sum();
        let done = AtomicBool::new(false);
        let mut result = Ok(());
        std::thread::scope(|scope| {
            scope.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    if cancelled() {
                        stop.store(true, Ordering::Relaxed);
                    }
                    if let Some(s) = progress_state.snapshot() {
                        progress(Progress {
                            files: s.files_done,
                            files_total: s.files_total,
                            bytes: s.bytes_done,
                            bytes_total: s.bytes_total,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            result = f();
            done.store(true, Ordering::Relaxed);
        });
        (total, bytes_total, result)
    };
    let state = horizon_fetch::Progress::new();
    if wanted.assets {
        let index = &json["assetIndex"];
        let (Some(id), Some(url), Some(sha1)) = (
            index["id"].as_str(),
            index["url"].as_str(),
            index["sha1"].as_str(),
        ) else {
            return Err(format!("The {v} version file lists no asset index."));
        };
        let index_item = Item {
            url: url.into(),
            sha1: sha1.into(),
            size: index["size"].as_u64(),
            path: root.join("assets/indexes").join(format!("{id}.json")),
        };
        let (_, _, fetched) = hooks(&state, std::slice::from_ref(&index_item), &mut || {
            fetch_all(
                std::slice::from_ref(&index_item),
                &options,
                &transport,
                &state,
                &stop,
                &log,
            )
        });
        fetched.map_err(explain)?;
        let mut objects: Value =
            serde_json::from_slice(&std::fs::read(&index_item.path).map_err(|e| e.to_string())?)
                .map_err(|_| "The asset index is not valid JSON.".to_string())?;
        // A hash that is not one is never a path: left out, not an error.
        if let Some(map) = objects["objects"].as_object_mut() {
            map.retain(|_, o| {
                o["hash"]
                    .as_str()
                    .is_some_and(|h| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            });
        }
        items.extend(mojang::objects(&objects, root, &selection)?);
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));
    items.dedup_by(|a, b| a.path == b.path);
    let (total, bytes_total, result) = hooks(&state, &items, &mut || {
        fetch_all(&items, &options, &transport, &state, &stop, &log)
    });
    result.map_err(explain)?;
    if cancelled() {
        return Err("The download was cancelled.".into());
    }
    progress(Progress {
        files: total,
        files_total: total,
        bytes: bytes_total,
        bytes_total,
    });
    Ok(Installed {
        root: root.to_path_buf(),
        client_jar: version_dir.join(format!("{v}.jar")),
        server_jar: wanted
            .server
            .then(|| version_dir.join(format!("{v}-server.jar"))),
    })
}

#[cfg(test)]
mod tests;
