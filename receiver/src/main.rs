use std::env;
use std::error::Error;
use std::fmt::Display;
use actix_web::web;
use actix_web::{post, App, HttpRequest, HttpServer, Responder};
use actix_web::http::StatusCode;
use env_logger::Env;
use log::info;
use regex::Regex;
use unidiff::PatchSet;
use webbed_hook_core::webhook::{Change, WebhookRequest, WebhookResponse};

fn find_default_branch_change<'a>(branch_name: &'a str, changes: &'a Vec<Change>) -> Option<&'a Change> {
    let ref_name = &format!("refs/heads/{}", branch_name);
    for change in changes {
        match change {
            Change::AddRef { name, .. } if name == ref_name => return Some(change),
            Change::RemoveRef { name, .. } if name == ref_name => return Some(change),
            Change::UpdateRef { name, .. } if name == ref_name => return Some(change),
            _ => {}
        }
    }

    None
}

#[post("/validate")]
async fn validate(req: HttpRequest, body: web::Json<WebhookRequest>) -> impl Responder {
    let payload = body.0;
    info!("request: {:?} with body: {:?}", req, payload);

    let patch = match find_default_branch_change(&payload.default_branch, &payload.changes) {
        Some(Change::UpdateRef { patch, .. }) => patch,
        _ => return accept(format!("no change to {}", payload.default_branch).as_str()),
    };

    let patch_str = match patch {
        Some(patch) => patch,
        None => return accept("no files changed!"),
    };

    let restrict_glob_pattern = match env::var("RESTRICT_GLOB_PATTERN") {
        Ok(pattern) => pattern,
        Err(_) => return accept("not restricting file changes"),
    };

    let restricted_regex_pattern = Regex::new(format!("^{}$", restrict_glob_pattern).as_str())
        .expect("glob pattern should compile as a regex after translation");

    let mut patch = PatchSet::new();
    if let Err(err) = patch.parse(patch_str) {
        return error_reject("unable to parse patch", err);
    }

    for file in patch.files() {
        if let Some(source) = &file.source_file.strip_prefix("a/") {
            if file_matches(&restricted_regex_pattern, source) {
                return invalid_reject(source)
            }
        }
        if let Some(target) = &file.target_file.strip_prefix("b/") {
            if file_matches(&restricted_regex_pattern, target) {
                return invalid_reject(target)
            }
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
    let response = WebhookResponse(vec![format!("accepted: {}", msg)]);
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

const DEFAULT_PORT: u16 = 8080;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let env = Env::default()
        .default_filter_or("info");
    env_logger::init_from_env(env);
    let listen_port = match env::var("LISTEN_PORT") {
        Ok(s) => s.parse::<u16>().unwrap_or(DEFAULT_PORT),
        Err(_) => DEFAULT_PORT
    };
    HttpServer::new(|| App::new().service(validate))
        .bind(("0.0.0.0", listen_port))?
        .run()
        .await
}