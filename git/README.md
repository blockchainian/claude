# git

A VS Code extension that opens a commit, a repository folder or a GitHub pull request in VS Code from a link, so a terminal app can show them with `open`:

```sh
open "vscode://blockchainian.git/view-commit?repo=/abs/path/to/repo&sha=1e12670"
open "vscode://blockchainian.git/view-folder?path=/abs/path/to/repo"
open "vscode://blockchainian.git/view-pr?repo=/abs/path/to/repo&url=https://github.com/owner/repo/pull/12"
```

- `view-commit` opens the repository in VS Code's git view, then runs the built-in `git.viewCommit` on `sha`, any commit name git accepts.
- `view-folder` focuses the window that has `path` open, or opens it in a new window, never replacing the current one. A plain `vscode://file/<folder>` link replaces the last active window instead.
- `view-pr` opens the pull request's description in the window of the repository holding `repo`, opening or focusing that window first. It needs the GitHub Pull Requests extension.

## Install

Link the folder into VS Code's extensions directory, then restart VS Code:

```sh
ln -sfn "$PWD/git" ~/.vscode/extensions/blockchainian.git-0.2.0
```

VS Code records the extension by version in `~/.vscode/extensions/extensions.json`, so after a version bump, quit VS Code, rename the link, and point the `blockchainian.git` entry's `version`, `relativeLocation` and `location` at the new name; otherwise VS Code marks the new folder obsolete and skips it.
