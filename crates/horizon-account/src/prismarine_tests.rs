//! Independent HTTP fixtures: no Microsoft account, external service or real credential.
use super::*;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
};

fn verify(headers: &[(String, String)], body: &str, path: &str, jwk: &Value) {
    let header = headers
        .iter()
        .find(|(k, _)| k == "signature")
        .expect("signed request")
        .1
        .clone();
    let bytes = STANDARD.decode(header).unwrap();
    assert_eq!(bytes.len(), 76);
    assert_eq!(&bytes[..4], &1u32.to_be_bytes());
    let mut point = vec![4];
    for key in ["x", "y"] {
        point.extend(URL_SAFE_NO_PAD.decode(jwk[key].as_str().unwrap()).unwrap());
    }
    let key = VerifyingKey::from_sec1_bytes(&point).unwrap();
    let mut message = Vec::new();
    message.extend(&bytes[..4]);
    message.push(0);
    message.extend(&bytes[4..12]);
    message.push(0);
    for item in ["POST", path, "", body] {
        message.extend(item.as_bytes());
        message.push(0);
    }
    let signature = Signature::from_slice(&bytes[12..]).unwrap();
    key.verify(&message, &signature)
        .expect("signature binds exact path and request bytes");
    message.push(b'x');
    assert!(key.verify(&message, &signature).is_err());
}

fn fixture(owned: bool) -> (Endpoints, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let ep = Endpoints {
        flow: MicrosoftFlow::PrismarineLive,
        client_id: Some(PRISMARINE_CLIENT_ID.into()),
        microsoft: format!("{base}/live"),
        xbox_user: format!("{base}/xbox/user/authenticate"),
        xbox_device: format!("{base}/xbox/device/authenticate"),
        xbox_title: format!("{base}/xbox/title/authenticate"),
        xsts: format!("{base}/xbox/xsts/authorize"),
        minecraft: format!("{base}/mc"),
    };
    let handle = std::thread::spawn(move || {
        let paths = [
            "/live/oauth20_connect.srf",
            "/live/oauth20_token.srf",
            "/xbox/user/authenticate",
            "/xbox/device/authenticate",
            "/xbox/title/authenticate",
            "/xbox/xsts/authorize",
            "/mc/launcher/login",
            "/mc/entitlements/mcstore",
            "/mc/minecraft/profile",
            "/live/oauth20_token.srf",
        ];
        let mut jwk = Value::Null;
        for (step, path) in paths.iter().enumerate().take(if owned { 10 } else { 8 }) {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let target = first.split_whitespace().nth(1).unwrap();
            assert_eq!(target.split('?').next().unwrap(), *path);
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                let (key, value) = line.trim_end().split_once(':').unwrap();
                headers.push((key.to_ascii_lowercase(), value.trim().to_owned()));
            }
            let length = headers
                .iter()
                .find(|(k, _)| k == "content-length")
                .map_or(0, |(_, v)| v.parse::<usize>().unwrap());
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let body = String::from_utf8(bytes).unwrap();
            let payload: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            if step == 0 || step == 1 || step == 9 {
                let values: HashMap<_, _> = url::form_urlencoded::parse(body.as_bytes())
                    .into_owned()
                    .collect();
                assert_eq!(values["client_id"], PRISMARINE_CLIENT_ID);
                if step == 0 || step == 9 {
                    assert_eq!(values["scope"], prismarine::LIVE_SCOPE);
                }
                if step == 1 {
                    assert_eq!(values["device_code"], "private-device-code");
                    assert!(headers
                        .iter()
                        .any(|(k, v)| k == "cookie" && v == "fixture=bound"));
                }
                if step == 9 {
                    assert_eq!(values["refresh_token"], "private-refresh");
                }
            }
            if (2..=5).contains(&step) {
                let contract = headers
                    .iter()
                    .find(|(k, _)| k == "x-xbl-contract-version")
                    .unwrap();
                assert_eq!(contract.1, if step == 2 { "2" } else { "1" });
            }
            if step == 2 {
                assert_eq!(payload["Properties"]["RpsTicket"], "t=private-access");
            }
            if step == 3 {
                jwk = payload["Properties"]["ProofKey"].clone();
                assert_eq!(payload["Properties"]["DeviceType"], "Nintendo");
            }
            if (3..=5).contains(&step) {
                verify(&headers, &body, path, &jwk);
            }
            if step == 4 {
                assert_eq!(payload["Properties"]["DeviceToken"], "device-token");
            }
            if step == 5 {
                assert_eq!(payload["Properties"]["TitleToken"], "title-token");
                assert_eq!(payload["Properties"]["UserTokens"], json!(["user-token"]));
            }
            let response = match step {
                0 => {
                    json!({"user_code":"PUBLIC","device_code":"private-device-code","verification_uri":"https://www.microsoft.com/link","interval":1,"expires_in":900})
                }
                1 | 9 => json!({"access_token":"private-access","refresh_token":"private-refresh"}),
                2 => json!({"Token":"user-token"}),
                3 => json!({"Token":"device-token"}),
                4 => json!({"Token":"title-token"}),
                5 => {
                    json!({"Token":"xsts-token","DisplayClaims":{"xui":[{"uhs":"hash","xid":"123"}]}})
                }
                6 => {
                    assert_eq!(payload["xtoken"], "XBL3.0 x=hash;xsts-token");
                    json!({"access_token":"mc-token","expires_in":86400})
                }
                7 => {
                    if owned {
                        json!({"items":[{"name":"game_minecraft"}]})
                    } else {
                        json!({"items":[]})
                    }
                }
                8 => json!({"id":"0123456789abcdef0123456789abcdef","name":"Owner"}),
                _ => unreachable!(),
            };
            let body = response.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n{}\r\n{}",body.len(),if step==0 {"Set-Cookie: fixture=bound; HttpOnly\r\n"}else{""},body).unwrap();
        }
    });
    (ep, handle)
}

