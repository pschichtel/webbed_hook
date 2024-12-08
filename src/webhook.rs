use std::env;
use std::fmt::Display;
use std::str::FromStr;
use crate::configuration::Hook;
use reqwest::redirect;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use crate::gitlab::{get_gitlab_metadata, GitlabMetadata};
use crate::util::env_as;

#[derive(Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct ChangeWithPatch {
    pub old_commit: String,
    pub new_commit: String,
    pub ref_name: String,
    pub patch: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum Metadata {
    #[serde(rename = "gitlab")]
    GitLab(GitlabMetadata),

    #[serde(rename = "none")]
    None,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct Request<'a> {
    pub version: &'a str,
    pub config: &'a Value,
    pub changes: &'a Vec<ChangeWithPatch>,
    pub push_options: &'a Vec<String>,
    pub signature: Option<PushSignature>,
    pub metadata: Metadata,
}

#[derive(Debug)]
pub struct WebhookResult(pub bool, pub Vec<String>);

fn get_push_options() -> Vec<String> {
    let option_count_str = env_as("GIT_PUSH_OPTION_COUNT")
        .unwrap_or(0u64);
    if option_count_str == 0 {
        return vec![];
    }
    (0..option_count_str).filter_map(|n| {
        env::var(format!("GIT_PUSH_OPTION_{}", n)).ok()
    }).collect()
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum PushSignatureStatus {
    #[serde(rename = "good")]
    Good,
    #[serde(rename = "bad")]
    Bad,
    #[serde(rename = "unknown-validity")]
    UnknownValidity,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "expired-key")]
    ExpiredKey,
    #[serde(rename = "revoked-key")]
    RevokedKey,
    #[serde(rename = "cannot-check")]
    CannotCheck,
    #[serde(rename = "no-signature")]
    NoSignature,
}

impl FromStr for PushSignatureStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "G" => Ok(PushSignatureStatus::Good),
            "B" => Ok(PushSignatureStatus::Bad),
            "U" => Ok(PushSignatureStatus::UnknownValidity),
            "X" => Ok(PushSignatureStatus::Expired),
            "Y" => Ok(PushSignatureStatus::ExpiredKey),
            "R" => Ok(PushSignatureStatus::RevokedKey),
            "E" => Ok(PushSignatureStatus::CannotCheck),
            "N" => Ok(PushSignatureStatus::NoSignature),
            _ => Err(format!("Unknown signature status: {}", s)),
        }
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum CertificateNonce {
    #[serde(rename = "unsolicited")]
    Unsolicited { nonce: String },
    #[serde(rename = "missing")]
    Missing,
    #[serde(rename = "bad")]
    Bad { nonce: String },
    #[serde(rename = "ok")]
    Ok { nonce: String },
    #[serde(rename = "slop")]
    Slop { nonce: String, stale_seconds: u32 },
}

fn get_nonce() -> Option<String> {
    env_as::<String>("GIT_PUSH_CERT_NONCE")
}

fn get_certificate_nonce() -> CertificateNonce {
    let status = match env_as::<String>("GIT_PUSH_CERT_NONCE_STATUS") {
        Some(n) => n,
        None => return CertificateNonce::Missing,
    };

    match status.as_str() {
        "UNSOLICITED" => match get_nonce() {
            Some(nonce) => CertificateNonce::Unsolicited { nonce },
            None => CertificateNonce::Missing
        },
        "MISSING" => CertificateNonce::Missing,
        "BAD" => match get_nonce() {
            Some(nonce) => CertificateNonce::Bad { nonce },
            None => CertificateNonce::Missing
        },
        "OK" => match get_nonce() {
            Some(nonce) => CertificateNonce::Ok { nonce },
            None => CertificateNonce::Missing
        },
        "SLOP" => {
            match get_nonce() {
                Some(nonce) => {
                    let stale_seconds = env_as::<u32>("GIT_PUSH_CERT_NONCE_SLOP")
                        .unwrap_or_default();
                    CertificateNonce::Slop {nonce, stale_seconds}
                },
                None => CertificateNonce::Missing,
            }
        },
        _ => CertificateNonce::Missing
    }
}

#[derive(Serialize, Debug)]
pub struct PushSignature {
    certificate: String,
    signer: String,
    key: String,
    status: PushSignatureStatus,
    nonce: CertificateNonce,
}

fn get_push_signature() -> Option<PushSignature> {
    let cert = match env_as::<String>("GIT_PUSH_CERT") {
        Some(cert) => cert,
        None => return None,
    };
    let signer = match env_as::<String>("GIT_PUSH_CERT_SIGNER") {
        Some(s) => s,
        None => return None,
    };
    let key = match env_as::<String>("GIT_PUSH_CERT_KEY") {
        Some(k) => k,
        None => return None,
    };
    let status = match env_as::<PushSignatureStatus>("GIT_PUSH_CERT_STATUS") {
        Some(s) => s,
        None => return None,
    };
    let nonce = get_certificate_nonce();

    Some(PushSignature {
        certificate: cert,
        signer,
        key,
        status,
        nonce
    })
}

fn get_metadata() -> Metadata {
    get_gitlab_metadata()
        .map(Metadata::GitLab)
        .unwrap_or(Metadata::None)
}

pub enum HookError {
    Request(reqwest::Error),
    Validation(String),
}

impl Display for HookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookError::Request(e) => {
                write!(f, "Request error: {}", e)
            }
            HookError::Validation(msg) => {
                write!(f, "Validation error: {}", msg)
            }
        }
    }
}

const MAX_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

pub fn perform_request(hook: &Hook, changes: Vec<ChangeWithPatch>) -> Result<WebhookResult, HookError> {
    let connect_timeout = hook.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT);
    if connect_timeout > MAX_CONNECT_TIMEOUT {
        return Err(HookError::Validation(format!("Connect timeout of {}ms is longer than maximum value of {}ms", connect_timeout.as_millis(), &MAX_CONNECT_TIMEOUT.as_millis())))
    }

    let request_timeout = hook.request_timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT);
    if connect_timeout > MAX_REQUEST_TIMEOUT {
        return Err(HookError::Validation(format!("Request timeout of {}ms is longer than maximum value of {}ms", request_timeout.as_millis(), &MAX_REQUEST_TIMEOUT.as_millis())))
    }

    let client = reqwest::blocking::Client::builder()
        .redirect(redirect::Policy::limited(5))
        .connect_timeout(connect_timeout)
        .timeout(request_timeout)
        .tcp_keepalive(None)
        .deflate(false)
        .http1_only()
        .build()
        .expect("Failed to build the client, this is a bug!");
    let config = match hook.config {
        Some(ref c) => c,
        None => &Value::Null,
    };

    let request_body = Request{
        version: "1",
        config,
        changes: &changes,
        push_options: &get_push_options(),
        signature: get_push_signature(),
        metadata: get_metadata(),
    };

    client.post(hook.url.0.clone())
        .json(&request_body)
        .send()
        .map(|res| {
            let success = res.status().is_success();
            let messages = res.json::<Vec<String>>().ok().unwrap_or_default();
            WebhookResult(success, messages)
        })
        .map_err(HookError::Request)
}