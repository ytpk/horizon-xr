//! Microsoft account sign-in, shared by the desktop launcher and the Quest: the OAuth device-code
//! flow, then Xbox Live, XSTS and the Minecraft services, the game ownership check, the profile,
//! renewal, and where the secrets are kept ([`SecretStore`]). No UI, no keychain backends (the
//! launcher adds those), and no tokens in logs, errors or `Debug` output.
//!
//! `HORIZON_MSA_CLIENT_ID` at run time overrides the build-time configuration. An unset ID
//! selects experimental Prismarine-compatible Live sign-in; a custom ID selects Azure, and
//! an explicitly empty value disables sign-in (docs/microsoft-account.md).
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod prismarine;
pub use prismarine::DEFAULT_CLIENT_ID as PRISMARINE_CLIENT_ID;
mod store;
pub use store::*;

/// Public progress only; no account credentials or provider response bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Keychain,
    Microsoft,
    Xbox,
    Minecraft,
    Ownership,
    Profile,
    Saving,
    SigningOut,
}

impl Phase {
    /// The words for a phase on screen (English; callers translate them).
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Waiting for account access…",
            Self::Keychain => "Reading keychain…",
            Self::Microsoft => "Renewing Microsoft sign-in…",
            Self::Xbox => "Signing in to Xbox Live…",
            Self::Minecraft => "Getting Minecraft session…",
            Self::Ownership => "Checking Minecraft ownership…",
            Self::Profile => "Loading Minecraft profile…",
            Self::Saving => "Saving sign-in…",
            Self::SigningOut => "Signing out…",
        }
    }
}

/// How requests leave: the user agent, and a hook that adds the player's proxy for a URL (the
/// launcher's setting). Set once at start; without it requests go direct as `horizon-account`.
pub struct Transport {
    pub user_agent: String,
    pub proxy: Option<fn(ureq::AgentBuilder, &str) -> ureq::AgentBuilder>,
}

static TRANSPORT: OnceLock<Transport> = OnceLock::new();

/// Sets the transport; the first call wins, later ones are ignored.
pub fn set_transport(transport: Transport) {
    let _ = TRANSPORT.set(transport);
}

fn transport() -> &'static Transport {
    TRANSPORT.get_or_init(|| Transport {
        user_agent: format!("horizon-account/{}", env!("CARGO_PKG_VERSION")),
        proxy: None,
    })
}

/// The process lock of the stores that share one keychain ([`SecretStore::operations`]).
pub fn process_lock() -> &'static Mutex<()> {
    static OPERATIONS: Mutex<()> = Mutex::new(());
    &OPERATIONS
}

pub const SCOPE: &str = "XboxLive.signin offline_access";
pub const NOT_CONFIGURED: &str = "This build of the launcher has no Microsoft sign-in (HORIZON_MSA_CLIENT_ID), so it can't start the game.";
/// What every Play says while no account in the launcher owns the game.
pub const NEEDS_OWNER: &str =
    "Sign in with a Microsoft account that owns Minecraft: Java Edition to play";
pub const DOES_NOT_OWN: &str = "This account doesn't own Minecraft: Java Edition.";
pub const NO_PROFILE: &str =
    "This account has no Minecraft profile yet. Create one at minecraft.net, then sign in again.";
pub const OTHER_PROFILE: &str =
    "The Microsoft account now has another Minecraft profile. Sign in again.";
pub const SIGN_IN_AGAIN: &str = "Your Microsoft sign-in expired. Sign in again.";
/// The Minecraft token is renewed when it has less than this left.
pub const RENEW_BEFORE: u64 = 3600;

/// The configured app: run-time overrides build-time; an empty value disables sign-in.
/// When neither is set, use prismarine-auth's public Minecraft Switch Live title.
pub fn client_id() -> Option<String> {
    std::env::var("HORIZON_MSA_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("HORIZON_MSA_CLIENT_ID").map(str::to_owned))
        .or_else(|| Some(PRISMARINE_CLIENT_ID.to_owned()))
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

/// Microsoft application protocol; cached refresh credentials are bound to their client ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicrosoftFlow {
    Azure,
    PrismarineLive,
}

/// Where the requests go: the real services, or a mock in tests.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub flow: MicrosoftFlow,
    pub client_id: Option<String>,
    pub xbox_device: String,
    pub xbox_title: String,
    /// The OAuth 2.0 v2 endpoints for personal accounts (`…/devicecode`, `…/token`).
    pub microsoft: String,
    pub xbox_user: String,
    pub xsts: String,
    /// api.minecraftservices.com.
    pub minecraft: String,
}

