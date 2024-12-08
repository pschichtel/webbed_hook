use crate::webhook::{perform_request, ChangeWithPatch, WebhookResult};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use path_clean::PathClean;
use std::env;
use std::ffi::OsStr;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Output, Stdio};
use crate::configuration::{Configuration, Hook};

mod webhook;
mod configuration;
mod gitlab;
mod util;

#[derive(Debug)]
pub struct Change {
    pub old_commit: String,
    pub new_commit: String,
    pub ref_name: String,
}

fn run_git_command<I, S>(args: I) -> Option<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(output)
            } else {
                None
            }
        })
}

fn load_config_from_default_branch() -> Option<Configuration> {
    run_git_command(["show", "HEAD:hooks.json"])
        .and_then(|output| {
            return serde_json::from_slice::<Configuration>(output.stdout.as_slice()).ok()
        })
}

fn format_patch(old_commit: &str, new_commit: &str) -> Option<String> {
    run_git_command(["format-patch", "--stdout", format!("{}..{}", old_commit, new_commit).as_str()])
        .map(|output| {
            BASE64_STANDARD.encode(output.stdout.as_slice())
        })
}

pub fn get_absolute_program_path() -> Result<PathBuf, std::io::Error> {
    let program_name = env::args().next().expect("No program name provided");
    let path = Path::new(program_name.as_str());
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        env::current_dir().map(|p| p.join(path))
    }.map(|p| p.clean())
}

fn applies_to_changes(hook: &Hook, changes: &Vec<Change>) -> bool {
    for change in changes {
        if hook.applies_to(change.ref_name.as_str()) {
            return true;
        }
    }
    false
}

fn main() {
    let config = match load_config_from_default_branch() {
        Some(configuration) => configuration,
        None => exit(0),
    };

    let config = match config {
        Configuration::Version1(v1) => v1
    };

    let hook = config.select_hook();
    if let Some(hook) = hook {
        let stdin = std::io::stdin();
        let changes = stdin.lock().lines()
            .into_iter()
            .filter_map(|line| line.ok())
            .map(|line| {
                let parts = line.split(' ').collect::<Vec<_>>();
                let old_commit = parts[0].to_owned();
                let new_commit = parts[1].to_owned();
                Change {
                    old_commit,
                    new_commit,
                    ref_name: parts[2].to_owned(),
                }
            })
            .collect::<Vec<Change>>();

        if !applies_to_changes(&hook, &changes) {
            exit(0);
        }

        let with_patch = changes.into_iter().map(|change| {
            let patch = format_patch(&change.old_commit, &change.new_commit);
            ChangeWithPatch {
                old_commit: change.old_commit,
                new_commit: change.new_commit,
                ref_name: change.ref_name,
                patch,
            }
        }).collect();

        match perform_request(&hook, with_patch) {
            Ok(WebhookResult(success, messages)) => {
                if success {
                    for message in messages {
                        println!("{}", message);
                    }
                } else {
                    for message in messages {
                        eprintln!("{}", message);
                    }
                    exit(1);
                }
            }
            Err(error) => {
                eprintln!("hook failed: {}", error);
                exit(1);
            }
        }

    }
}
