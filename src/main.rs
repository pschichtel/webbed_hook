mod configuration;
mod webhook;
mod util;
mod gitlab;
mod git;
mod rule;

use std::cell::LazyCell;
use crate::rule::{RuleAction, RuleContext, RuleResult};
use crate::configuration::{Configuration, HookBypass, HookType};
use crate::git::{diff, diff_name_status, get_default_branch, git_log_for_range, git_log_limited, git_show_file_from_default_branch, merge_base, FileStatus};
use crate::util::env_as;
use path_clean::PathClean;
use std::env;
use std::error::Error;
use std::fmt::Display;
use std::io::BufRead;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::exit;
use webbed_hook_core::webhook::{GitLogEntry};

pub struct GitData {
    pub patch: Box<dyn Deref<Target=Option<String>>>,
    pub log: Box<dyn Deref<Target=Vec<GitLogEntry>>>,
    pub file_status: Box<dyn Deref<Target=Vec<(FileStatus, String)>>>,
}

pub enum Change {
    AddRef {
        name: String,
        commit: String,
        git_data: GitData,
    },
    RemoveRef {
        name: String,
        commit: String,
    },
    UpdateRef {
        name: String,
        old_commit: String,
        new_commit: String,
        merge_base: Option<String>,
        force: bool,
        git_data: GitData,
    }
}

impl Change {
    pub fn ref_name(&self) -> &str {
        match self {
            Change::AddRef { name, .. } => name.as_str(),
            Change::RemoveRef { name, .. } => name.as_str(),
            Change::UpdateRef { name, .. } => name.as_str(),
        }
    }
}

#[derive(Debug, Clone)]
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

fn lazy_diff(old_commit: &str, new_commit: &str) -> Box<dyn Deref<Target=Option<String>>> {
    let old_commit = old_commit.to_owned();
    let new_commit = new_commit.to_owned();

    Box::new(LazyCell::new(move || diff(old_commit.as_str(), new_commit.as_str())))
}

fn lazy_file_status(old_commit: &str, new_commit: &str) -> Box<dyn Deref<Target=Vec<(FileStatus, String)>>> {
    let old_commit = old_commit.to_owned();
    let new_commit = new_commit.to_owned();

    Box::new(LazyCell::new(move || diff_name_status(old_commit.as_str(), new_commit.as_str())))
}

fn lazy_log(base: &Option<String>, new_commit: &str) -> Box<dyn Deref<Target=Vec<GitLogEntry>>> {
    let new_commit = new_commit.to_owned();
    match base {
        Some(base) => {
            let base = base.to_owned();
            Box::new(LazyCell::new(move || git_log_for_range(base.as_str(), new_commit.as_str())))
        },
        None => {
            Box::new(LazyCell::new(move || git_log_limited(100, new_commit.as_str())))
        }
    }
}

fn resolve_change(line: ChangeLine, default_branch: &str) -> Option<Change> {
    let old_exists = !is_hash_all_zeros(&line.old_commit);
    let new_exists = !is_hash_all_zeros(&line.new_commit);
    let patch = lazy_diff(&line.old_commit, &line.new_commit);
    let file_status = lazy_file_status(&line.old_commit, &line.new_commit);

    match (old_exists, new_exists) {
        (true, true) => {
            let merge_base = merge_base(&line.old_commit, &line.new_commit);
            let log = lazy_log(&merge_base, &line.new_commit);
            let force = match merge_base {
                Some(ref base) => base != &line.old_commit,
                None => true
            };
            let git_data = GitData {
                patch,
                log,
                file_status,
            };
            Some(Change::UpdateRef {
                name: line.ref_name,
                old_commit: line.old_commit,
                new_commit: line.new_commit,
                merge_base,
                force,
                git_data,
            })
        },
        (true, false) => Some(Change::RemoveRef {
            name: line.ref_name,
            commit: line.old_commit,
        }),
        (false, true) => {
            let merge_base = merge_base(default_branch, &line.new_commit);
            let log = lazy_log(&merge_base, &line.new_commit);
            let git_data = GitData {
                patch,
                log,
                file_status,
            };
            Some(Change::AddRef {
                name: line.ref_name,
                commit: line.new_commit,
                git_data,
            })
        },
        (false, false) => None
    }

}

fn resolve_changes(changes: Vec<ChangeLine>, default_branch: &str) -> Vec<Change> {
    changes.into_iter()
        .filter_map(|line| resolve_change(line, default_branch))
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
    if let Some(bypass) = bypass {
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

fn load_config<E: Error, T: FnOnce(&str) -> Result<Configuration, E>>(name: &str, parse: T) -> Result<Option<Configuration>, String> {
    git_show_file_from_default_branch(name)
        .and_then(|content| {
            match content {
                Some(content) => parse(content.as_str())
                    .map(|c| Some(c))
                    .map_err(|err| err.to_string()),
                None => Ok(None)
            }
        })
}

fn load_config_from_default_branch() -> Result<Option<Configuration>, String> {
    if let Some(yaml) = load_config("hooks.yaml", |s| serde_yml::from_str(s))? {
        return Ok(Some(yaml))
    }
    if let Some(yaml) = load_config("hooks.yml", |s| serde_yml::from_str(s))? {
        return Ok(Some(yaml))
    }
    if let Some(toml) = load_config("hooks.toml", |s| toml::from_str(s))? {
        return Ok(Some(toml))
    }
    Ok(None)
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
        Ok(Some(configuration)) => configuration,
        Ok(None) => exit(0),
        Err(err) => {
            eprintln!("Failed to parse hook configuration: {}", err);
            exit(0)
        }
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

        let resolved_changes = resolve_changes(changes, default_branch.as_str());

        for change in resolved_changes.iter() {
            let ctx = RuleContext {
                default_branch: default_branch.as_str(),
                push_options: push_options.as_slice(),
                change,
                config: &config,
            };

            match hook.rule.evaluate(&ctx, 0) {
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
