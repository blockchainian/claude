use crate::model::PullRequest;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};

pub(super) fn creates_pull_request(command: &str) -> bool {
    static CREATE: LazyLock<syntect::parsing::Regex> =
        LazyLock::new(|| syntect::parsing::Regex::new(r"\bgh\s+pr\s+create\b".into()));
    CREATE.is_match(command)
}

#[derive(Clone, Debug, Default)]
pub(super) struct CreatedPullRequest {
    pub pr: Option<PullRequest>,
    origins: HashMap<PathBuf, Option<String>>,
}

impl CreatedPullRequest {
    /// Returns whether output contains a PR URL, retaining only this directory's origin PRs.
    pub fn record(&mut self, cwd: &Path, output: &str) -> bool {
        let Some(pr) = output
            .lines()
            .rev()
            .find_map(|line| PullRequest::from_url(line.trim()))
        else {
            return false;
        };
        let origin = self.origins.entry(cwd.to_owned()).or_insert_with(|| {
            let output = Command::new("git")
                .current_dir(cwd)
                .args(["remote", "get-url", "origin"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let remote = std::str::from_utf8(&output.stdout).ok()?.trim();
            remote_repository(remote).map(str::to_owned)
        });
        let repo = pr
            .url
            .strip_prefix("https://github.com/")
            .unwrap()
            .rsplit_once("/pull/")
            .unwrap()
            .0;
        if origin
            .as_deref()
            .is_some_and(|origin| origin.eq_ignore_ascii_case(repo))
        {
            self.pr = Some(pr);
        }
        true
    }
}

fn remote_repository(remote: &str) -> Option<&str> {
    let repo = if let Some(repo) = remote.strip_prefix("https://github.com/") {
        repo
    } else {
        let (address, repo) = remote.rsplit_once(':')?;
        let (user, host) = address.split_once('@')?;
        if user.is_empty() || host.is_empty() || address.contains('/') {
            return None;
        }
        repo
    };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let (owner, name) = repo.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some(repo)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(crate) fn checkout(remote: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(dir.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .current_dir(dir.path())
                .args(["remote", "add", "origin", remote])
                .status()
                .unwrap()
                .success()
        );
        dir
    }

    #[test]
    fn command_matches_create_words() {
        for command in [
            "gh pr create",
            "gh\tpr\ncreate --draft",
            "env GH_TOKEN=x gh pr create | tail -1",
        ] {
            assert!(creates_pull_request(command), "{command}");
        }
        for command in [
            "gh pr view",
            "xgh pr create",
            "gh pr created",
            "python3 -c \"subprocess.run(['gh', 'pr', 'create'])\"",
        ] {
            assert!(!creates_pull_request(command), "{command}");
        }
    }

    #[test]
    fn scp_origins_accept_users_hosts_and_ssh_aliases() {
        for (remote, expected) in [
            (
                "git@blockchainian:blockchainian/claude.git",
                "blockchainian/claude",
            ),
            (
                "git@example-org:example-org/wallet.git",
                "example-org/wallet",
            ),
            ("alice@code.example.org:Example/Project", "Example/Project"),
        ] {
            assert_eq!(remote_repository(remote), Some(expected), "{remote}");
            let dir = checkout(remote);
            let mut created = CreatedPullRequest::default();
            assert!(created.record(
                dir.path(),
                &format!("https://github.com/{expected}/pull/52")
            ));
            assert_eq!(
                created.pr.as_ref().map(|pr| pr.number),
                Some(52),
                "{remote}"
            );
            assert!(created.record(dir.path(), "https://github.com/another/repository/pull/850"));
            assert_eq!(created.pr.as_ref().map(|pr| pr.number), Some(52));
        }
        for remote in [
            "git@:owner/repo",
            "@host:owner/repo",
            "host:owner/repo",
            "git@host:/owner/repo",
            "git@host:owner/repo/extra",
            "git@host:owner/",
        ] {
            assert_eq!(remote_repository(remote), None, "{remote}");
        }
    }

    #[test]
    fn records_last_url_only_for_origin_and_keeps_accepted_pr() {
        for remote in [
            "git@github.com:Example/Project.git",
            "https://github.com/Example/Project.git",
            "https://github.com/Example/Project",
        ] {
            let dir = checkout(remote);
            let mut created = CreatedPullRequest::default();
            assert!(!created.record(
                dir.path(),
                "pull request create failed\n[exited with code 0]"
            ));
            assert!(created.origins.is_empty());
            assert!(created.record(dir.path(), "https://github.com/example/project/pull/1\nhttps://github.com/example/project/pull/2\n[exited with code 1]"));
            assert_eq!(created.pr.as_ref().unwrap().number, 2);
            assert!(created.record(dir.path(), "https://github.com/elsewhere/project/pull/3"));
            assert_eq!(created.pr.as_ref().unwrap().number, 2);
            assert!(
                Command::new("git")
                    .current_dir(dir.path())
                    .args([
                        "remote",
                        "set-url",
                        "origin",
                        "https://github.com/elsewhere/project"
                    ])
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(created.record(dir.path(), "https://github.com/example/project/pull/4"));
            assert_eq!(created.pr.as_ref().unwrap().number, 4);
            assert_eq!(created.origins.len(), 1);
        }
    }

    #[test]
    fn caches_unavailable_origin_and_uses_last_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let mut created = CreatedPullRequest::default();
        assert!(created.record(dir.path(), "https://github.com/example/project/pull/1"));
        assert_eq!(created.pr, None);
        assert_eq!(created.origins.len(), 1);
        let dir = checkout("git@github.com:example/project.git");
        assert!(created.record(
            dir.path(),
            "https://github.com/example/project/pull/2\nhttps://github.com/other/project/pull/3"
        ));
        assert_eq!(created.pr, None);
    }
}
