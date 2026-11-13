//! The pinned version's files from Mojang's version file: what to fetch ([`plan`]) and the
//! download itself ([`download`]), in the official launcher's folder layout.
use crate::{
    fetch_all, fetch_each, fetch_manifest, Bodies, Error, Fetch, Item, Options, Pins, Progress,
    Transport,
};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub const OBJECTS: &str = "https://resources.download.minecraft.net";

/// Which files of the pinned version to fetch.
pub struct Selection<'a> {
    /// Mojang's name for this system in a library's rules: `linux`, `osx`, `windows`. The Quest
    /// is `linux` (an Android build takes the Linux libraries; the exporters use none of them
    /// natively).
    pub os: &'a str,
    /// Language codes (`en_us`, `ru_ru`) of the `lang` assets to fetch; `None` for all of them.
    pub languages: Option<Vec<String>>,
    pub stage: Stage,
    /// Whether an asset that cannot be fetched fails the download (the Quest, whose next step
    /// copies them all or none) or is a line in the log (the launcher).
    pub strict_assets: bool,
    /// The pinned server jar's sha1: also fetch the server jar (the preparer's data generator
    /// runs it) as `versions/<v>/<v>-server.jar`.
    pub server_sha1: Option<&'a str>,
    /// Where the asset objects are; Mojang's, unless a test points it at a mock.
    pub objects_base: String,
}

impl<'a> Selection<'a> {
    /// Everything, as the desktop launcher downloads it.
    pub fn all(os: &'a str) -> Self {
        Self {
            os,
            languages: None,
            stage: Stage::All,
            strict_assets: false,
            server_sha1: None,
            objects_base: OBJECTS.into(),
        }
    }
}

/// Which part of the download: the game first, the sounds (the largest part) apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    All,
    /// The client, libraries, logging config, asset index and every asset but the sounds.
    Core,
    /// Only `minecraft/sounds/` and `sounds.json`.
    Sounds,
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn text(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .map(String::from)
        .ok_or_else(|| format!("The version file has no {key} in {v}"))
}

/// A path from the version file, refused if it could leave the folder.
pub fn relative(path: &str) -> Result<PathBuf, String> {
    let safe = !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    safe.then(|| PathBuf::from(path))
        .ok_or_else(|| format!("Unsafe path {path:?} in the version file"))
}

/// Whether a library's rules let this system (`osx`, `windows`, `linux`) use it: the last
/// rule that matches decides.
pub fn allowed(library: &Value, os: &str) -> bool {
    let Some(rules) = library["rules"].as_array() else {
        return true;
    };
    rules
        .iter()
        .rfind(|rule| rule["os"]["name"].as_str().is_none_or(|name| name == os))
        .is_some_and(|rule| rule["action"] == "allow")
}

/// The classifier holding a library's natives for `os` (`natives-osx`), from its `natives` map;
/// `${arch}` is this system's word size, as old versions name Windows' natives.
pub fn native_classifier(library: &Value, os: &str) -> Option<String> {
    let arch = if cfg!(target_pointer_width = "64") {
        "64"
    } else {
        "32"
    };
    library["natives"][os]
        .as_str()
        .map(|key| key.replace("${arch}", arch))
}

/// The language code of a `minecraft/lang/<code>.json` asset name.
fn language_of(name: &str) -> Option<String> {
    let file = name.strip_prefix("minecraft/lang/")?;
    Some(
        file.rsplit_once('.')
            .map_or(file, |(code, _)| code)
            .to_lowercase(),
    )
}

fn is_sound(name: &str) -> bool {
    name.starts_with("minecraft/sounds/") || name == "minecraft/sounds.json"
}

/// The asset index's objects of the selection, once each.
pub fn objects(index: &Value, dir: &Path, sel: &Selection) -> Result<Vec<Item>, String> {
    let objects = index["objects"]
        .as_object()
        .ok_or("The asset index has no objects")?;
    let mut chosen: std::collections::BTreeMap<&str, u64> = Default::default();
    for (name, object) in objects {
        let sound = is_sound(name);
        let wanted = match sel.stage {
            Stage::All => true,
            Stage::Core => !sound,
            Stage::Sounds => sound,
        };
        let language_ok = match (&sel.languages, language_of(name)) {
            (Some(wanted), Some(code)) => wanted.iter().any(|w| w.to_lowercase() == code),
            _ => true,
        };
        if !wanted || !language_ok {
            continue;
        }
        if let (Some(hash), Some(size)) = (object["hash"].as_str(), object["size"].as_u64()) {
            chosen.insert(hash, size);
        }
    }
    chosen
        .into_iter()
        .map(|(hash, size)| {
            if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("Bad asset hash {hash:?} in the asset index"));
            }
            let path = format!("{}/{hash}", &hash[..2]);
            Ok(Item {
                url: format!("{}/{path}", sel.objects_base),
                sha1: hash.to_string(),
                size: Some(size),
                path: dir.join("assets/objects").join(path),
            })
        })
        .collect()
}

