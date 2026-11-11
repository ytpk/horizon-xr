// SPDX-License-Identifier: MIT
// Native adaptation of prismarine-auth's LiveTokenManager / XboxTokenManager.
// Copyright (c) 2020 PrismarineJS; MIT (full notice in THIRD_PARTY_NOTICES.md).
//! Microsoft Live + Xbox proof-of-possession, without a Node.js runtime.
//! Reference: PrismarineJS/prismarine-auth a7f4b0b8f79b7526496c99a4794c1ecc1a36dd51.
use super::*;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use p256::ecdsa::{signature::Signer, Signature, SigningKey};

/// prismarine-auth's documented default title. This is Microsoft's Minecraft
/// for Nintendo Switch registration, not an application owned by PrismarineJS.
pub const DEFAULT_CLIENT_ID: &str = "00000000441cc96b";
pub const LIVE_SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";

struct Proof {
    key: SigningKey,
    public: Value,
}
impl Proof {
    fn new() -> Result<Self, String> {
        let key = loop {
            let mut bytes = [0; 32];
            getrandom::fill(&mut bytes)
                .map_err(|_| "Could not create an Xbox proof key.".to_string())?;
            if let Ok(key) = SigningKey::from_bytes((&bytes).into()) {
                break key;
            }
        };
        let point = key.verifying_key().to_encoded_point(false);
        let public = json!({"kty":"EC", "crv":"P-256", "alg":"ES256", "use":"sig",
            "x":URL_SAFE_NO_PAD.encode(point.x().expect("uncompressed point")),
            "y":URL_SAFE_NO_PAD.encode(point.y().expect("uncompressed point"))});
        Ok(Self { key, public })
    }
    fn signature(&self, url: &str, body: &str, seconds: u64) -> Result<String, String> {
        let url: url::Url = url
            .parse()
            .map_err(|_| "Invalid Xbox endpoint.".to_string())?;
        // prismarine-auth signs pathname, not the query string.
        let path = url.path();
        let timestamp = seconds
            .saturating_add(11_644_473_600)
            .saturating_mul(10_000_000);
        let mut message = Vec::new();
        message.extend(1u32.to_be_bytes());
        message.push(0);
        message.extend(timestamp.to_be_bytes());
        message.push(0);
        for field in ["POST", path, "", body] {
            message.extend(field.as_bytes());
            message.push(0);
        }
        let signature: Signature = self.key.sign(&message);
        let mut header = Vec::new();
        header.extend(1u32.to_be_bytes());
        header.extend(timestamp.to_be_bytes());
        header.extend(signature.to_bytes());
        Ok(STANDARD.encode(header))
    }
    fn post(&self, url: &str, value: Value) -> Result<(u16, Value), String> {
        let body = serde_json::to_string(&value)
            .map_err(|_| "Could not encode Xbox request.".to_string())?;
        let signature = self.signature(url, &body, now())?;
        let response = request_to("POST", url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json")
            .set(
                "x-xbl-contract-version",
                if url::Url::parse(url).is_ok_and(|u| u.path().ends_with("/user/authenticate")) {
                    "2"
                } else {
                    "1"
                },
            )
            .set("Signature", &signature)
            .send_string(&body);
        let response = match response {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(_) => {
                return Err("Could not reach Xbox Live. Check your internet connection.".into())
            }
        };
        let status = response.status();
        let value = response.into_json().unwrap_or(Value::Null);
        Ok((status, value))
    }
    fn token(&self, url: &str, value: Value) -> Result<String, String> {
        let (status, value) = self.post(url, value)?;
        if let Some(code) = value.get("XErr").and_then(Value::as_u64) {
            return Err(xsts_error(code));
        }
        str_field(&value, "Token")
            .filter(|_| status == 200)
            .ok_or_else(|| "Xbox Live refused the sign-in.".to_string())
    }
}
fn device_id() -> Result<String, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| "Could not create an Xbox device identifier.".to_string())?;
    bytes[6] = (bytes[6] & 15) | 64;
    bytes[8] = (bytes[8] & 63) | 128;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{{{}-{}-{}-{}-{}}}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

pub(super) fn minecraft_login(
    ep: &Endpoints,
    access: &str,
    progress: &dyn Fn(Phase),
) -> Result<Minecraft, String> {
    progress(Phase::Xbox);
    let proof = Proof::new()?;
    let user = proof.token(
        &ep.xbox_user,
        json!({"Properties":{
        "AuthMethod":"RPS", "SiteName":"user.auth.xboxlive.com", "RpsTicket":format!("t={access}")},
        "RelyingParty":"http://auth.xboxlive.com", "TokenType":"JWT"}),
    )?;
    let device = proof.token(
        &ep.xbox_device,
        json!({"Properties":{
        "AuthMethod":"ProofOfPossession", "Id":device_id()?, "SerialNumber":device_id()?,
        "DeviceType":"Nintendo", "Version":"0.0.0", "ProofKey":proof.public},
        "RelyingParty":"http://auth.xboxlive.com", "TokenType":"JWT"}),
    )?;
    let title = proof.token(
        &ep.xbox_title,
        json!({"Properties":{
        "AuthMethod":"RPS", "DeviceToken":device, "RpsTicket":format!("t={access}"),
        "SiteName":"user.auth.xboxlive.com", "ProofKey":proof.public},
        "RelyingParty":"http://auth.xboxlive.com", "TokenType":"JWT"}),
    )?;
    let (status, xsts) = proof.post(
        &ep.xsts,
        json!({"Properties":{
        "SandboxId":"RETAIL", "UserTokens":[user], "DeviceToken":device,
        "TitleToken":title, "ProofKey":proof.public},
        "RelyingParty":"rp://api.minecraftservices.com/", "TokenType":"JWT"}),
    )?;
    if let Some(code) = xsts.get("XErr").and_then(Value::as_u64) {
        return Err(xsts_error(code));
    }
    let claims = xsts.pointer("/DisplayClaims/xui/0");
    let (Some(token), Some(uhs)) = (
        str_field(&xsts, "Token").filter(|_| status == 200),
        claims.and_then(|c| str_field(c, "uhs")),
    ) else {
        return Err("Xbox Live refused the sign-in.".into());
    };
    progress(Phase::Minecraft);
    let (token, expires_in) = minecraft_token(ep, &format!("XBL3.0 x={uhs};{token}"))?;
    Ok(Minecraft {
        token,
        expires_at: now() + expires_in,
        xuid: claims.and_then(|c| str_field(c, "xid")),
    })
}

pub(super) fn device_code(ep: &Endpoints, client_id: &str) -> Result<DeviceCode, String> {
    let response =
        request_to("POST", &format!("{}/oauth20_connect.srf", ep.microsoft)).send_form(&[
            ("client_id", client_id),
            ("scope", LIVE_SCOPE),
            ("response_type", "device_code"),
        ]);
    let response = match response {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(_) => return Err("Could not reach Microsoft. Check your internet connection.".into()),
    };
    let cookies = response
        .all("set-cookie")
        .into_iter()
        .filter_map(|c| c.split(';').next())
        .map(str::to_owned)
        .collect();
    let status = response.status();
    let value = response.into_json().unwrap_or(Value::Null);
    parse_device_code(status, &value, cookies)
}
