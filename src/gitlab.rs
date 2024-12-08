use crate::util::env_as;
use std::env;
use webbed_hook_core::gitlab::{GitlabId, GitlabMetadata, GitlabProtocol, GitlabRepository};

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