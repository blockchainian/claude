//! Repository preparation for new tasks.
use std::{
    fs, io,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewTask {
    pub cwd: PathBuf,
    /// The session's display name.
    pub name: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt: String,
    pub images: Vec<PathBuf>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskDefaults {
    pub model: Option<String>,
    pub effort: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Repository {
    pub root: PathBuf,
    pub directory: PathBuf,
    pub branch: String,
    pub trunk: String,
}
fn git(directory: &Path, args: &[&std::ffi::OsStr]) -> io::Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(io::Error::other)
}
fn trunk_ref(root: &Path) -> io::Result<String> {
    use std::ffi::OsStr;
    if let Ok(reference) = git(
        root,
        &[
            OsStr::new("symbolic-ref"),
            OsStr::new("refs/remotes/origin/HEAD"),
        ],
    ) {
        git(
            root,
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new(&reference),
            ],
        )?;
        return Ok(reference);
    }
    for branch in ["main", "master"] {
        let reference = format!("refs/heads/{branch}");
        if git(
            root,
            &[
                OsStr::new("show-ref"),
                OsStr::new("--verify"),
                OsStr::new(&reference),
            ],
        )
        .is_ok()
        {
            return Ok(reference);
        }
    }
    Err(io::Error::other(
        "Repository has no origin/HEAD, main or master trunk",
    ))
}
pub fn repository_if_present(directory: &Path) -> io::Result<Option<Repository>> {
    let directory = fs::canonicalize(directory)?;
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .env("LC_ALL", "C")
        .current_dir(&directory)
        .output()?;
    if output.status.success() {
        return repository(&directory).map(Some);
    }
    let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if error == "fatal: not a git repository (or any of the parent directories): .git"
        || error.starts_with("fatal: not a git repository (or any parent up to mount point")
    {
        for ancestor in directory.ancestors() {
            match fs::symlink_metadata(ancestor.join(".git")) {
                Ok(_) => return Err(io::Error::other(error)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        return Ok(None);
    }
    Err(io::Error::other(error))
}

pub fn repository(directory: &Path) -> io::Result<Repository> {
    use std::ffi::OsStr;
    let directory = fs::canonicalize(directory)?;
    let root = fs::canonicalize(git(
        &directory,
        &[OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
    )?)?;
    let branch = git(
        &root,
        &[
            OsStr::new("symbolic-ref"),
            OsStr::new("--short"),
            OsStr::new("HEAD"),
        ],
    )?;
    let reference = trunk_ref(&root)?;
    let trunk = reference
        .strip_prefix("refs/remotes/origin/")
        .or_else(|| reference.strip_prefix("refs/heads/"))
        .ok_or_else(|| io::Error::other("Unexpected trunk reference"))?
        .to_owned();
    Ok(Repository {
        root,
        directory,
        branch,
        trunk,
    })
}
/// The branch a new task runs on: one the repository has, or a new one from its checked-out commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BranchChoice {
    Existing(String),
    New(String),
}

impl BranchChoice {
    pub fn name(&self) -> &str {
        match self {
            Self::Existing(name) | Self::New(name) => name,
        }
    }
}

/// The repository's local branches, by name.
pub fn local_branches(directory: &Path) -> io::Result<Vec<String>> {
    use std::ffi::OsStr;
    let output = git(
        directory,
        &[
            OsStr::new("for-each-ref"),
            OsStr::new("--format=%(refname:short)"),
            OsStr::new("refs/heads"),
        ],
    )?;
    let mut branches: Vec<_> = output.lines().map(str::to_owned).collect();
    branches.sort();
    Ok(branches)
}

/// The checkout, primary or linked, that has `branch` checked out.
fn branch_checkout(root: &Path, branch: &str) -> io::Result<Option<PathBuf>> {
    use std::ffi::OsStr;
    let list = git(
        root,
        &[
            OsStr::new("worktree"),
            OsStr::new("list"),
            OsStr::new("--porcelain"),
        ],
    )?;
    let reference = format!("branch refs/heads/{branch}");
    Ok(list.split("\n\n").find_map(|entry| {
        let mut lines = entry.lines();
        let path = lines.next()?.strip_prefix("worktree ")?;
        lines
            .any(|line| line == reference)
            .then(|| PathBuf::from(path))
    }))
}

/// The directory a new task on `choice` runs in: the checkout that already has the branch, or a
/// new worktree under `root/<repository>/<branch>`, with `/` in the branch written as `+`.
pub fn create_worktree(
    directory: &Path,
    root: &Path,
    choice: &BranchChoice,
    config_path: &Path,
) -> io::Result<PathBuf> {
    use std::ffi::OsStr;
    let repo = repository(directory)?;
    let relative = repo
        .directory
        .strip_prefix(&repo.root)
        .map_err(io::Error::other)?;
    let name = choice.name();
    if let BranchChoice::Existing(branch) = choice
        && let Some(checkout) = branch_checkout(&repo.root, branch)?
    {
        return Ok(checkout.join(relative));
    }
    git(
        &repo.root,
        &[
            OsStr::new("check-ref-format"),
            OsStr::new("--branch"),
            OsStr::new(name),
        ],
    )?;
    if root.as_os_str().is_empty() {
        return Err(io::Error::other("Worktree root is unset"));
    }
    // Worktrees of one repository share a folder named after its primary checkout.
    let primary = crate::repository::repository_identity(&repo.root)
        .map_or_else(|| repo.root.clone(), |identity| identity.primary_root);
    let repository = primary
        .file_name()
        .ok_or_else(|| io::Error::other("Repository has no name"))?;
    let root = root.join(repository);
    fs::create_dir_all(&root)?;
    let target = root.join(name.replace('/', "+"));
    match fs::symlink_metadata(&target) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Worktree name already exists",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let probe = root.join(format!(
        ".agents-write-{}-{}",
        std::process::id(),
        NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    drop(file);
    fs::remove_file(probe)?;
    let add = match choice {
        BranchChoice::Existing(_) => vec![
            OsStr::new("worktree"),
            OsStr::new("add"),
            target.as_os_str(),
            OsStr::new(name),
        ],
        BranchChoice::New(_) => vec![
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("-b"),
            OsStr::new(name),
            target.as_os_str(),
            OsStr::new("HEAD"),
        ],
    };
    git(&repo.root, &add)?;
    let prepared = (|| {
        copy_environment(&repo.root, &target)?;
        trust_directory(config_path, &target)?;
        Ok(target.join(relative))
    })();
    prepared.map_err(|error: io::Error| {
        io::Error::new(
            error.kind(),
            format!("{error}; worktree remains at {}", target.display()),
        )
    })
}
pub fn copy_environment(source: &Path, destination: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            // Only matching files create destination directories.
            copy_environment(&entry.path(), &target)?;
        } else if kind.is_file() {
            use std::os::unix::ffi::OsStrExt;
            let name = entry.file_name();
            if !name.as_bytes().starts_with(b".env") && !name.as_bytes().starts_with(b".dev.vars") {
                continue;
            }
            match fs::symlink_metadata(&target) {
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            fs::create_dir_all(target.parent().unwrap())?;
            let mut source = fs::File::open(entry.path())?;
            let metadata = source.metadata()?;
            let mut output = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            io::copy(&mut source, &mut output)?;
            output.set_permissions(metadata.permissions())?;
            output.set_times(
                fs::FileTimes::new()
                    .set_accessed(metadata.accessed()?)
                    .set_modified(metadata.modified()?),
            )?;
        }
    }
    Ok(())
}
static NEXT_FILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn skip_space(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}
fn string_end(bytes: &[u8], mut index: usize) -> usize {
    index += 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}
fn value_end(bytes: &[u8], start: usize) -> usize {
    if bytes[start] == b'"' {
        return string_end(bytes, start);
    }
    if matches!(bytes[start], b'{' | b'[') {
        let mut depth = 1;
        let mut index = start + 1;
        while index < bytes.len() {
            match bytes[index] {
                b'"' => index = string_end(bytes, index),
                b'{' | b'[' => {
                    depth += 1;
                    index += 1;
                }
                b'}' | b']' => {
                    depth -= 1;
                    index += 1;
                    if depth == 0 {
                        return index;
                    }
                }
                _ => index += 1,
            }
        }
        return index;
    }
    let mut index = start;
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && !matches!(bytes[index], b',' | b'}' | b']')
    {
        index += 1;
    }
    index
}
fn members(
    bytes: &[u8],
    span: std::ops::Range<usize>,
) -> io::Result<Vec<(String, std::ops::Range<usize>)>> {
    if bytes[span.start] != b'{' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude trust node must be an object",
        ));
    }
    let mut result = Vec::new();
    let mut index = skip_space(bytes, span.start + 1);
    while bytes[index] != b'}' {
        let end = string_end(bytes, index);
        let key: String = serde_json::from_slice(&bytes[index..end])?;
        if result.iter().any(|(existing, _)| existing == &key) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Duplicate Claude configuration key",
            ));
        }
        index = skip_space(bytes, end) + 1;
        index = skip_space(bytes, index);
        let end = value_end(bytes, index);
        result.push((key, index..end));
        index = skip_space(bytes, end);
        if bytes[index] == b',' {
            index = skip_space(bytes, index + 1);
        }
    }
    Ok(result)
}
/// Adds trust while preserving every byte outside the target JSON insertion or value.
pub fn trust_directory(config_path: &Path, directory: &Path) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let directory = fs::canonicalize(directory)?;
    let key = directory.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Trust directory is not UTF-8")
    })?;
    let (bytes, permissions) = match fs::read(config_path) {
        Ok(bytes) => (bytes, Some(fs::metadata(config_path)?.permissions())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (b"{}".to_vec(), None),
        Err(error) => return Err(error),
    };
    let _: serde_json::Value = serde_json::from_slice(&bytes)?;
    let start = skip_space(&bytes, 0);
    let mut span = start..value_end(&bytes, start);
    let keys = ["projects", key, "hasTrustDialogAccepted"];
    let (replace, text) = {
        let mut found = None;
        for (index, key) in keys.iter().enumerate() {
            let fields = members(&bytes, span.clone())?;
            if let Some((_, value)) = fields.iter().find(|(field, _)| field == key) {
                if index == keys.len() - 1 {
                    let accepted: bool = serde_json::from_slice(&bytes[value.clone()])?;
                    if accepted {
                        return Ok(());
                    }
                    found = Some((value.clone(), b"true".to_vec()));
                    break;
                }
                span = value.clone();
            } else {
                let mut value = serde_json::Value::Bool(true);
                for nested in keys[index + 1..].iter().rev() {
                    value = serde_json::json!({*nested: value});
                }
                let comma = if fields.is_empty() { "" } else { "," };
                let text = format!(
                    "{comma}{}:{}",
                    serde_json::to_string(key)?,
                    serde_json::to_string(&value)?
                )
                .into_bytes();
                found = Some((span.end - 1..span.end - 1, text));
                break;
            }
        }
        found.ok_or_else(|| io::Error::other("Trust target was not located"))?
    };
    let mut result = Vec::with_capacity(bytes.len() + text.len());
    result.extend_from_slice(&bytes[..replace.start]);
    result.extend_from_slice(&text);
    result.extend_from_slice(&bytes[replace.end..]);
    let parent = config_path.parent().unwrap_or(Path::new("."));
    let temporary = parent.join(format!(
        ".agents-trust-{}-{}",
        std::process::id(),
        NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(&result)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, config_path)
    })();
    if let Err(error) = result {
        fs::remove_file(&temporary)?;
        return Err(error);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        process::Command,
    };
    fn git(path: &Path, args: &[&str]) -> String {
        let o = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    }
    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-b", "main"]);
        fs::create_dir(d.path().join("sub")).unwrap();
        fs::write(d.path().join("sub/file"), "tracked").unwrap();
        git(d.path(), &["add", "sub/file"]);
        git(
            d.path(),
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
        );
        d
    }
    #[test]
    fn optional_repository_distinguishes_plain_directories_from_git_errors() {
        let plain = tempfile::tempdir().unwrap();
        assert_eq!(repository_if_present(plain.path()).unwrap(), None);

        let repository = repo();
        let found = repository_if_present(&repository.path().join("sub"))
            .unwrap()
            .unwrap();
        assert_eq!(found.branch, "main");
        assert_eq!(found.trunk, "main");
        git(repository.path(), &["branch", "-m", "feature"]);
        assert!(repository_if_present(repository.path()).is_err());

        let corrupt = tempfile::tempdir().unwrap();
        fs::create_dir(corrupt.path().join(".git")).unwrap();
        assert!(repository_if_present(corrupt.path()).is_err());
        fs::remove_dir(corrupt.path().join(".git")).unwrap();
        fs::write(corrupt.path().join(".git"), "gitdir: /does-not-exist\n").unwrap();
        assert!(repository_if_present(corrupt.path()).is_err());

        let missing = plain.path().join("missing");
        assert!(repository_if_present(&missing).is_err());
        let file = plain.path().join("file");
        fs::write(&file, "not a directory").unwrap();
        assert!(repository_if_present(&file).is_err());
    }
    #[test]
    fn detects_trunk_and_rebases_heading_into_real_worktree() {
        let d = repo();
        let trees = tempfile::tempdir().unwrap();
        let config = trees.path().join("claude.json");
        let r = repository(&d.path().join("sub")).unwrap();
        assert_eq!(r.branch, "main");
        assert_eq!(r.trunk, "main");
        let repository = d.path().file_name().unwrap();
        let cwd = create_worktree(&r.directory, trees.path(), &new("task"), &config).unwrap();
        assert_eq!(cwd, trees.path().join(repository).join("task/sub"));
        assert_eq!(git(&cwd, &["branch", "--show-current"]), "task");
        assert_eq!(fs::read_to_string(cwd.join("file")).unwrap(), "tracked");
        let v: serde_json::Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
        assert_eq!(
            v["projects"][fs::canonicalize(trees.path().join(repository).join("task"))
                .unwrap()
                .to_str()
                .unwrap()]["hasTrustDialogAccepted"],
            true
        );
    }
    fn new(name: &str) -> BranchChoice {
        BranchChoice::New(name.into())
    }
    #[test]
    fn existing_branches_reuse_their_worktree_or_get_one_and_slashes_become_plus() {
        let d = repo();
        let repository = d.path().file_name().unwrap();
        git(d.path(), &["branch", "zeta"]);
        git(d.path(), &["branch", "feature/old"]);
        assert_eq!(
            local_branches(d.path()).unwrap(),
            ["feature/old", "main", "zeta"]
        );
        let trees = tempfile::tempdir().unwrap();
        let root = trees.path();
        let config = trees.path().join("claude.json");
        let existing = |name: &str| BranchChoice::Existing(name.into());
        let checkout = create_worktree(&d.path().join("sub"), root, &existing("main"), &config);
        assert_eq!(
            checkout.unwrap(),
            fs::canonicalize(d.path()).unwrap().join("sub")
        );
        let old = create_worktree(d.path(), root, &existing("feature/old"), &config).unwrap();
        assert_eq!(old, root.join(repository).join("feature+old"));
        assert_eq!(git(&old, &["branch", "--show-current"]), "feature/old");
        let again = create_worktree(d.path(), root, &existing("feature/old"), &config).unwrap();
        assert_eq!(
            fs::canonicalize(again).unwrap(),
            fs::canonicalize(&old).unwrap()
        );
        let created = create_worktree(d.path(), root, &new("feature/x"), &config).unwrap();
        assert_eq!(created, root.join(repository).join("feature+x"));
        assert_eq!(git(&created, &["branch", "--show-current"]), "feature/x");
        fs::create_dir_all(root.join(repository).join("taken+x")).unwrap();
        assert!(create_worktree(d.path(), root, &new("taken/x"), &config).is_err());
    }
    #[test]
    fn worktree_starts_from_the_checked_out_branch_and_names_its_repository() {
        let d = repo();
        git(d.path(), &["checkout", "-b", "feature"]);
        fs::write(d.path().join("sub/feature"), "feature").unwrap();
        git(d.path(), &["add", "sub/feature"]);
        git(
            d.path(),
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-m",
                "feature",
            ],
        );
        let trees = tempfile::tempdir().unwrap();
        let root = trees.path().join("missing/root");
        let config = trees.path().join("claude.json");
        let first = create_worktree(d.path(), &root, &new("task"), &config).unwrap();
        let repository = d.path().file_name().unwrap();
        assert_eq!(first, root.join(repository).join("task"));
        assert_eq!(
            fs::read_to_string(first.join("sub/feature")).unwrap(),
            "feature"
        );
        let second = create_worktree(&first, &root, &new("nested"), &config).unwrap();
        assert_eq!(second, root.join(repository).join("nested"));
    }
    #[test]
    fn origin_head_wins_and_master_is_supported() {
        let d = repo();
        git(d.path(), &["branch", "trunk"]);
        git(
            d.path(),
            &["update-ref", "refs/remotes/origin/trunk", "HEAD"],
        );
        git(
            d.path(),
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/trunk",
            ],
        );
        assert_eq!(repository(d.path()).unwrap().trunk, "trunk");
        git(
            d.path(),
            &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
        );
        git(d.path(), &["branch", "-m", "master"]);
        assert_eq!(repository(d.path()).unwrap().trunk, "master");
    }
    #[test]
    fn rejects_invalid_name_and_collision_without_new_branch() {
        let d = repo();
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join(d.path().file_name().unwrap()).join("taken")).unwrap();
        for name in ["bad name", "taken"] {
            assert!(
                create_worktree(d.path(), t.path(), &new(name), &t.path().join("config")).is_err()
            );
            assert!(
                !Command::new("git")
                    .args(["show-ref", "--verify", &format!("refs/heads/{name}")])
                    .current_dir(d.path())
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
    }
    #[test]
    fn copies_hidden_ignored_env_files_preserving_mode_and_time_and_skipping_existing() {
        let d = repo();
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join(".hidden/nested")).unwrap();
        fs::write(d.path().join(".hidden/nested/.env.local"), "secret").unwrap();
        let p = d.path().join(".dev.vars.local");
        fs::write(&p, "vars").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        let stamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1700000000);
        fs::File::open(&p)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(stamp))
            .unwrap();
        fs::write(d.path().join(".env"), "source").unwrap();
        fs::write(t.path().join(".env"), "keep").unwrap();
        fs::write(d.path().join(".git/.env"), "excluded").unwrap();
        symlink(&p, d.path().join(".env.link")).unwrap();
        copy_environment(d.path(), t.path()).unwrap();
        assert_eq!(
            fs::read_to_string(t.path().join(".hidden/nested/.env.local")).unwrap(),
            "secret"
        );
        assert_eq!(fs::read_to_string(t.path().join(".env")).unwrap(), "keep");
        assert!(!t.path().join(".env.link").exists());
        assert!(!t.path().join(".git/.env").exists());
        let m = fs::metadata(t.path().join(".dev.vars.local")).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o640);
        assert_eq!(m.modified().unwrap(), stamp);
    }
    #[test]
    fn trust_inserts_only_target_and_is_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config");
        let before = include_bytes!("../tests/fixtures/task-trust.json");
        fs::write(&p, before).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        trust_directory(&p, d.path()).unwrap();
        let after = fs::read(&p).unwrap();
        let root_end = before.iter().rposition(|b| *b == b'}').unwrap();
        let offset = before[..root_end].iter().rposition(|b| *b == b'}').unwrap();
        assert!(after.starts_with(&before[..offset]));
        assert!(after.ends_with(&before[offset..]));
        trust_directory(&p, d.path()).unwrap();
        assert_eq!(fs::read(&p).unwrap(), after);
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
    #[test]
    fn trust_changes_false_only_and_rejects_invalid_required_nodes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config");
        let key =
            serde_json::to_string(fs::canonicalize(d.path()).unwrap().to_str().unwrap()).unwrap();
        let before = format!(
            "{{ \"projects\": {{ {key}: {{ \"hasTrustDialogAccepted\" : false, \"nested\": [{{\"✓\":true}}] }} }}, \"other\":1 }}\n"
        );
        fs::write(&p, &before).unwrap();
        trust_directory(&p, d.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&p).unwrap(),
            before.replacen("false", "true", 1)
        );
        for invalid in ["{broken", "{\"projects\":null}", "[]"] {
            fs::write(&p, invalid).unwrap();
            assert!(trust_directory(&p, d.path()).is_err());
            assert_eq!(fs::read_to_string(&p).unwrap(), invalid);
        }
    }
    #[test]
    fn creation_failure_reports_the_surviving_worktree() {
        let source = repo();
        let trees = tempfile::tempdir().unwrap();
        let config = trees.path().join("config");
        fs::write(&config, "{broken").unwrap();
        let error =
            create_worktree(source.path(), trees.path(), &new("task"), &config).unwrap_err();
        assert!(error.to_string().contains("worktree remains at"));
        let target = trees
            .path()
            .join(source.path().file_name().unwrap())
            .join("task");
        assert!(target.join(".git").is_file());
        assert_eq!(git(&target, &["branch", "--show-current"]), "task");
    }
    #[test]
    fn trust_decodes_escaped_path_and_inserts_only_missing_flag() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("quoted\"✓");
        fs::create_dir(&project).unwrap();
        let key =
            serde_json::to_string(fs::canonicalize(&project).unwrap().to_str().unwrap()).unwrap();
        let before =
            format!("{{\"projects\":{{{key}:{{ \"nested\":[{{\"escaped\":\"\\\\\\\"\"}}] }}}}}}\n");
        let path = directory.path().join("config");
        fs::write(&path, &before).unwrap();
        trust_directory(&path, &project).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(
            parsed["projects"][fs::canonicalize(project).unwrap().to_str().unwrap()]["hasTrustDialogAccepted"],
            true
        );
        assert_eq!(
            after.replace(",\"hasTrustDialogAccepted\":true", ""),
            before
        );
    }
}
