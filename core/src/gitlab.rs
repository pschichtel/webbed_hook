use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum GitlabId {
    #[serde(rename = "user")]
    User { id: u64 },
    #[serde(rename = "key")]
    Key { id: u64 },
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
                .map(|id| GitlabId::User{id})
                .map_err(|e| GitlabParseError::ParseIntError(e))
        }
        if s.starts_with("key-") {
            return s.parse::<u64>()
                .map(|id| GitlabId::Key{id})
                .map_err(|e| GitlabParseError::ParseIntError(e))
        }
        Err(GitlabParseError::UnsupportedInput(s.to_string()))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum GitlabProtocol {
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum GitlabRepository {
    #[serde(rename = "project")]
    ProjectId { id: u64 },
}

impl FromStr for GitlabRepository {
    type Err = GitlabParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("project-") {
            return s.parse::<u64>()
                .map(|id| GitlabRepository::ProjectId{id})
                .map_err(|e| GitlabParseError::ParseIntError(e))
        }
        Err(GitlabParseError::UnsupportedInput(s.to_string()))
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct GitlabMetadata {
    pub id: GitlabId,
    pub project_path: String,
    pub protocol: GitlabProtocol,
    pub repository: GitlabRepository,
    pub username: String,
}
