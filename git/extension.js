const { execFile } = require('child_process');
const fs = require('fs/promises');
const vscode = require('vscode');

const PENDING_FOR_MS = 60_000;

/**
 * Routes `vscode://blockchainian.git/<view>?<query>` links:
 * - `view-commit?repo=<abs path>&sha=<sha>` opens VS Code's commit view.
 * - `view-folder?path=<abs path>` opens the folder in its own window.
 * - `view-pr?repo=<abs path>&url=<GitHub pull request URL>` opens the pull request in the window of the repository holding `repo`.
 */
function activate(context) {
  context.subscriptions.push(
    vscode.window.registerUriHandler({
      async handleUri(uri) {
        const query = new URLSearchParams(uri.query);
        const view = VIEWS[uri.path];
        if (!view) {
          vscode.window.showErrorMessage(`Unknown git link: ${uri.path}`);
          return;
        }
        const missing = view.params.filter((name) => !query.get(name));
        if (missing.length > 0) {
          vscode.window.showErrorMessage(`A ${uri.path.slice(1)} link needs ${missing.join(' and ')}.`);
          return;
        }
        await view.open(context, ...view.params.map((name) => query.get(name)));
      },
    }),
    vscode.window.onDidChangeWindowState((state) => {
      if (state.focused) {
        openPendingPullRequest(context);
      }
    }),
  );
  openPendingPullRequest(context);
}

const VIEWS = {
  '/view-commit': { params: ['repo', 'sha'], open: viewCommit },
  '/view-folder': { params: ['path'], open: (_, path) => viewFolder(path) },
  '/view-pr': { params: ['repo', 'url'], open: viewPullRequest },
};

async function viewCommit(_, repo, sha) {
  // The git extension only resolves repositories it has opened.
  await vscode.commands.executeCommand('git.openRepository', repo);
  await vscode.commands.executeCommand('git.viewCommit', vscode.Uri.file(repo), sha);
}

/**
 * Focuses the window that has `path` open, or opens it, never replacing a window that has a folder.
 * A link that starts VS Code lands in an empty window, so the folder opens in that one.
 */
async function viewFolder(path) {
  await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(path), {
    forceNewWindow: (vscode.workspace.workspaceFolders ?? []).length > 0,
  });
}

/**
 * Opens the pull request in the window of `repo`. A link lands in the last active window,
 * so another window records the request and hands focus to the repository's window,
 * which opens it once it is focused. Outside a repository it opens in this window.
 */
async function viewPullRequest(context, path, url) {
  const repo = await repositoryRoot(path);
  if (!repo || isOpenHere(repo)) {
    await openPullRequest(url);
    return;
  }
  await fs.mkdir(context.globalStorageUri.fsPath, { recursive: true });
  await fs.writeFile(pendingFile(context), JSON.stringify({ repo, url, at: Date.now() }));
  await viewFolder(repo);
}

async function openPendingPullRequest(context) {
  const pending = await fs.readFile(pendingFile(context), 'utf8').then(JSON.parse, () => undefined);
  if (!pending || Date.now() - pending.at > PENDING_FOR_MS || !isOpenHere(pending.repo)) {
    return;
  }
  await fs.rm(pendingFile(context), { force: true });
  await openPullRequest(pending.url);
}

/** The pull request a window handed over, in storage every window of this extension shares. */
function pendingFile(context) {
  return vscode.Uri.joinPath(context.globalStorageUri, 'pending-pull-request.json').fsPath;
}

function repositoryRoot(path) {
  return new Promise((resolve) => {
    execFile('git', ['-C', path, 'rev-parse', '--show-toplevel'], (error, stdout) => {
      resolve(error ? undefined : stdout.trimEnd());
    });
  });
}

function isOpenHere(path) {
  return (vscode.workspace.workspaceFolders ?? []).some((folder) => folder.uri.fsPath === path);
}

/** Hands the pull request to the GitHub Pull Requests extension's description view. */
async function openPullRequest(url) {
  const link = new URL('vscode://github.vscode-pull-request-github/open-pull-request-webview');
  link.searchParams.set('uri', url);
  await vscode.env.openExternal(vscode.Uri.parse(link.toString(), true));
}

module.exports = { activate };