#[test]
fn live_flow_signs_xbox_requests_checks_ownership_and_refreshes_with_the_same_provider() {
    let (ep, thread) = fixture(true);
    let code = device_code(&ep, PRISMARINE_CLIENT_ID).unwrap();
    let debug = format!("{code:?}");
    assert!(!debug.contains("private-device") && !debug.contains("fixture=bound"));
    let ms = match poll(&ep, PRISMARINE_CLIENT_ID, &code).unwrap() {
        Poll::Done(ms) => ms,
        _ => panic!("mock completed"),
    };
    let (account, secrets) = finish_sign_in(&ep, ms).unwrap();
    assert_eq!(account.name, "Owner");
    assert!(secrets.owns);
    assert_eq!(secrets.client_id.as_deref(), Some(PRISMARINE_CLIENT_ID));
    assert!(!format!("{secrets:?}").contains("private-refresh"));
    assert_eq!(
        refresh_ms(&ep, PRISMARINE_CLIENT_ID, &secrets.refresh_token)
            .unwrap()
            .access,
        "private-access"
    );
    thread.join().unwrap();
}

#[test]
fn a_live_sign_in_without_java_entitlement_is_refused_before_profile_or_download() {
    let (ep, thread) = fixture(false);
    let code = device_code(&ep, PRISMARINE_CLIENT_ID).unwrap();
    let ms = match poll(&ep, PRISMARINE_CLIENT_ID, &code).unwrap() {
        Poll::Done(ms) => ms,
        _ => panic!("mock completed"),
    };
    assert_eq!(finish_sign_in(&ep, ms).unwrap_err(), DOES_NOT_OWN);
    thread.join().unwrap();
}

#[test]
fn changing_application_never_sends_the_old_refresh_credential_to_the_new_one() {
    let ep = Endpoints {
        flow: MicrosoftFlow::PrismarineLive,
        client_id: Some(PRISMARINE_CLIENT_ID.into()),
        ..Endpoints::azure()
    };
    let secrets = Secrets::new(
        "old-refresh".into(),
        Minecraft {
            token: "mc".into(),
            expires_at: now() + 86400,
            xuid: None,
        },
    );
    let account = Account {
        uuid: "uuid".into(),
        name: "name".into(),
        skin: None,
        cape: None,
    };
    for old in [None, Some("old-client".to_owned())] {
        let mut secrets = secrets.clone();
        secrets.client_id = old;
        match check(&ep, Some(PRISMARINE_CLIENT_ID), &account, secrets) {
            Check::NoAnswer(why, s) => {
                assert_eq!(why, SIGN_IN_AGAIN);
                assert!(s.sign_in_required);
            }
            _ => panic!("requires interactive sign-in"),
        }
    }
}