impl Endpoints {
    pub fn live() -> Self {
        let mut ep = Self::azure();
        ep.client_id = client_id();
        if ep.client_id.as_deref() == Some(PRISMARINE_CLIENT_ID) {
            ep.flow = MicrosoftFlow::PrismarineLive;
            ep.microsoft = std::env::var("HORIZON_MSA_MICROSOFT_ENDPOINT")
                .ok()
                .filter(|_| cfg!(debug_assertions))
                .unwrap_or_else(|| "https://login.live.com".into());
        }
        ep
    }

    /// Azure OAuth v2 endpoints, also a base for isolated service fixtures.
    pub fn azure() -> Self {
        // Development builds only: `HORIZON_MSA_MICROSOFT_ENDPOINT` points the device-code and
        // token requests at a local mock, to see and screenshot the sign-in panel.
        let microsoft = std::env::var("HORIZON_MSA_MICROSOFT_ENDPOINT")
            .ok()
            .filter(|_| cfg!(debug_assertions))
            .unwrap_or_else(|| "https://login.microsoftonline.com/consumers/oauth2/v2.0".into());
        Self {
            flow: MicrosoftFlow::Azure,
            client_id: None,
            xbox_device: "https://device.auth.xboxlive.com/device/authenticate".into(),
            xbox_title: "https://title.auth.xboxlive.com/title/authenticate".into(),
            microsoft,
            xbox_user: "https://user.auth.xboxlive.com/user/authenticate".into(),
            xsts: "https://xsts.auth.xboxlive.com/xsts/authorize".into(),
            minecraft: "https://api.minecraftservices.com".into(),
        }
    }
}

/// A request to `url`: the launcher's user agent, a 20 s timeout, and the player's proxy when one
/// is set (`proxy`), since a sign-in has to get out the way downloads do.
pub fn request_to(method: &str, url: &str) -> ureq::Request {
    let transport = transport();
    let builder = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .user_agent(&transport.user_agent);
    let builder = match transport.proxy {
        Some(proxy) => proxy(builder, url),
        None => builder,
    };
    builder.build().request(method, url)
}

/// One request's result: the status and the JSON body (empty when there is none). Transport
/// errors name the service, never the URL's query or the request.
pub fn send(
    request: ureq::Request,
    body: Option<Body>,
    service: &str,
) -> Result<(u16, Value), String> {
    let result = match body {
        None => request.call(),
        Some(Body::Form(fields)) => request.send_form(&fields),
        Some(Body::Json(value)) => request.set("Accept", "application/json").send_json(value),
    };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(_)) => {
            return Err(format!(
                "Could not reach {service}. Check your internet connection."
            ));
        }
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

pub enum Body<'a> {
    Form(Vec<(&'a str, &'a str)>),
    Json(Value),
}

pub fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ----- The device-code flow ----------------------------------------------------------------------

/// What the player types at the verification page.
#[derive(Clone, PartialEq)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_at: u64,
    pub device_code: String,
    /// Live device-flow cookies; never displayed or included in Debug output.
    pub live_cookies: Vec<String>,
}

impl std::fmt::Debug for DeviceCode {
    // The device code can be traded for tokens once the player approves: never shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .finish_non_exhaustive()
    }
}

/// The Microsoft tokens; the access token only lasts for the Xbox step.
pub struct MsTokens {
    pub access: String,
    pub refresh: String,
}

impl std::fmt::Debug for MsTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MsTokens(<hidden>)")
    }
}

pub enum Poll {
    Pending,
    /// The server asked to poll less often: wait 5 s more.
    SlowDown,
    Done(MsTokens),
}

