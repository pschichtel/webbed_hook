use std::ffi::OsStr;
use std::io::{BufRead, Error, Lines};
use std::process::{Command, Output, Stdio};
use std::str::FromStr;
use webbed_hook_core::webhook::{convert_to_utc_rfc3339, DateTime, GitLogEntry, Utc};

const MULTILINE_INDENT: usize = 4;

fn run_git_command<I, S>(args: I) -> Result<Option<Output>, Error>
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
        .map(|output| {
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

pub fn git_show_file_from_default_branch(file: &str) -> Result<Option<String>, String> {
    run_git_command(["show", format!("HEAD:{}", file).as_str()])
        .map_err(|err| err.to_string())
        .and_then(|output| {
            match output {
                Some(output) => String::from_utf8(output.stdout)
                    .map(|s| Some(s))
                    .map_err(|err| format!("invalid utf-8: {}", err).to_string()),
                None => Ok(None)
            }
        })
}

pub fn diff(old_commit: &str, new_commit: &str) -> Option<String> {
    run_git_command(["diff", format!("{}..{}", old_commit, new_commit).as_str()])
        .ok()
        .flatten()
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

#[derive(PartialEq, Debug)]
pub enum FileStatus {
    Added,
    Copied,
    Deleted,
    Modified,
    Renamed,
    TypeChanged,
    Unmerged,
    Unknown,
    BrokenPairing,
}

impl FromStr for FileStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "A" => Ok(FileStatus::Added),
            "C" => Ok(FileStatus::Copied),
            "D" => Ok(FileStatus::Deleted),
            "M" => Ok(FileStatus::Modified),
            "R" => Ok(FileStatus::Renamed),
            "T" => Ok(FileStatus::TypeChanged),
            "U" => Ok(FileStatus::Unmerged),
            "X" => Ok(FileStatus::Unknown),
            "B" => Ok(FileStatus::BrokenPairing),
            _ => Err(format!("unknown file status: {}", s)),
        }
    }
}

fn parse_name_status<T: Iterator<Item=Result<String, Error>>>(lines: &mut T) -> Vec<(FileStatus, String)> {
    lines
        .filter_map(|line| {
            let line = line.ok()?;
            let mut iter = line.trim().split_ascii_whitespace();
            let status = FileStatus::from_str(iter.next()?).ok()?;
            let name = iter.next()?;
            if let Some(_) = iter.next() {
                None
            } else {
                Some((status, name.to_string()))
            }
        })
        .collect::<Vec<_>>()
}

pub fn diff_name_status(old_commit: &str, new_commit: &str) -> Vec<(FileStatus, String)> {
    run_git_command(["diff", "--name-status", format!("{}..{}", old_commit, new_commit).as_str()])
        .ok()
        .flatten()
        .map(|output| {
            let mut lines = output.stdout.lines();
            parse_name_status(&mut lines)
        })
        .unwrap_or_default()
}

pub fn merge_base(old_commit: &str, new_commit: &str) -> Option<String> {
    run_git_command(vec!["merge-base", old_commit, new_commit])
        .ok()
        .flatten()
        .and_then(|output| {
            String::from_utf8(output.stdout).map(|s| s.as_str().trim().to_string()).ok()
        })
}

fn git_log(args: Vec<&str>) -> Vec<GitLogEntry> {
    let format = format!("--format=commit%n%H%n%P%n%n%aN <%aE>%n%aI%n%cN <%cE>%n%cI%n%GK%n%w(0,{0},{0})%B%n", MULTILINE_INDENT);
    let mut full_args = vec!["log", "--reverse", format.as_str()];
    full_args.extend(args);
    run_git_command(full_args)
        .ok()
        .flatten()
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
        .ok()
        .flatten()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|branch_name| branch_name.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use super::*;

    #[test]
    fn test_name_status_parsing() {
        let name_status_text = indoc! {"
            M       Cargo.lock
            M       Cargo.toml
            M       README.md
            M       core/Cargo.toml
            M       core/src/webhook.rs
            M       src/configuration.rs
            M       src/git.rs
            M       src/main.rs
            A       src/rule.rs
            M       src/webhook.rs
        "};

        let mut line_iter = name_status_text.lines().map(|s| Ok(s.to_owned()));
        let actual = parse_name_status(&mut line_iter);
        let expected = vec![
            (FileStatus::Modified, "Cargo.lock".to_owned()),
            (FileStatus::Modified, "Cargo.toml".to_owned()),
            (FileStatus::Modified, "README.md".to_owned()),
            (FileStatus::Modified, "core/Cargo.toml".to_owned()),
            (FileStatus::Modified, "core/src/webhook.rs".to_owned()),
            (FileStatus::Modified, "src/configuration.rs".to_owned()),
            (FileStatus::Modified, "src/git.rs".to_owned()),
            (FileStatus::Modified, "src/main.rs".to_owned()),
            (FileStatus::Added, "src/rule.rs".to_owned()),
            (FileStatus::Modified, "src/webhook.rs".to_owned()),
        ];
        assert_eq!(actual, expected);
    }
}