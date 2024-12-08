use crate::gitlab::GitlabMetadata;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct ChangeWithPatch {
    pub old_commit: String,
    pub new_commit: String,
    pub ref_name: String,
    pub patch: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum Metadata {
    #[serde(rename = "gitlab")]
    GitLab(GitlabMetadata),

    #[serde(rename = "none")]
    None,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct PushSignature {
    pub certificate: String,
    pub signer: String,
    pub key: String,
    pub status: PushSignatureStatus,
    pub nonce: CertificateNonce,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct WebhookRequest {
    pub version: String,
    pub config: Value,
    pub changes: Vec<ChangeWithPatch>,
    pub push_options: Vec<String>,
    pub signature: Option<PushSignature>,
    pub metadata: Metadata,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WebhookResponse(pub Vec<String>);

impl Default for WebhookResponse {
    fn default() -> Self {
        WebhookResponse(Vec::default())
    }
}