pub fn device_code(ep: &Endpoints, client_id: &str) -> Result<DeviceCode, String> {
    if ep.flow == MicrosoftFlow::PrismarineLive {
        return prismarine::device_code(ep, client_id);
    }
    let (status, v) = send(
        request_to("POST", &format!("{}/devicecode", ep.microsoft)),
        Some(Body::Form(vec![("client_id", client_id), ("scope", SCOPE)])),
        "Microsoft",
    )?;
    parse_device_code(status, &v, Vec::new())
}

fn parse_device_code(
    status: u16,
    v: &Value,
    live_cookies: Vec<String>,
) -> Result<DeviceCode, String> {
    match (
        status,
        str_field(v, "user_code"),
        str_field(v, "device_code"),
    ) {
        (200, Some(user_code), Some(device_code)) => Ok(DeviceCode {
            user_code,
            verification_uri: str_field(v, "verification_uri")
                .unwrap_or_else(|| "https://www.microsoft.com/link".into()),
            interval: v
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .max(1),
            expires_at: now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(900),
            device_code,
            live_cookies,
        }),
        _ => Err(oauth_error(v)),
    }
}

/// The OAuth error codes as messages for the player.
pub fn oauth_error(v: &Value) -> String {
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_declined") => "Sign-in was declined in the browser.".into(),
        Some("expired_token") => "The sign-in code expired. Start again.".into(),
        Some("bad_verification_code") => "The sign-in code was not recognised. Start again.".into(),
        Some("invalid_grant") => SIGN_IN_AGAIN.into(),
        Some("invalid_client" | "unauthorized_client") => {
            "Microsoft refused this launcher's app registration (client ID).".into()
        }
        Some(other) => format!("Microsoft sign-in failed ({other})."),
        None => "Microsoft sign-in failed.".into(),
    }
}

/// One poll of the token endpoint while the player signs in.
pub fn poll(ep: &Endpoints, client_id: &str, code: &DeviceCode) -> Result<Poll, String> {
    let mut request = request_to(
        "POST",
        &format!(
            "{}/{}",
            ep.microsoft,
            if ep.flow == MicrosoftFlow::PrismarineLive {
                "oauth20_token.srf"
            } else {
                "token"
            }
        ),
    );
    if ep.flow == MicrosoftFlow::PrismarineLive {
        request = request.query("client_id", client_id);
        if !code.live_cookies.is_empty() {
            request = request.set("Cookie", &code.live_cookies.join("; "));
        }
    }
    let (status, v) = send(
        request,
        Some(Body::Form(vec![
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id),
            ("device_code", &code.device_code),
        ])),
        "Microsoft",
    )?;
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => return Ok(Poll::Pending),
        Some("slow_down") => return Ok(Poll::SlowDown),
        _ => {}
    }
    ms_tokens(status, &v).map(Poll::Done)
}

pub fn ms_tokens(status: u16, v: &Value) -> Result<MsTokens, String> {
    match (
        status,
        str_field(v, "access_token"),
        str_field(v, "refresh_token"),
    ) {
        (200, Some(access), Some(refresh)) => Ok(MsTokens { access, refresh }),
        _ => Err(oauth_error(v)),
    }
}

