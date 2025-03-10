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

#[cfg(test)]
mod tests {
    use webbed_hook_core::gitlab::GitlabProtocol::SSH;
    use super::*;

    #[test]
    fn test_metadata_gathering() {
        unsafe {
            env::set_var("GL_USERNAME", "some-user");
            env::set_var("GL_ID", "key-123123");
            env::set_var("GL_PROJECT_PATH", "some-group/some-project");
            env::set_var("GL_REPOSITORY", "project-456456");
            env::set_var("GL_PROTOCOL", "ssh");
        }

        let expected = GitlabMetadata {
            id: GitlabId::Key { id: 123123 },
            project_path: "some-group/some-project".to_string(),
            protocol: SSH,
            repository: GitlabRepository::ProjectId { id: 456456 },
            username: "some-user".to_string(),
        };
        assert_eq!(get_gitlab_metadata(), Some(expected));
    }
}