// ABOUTME: Reads all of standard input as one UTF-8 string.
// ABOUTME: Shared by the daemon task runner and codex-manager's Stop hook.

export function readStdin() {
  return new Promise((resolve) => {
    let text = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => { text += chunk; });
    process.stdin.on("end", () => resolve(text));
  });
}