/// Wait for the player to finish in the browser, polling as asked, until the code expires or
/// `cancelled` says stop.
pub fn wait_for_sign_in(
    ep: &Endpoints,
    client_id: &str,
    code: &DeviceCode,
    cancelled: &dyn Fn() -> bool,
) -> Result<MsTokens, String> {
    let mut interval = code.interval;
    loop {
        let until = std::time::Instant::now() + Duration::from_secs(interval);
        while std::time::Instant::now() < until {
            if cancelled() {
                return Err("Sign-in cancelled.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if now() > code.expires_at {
            return Err("The sign-in code expired. Start again.".into());
        }
        match poll(ep, client_id, code)? {
            Poll::Pending => {}
            Poll::SlowDown => interval += 5,
            Poll::Done(tokens) => return Ok(tokens),
        }
    }
}

pub fn refresh_ms(ep: &Endpoints, client_id: &str, refresh: &str) -> Result<MsTokens, String> {
    let (status, v) = send(
        request_to(
            "POST",
            &format!(
                "{}/{}",
                ep.microsoft,
                if ep.flow == MicrosoftFlow::PrismarineLive {
                    "oauth20_token.srf"
                } else {
                    "token"
                }
            ),
        ),
        Some(Body::Form(vec![
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh),
            (
                "scope",
                if ep.flow == MicrosoftFlow::PrismarineLive {
                    prismarine::LIVE_SCOPE
                } else {
                    SCOPE
                },
            ),
        ])),
        "Microsoft",
    )?;
    ms_tokens(status, &v)
}

// ----- Xbox Live, XSTS, Minecraft ----------------------------------------------------------------

/// The XSTS `XErr` codes, as the official launcher explains them.
pub fn xsts_error(code: u64) -> String {
    match code {
        2148916233 => "This Microsoft account has no Xbox profile yet. Sign in once at xbox.com to create one, then try again.".into(),
        2148916235 => "Xbox Live is not available in your country or region.".into(),
        2148916236 | 2148916237 => "This account needs adult verification on xbox.com before it can play.".into(),
        2148916238 => "This is a child account: an adult has to add it to a Microsoft family first.".into(),
        other => format!("Xbox Live refused the sign-in (error {other})."),
    }
}

/// The Minecraft access token from the Microsoft one, through Xbox Live and XSTS.
pub struct Minecraft {
    pub token: String,
    pub expires_at: u64,
    pub xuid: Option<String>,
}

pub fn minecraft_login(
    ep: &Endpoints,
    ms_access: &str,
    progress: &dyn Fn(Phase),
) -> Result<Minecraft, String> {
    if ep.flow == MicrosoftFlow::PrismarineLive {
        return prismarine::minecraft_login(ep, ms_access, progress);
    }
    progress(Phase::Xbox);
    let (status, xbl) = send(
        request_to("POST", &ep.xbox_user),
        Some(Body::Json(json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    let xbl_token = str_field(&xbl, "Token")
        .filter(|_| status == 200)
        .ok_or("Xbox Live refused the sign-in.")?;
    let (status, xsts) = send(
        request_to("POST", &ep.xsts),
        Some(Body::Json(json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    if let Some(code) = xsts.get("XErr").and_then(Value::as_u64) {
        return Err(xsts_error(code));
    }
    let claims = xsts.pointer("/DisplayClaims/xui/0");
    let (Some(xsts_token), Some(uhs)) = (
        str_field(&xsts, "Token").filter(|_| status == 200),
        claims.and_then(|c| str_field(c, "uhs")),
    ) else {
        return Err("Xbox Live refused the sign-in.".into());
    };
    let xuid = claims.and_then(|c| str_field(c, "xid"));
    progress(Phase::Minecraft);
    let (token, expires_in) = minecraft_token(ep, &format!("XBL3.0 x={uhs};{xsts_token}"))?;
    Ok(Minecraft {
        token,
        expires_at: now() + expires_in,
        xuid,
    })
}

/// The Minecraft access token and its lifetime from the XSTS one (`XBL3.0 x=<uhs>;<token>`).
/// `launcher/login` first: a new account's token from `login_with_xbox` names no profile, so
/// joining an online-mode server answers "Profile not found" (found and diagnosed by
/// martipops). `login_with_xbox` stays the fallback, so a sign-in that worked before still does
/// when the other refuses.
pub fn minecraft_token(ep: &Endpoints, xtoken: &str) -> Result<(String, u64), String> {
    let lifetime = |v: &Value| v.get("expires_in").and_then(Value::as_u64).unwrap_or(86400);
    let launcher = send(
        request_to("POST", &format!("{}/launcher/login", ep.minecraft)),
        Some(Body::Json(
            json!({ "xtoken": xtoken, "platform": "PC_LAUNCHER" }),
        )),
        "the Minecraft services",
    );
    if let Ok((200, v)) = &launcher {
        if let Some(token) = str_field(v, "access_token") {
            return Ok((token, lifetime(v)));
        }
    }
    let (status, mc) = send(
        request_to(
            "POST",
            &format!("{}/authentication/login_with_xbox", ep.minecraft),
        ),
        Some(Body::Json(json!({ "identityToken": xtoken }))),
        "the Minecraft services",
    )?;
    match (status, str_field(&mc, "access_token")) {
        (200, Some(token)) => Ok((token, lifetime(&mc))),
        // New Azure apps are refused until Mojang approves them (Minecraft API access).
        (403, _) => Err(
            "The Minecraft services refused this launcher: its app is not approved by Mojang yet."
                .into(),
        ),
        (429, _) => Err("Too many sign-ins in a short time. Wait a minute and try again.".into()),
        _ => Err("The Minecraft services refused the sign-in.".into()),
    }
}

pub fn bearer(request: ureq::Request, token: &str) -> ureq::Request {
    request.set("Authorization", &format!("Bearer {token}"))
}

/// Whether the account owns Minecraft: Java Edition (Game Pass included).
pub fn owns_game(ep: &Endpoints, token: &str) -> Result<bool, String> {
    let (status, v) = send(
        bearer(
            request_to("GET", &format!("{}/entitlements/mcstore", ep.minecraft)),
            token,
        ),
        None,
        "the Minecraft services",
    )?;
    if status != 200 {
        return Err("Could not check which games this account owns.".into());
    }
    Ok(v.get("items")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|i| {
                matches!(
                    i.get("name").and_then(Value::as_str),
                    Some("game_minecraft" | "product_minecraft")
                )
            })
        }))
}

/// A Minecraft profile, as the launcher shows and remembers it (no secrets).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    /// The profile UUID without dashes.
    pub uuid: String,
    pub name: String,
    /// The active skin's URL (textures.minecraft.net) and model ("classic" or "slim").
    #[serde(default)]
    pub skin: Option<(String, String)>,
    #[serde(default)]
    pub cape: Option<String>,
}

/// The profile; none when the account has none (it doesn't own the game, or never made one).
pub fn profile(ep: &Endpoints, token: &str) -> Result<Option<Account>, String> {
    let (status, v) = send(
        bearer(
            request_to("GET", &format!("{}/minecraft/profile", ep.minecraft)),
            token,
        ),
        None,
        "the Minecraft services",
    )?;
    if status == 404 {
        return Ok(None);
    }
    let (Some(uuid), Some(name)) = (str_field(&v, "id"), str_field(&v, "name")) else {
        return Err("Could not read the Minecraft profile.".into());
    };
    let active = |key: &str| {
        v.get(key)
            .and_then(Value::as_array)
            .and_then(|list| {
                list.iter()
                    .find(|s| s.get("state").and_then(Value::as_str) == Some("ACTIVE"))
            })
            .cloned()
    };
    Ok(Some(Account {
        uuid: uuid.replace('-', "").to_lowercase(),
        name,
        skin: active("skins").and_then(|s| {
            Some((
                str_field(&s, "url")?,
                str_field(&s, "variant")
                    .unwrap_or_else(|| "CLASSIC".into())
                    .to_lowercase(),
            ))
        }),
        cape: active("capes").and_then(|c| str_field(&c, "url")),
    }))
}

/// What the keychain keeps for one account.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Secrets {
    /// Issuing application; old Azure-only stores have no binding.
    #[serde(default)]
    pub client_id: Option<String>,
    pub refresh_token: String,
    #[serde(default)]
    pub sign_in_required: bool,
    pub minecraft_token: String,
    pub minecraft_expires_at: u64,
    #[serde(default)]
    pub xuid: Option<String>,
    /// The last answer the Minecraft services gave: whether the account owns the game. It is
    /// kept here, beside the tokens, so that no plain file can say yes. Accounts saved before it
    /// existed passed the same check at sign-in, which refuses everyone else.
    #[serde(default = "owned_at_sign_in")]
    pub owns: bool,
}
pub fn owned_at_sign_in() -> bool {
    true
}
impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secrets(<hidden>)")
    }
}

