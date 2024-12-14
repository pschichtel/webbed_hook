use std::ffi::OsStr;
use std::io::{BufRead, Lines};
use std::process::{Command, Output, Stdio};
use webbed_hook_core::webhook::{convert_to_utc_rfc3339, DateTime, GitLogEntry, Utc};
use crate::configuration::Configuration;

const MULTILINE_INDENT: usize = 4;

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

fn parse_indented_multiline_string(lines: &mut Lines<&[u8]>) -> String {
    let mut message = String::new();
    while let Some(Ok(ref line)) = lines.next() {
        if line.is_empty() {
            break;
        }
        if !message.is_empty() {
            message.push('\n');
        }
        message.push_str(&line.as_str()[MULTILINE_INDENT..]);
    }
    message
}

fn parse_single_optional_line(lines: &mut Lines<&[u8]>) -> Result<Option<String>, String> {
    match lines.next() {
        Some(line) => line
            .map_err(|err| err.to_string())
            .map(|line| {
                println!("some line: {}", line);
                if line.is_empty() { None } else { Some(line) }
            }),
        None => {
            println!("no line!");
            Err("no more lines".to_string())
        },
    }
}

fn parse_single_line(lines: &mut Lines<&[u8]>) -> Result<String, String> {
    match lines.next() {
        Some(line) => line.map_err(|err| err.to_string()),
        None => Err("no more lines".to_string()),
    }
}

fn parse_single_date_line(lines: &mut Lines<&[u8]>) -> Result<DateTime<Utc>, String> {
    parse_single_line(lines).and_then(|date| {
        convert_to_utc_rfc3339(date.as_str()).map_err(|_| "broken date".to_string())
    })
}

fn parse_lines_until_empty(lines: &mut Lines<&[u8]>) -> Vec<String> {
    let mut output: Vec<String> = Vec::new();
    loop {
        match lines.next() {
            Some(Ok(line)) => {
                if line.is_empty() {
                    break
                } else {
                    output.push(line);
                }
            }
            _ => {
                break
            }
        }
    }
    output
}

fn parse_log_entry(lines: &mut Lines<&[u8]>) -> Result<Option<GitLogEntry>, String> {
    loop {
        match lines.next() {
            Some(Ok(line)) if line == "commit" => {
                break
            }
            None => {
                return Ok(None)
            }
            _ => {}
        }
    }

    let hash = parse_single_line(lines)?;
    let parents = parse_lines_until_empty(lines);
    let author = parse_single_line(lines)?;
    let author_date = parse_single_date_line(lines)?;
    let committer = parse_single_line(lines)?;
    let committer_date = parse_single_date_line(lines)?;
    let signed_by_key_id = parse_single_optional_line(lines)?;

    let message = parse_indented_multiline_string(lines);

    Ok(Some(GitLogEntry {
        hash,
        parents,
        author,
        author_date,
        committer,
        committer_date,
        signed_by_key_id,
        message,
    }))
}

fn parse_log(lines: &mut Lines<&[u8]>) -> Vec<GitLogEntry> {
    let mut output: Vec<GitLogEntry> = Vec::new();
    loop {
        match parse_log_entry(lines) {
            Ok(Some(entry)) => output.push(entry),
            Ok(None) => break,
            _ => {}
        }
    }
    output
}

pub fn load_config_from_default_branch() -> Option<Configuration> {
    run_git_command(["show", "HEAD:hooks.json"])
        .and_then(|output| {
            serde_json::from_slice::<Configuration>(output.stdout.as_slice()).ok()
        })
}

pub fn format_patch(old_commit: &str, new_commit: &str) -> Option<String> {
    run_git_command(["format-patch", "--stdout", format!("{}..{}", old_commit, new_commit).as_str()])
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

pub fn merge_base(old_commit: &str, new_commit: &str) -> Option<String> {
    run_git_command(vec!["merge-base", old_commit, new_commit])
        .and_then(|output| {
            String::from_utf8(output.stdout).map(|s| s.as_str().trim().to_string()).ok()
        })
}

fn git_log(args: Vec<&str>) -> Vec<GitLogEntry> {
    let format = format!("--format=commit%n%H%n%P%n%n%aN <%aE>%n%aI%n%cN <%cE>%n%cI%n%GK%n%w(0,{0},{0})%B%n", MULTILINE_INDENT);
    let mut full_args = vec!["log", "--reverse", format.as_str()];
    full_args.extend(args);
    run_git_command(full_args)
        .map(|output| {
            let mut lines = output.stdout.lines();
            parse_log(&mut lines)
        })
        .unwrap_or_default()
}

pub fn git_log_for_range(from: &str, to: &str) -> Vec<GitLogEntry> {
    git_log(vec![format!("{}..{}", from, to).as_str()])
}

pub fn git_log_limited(limit: u32, to: &str) -> Vec<GitLogEntry> {
    git_log(vec![format!("--max-count={}", limit).as_str(), to])
}

pub fn get_default_branch() -> Option<String> {
    run_git_command(["rev-parse", "--abbrev-ref", "HEAD"])
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|branch_name| branch_name.trim_end().to_string())
}