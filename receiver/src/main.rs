use std::env;
use std::error::Error;
use std::fmt::Display;
use actix_web::web;
use actix_web::{post, App, HttpRequest, HttpServer, Responder};
use actix_web::http::StatusCode;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use env_logger::Env;
use log::info;
use regex::Regex;
use unidiff::PatchSet;
use webbed_hook_core::webhook::{ChangeWithPatch, WebhookRequest, WebhookResponse};

fn find_default_branch_change<'a>(branch_name: &'a str, changes: &'a Vec<ChangeWithPatch>) -> Option<&'a ChangeWithPatch> {
    let ref_name = format!("refs/heads/{}", branch_name);
    for change in changes {
        if change.ref_name == ref_name {
            return Some(change);
        }
    }

    None
}

#[post("/validate")]
async fn validate(req: HttpRequest, body: web::Json<WebhookRequest>) -> impl Responder {
    let payload = body.0;
    info!("request: {:?} with body: {:?}", req, payload);

    let default_branch_change = match find_default_branch_change(&payload.default_branch, &payload.changes) {
        Some(change) => change,
        None => return accept(format!("accepted: no change to {}", payload.default_branch).as_str()),
    };

    let encoded_patch = match default_branch_change.patch {
        Some(ref patch) => patch,
        None => return accept("accepted: no files changed!"),
    };

    let restrict_glob_pattern = match env::var("RESTRICT_GLOB_PATTERN") {
        Ok(pattern) => pattern,
        Err(_) => return accept("accepted: not restricting file changes"),
    };

    let restricted_regex_pattern = Regex::new(format!("^{}$", restrict_glob_pattern).as_str())
        .expect("glob pattern should compile as a regex after translation");

    let patch_bytes = match BASE64_STANDARD.decode(encoded_patch.as_str()) {
        Ok(patch) => patch,
        Err(err) => {
            return error_reject("invalid base64 patch", err);
        }
    };

    let mut patch = PatchSet::new();
    if let Err(err) = patch.parse_bytes(patch_bytes.as_slice()) {
        return error_reject("unable to parse patch", err);
    }

    for file in patch.files() {
        let source = &file.source_file[2..];
        let target = &file.target_file[2..];
        if file_matches(&restricted_regex_pattern, source) {
            return invalid_reject(source)
        }
        if file_matches(&restricted_regex_pattern, target) {
            return invalid_reject(target)
        }
    }

    accept_empty()
}

fn file_matches(regex: &Regex, file_name: &str) -> bool {
    regex.is_match(file_name)
}

fn accept_empty() -> (web::Json<WebhookResponse>, StatusCode) {
    let response = WebhookResponse(vec![]);
    let responder = web::Json(response);
    (responder, StatusCode::OK)
}

fn accept<T: Display>(msg: T) -> (web::Json<WebhookResponse>, StatusCode) {
    let response = WebhookResponse(vec![format!("rejected: {}", msg)]);
    let responder = web::Json(response);
    (responder, StatusCode::OK)
}

fn error_reject<E: Error>(msg: &str, err: E) -> (web::Json<WebhookResponse>, StatusCode) {
    let response = WebhookResponse(vec![format!("rejected: {}: {}", msg, err)]);
    let responder = web::Json(response);
    (responder, StatusCode::BAD_REQUEST)
}

fn invalid_reject<T: Display>(file_name: T) -> (web::Json<WebhookResponse>, StatusCode) {
    let response = WebhookResponse(vec![format!("rejected: illegal file {} modified", file_name)]);
    let responder = web::Json(response);
    (responder, StatusCode::CONFLICT)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let env = Env::default()
        .default_filter_or("info");
    env_logger::init_from_env(env);
    HttpServer::new(|| App::new().service(validate))
        .bind(("0.0.0.0", 8080))?
        .run()
        .await
}