/// From Microsoft tokens to a ready account: Xbox, XSTS, Minecraft, ownership, profile.
pub fn finish_sign_in(ep: &Endpoints, ms: MsTokens) -> Result<(Account, Secrets), String> {
    finish_sign_in_with_progress(ep, ms, &|_| {})
}

pub fn finish_sign_in_with_progress(
    ep: &Endpoints,
    ms: MsTokens,
    progress: &dyn Fn(Phase),
) -> Result<(Account, Secrets), String> {
    let mc = minecraft_login(ep, &ms.access, progress)?;
    progress(Phase::Ownership);
    if !owns_game(ep, &mc.token)? {
        return Err(DOES_NOT_OWN.into());
    }
    progress(Phase::Profile);
    let account = profile(ep, &mc.token)?.ok_or(NO_PROFILE)?;
    let mut secrets = Secrets::new(ms.refresh, mc);
    secrets.client_id = ep.client_id.clone();
    Ok((account, secrets))
}

impl Secrets {
    pub fn new(refresh_token: String, mc: Minecraft) -> Self {
        Self {
            refresh_token,
            client_id: None,
            sign_in_required: false,
            minecraft_token: mc.token,
            minecraft_expires_at: mc.expires_at,
            xuid: mc.xuid,
            owns: true,
        }
    }
}

