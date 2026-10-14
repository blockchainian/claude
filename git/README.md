# git

A VS Code extension that opens a commit's changes in VS Code's multi-file diff view from a link, so a terminal app can show a commit with `open`:

```sh
open "vscode://blockchainian.git/view-commit?repo=/abs/path/to/repo&sha=1e12670"
```

`repo` is the repository's absolute path and `sha` any commit name git accepts. The extension opens the repository in VS Code's git view, then runs the built-in `git.viewCommit`.

## Install

Link the folder into VS Code's extensions directory, then restart VS Code:

```sh
ln -sfn "$PWD/git" ~/.vscode/extensions/blockchainian.git-0.1.0
```
