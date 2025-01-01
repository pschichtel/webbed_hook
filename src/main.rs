mod configuration;
mod webhook;
mod util;
mod gitlab;
mod git;
mod rule;

use crate::rule::{RuleAction, RuleContext, RuleResult};
use crate::configuration::{Configuration, HookBypass, HookType};
use crate::git::{format_patch, get_default_branch, git_log_for_range, git_log_limited, git_show_file_from_default_branch, merge_base};
use crate::util::env_as;
use path_clean::PathClean;
use std::env;
use std::error::Error;
use std::fmt::Display;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::exit;
use webbed_hook_core::webhook::Change;

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

fn resolve_change(line: ChangeLine) -> Option<Change> {
    let old_exists = !is_hash_all_zeros(&line.old_commit);
    let new_exists = !is_hash_all_zeros(&line.new_commit);

    match (old_exists, new_exists) {
        (true, true) => {
            let patch = format_patch(&line.old_commit, &line.new_commit);
            let merge_base = merge_base(&line.old_commit, &line.new_commit);
            let log = match merge_base {
                Some(ref base) => Some(git_log_for_range(base, &line.new_commit)),
                None => Some(git_log_limited(100, &line.new_commit))
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

fn resolve_changes(changes: Vec<ChangeLine>) -> Vec<Change> {
    changes.into_iter()
        .filter_map(|line| resolve_change(line))
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
    load_config("hooks.yaml", |s| serde_yml::from_str(s))
        .or_else(|| load_config("hooks.yml", |s| serde_yml::from_str(s)))
        .or_else(|| load_config("hooks.toml", toml::from_str))
}

fn accept<T: Display>(messages: Vec<T>) {
    for msg in messages {
        println!("{}", msg);
    }
    exit(0);
}

fn reject<T: Display>(messages: Vec<T>) {
    for msg in messages {
        eprintln!("{}", msg);
    }
    exit(1);
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

        let changes = match get_changes(hook_type) {
            Some(changes) => changes,
            None => {
                exit(0);
            }
        };

        let resolved_changes = resolve_changes(changes);

        for change in resolved_changes.iter() {
            let ctx = RuleContext {
                default_branch: default_branch.as_str(),
                push_options: push_options.as_slice(),
                change,
            };

            match hook.rule.evaluate(&ctx) {
                Ok(RuleResult { action, messages }) => {
                    match action {
                        RuleAction::Accept => accept(messages),
                        RuleAction::Continue => accept(messages),
                        RuleAction::Reject => reject(messages),
                    }
                }
                Err(err) => {
                    let reject_on_err = hook.reject_on_error.unwrap_or(true);
                    if reject_on_err {
                        reject(vec![format!("change rejected, evaluation failed: {}", err)]);
                    } else {
                        accept(vec![format!("change accepted, but evaluation failed: {}", err)]);
                    }
                }
            }
        }
    }
}
