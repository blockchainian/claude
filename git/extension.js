const vscode = require('vscode');

/** Routes `vscode://blockchainian.git/view-commit?repo=<abs path>&sha=<sha>` to VS Code's commit view. */
function activate(context) {
  context.subscriptions.push(
    vscode.window.registerUriHandler({
      async handleUri(uri) {
        if (uri.path !== '/view-commit') {
          vscode.window.showErrorMessage(`Unknown git link: ${uri.path}`);
          return;
        }
        const query = new URLSearchParams(uri.query);
        const repo = query.get('repo');
        const sha = query.get('sha');
        if (!repo || !sha) {
          vscode.window.showErrorMessage('A view-commit link needs repo and sha.');
          return;
        }
        // The git extension only resolves repositories it has opened.
        await vscode.commands.executeCommand('git.openRepository', repo);
        await vscode.commands.executeCommand('git.viewCommit', vscode.Uri.file(repo), sha);
      },
    }),
  );
}

module.exports = { activate };
