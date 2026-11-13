//! The downloader against a local HTTP server (std only) that serves small fake files, as Mojang's
//! does: Range requests, a connection cut half way, a 403, a wrong body, a full disk (injected)
//! and a cancel. Never Mojang's files.
use horizon_fetch::mojang::{download, Selection, Stage};
use horizon_fetch::{fetch_all, sha1_hex, Error, Item, Options, Pins, Progress, Ureq};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default, Clone)]
struct Route {
    body: Vec<u8>,
    /// Answer this status with no body instead.
    status: Option<u16>,
    /// The first request gets only this many bytes, then the connection closes.
    cut_first_at: Option<usize>,
    /// Write 16 KB at a time with a pause.
    slow: bool,
}

#[derive(Default)]
struct State {
    routes: HashMap<String, Route>,
    /// (path, the Range's start) of every request, in order.
    asked: Vec<(String, Option<u64>)>,
    cut_done: Vec<String>,
}

struct Mock {
    base: String,
    state: Arc<Mutex<State>>,
}

impl Mock {
    fn start(routes: Vec<(&str, Route)>) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State {
            routes: routes
                .into_iter()
                .map(|(p, r)| (p.to_string(), r))
                .collect(),
            ..Default::default()
        }));
        let shared = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let state = shared.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let path = line.split(' ').nth(1).unwrap_or("/").to_string();
                    let mut range = None;
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).unwrap();
                        if header.trim().is_empty() {
                            break;
                        }
                        if let Some(r) = header.to_lowercase().strip_prefix("range: bytes=") {
                            range = r.trim().trim_end_matches('-').parse::<u64>().ok();
                        }
                    }
                    let (route, cut) = {
                        let mut s = state.lock().unwrap();
                        s.asked.push((path.clone(), range));
                        let route = s.routes.get(&path).cloned();
                        let cut = route.as_ref().is_some_and(|r| r.cut_first_at.is_some())
                            && !s.cut_done.contains(&path);
                        if cut {
                            s.cut_done.push(path.clone());
                        }
                        (route, cut)
                    };
                    let Some(route) = route else {
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    };
                    if let Some(status) = route.status {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        return;
                    }
                    let from = range.unwrap_or(0) as usize;
                    let body = &route.body[from.min(route.body.len())..];
                    let head = match range {
                        Some(_) => format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            route.body.len() - 1,
                            route.body.len(),
                            body.len()
                        ),
                        None => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()),
                    };
                    let _ = stream.write_all(head.as_bytes());
                    let send = if cut {
                        &body[..route.cut_first_at.unwrap().min(body.len())]
                    } else {
                        body
                    };
                    for chunk in send.chunks(16 << 10) {
                        if stream.write_all(chunk).is_err() {
                            return;
                        }
                        if route.slow {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                    let _ = stream.flush();
                    let _ = stream.shutdown(Shutdown::Both);
                });
            }
        });
        Mock { base, state }
    }

    fn asked(&self) -> Vec<(String, Option<u64>)> {
        self.state.lock().unwrap().asked.clone()
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("horizon-fetch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

fn item(mock: &Mock, path: &str, body: &[u8], dir: &std::path::Path) -> Item {
    Item {
        url: format!("{}{path}", mock.base),
        sha1: sha1_hex(body),
        size: Some(body.len() as u64),
        path: dir.join(path.trim_start_matches('/')),
    }
}

static NEVER: AtomicBool = AtomicBool::new(false);

fn run(items: &[Item], options: &Options, cancel: &AtomicBool) -> Result<(), Error> {
    fetch_all(
        items,
        options,
        &Ureq::new("horizon-fetch-test"),
        &Progress::new(),
        cancel,
        &|_| {},
    )
}

#[test]
fn a_connection_cut_half_way_continues_with_a_range_request() {
    let body = data(300_000);
    let mock = Mock::start(vec![(
        "/big.jar",
        Route {
            body: body.clone(),
            cut_first_at: Some(100_000),
            ..Default::default()
        },
    )]);
    let dir = scratch("cut");
    let items = [item(&mock, "/big.jar", &body, &dir)];
    run(&items, &Options::default(), &NEVER).unwrap();
    assert_eq!(std::fs::read(&items[0].path).unwrap(), body);
    assert!(!dir.join("big.jar.partial").exists());
    let asked = mock.asked();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert_eq!(asked[0].1, None);
    // The second request starts exactly where the first stopped (what reached the disk).
    let from = asked[1].1.expect("the retry asks for a range");
    assert!(from > 0 && from <= 100_000, "{from}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_partial_file_from_an_earlier_run_fetches_only_the_rest() {
    let body = data(200_000);
    let mock = Mock::start(vec![(
        "/lib.jar",
        Route {
            body: body.clone(),
            ..Default::default()
        },
    )]);
    let dir = scratch("earlier");
    let items = [item(&mock, "/lib.jar", &body, &dir)];
    std::fs::write(dir.join("lib.jar.partial"), &body[..150_000]).unwrap();
    let progress = Progress::new();
    fetch_all(
        &items,
        &Options::default(),
        &Ureq::new("t"),
        &progress,
        &NEVER,
        &|_| {},
    )
    .unwrap();
    assert_eq!(std::fs::read(&items[0].path).unwrap(), body);
    assert_eq!(mock.asked(), vec![("/lib.jar".to_string(), Some(150_000))]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_wrong_sha1_is_refused_and_leaves_no_file() {
    let wanted = data(5000);
    let mut served = wanted.clone();
    served[10] ^= 1;
    let mock = Mock::start(vec![(
        "/x.jar",
        Route {
            body: served,
            ..Default::default()
        },
    )]);
    let dir = scratch("sha");
    let items = [item(&mock, "/x.jar", &wanted, &dir)];
    let error = run(&items, &Options::default(), &NEVER).unwrap_err();
    assert!(
        matches!(&error, Error::BadHash { actual, .. } if *actual != items[0].sha1),
        "{error}"
    );
    assert!(error.to_string().contains("is not the one Horizon pins"));
    assert!(!items[0].path.exists());
    assert!(!dir.join("x.jar.partial").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_403_stops_at_once_without_retries() {
    let body = data(100);
    let mock = Mock::start(vec![(
        "/denied",
        Route {
            status: Some(403),
            ..Default::default()
        },
    )]);
    let dir = scratch("403");
    let items = [item(&mock, "/denied", &body, &dir)];
    let error = run(&items, &Options::default(), &NEVER).unwrap_err();
    assert!(
        matches!(error, Error::Status { status: 403, .. }),
        "{error}"
    );
    assert_eq!(mock.asked().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_refusal_the_caller_calls_transient_is_tried_again_and_then_reported() {
    let body = data(100);
    let mock = Mock::start(vec![(
        "/limited",
        Route {
            status: Some(429),
            ..Default::default()
        },
    )]);
    let dir = scratch("429");
    let items = [item(&mock, "/limited", &body, &dir)];
    // A shared mobile address's limit: Starling's phone app waits and tries again.
    let options = Options {
        retries: 2,
        wait: Duration::from_millis(1),
        growing: true,
        transient: |status| status == 429 || horizon_fetch::server_trouble(status),
        ..Options::default()
    };
    let error = run(&items, &options, &NEVER).unwrap_err();
    assert!(
        matches!(
            error,
            Error::Network { .. } | Error::Status { status: 429, .. }
        ),
        "{error}"
    );
    assert_eq!(mock.asked().len(), 3, "one try and two more");
    // The default policy keeps 4xx final (the 403 test above).
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_full_disk_is_reported_before_any_request() {
    let body = data(10_000_000);
    let mock = Mock::start(vec![(
        "/huge.jar",
        Route {
            body: body.clone(),
            ..Default::default()
        },
    )]);
    let dir = scratch("full");
    let items = [item(&mock, "/huge.jar", &body, &dir)];
    let options = Options {
        free_space: |_| Some(4_000_000),
        ..Options::default()
    };
    let error = run(&items, &options, &NEVER).unwrap_err();
    assert_eq!(
        error,
        Error::NoSpace {
            need: 10_000_000,
            free: 4_000_000
        }
    );
    assert!(mock.asked().is_empty());
    // The reserve counts: room for the file alone is not enough when 1 MB must stay free.
    let options = Options {
        free_space: |_| Some(10_500_000),
        reserve: 1_000_000,
        ..Options::default()
    };
    assert!(matches!(
        run(&items, &options, &NEVER),
        Err(Error::NoSpace { .. })
    ));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_cancel_stops_mid_file_keeps_it_and_the_next_run_goes_on() {
    let body = data(2_000_000);
    let mock = Mock::start(vec![(
        "/slow.jar",
        Route {
            body: body.clone(),
            slow: true,
            ..Default::default()
        },
    )]);
    let dir = scratch("cancel");
    let items = [item(&mock, "/slow.jar", &body, &dir)];
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Progress::new());
    let watcher = {
        let (cancel, progress) = (cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            while progress.snapshot().is_none_or(|s| s.bytes_done < 200_000) {
                std::thread::sleep(Duration::from_millis(5));
            }
            cancel.store(true, Relaxed);
        })
    };
    let first = fetch_all(
        &items,
        &Options::default(),
        &Ureq::new("t"),
        &progress,
        &cancel,
        &|_| {},
    );
    watcher.join().unwrap();
    assert_eq!(first, Err(Error::Cancelled));
    assert!(!items[0].path.exists());
    let kept = std::fs::metadata(dir.join("slow.jar.partial"))
        .unwrap()
        .len();
    assert!((200_000..2_000_000).contains(&kept), "{kept}");
    // Not cancelled any more: the rest comes with a Range from what was kept.
    run(&items, &Options::default(), &NEVER).unwrap();
    assert_eq!(std::fs::read(&items[0].path).unwrap(), body);
    assert_eq!(mock.asked().last().unwrap().1, Some(kept));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_whole_file_already_there_is_not_asked_for_and_progress_ends() {
    let (a, b) = (data(30_000), data(50_000));
    let mock = Mock::start(vec![
        (
            "/a",
            Route {
                body: a.clone(),
                ..Default::default()
            },
        ),
        (
            "/b",
            Route {
                body: b.clone(),
                ..Default::default()
            },
        ),
    ]);
    let dir = scratch("progress");
    let items = [item(&mock, "/a", &a, &dir), item(&mock, "/b", &b, &dir)];
    std::fs::write(&items[0].path, &a).unwrap();
    let progress = Progress::new();
    fetch_all(
        &items,
        &Options::default(),
        &Ureq::new("t"),
        &progress,
        &NEVER,
        &|_| {},
    )
    .unwrap();
    // `a` was whole: never asked for.
    assert_eq!(mock.asked(), vec![("/b".to_string(), None)]);
    assert_eq!(
        progress.snapshot(),
        None,
        "no snapshot once the download is over"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// A version file and index served by the mock, with two sounds and three languages.
fn mojang_mock(dir: &std::path::Path) -> (Mock, Pins, Vec<(String, Vec<u8>)>) {
    let objects: Vec<(String, Vec<u8>)> = [
        ("minecraft/lang/en_us.json", b"{\"a\":1}".to_vec()),
        ("minecraft/lang/ru_ru.json", b"{\"a\":2}".to_vec()),
        ("minecraft/lang/de_de.json", b"{\"a\":3}".to_vec()),
        ("minecraft/textures/stone.png", b"png".to_vec()),
        ("minecraft/sounds/a.ogg", b"ogg-a".to_vec()),
        ("minecraft/sounds.json", b"{}".to_vec()),
    ]
    .into_iter()
    .map(|(n, d)| (n.to_string(), d))
    .collect();
    let mut index = serde_json::Map::new();
    let mut routes = Vec::new();
    let mut routes_owned: Vec<(String, Route)> = Vec::new();
    for (name, body) in &objects {
        let hash = sha1_hex(body);
        index.insert(
            name.clone(),
            serde_json::json!({"hash": hash, "size": body.len()}),
        );
        routes_owned.push((
            format!("/objects/{}/{hash}", &hash[..2]),
            Route {
                body: body.clone(),
                ..Default::default()
            },
        ));
    }
    let index = serde_json::json!({ "objects": index }).to_string();
    let client = b"client".to_vec();
    routes_owned.push((
        "/client.jar".into(),
        Route {
            body: client.clone(),
            ..Default::default()
        },
    ));
    routes_owned.push((
        "/34.json".into(),
        Route {
            body: index.clone().into_bytes(),
            ..Default::default()
        },
    ));
    for (p, r) in &routes_owned {
        routes.push((p.as_str(), r.clone()));
    }
    let mock = Mock::start(routes);
    let manifest = serde_json::json!({
        "id": "26.3",
        "assetIndex": {"id": "34", "sha1": sha1_hex(index.as_bytes()), "size": index.len(), "url": format!("{}/34.json", mock.base)},
        "downloads": {"client": {"sha1": sha1_hex(&client), "size": client.len(), "url": format!("{}/client.jar", mock.base)}},
        "libraries": [],
    })
    .to_string();
    // The version file is already there and pinned, so nothing asks piston-meta.
    let path = dir.join("versions/26.3/26.3.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &manifest).unwrap();
    let pins = Pins::read(&serde_json::json!({"minecraft": {
        "version": "26.3", "client_sha1": sha1_hex(&client), "manifest_sha1": sha1_hex(manifest.as_bytes()),
    }}))
    .unwrap();
    (mock, pins, objects)
}

fn selection(mock: &Mock, languages: &[&str], stage: Stage) -> Selection<'static> {
    Selection {
        languages: Some(languages.iter().map(|s| s.to_string()).collect()),
        stage,
        strict_assets: true,
        objects_base: format!("{}/objects", mock.base),
        ..Selection::all("linux")
    }
}

fn stored(dir: &std::path::Path, body: &[u8]) -> bool {
    let hash = sha1_hex(body);
    dir.join(format!("assets/objects/{}/{hash}", &hash[..2]))
        .is_file()
}

#[test]
fn only_the_chosen_languages_come_first_and_the_sounds_after() {
    let dir = scratch("languages");
    let (mock, pins, _) = mojang_mock(&dir);
    let go = |sel: &Selection| {
        download(
            &pins,
            &dir,
            sel,
            &Options::default(),
            &Ureq::new("t"),
            &Progress::new(),
            &NEVER,
            &|_| {},
        )
    };
    go(&selection(&mock, &["en_us", "ru_ru"], Stage::Core)).unwrap();
    assert!(dir.join("versions/26.3/26.3.jar").is_file());
    assert!(stored(&dir, b"{\"a\":1}") && stored(&dir, b"{\"a\":2}") && stored(&dir, b"png"));
    assert!(!stored(&dir, b"{\"a\":3}"), "German was not chosen");
    assert!(
        !stored(&dir, b"ogg-a") && !stored(&dir, b"{}"),
        "sounds are the second stage"
    );
    let sound = sha1_hex(b"ogg-a");
    assert!(
        mock.asked().iter().all(|(p, _)| !p.ends_with(&sound)),
        "the core stage asked for a sound"
    );
    go(&selection(&mock, &["en_us", "ru_ru"], Stage::Sounds)).unwrap();
    assert!(stored(&dir, b"ogg-a") && stored(&dir, b"{}"));
    assert!(!stored(&dir, b"{\"a\":3}"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_sound_that_cannot_be_fetched_fails_a_strict_download_only() {
    let dir = scratch("strict-sounds");
    let (mock, pins, _) = mojang_mock(&dir);
    let hash = sha1_hex(b"ogg-a");
    mock.state
        .lock()
        .unwrap()
        .routes
        .remove(&format!("/objects/{}/{hash}", &hash[..2]));
    let go = |sel: &Selection| {
        download(
            &pins,
            &dir,
            sel,
            &Options::default(),
            &Ureq::new("t"),
            &Progress::new(),
            &NEVER,
            &|_| {},
        )
    };
    let error = go(&selection(&mock, &["en_us"], Stage::Sounds)).unwrap_err();
    assert!(error.to_string().contains("answered 404"), "{error}");
    let mut lenient = selection(&mock, &["en_us"], Stage::Sounds);
    lenient.strict_assets = false;
    go(&lenient).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_missing_core_asset_fails_a_strict_download_but_not_a_sounds_one() {
    let dir = scratch("strict");
    let (mock, pins, _) = mojang_mock(&dir);
    let hash = sha1_hex(b"png");
    mock.state
        .lock()
        .unwrap()
        .routes
        .remove(&format!("/objects/{}/{hash}", &hash[..2]));
    let go = |sel: &Selection| {
        download(
            &pins,
            &dir,
            sel,
            &Options::default(),
            &Ureq::new("t"),
            &Progress::new(),
            &NEVER,
            &|_| {},
        )
    };
    let error = go(&selection(&mock, &["en_us"], Stage::Core)).unwrap_err();
    assert!(error.to_string().contains("answered 404"), "{error}");
    let mut lenient = selection(&mock, &["en_us"], Stage::Core);
    lenient.strict_assets = false;
    go(&lenient).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
