use crate::util::env_as;
use serde::Serialize;
use std::env;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
enum GitlabId {
    #[serde(rename = "user")]
    User(u64),
    #[serde(rename = "key")]
    Key(u64),
}

#[derive(Debug)]
pub enum GitlabParseError {
    UnsupportedInput(String),
    ParseIntError(std::num::ParseIntError),
}

impl Display for GitlabParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            GitlabParseError::UnsupportedInput(input) => {
                write!(f, "unsupported input: {}", input)
            }
            GitlabParseError::ParseIntError(err) => {
                write!(f, "unable to parse id as int: {}", err)
            }
        }
    }
}

impl FromStr for GitlabId {
    type Err = GitlabParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("user-") {
            return s.parse::<u64>()
                .map(GitlabId::User)
                .map_err(|e| GitlabParseError::ParseIntError(e))
        }
        if s.starts_with("key-") {
            return s.parse::<u64>()
                .map(GitlabId::Key)
                .map_err(|e| GitlabParseError::ParseIntError(e))
        }
        Err(GitlabParseError::UnsupportedInput(s.to_string()))
    }
}

#[derive(Serialize, Debug)]
enum GitlabProtocol {
    #[serde(rename = "http")]
    HTTP,
    #[serde(rename = "ssh")]
    SSH,
    #[serde(rename = "web")]
    WEB,
}

impl FromStr for GitlabProtocol {
    type Err = GitlabParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "http" => Ok(GitlabProtocol::HTTP),
            "ssh" => Ok(GitlabProtocol::SSH),
            "web" => Ok(GitlabProtocol::WEB),
            _ => Err(GitlabParseError::UnsupportedInput(s.to_string())),
        }
    }
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
pub enum GitlabRepository {
    #[serde(rename = "project")]
    ProjectId(u64)
}

impl FromStr for GitlabRepository {
    type Err = GitlabParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("project-") {
            return s.parse::<u64>()
                .map(GitlabRepository::ProjectId)
                .map_err(|e| GitlabParseError::ParseIntError(e))
        }
        Err(GitlabParseError::UnsupportedInput(s.to_string()))
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct GitlabMetadata {
    id: GitlabId,
    project_path: String,
    protocol: GitlabProtocol,
    repository: GitlabRepository,
    username: String,
}

pub fn get_gitlab_metadata() -> Option<GitlabMetadata> {
    let id = match env_as::<GitlabId>("GL_ID") {
        Some(v) => v,
        None => return None,
    };
    let project_path = match env::var("GL_PROJECT_PATH").ok() {
        Some(v) => v,
        None => return None,
    };
    let protocol = match env_as::<GitlabProtocol>("GL_PROTOCOL") {
        Some(v) => v,
        None => return None,
    };
    let repository = match env_as::<GitlabRepository>("GL_REPOSITORY") {
        Some(v) => v,
        None => return None,
    };
    let username = match env::var("GL_USERNAME").ok() {
        Some(v) => v,
        None => return None,
    };

    Some(GitlabMetadata {
        id,
        project_path,
        protocol,
        repository,
        username,
    })
}