/// The game's own files from the version file: client jar, libraries for `os`, the logging
/// config, and the server jar when asked. Returns them with how many are libraries.
pub fn game_files(
    pins: &Pins,
    manifest: &Value,
    dir: &Path,
    sel: &Selection,
) -> Result<(Vec<Item>, usize), String> {
    let v = &pins.version;
    let versions = dir.join("versions").join(v);
    let client = &manifest["downloads"]["client"];
    // A version Horizon doesn't pin takes its client's sha1 from its version file, itself
    // checked against the sha1 Mojang's version list gives (`Pins::for_version`).
    let client_sha1 = match pins.client_sha1.as_str() {
        "" => text(client, "sha1")?,
        pinned => pinned.to_string(),
    };
    let mut game = vec![Item {
        url: text(client, "url")?,
        sha1: client_sha1,
        size: client["size"].as_u64(),
        path: versions.join(format!("{v}.jar")),
    }];
    if let Some(sha1) = sel.server_sha1 {
        let server = &manifest["downloads"]["server"];
        game.push(Item {
            url: text(server, "url")?,
            sha1: sha1.to_string(),
            size: server["size"].as_u64(),
            path: versions.join(format!("{v}-server.jar")),
        });
    }
    let first_library = game.len();
    let libraries = manifest["libraries"]
        .as_array()
        .ok_or("The version file has no libraries")?;
    for library in libraries.iter().filter(|l| allowed(l, sel.os)) {
        // Before 1.19 a library's natives for this system are a classifier of its own.
        let native = native_classifier(library, sel.os)
            .map(|key| &library["downloads"]["classifiers"][key.as_str()]);
        for artifact in [Some(&library["downloads"]["artifact"]), native]
            .into_iter()
            .flatten()
            .filter(|a| !a.is_null())
        {
            game.push(Item {
                url: text(artifact, "url")?,
                sha1: text(artifact, "sha1")?,
                size: artifact["size"].as_u64(),
                path: dir
                    .join("libraries")
                    .join(relative(&text(artifact, "path")?)?),
            });
        }
    }
    let libraries = game.len() - first_library;
    // The logging setup the game is started with (`logging.client.argument`): for 1.7 to 1.18,
    // Mojang's fixed one is what keeps Log4Shell out.
    let logging = &manifest["logging"]["client"]["file"];
    if !logging.is_null() {
        game.push(Item {
            url: text(logging, "url")?,
            sha1: text(logging, "sha1")?,
            size: logging["size"].as_u64(),
            path: dir
                .join("assets/log_configs")
                .join(relative(&text(logging, "id")?)?),
        });
    }
    Ok((game, libraries))
}

/// Downloads the selection of the pinned version into `dir`; files already there and whole are
/// kept, half-fetched ones continue. See [`Selection`] for what is fetched and what an asset
/// that fails means.
#[allow(clippy::too_many_arguments)]
pub fn download(
    pins: &Pins,
    dir: &Path,
    sel: &Selection,
    options: &Options,
    transport: &dyn Transport,
    progress: &Progress,
    cancel: &AtomicBool,
    log: &dyn Fn(String),
) -> Result<(), Error> {
    let v = &pins.version;
    let versions = dir.join("versions").join(v);
    let manifest_path = versions.join(format!("{v}.json"));
    // The version file and the asset index are small: fetched whole, checked, written whole.
    let whole = |url: &str| -> Result<Vec<u8>, String> {
        let mut body = Vec::new();
        let opened = transport.open(url, 0).map_err(|e| e.to_string())?;
        std::io::Read::read_to_end(&mut { opened.reader }, &mut body)
            .map_err(|e| format!("{url} stopped ({e})"))?;
        Ok(body)
    };
    // Cancel is polled by the transfers; the first fetches here are a few KB.
    fetch_manifest(pins, &manifest_path, &whole, log).map_err(Error::Other)?;
    let manifest = read_json(&manifest_path).map_err(Error::Other)?;
    if sel.stage != Stage::Sounds {
        let (game, libraries) = game_files(pins, &manifest, dir, sel).map_err(Error::Other)?;
        log(format!(
            "Downloading Minecraft {v} from Mojang: the game and {libraries} libraries"
        ));
        fetch_all(&game, options, transport, progress, cancel, log)?;
    }
    let index = &manifest["assetIndex"];
    let field = |key: &str| text(index, key).map_err(Error::Other);
    let index = Item {
        url: field("url")?,
        sha1: field("sha1")?,
        size: index["size"].as_u64(),
        path: dir
            .join("assets/indexes")
            .join(relative(&format!("{}.json", field("id")?)).map_err(Error::Other)?),
    };
    let required = sel.strict_assets;
    let assets = fetch_all(
        std::slice::from_ref(&index),
        options,
        transport,
        progress,
        cancel,
        log,
    )
    .and_then(|()| {
        let objects = objects(&read_json(&index.path).map_err(Error::Other)?, dir, sel)
            .map_err(Error::Other)?;
        let what = if sel.stage == Stage::Sounds {
            "the sounds"
        } else {
            "the assets"
        };
        log(format!("Downloading {what} ({} files)", objects.len()));
        // A file that fails does not stop the others: the next download fetches what is left.
        fetch_each(
            &objects, options, transport, progress, cancel, log, required,
        )
    });
    match assets {
        Err(Error::Cancelled) => Err(Error::Cancelled),
        Err(e @ Error::NoSpace { .. }) => Err(e),
        Err(e) if required => Err(e),
        Err(e) => {
            log(format!("The sounds could not all be downloaded: {e}"));
            Ok(())
        }
        Ok(()) => Ok(()),
    }
}

/// [`download`] through a whole-body function, for callers that bring their own fetch.
#[allow(clippy::too_many_arguments)]
pub fn download_with(
    pins: &Pins,
    dir: &Path,
    sel: &Selection,
    options: &Options,
    fetch: Fetch,
    progress: &Progress,
    cancel: &AtomicBool,
    log: &dyn Fn(String),
) -> Result<(), Error> {
    download(
        pins,
        dir,
        sel,
        options,
        &Bodies(fetch),
        progress,
        cancel,
        log,
    )
}
