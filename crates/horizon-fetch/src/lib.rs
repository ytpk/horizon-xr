//! The player's own download of the pinned Minecraft version from Mojang, for an account the
//! owner gate said owns the game (docs/microsoft-account.md): the version JSON, the client jar,
//! this system's libraries and the assets, each checked against the sha1 (and size) Mojang
//! lists, in the official launcher's layout. Nothing of Mojang's is part of Horizon.
//!
//! What this crate adds over a whole-file fetch: a half-downloaded file is kept as `<name>.partial`
//! and continued with an HTTP `Range` request; the free disk space is checked before the first
//! byte; progress is a model a screen can draw ([`Progress`]); a cancel stops between chunks and
//! keeps what was fetched; and only the chosen languages (and, as a second stage, the sounds)
//! are downloaded ([`mojang::Selection`]).
//!
//! The transport is a trait ([`Transport`]): [`Ureq`] for real downloads, [`Bodies`] around a
//! whole-body function (the launcher's mocks).
mod engine;
pub mod mojang;
mod progress;
mod transport;

pub use engine::{fetch_all, fetch_each, free_space, server_trouble, Item, Options};
pub use progress::{Progress, Snapshot};
pub use transport::{Bodies, Body, Transport, Ureq};

use serde_json::Value;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::Read;
use std::path::Path;

/// Fetches a URL's body (from several threads for a download).
pub type Fetch<'a> = &'a (dyn Fn(&str) -> Result<Vec<u8>, String> + Sync);

/// The text the launcher shows (and compares) when the player cancels.
pub const CANCELLED: &str = "Download cancelled. Download again to go on where it stopped.";

/// Why a download stopped.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The player cancelled; what was fetched is kept.
    Cancelled,
    /// Not enough free space: `need` bytes to fetch, `free` bytes there.
    NoSpace { need: u64, free: u64 },
    /// The server refused or did not have the file (403, 404...).
    Status { url: String, status: u16 },
    /// The file is not the one pinned: its sha1 differs.
    BadHash { url: String, actual: String },
    /// The file has another size than Mojang lists.
    BadSize {
        url: String,
        actual: u64,
        listed: u64,
    },
    /// The connection failed or the transport said so.
    Network { url: String, message: String },
    /// Writing or reading a local file failed.
    Io(String),
    /// Anything else (a bad version file, several failures).
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str(CANCELLED),
            Self::NoSpace { need, free } => write!(
                f,
                "Not enough free space: {} MB needed, {} MB free.",
                need.div_ceil(1 << 20),
                free >> 20
            ),
            Self::Status { url, status } => write!(f, "{url} answered {status}"),
            Self::BadHash { url, actual } => {
                write!(f, "{url} is not the one Horizon pins (sha1 {actual})")
            }
            Self::BadSize {
                url,
                actual,
                listed,
            } => write!(f, "{url} is {actual} bytes, not the {listed} Mojang lists"),
            Self::Network { url, message } => write!(f, "{url} {message}"),
            Self::Io(message) | Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

/// What a version's research-lock.json pins about the game files.
pub struct Pins {
    pub version: String,
    pub client_sha1: String,
    /// The version JSON's sha1, then the earlier ones Mojang republished.
    pub manifests: Vec<String>,
}

impl Pins {
    pub fn read(lock: &Value) -> Option<Pins> {
        let game = &lock["minecraft"];
        let text = |key: &str| game[key].as_str().map(String::from);
        let mut manifests = vec![text("manifest_sha1")?];
        if let Some(list) = game["accepted_manifest_sha1"].as_array() {
            manifests.extend(list.iter().filter_map(Value::as_str).map(String::from));
        }
        Some(Pins {
            version: text("version")?,
            client_sha1: text("client_sha1")?,
            manifests,
        })
    }

    /// Any version from Mojang's version list, by its id and its version file's sha1 (the
    /// list's): the client's sha1 then comes from that checked version file.
    pub fn for_version(id: &str, manifest_sha1: &str) -> Pins {
        Pins {
            version: id.to_string(),
            client_sha1: String::new(),
            manifests: vec![manifest_sha1.to_string()],
        }
    }

    pub fn manifest_accepted(&self, path: &Path) -> bool {
        sha1_file(path).is_some_and(|sha1| self.manifests.contains(&sha1))
    }
}

/// The pinned version JSON at `path`, from piston-meta (by its sha1) unless it is there.
pub fn fetch_manifest(
    pins: &Pins,
    path: &Path,
    fetch: Fetch,
    log: &dyn Fn(String),
) -> Result<(), String> {
    if pins.manifest_accepted(path) {
        return Ok(());
    }
    let (v, sha1) = (&pins.version, &pins.manifests[0]);
    log(format!("Downloading the {v} version file from Mojang"));
    let url = format!("https://piston-meta.mojang.com/v1/packages/{sha1}/{v}.json");
    let body =
        fetch_checked(fetch, &url, sha1).map_err(|e| format!("Mojang's {v} version file {e}"))?;
    write_file(path, &body)
}

pub fn sha1_hex(data: &[u8]) -> String {
    Sha1::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn sha1_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hash = Sha1::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        match file.read(&mut buffer).ok()? {
            0 => break,
            n => hash.update(&buffer[..n]),
        }
    }
    Some(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// A download checked against its sha1; the error says how it differs.
pub fn fetch_checked(fetch: Fetch, url: &str, sha1: &str) -> Result<Vec<u8>, String> {
    let body = fetch(url)?;
    match sha1_hex(&body) {
        actual if actual == sha1 => Ok(body),
        actual => Err(format!("is not the one Horizon pins (sha1 {actual})")),
    }
}

/// Writes a file whole: a partial one never takes its place.
pub fn write_file(path: &Path, data: &[u8]) -> Result<(), String> {
    let error = |e: std::io::Error| format!("{}: {e}", path.display());
    fs::create_dir_all(path.parent().unwrap()).map_err(error)?;
    let partial = path.with_file_name(format!(
        "{}.partial",
        path.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&partial, data)
        .and_then(|_| fs::rename(&partial, path))
        .map_err(error)
}
