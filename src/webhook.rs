use crate::configuration::Hook;
use reqwest::redirect;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

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
struct Request<'a> {
    pub version: &'a str,
    pub config: &'a Value,
    pub changes: Vec<ChangeWithPatch>,
}

#[derive(Debug)]
pub struct WebhookResult(pub bool, pub Vec<String>);

pub fn perform_request(hook: &Hook, changes: Vec<ChangeWithPatch>) -> Result<WebhookResult, reqwest::Error> {
    let client = reqwest::blocking::Client::builder()
        .redirect(redirect::Policy::limited(5))
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(10))
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
        changes
    };

    client.post(hook.url.0.clone())
        .json(&request_body)
        .send()
        .map(|res| {
            let success = res.status().is_success();
            let messages = res.json::<Vec<String>>().ok().unwrap_or_default();
            WebhookResult(success, messages)
        })
}