/// What asking the Minecraft services again found for one account.
pub enum Check {
    /// It owns the game: its profile as it is now (the name may have changed) and its tokens.
    Owns(Account, Secrets),
    /// The services said it doesn't own the game (or has no profile any more).
    DoesNot(Secrets),
    /// No answer (no network, a sign-in that can't be renewed): the last answer stands.
    NoAnswer(String, Secrets),
}

/// Asks whether `account` owns the game, with a Minecraft token renewed through the refresh token
/// first when it has less than an hour left. The secrets come back renewed whatever the answer.
pub fn check(
    ep: &Endpoints,
    client_id: Option<&str>,
    account: &Account,
    secrets: Secrets,
) -> Check {
    check_with(ep, client_id, account, secrets, false, &|_| {})
}

pub fn check_with(
    ep: &Endpoints,
    client_id: Option<&str>,
    account: &Account,
    mut secrets: Secrets,
    force: bool,
    progress: &dyn Fn(Phase),
) -> Check {
    if secrets
        .client_id
        .as_deref()
        .is_some_and(|old| Some(old) != client_id)
        || (secrets.client_id.is_none() && ep.flow == MicrosoftFlow::PrismarineLive)
    {
        secrets.sign_in_required = true;
        return Check::NoAnswer(SIGN_IN_AGAIN.into(), secrets);
    }
    if secrets.sign_in_required {
        return Check::NoAnswer(SIGN_IN_AGAIN.into(), secrets);
    }
    if force || secrets.minecraft_expires_at <= now() + RENEW_BEFORE {
        progress(Phase::Microsoft);
        let renewed = client_id
            .ok_or_else(|| NOT_CONFIGURED.to_string())
            .and_then(|id| refresh_ms(ep, id, &secrets.refresh_token));
        let ms = match renewed {
            Ok(ms) => ms,
            Err(e) => {
                secrets.sign_in_required = e == SIGN_IN_AGAIN;
                return Check::NoAnswer(e, secrets);
            }
        };
        // Microsoft can rotate its refresh credential even if a later service fails.
        secrets.refresh_token = ms.refresh;
        match minecraft_login(ep, &ms.access, progress) {
            Ok(mc) => {
                secrets = Secrets {
                    owns: secrets.owns,
                    client_id: ep.client_id.clone(),
                    ..Secrets::new(secrets.refresh_token, mc)
                }
            }
            Err(e) => return Check::NoAnswer(e, secrets),
        }
    }
    let token = &secrets.minecraft_token;
    progress(Phase::Ownership);
    match owns_game(ep, token).and_then(|owns| {
        if !owns {
            return Ok(None);
        }
        progress(Phase::Profile);
        profile(ep, token)
    }) {
        Err(e) => Check::NoAnswer(e, secrets),
        Ok(None) => Check::DoesNot(Secrets {
            owns: false,
            ..secrets
        }),
        Ok(Some(p)) if p.uuid != account.uuid => Check::NoAnswer(
            OTHER_PROFILE.into(),
            Secrets {
                sign_in_required: true,
                ..secrets
            },
        ),
        Ok(Some(p)) => Check::Owns(
            p,
            Secrets {
                owns: true,
                ..secrets
            },
        ),
    }
}

#[cfg(test)]
mod prismarine_tests;
