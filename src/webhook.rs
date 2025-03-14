use std::fmt::Display;
use reqwest::redirect;
use std::time::Duration;
use webbed_hook_core::webhook::{CertificateNonce, Change, Metadata, PushSignature, PushSignatureStatus, Value, WebhookRequest, WebhookResponse};
use crate::rule::WebhookRule;
use crate::gitlab::get_gitlab_metadata;
use crate::util::env_as;

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

#[derive(Debug)]
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

#[derive(Debug)]
pub struct WebhookResult(pub bool, pub WebhookResponse);

pub fn perform_request(default_branch: &str, push_options: Vec<String>, condition: &WebhookRule, changes: Vec<Change>) -> Result<WebhookResult, HookError> {
    let connect_timeout = condition.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT);
    if connect_timeout > MAX_CONNECT_TIMEOUT {
        return Err(HookError::Validation(format!("Connect timeout of {}ms is longer than maximum value of {}ms", connect_timeout.as_millis(), &MAX_CONNECT_TIMEOUT.as_millis())))
    }

    let request_timeout = condition.request_timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT);
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
    let config = match condition.config {
        Some(ref c) => c.clone(),
        None => Value::Null,
    };

    let request_body = WebhookRequest {
        version: "1".to_string(),
        default_branch: default_branch.to_string(),
        config,
        changes,
        push_options,
        signature: get_push_signature(),
        metadata: get_metadata(),
    };
    
    if let Some(ref greetings) = condition.greeting_messages {
        for greeting in greetings {
            println!("{}", greeting);
        }
    }

    client.post(condition.url.0.clone())
        .json(&request_body)
        .send()
        .map(|res| {
            let success = res.status().is_success();
            let messages = res.json::<WebhookResponse>().ok().unwrap_or_default();
            WebhookResult(success, messages)
        })
        .map_err(HookError::Request)
}