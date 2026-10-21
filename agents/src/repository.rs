//! Identifies the git repository behind a directory without running git, so linked worktrees
//! group under their primary checkout.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryIdentity {
    /// The primary checkout's `.git` directory, shared by all of its worktrees.
    pub common_dir: PathBuf,
    /// The directory inside the checkout, relative to the checkout root.
    pub relative_cwd: PathBuf,
    /// The primary checkout's root.
    pub primary_root: PathBuf,
}

/// Trusts only administrative links that point back at each other; anything odd yields `None`.
pub fn repository_identity(cwd: &Path) -> Option<RepositoryIdentity> {
    let canonical_cwd = fs::canonicalize(cwd).ok()?;
    if !canonical_cwd.is_dir() {
        return None;
    }
    let checkout_root = fs::canonicalize(checkout_root(&canonical_cwd)?).ok()?;
    let relative_cwd = canonical_cwd
        .strip_prefix(&checkout_root)
        .ok()?
        .to_path_buf();
    let git_entry = checkout_root.join(".git");
    let entry_type = fs::symlink_metadata(&git_entry).ok()?.file_type();
    if entry_type.is_symlink() {
        return None;
    }
    let common_dir = if entry_type.is_dir() {
        fs::canonicalize(&git_entry).ok()?
    } else if entry_type.is_file() {
        let git_dir = read_git_path(&git_entry, &checkout_root, "gitdir:")?;
        if !git_dir.is_dir() {
            return None;
        }
        let common_dir = read_git_path(&git_dir.join("commondir"), &git_dir, "")?;
        if !common_dir.is_dir() {
            return None;
        }
        let registered_root = fs::canonicalize(common_dir.join("worktrees")).ok()?;
        if git_dir.parent()? != registered_root {
            return None;
        }
        let backlink = read_git_path(&git_dir.join("gitdir"), &git_dir, "")?;
        if backlink != fs::canonicalize(&git_entry).ok()? {
            return None;
        }
        common_dir
    } else {
        return None;
    };
    let primary_root = common_dir.parent()?.to_path_buf();
    let primary_git_entry = primary_root.join(".git");
    let primary_entry_type = fs::symlink_metadata(&primary_git_entry).ok()?.file_type();
    if !primary_entry_type.is_dir()
        || primary_entry_type.is_symlink()
        || fs::canonicalize(primary_git_entry).ok()? != common_dir
    {
        return None;
    }
    Some(RepositoryIdentity {
        common_dir,
        relative_cwd,
        primary_root,
    })
}

/// One row of the repository browser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowseEntry {
    Parent(PathBuf),
    /// A folder that is not a repository but holds one a level or two below.
    Folder(PathBuf),
    /// A primary checkout; a linked worktree's `.git` file does not count.
    Repository(PathBuf),
}

/// Lists `dir`'s parent, then its subfolders that are, or lead to, primary repositories, by name.
pub fn browse_entries(dir: &Path) -> Vec<BrowseEntry> {
    let mut entries: Vec<_> = dir
        .parent()
        .map(|parent| BrowseEntry::Parent(parent.to_path_buf()))
        .into_iter()
        .collect();
    let mut children: Vec<_> = browsable_folders(dir)
        .filter_map(|path| {
            if is_primary_repository(&path) {
                Some(BrowseEntry::Repository(path))
            } else if leads_to_repository(&path, 2) {
                Some(BrowseEntry::Folder(path))
            } else {
                None
            }
        })
        .collect();
    children.sort_by_key(|entry| match entry {
        BrowseEntry::Parent(path) | BrowseEntry::Folder(path) | BrowseEntry::Repository(path) => {
            path.file_name().map(|name| name.to_os_string())
        }
    });
    entries.extend(children);
    entries
}

fn is_primary_repository(path: &Path) -> bool {
    path.join(".git").is_dir()
}

/// Real, visible subfolders: symlinks and dot-folders are skipped.
fn browsable_folders(dir: &Path) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
}

fn leads_to_repository(dir: &Path, depth: usize) -> bool {
    depth > 0
        && browsable_folders(dir)
            .any(|path| is_primary_repository(&path) || leads_to_repository(&path, depth - 1))
}

/// The nearest ancestor holding a `.git` entry: a file for linked worktrees, a directory with `HEAD` otherwise.
fn checkout_root(base_dir: &Path) -> Option<PathBuf> {
    let mut dir = base_dir.to_path_buf();
    loop {
        let dot_git = dir.join(".git");
        if dot_git.exists() && (!dot_git.is_dir() || dot_git.join("HEAD").exists()) {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn read_git_path(path: &Path, relative_to: &Path, prefix: &str) -> Option<PathBuf> {
    if !fs::symlink_metadata(path).ok()?.file_type().is_file() {
        return None;
    }
    let contents = fs::read_to_string(path).ok()?;
    let value = contents.trim().strip_prefix(prefix)?.trim();
    if value.is_empty() || value.contains('\n') || value.contains('\r') {
        return None;
    }
    fs::canonicalize(relative_to.join(value)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn browse_lists_primary_repositories_and_folders_that_lead_to_them() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        for path in [
            "b-repo/.git",
            "a-folder/inner/deep/.git",
            "c-empty/inner",
            ".hidden/repo/.git",
            "d-too-deep/one/two/three/.git",
        ] {
            fs::create_dir_all(dir.join(path)).unwrap();
        }
        fs::create_dir(dir.join("e-worktree")).unwrap();
        fs::write(dir.join("e-worktree/.git"), "gitdir: elsewhere").unwrap();
        fs::write(dir.join("f-file"), "").unwrap();
        assert_eq!(
            browse_entries(dir),
            [
                BrowseEntry::Parent(dir.parent().unwrap().to_path_buf()),
                BrowseEntry::Folder(dir.join("a-folder")),
                BrowseEntry::Repository(dir.join("b-repo")),
            ]
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn worktree_and_primary_share_an_identity() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("repo");
        fs::create_dir_all(primary.join("src")).unwrap();
        git(&primary, &["init", "-q", "-b", "main"]);
        git(
            &primary,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        let linked = temp.path().join("linked");
        git(
            &primary,
            &["worktree", "add", "-q", linked.to_str().unwrap()],
        );
        fs::create_dir_all(linked.join("src")).unwrap();

        let from_primary = repository_identity(&primary.join("src")).unwrap();
        let from_linked = repository_identity(&linked.join("src")).unwrap();
        assert_eq!(from_primary.common_dir, from_linked.common_dir);
        assert_eq!(
            from_linked.primary_root,
            fs::canonicalize(&primary).unwrap()
        );
        assert_eq!(from_linked.relative_cwd, PathBuf::from("src"));
        assert_eq!(repository_identity(temp.path()), None);
    }
}
