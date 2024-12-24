mod configuration;
mod webhook;
mod util;
mod gitlab;
mod git;

use crate::configuration::{Configuration, Hook, HookBypass, HookType};
use crate::git::{format_patch, get_default_branch, git_log_for_range, git_log_limited, merge_base, git_show_file_from_default_branch};
use crate::webhook::{perform_request, WebhookResult};
use path_clean::PathClean;
use std::env;
use std::error::Error;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::exit;
use webbed_hook_core::webhook::{Change, WebhookResponse};
use crate::util::env_as;

#[derive(Debug)]
pub struct ChangeLine {
    pub old_commit: String,
    pub new_commit: String,
    pub ref_name: String,
}

fn read_changes_from_stdin() -> Option<Vec<ChangeLine>> {
    let stdin = std::io::stdin();
    let changes = stdin.lock().lines()
        .into_iter()
        .filter_map(|line| line.ok())
        .map(|line| {
            let parts = line.split(' ').collect::<Vec<_>>();
            let old_commit = parts[0].to_owned();
            let new_commit = parts[1].to_owned();
            ChangeLine {
                old_commit,
                new_commit,
                ref_name: parts[2].to_owned(),
            }
        })
        .collect::<Vec<ChangeLine>>();
    if changes.is_empty() {
        None
    } else {
        Some(changes)
    }
}

fn read_change_from_args() -> Option<ChangeLine> {
    let mut args = env::args();
    let ref_name = args.next();
    let old_commit = args.next();
    let new_commit = args.next();

    match (ref_name, old_commit, new_commit) {
        (Some(ref_name), Some(old_commit), Some(new_commit)) => Some(ChangeLine {
            ref_name,
            old_commit,
            new_commit,
        }),
        _ => None,
    }
}

fn get_changes(hook_type: HookType) -> Option<Vec<ChangeLine>> {
    match hook_type {
        HookType::PreReceive => read_changes_from_stdin(),
        HookType::Update => read_change_from_args().map(|c| vec![c]),
        HookType::PostReceive => read_changes_from_stdin(),
    }
}

fn is_hash_all_zeros(hash: &str) -> bool {
    hash.chars().all(|c| c == '0')
}

fn resolve_change(line: ChangeLine, hook: &Hook) -> Option<Change> {
    let old_exists = !is_hash_all_zeros(&line.old_commit);
    let new_exists = !is_hash_all_zeros(&line.new_commit);

    match (old_exists, new_exists) {
        (true, true) => {
            let patch = if hook.include_patch.unwrap_or(true) {
                format_patch(&line.old_commit, &line.new_commit)
            } else { 
                None
            };
            let merge_base = merge_base(&line.old_commit, &line.new_commit);
            let log = if hook.include_log.unwrap_or(true) {
                match merge_base {
                    Some(ref base) => Some(git_log_for_range(base, &line.new_commit)),
                    None => Some(git_log_limited(100, &line.new_commit))
                }
            } else {
                None
            };
            let force = match merge_base {
                Some(ref base) => base != &line.old_commit,
                None => true
            };
            Some(Change::UpdateRef {
                name: line.ref_name,
                old_commit: line.old_commit,
                new_commit: line.new_commit,
                merge_base,
                force,
                patch,
                log,
            })
        },
        (true, false) => Some(Change::RemoveRef {
            name: line.ref_name,
            commit: line.old_commit,
        }),
        (false, true) => Some(Change::AddRef {
            name: line.ref_name,
            commit: line.new_commit,
        }),
        (false, false) => None
    }

}

fn resolve_changes(changes: Vec<ChangeLine>, hook: &Hook) -> Vec<Change> {
    changes.into_iter()
        .filter_map(|line| resolve_change(line, hook))
        .collect()
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

fn applies_to_changes(hook: &Hook, changes: &Vec<ChangeLine>) -> bool {
    for change in changes {
        if hook.applies_to(change.ref_name.as_str()) {
            return true;
        }
    }
    false
}

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

fn attempt_bypass(options: &Vec<String>, bypass: &Option<HookBypass>) {
    if let Some(ref bypass) = bypass {
        if options.contains(&bypass.push_option) {
            if let Some(ref messages) = bypass.messages {
                for line in messages {
                    println!("{}", line)
                }
            }
            exit(0)
        }
    }
}

fn load_config<E: Error, T: FnOnce(&str) -> Result<Configuration, E>>(name: &str, parse: T) -> Option<Configuration> {
    git_show_file_from_default_branch(name)
        .and_then(|content| parse(content.as_str()).ok())
}

fn load_config_from_default_branch() -> Option<Configuration> {
    load_config("hooks.json", |s| serde_json::from_str(s))
        .or_else(|| load_config("hooks.yaml", |s| serde_yml::from_str(s)))
        .or_else(|| load_config("hooks.yml", |s| serde_yml::from_str(s)))
        .or_else(|| load_config("hooks.toml", |s| toml::from_str(s)))
}

fn main() {
    let default_branch = match get_default_branch() {
        Some(branch) => branch,
        None => exit(0)
    };
    let config = match load_config_from_default_branch() {
        Some(configuration) => configuration,
        None => exit(0),
    };

    let config = match config {
        Configuration::Version1(v1) => v1
    };

    let push_options = get_push_options();
    attempt_bypass(&push_options, &config.bypass);

    if let Some((hook, hook_type)) = config.select_hook() {
        attempt_bypass(&push_options, &hook.bypass);
        
        let changes = match get_changes(hook_type) {
            Some(changes) => changes,
            None => {
                exit(0);
            }
        };

        if !applies_to_changes(&hook, &changes) {
            exit(0);
        }

        let resolved_changes = resolve_changes(changes, hook);

        match perform_request(default_branch, push_options, hook, resolved_changes) {
            Ok(WebhookResult(success, WebhookResponse(messages))) => {
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
                let reject = hook.reject_on_error.unwrap_or(true);
                exit(if reject { 1 } else { 0 });
            }
        }

    }
}
