"""Exercise privacy checks against actual commits in temporary repositories."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCANNER = Path(__file__).with_name("gitleaks-privacy.py")
ROOT = SCANNER.resolve().parents[2]
RANGE = SCANNER.with_name("commit_range.py")
# Execute the workflow's actual shell commands against each real repository.
def workflow_command(name):
    step = (ROOT / ".github/workflows/test.yml").read_text().split(
        f"      - name: {name}\n", 1
    )[1].split("      - name:", 1)[0]
    script = step.split("        run: |\n", 1)[1]
    lines = []
    for line in script.splitlines():
        if line and not line.startswith("          "):
            break
        lines.append(line[10:])
    command = "\n".join(lines)
    return command.replace(".github/scripts/commit_range.py", str(RANGE.resolve())).replace(
        ".github/scripts/gitleaks-privacy.py", str(SCANNER.resolve())
    )


CI_COMMAND = workflow_command("Scan commits for secrets and home paths")
PRIVACY_COMMAND = workflow_command("Scan added lines for private terms")


class PrivacyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.repo = Path(self.directory.name)
        shutil.copyfile(ROOT / ".gitleaks.toml", self.repo / ".gitleaks.toml")
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Test User")
        self.git("config", "user.email", "user@example.invalid")
        self.commit("base.txt", "initial\n")
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.check_output(
            ["git", *args], cwd=self.repo, text=True, stderr=subprocess.DEVNULL
        )

    def commit(self, name, content):
        (self.repo / name).parent.mkdir(parents=True, exist_ok=True)
        (self.repo / name).write_text(content)
        self.git("add", "--", name)
        self.git("commit", "-m", "test input")

    def scan(self, *args, denylist=""):
        return self.run_scan(["python3", str(SCANNER), *args], denylist)

    def scan_commits(self, denylist="", event="push", base=None):
        return self.run_scan(
            ["bash", "-eo", "pipefail", "-c", PRIVACY_COMMAND], denylist, event, base
        )

    def scan_secrets(self, event="push", base=None):
        return self.run_scan(["bash", "-eo", "pipefail", "-c", CI_COMMAND], "", event, base)

    def run_scan(self, command, denylist="", event="push", base=None, event_content=None):
        head = self.git("rev-parse", "HEAD").strip()
        base = self.base if base is None else base
        payload = (
            {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}
            if event == "pull_request" else {"before": base, "after": head}
        )
        with tempfile.TemporaryDirectory() as directory:
            event_path = Path(directory) / "event.json"
            event_path.write_text(
                json.dumps(payload) if event_content is None else event_content
            )
            result = subprocess.run(
                command, cwd=self.repo, text=True,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                env={**{key: value for key, value in os.environ.items()
                        if key not in ("GITHUB_EVENT_NAME", "GITHUB_EVENT_PATH")},
                     "GITHUB_EVENT_NAME": event, "GITHUB_EVENT_PATH": str(event_path),
                     "PRIVACY_DENYLIST": denylist},
            )
        return result.returncode, result.stdout

    def test_invalid_cli_is_rejected_without_echoing_input(self):
        sha = self.base
        log_opts = "--log-opts=--diff-merges=first-parent " + sha
        cases = [
            (), ("git",), ("staged",), ("denylist",), ("gitleaks",),
            ("git", "--pre-commit"), ("git", "--staged"),
            ("git", "--pre-commit", "--staged", log_opts),
            ("git", "--log-opts"), ("git", "--log-opts="),
            ("git", "--log-opts=" + sha),
            ("git", "--log-opts=--diff-merges=separate " + sha),
            ("git", "--log-opts=--diff-merges=first-parent HEAD"),
            ("git", "--log-opts=--diff-merges=first-parent " + sha[:-1]),
            ("git", "--log-opts=--diff-merges=first-parent " + "g" * 40),
            ("git", log_opts + "..private-input"),
            ("git", log_opts + " --all"),
            ("git", log_opts, log_opts),
            ("git", "--pre-commit", "--staged", "--staged"),
        ]
        for flag in ("--config=.gitleaks.toml", "--redact", "--no-banner"):
            cases.extend([
                ("git", "--pre-commit", "--staged", flag),
                ("git", log_opts, flag),
            ])
        for args in cases:
            with self.subTest(args=args):
                code, output = self.scan(*args)
                self.assertEqual((code, output), (2, "Privacy scan could not complete.\n"))

    def test_cli_range_ignores_privacy_revision_environment(self):
        self.commit("fixtures/data.txt", "sample-denied\n")
        head = self.git("rev-parse", "HEAD").strip()
        for revisions in (head, self.base + ".." + head):
            with self.subTest(revisions=revisions):
                code, output = self.run_scan(
                    ["python3", str(SCANNER), "git",
                     "--log-opts=--diff-merges=first-parent " + revisions],
                    denylist="sample-denied", event="unsupported", base="private-input",
                )
                self.assertEqual((code, output), (1, "fixtures/data.txt:1\n"))

    def test_git_failure_does_not_echo_tool_output(self):
        code, output = self.scan(
            "git", "--log-opts=--diff-merges=first-parent " + "f" * 40,
            denylist="sample-denied",
        )
        self.assertEqual((code, output), (2, "Privacy scan could not complete.\n"))

    def test_local_denylist(self):
        (self.repo / ".privacyignore").write_text("\nSample-Denied\n")
        self.commit("fixtures/data.txt", "sample-denied\n")
        code, output = self.scan_commits()
        self.assertEqual((code, output), (1, "fixtures/data.txt:1\n"))

    def test_environment_overrides_local_denylist(self):
        (self.repo / ".privacyignore").write_text("sample-denied\n")
        self.commit("fixtures/data.txt", "sample-denied\n")
        code, output = self.scan_commits("another-term")
        self.assertEqual((code, output), (0, ""))

    def test_staged_denylist_only_scans_added_staged_lines(self):
        (self.repo / ".privacyignore").write_text("Sample-Denied\n")
        self.commit("fixtures/data.txt", "sample-denied\nsafe\n")
        (self.repo / "fixtures/data.txt").write_text("sample-denied\nsafe edit\n")
        self.git("add", "--", "fixtures/data.txt")
        (self.repo / "fixtures/data.txt").write_text("sample-denied\nsample-denied\n")
        code, output = self.scan("git", "--pre-commit", "--staged")
        self.assertEqual((code, output), (0, ""))
        self.git("add", "--", "fixtures/data.txt")
        code, output = self.scan("git", "--pre-commit", "--staged")
        self.assertEqual((code, output), (1, "fixtures/data.txt:2\n"))

    def test_staged_missing_denylist_skips_without_revisions(self):
        result = subprocess.run(
            ["python3", str(SCANNER), "git", "--pre-commit", "--staged"], cwd=self.repo, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            env={key: value for key, value in os.environ.items()
                 if not key.startswith("PRIVACY_")},
        )
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertEqual(len(result.stdout.splitlines()), 1)
        self.assertIn("skipping", result.stdout)

    def test_deleted_file_does_not_match(self):
        self.commit("fixtures/data.txt", "sample-denied\n")
        self.base = self.git("rev-parse", "HEAD").strip()
        self.git("rm", "fixtures/data.txt")
        code, output = self.scan("git", "--pre-commit", "--staged", denylist="sample-denied")
        self.assertEqual((code, output), (0, ""))
        self.git("commit", "-m", "delete fixture")
        code, output = self.scan_commits("sample-denied")
        self.assertEqual((code, output), (0, ""))

    def test_ci_range_validation_and_initial_push(self):
        head = self.git("rev-parse", "HEAD").strip()
        for event, base, expected in (
            ("push", self.base, self.base + ".." + head),
            ("push", "0" * 40, head),
            ("pull_request", self.base, self.base + ".." + head),
        ):
            code, output = self.run_scan(["python3", str(RANGE)], event=event, base=base)
            self.assertEqual((code, output), (0, expected + "\n"))
        for event, base in (("push", "private-input"), ("pull_request", "0" * 40),
                            ("unsupported", self.base)):
            code, output = self.run_scan(["python3", str(RANGE)], event=event, base=base)
            self.assertEqual((code, output), (2, "Commit range could not be resolved.\n"))

    def test_ci_invalid_event_json_is_redacted(self):
        for event in ("push", "pull_request"):
            for content in (
                "", "private-input", "null", "[]", "{}", '{"before": null}',
                '{"pull_request": []}', '{"pull_request": {"base": null}}',
            ):
                with self.subTest(event=event, content=content):
                    code, output = self.run_scan(
                        ["python3", str(RANGE)], event=event, event_content=content
                    )
                    self.assertEqual(
                        (code, output), (2, "Commit range could not be resolved.\n")
                    )
            for field in ("base", "head"):
                for sha in (None, 123, [], {}, "a" * 39, "g" * 40, "f" * 40,
                            "private-input", "0" * 40):
                    if field == "base" and sha == "0" * 40 and event == "push":
                        continue
                    base = sha if field == "base" else self.base
                    head = sha if field == "head" else self.base
                    payload = (
                        {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}
                        if event == "pull_request" else {"before": base, "after": head}
                    )
                    with self.subTest(event=event, field=field, sha=sha):
                        code, output = self.run_scan(
                            ["python3", str(RANGE)], event=event,
                            event_content=json.dumps(payload),
                        )
                        self.assertEqual(
                            (code, output), (2, "Commit range could not be resolved.\n")
                        )

    def test_ci_missing_event_environment_or_file_is_redacted(self):
        env = {key: value for key, value in os.environ.items()
               if key not in ("GITHUB_EVENT_NAME", "GITHUB_EVENT_PATH")}
        for extra in ({}, {"GITHUB_EVENT_NAME": "push"},
                      {"GITHUB_EVENT_NAME": "push",
                       "GITHUB_EVENT_PATH": str(self.repo / "missing-event.json")}):
            with self.subTest(extra=extra):
                result = subprocess.run(
                    ["python3", str(RANGE)], cwd=self.repo, text=True,
                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env={**env, **extra},
                )
                self.assertEqual(
                    (result.returncode, result.stdout),
                    (2, "Commit range could not be resolved.\n"),
                )

    def test_ci_pull_request_uses_merge_base(self):
        self.git("checkout", "-b", "topic")
        self.commit("fixtures/topic.txt", "/" + "Users/someone/x/\n")
        topic = self.git("rev-parse", "HEAD").strip()
        self.git("checkout", "main")
        self.commit("fixtures/main.txt", "safe\n")
        main = self.git("rev-parse", "HEAD").strip()
        self.git("checkout", "topic")
        code, output = self.run_scan(["python3", str(RANGE)], event="pull_request", base=main)
        self.assertEqual((code, output), (0, self.base + ".." + topic + "\n"))
        code, output = self.scan_secrets(event="pull_request", base=main)
        self.assertEqual(code, 1, output)
        self.assertNotIn("someone", output)

    def test_ci_initial_push_detects_home_path(self):
        self.commit("fixtures/input.txt", "/" + "Users/someone/x/\n")
        code, output = self.scan_secrets(base="0" * 40)
        self.assertEqual(code, 1, output)
        self.assertNotIn("someone", output)

    def test_home_paths_and_json_fields(self):
        for prefix in ("/Users/", "/home/"):
            for name in ("realname", "user", "runner"):
                for field in (None, "cwd", "gitBranch"):
                    for escaped in (False, True) if field else (False,):
                        with self.subTest(prefix=prefix, name=name, field=field,
                                          escaped=escaped):
                            self.base = self.git("rev-parse", "HEAD").strip()
                            value = prefix + name + "/project/"
                            encoded = value.replace("/", "\\/") if escaped else value
                            content = f'{{"{field}":"{encoded}"}}\n' if field else value + "\n"
                            self.commit("fixtures/input.json", content)
                            code, output = self.scan_secrets()
                            self.assertEqual(code, 1, output)
                            self.assertNotIn(name, output)
                            self.assertNotIn(encoded, output)

    def test_secret_is_blocked_without_echoing_it(self):
        value = "AKIA" + "QWERTYUIOPASDFGH"
        self.commit("fixtures/key.txt", value + "\n")
        code, output = self.scan_secrets()
        self.assertEqual(code, 1, output)
        self.assertNotIn(value, output)

    def test_url_routes_are_not_home_directories(self):
        routes = ["/" + directory + "/:name/" for directory in ("users", "Home")]
        urls = ["https://example.com/" + directory + "/sample/project/"
                for directory in ("Users", "home", "users", "Home")]
        content = "\n".join(routes + urls + [
            json.dumps({field: value}).replace("/", "\\/")
            for field in ("cwd", "gitBranch") for value in urls
        ])
        self.commit("fixtures/routes.txt", content)
        code, output = self.scan_secrets()
        self.assertEqual(code, 0, output)

    def test_allowed_home_paths(self):
        values = [prefix + "example-user/project/" for prefix in ("/Users/", "/home/")]
        for escaped in (False, True):
            with self.subTest(escaped=escaped):
                content = json.dumps([
                    {"cwd": value, "gitBranch": value} for value in values
                ])
                self.base = self.git("rev-parse", "HEAD").strip()
                self.commit("fixtures/allowed.json",
                            content.replace("/", "\\/") if escaped else content)
                code, output = self.scan_secrets()
                self.assertEqual(code, 0, output)

    def test_allowed_plain_home_paths(self):
        self.commit("fixtures/allowed.txt", "\n".join(
            prefix + "example-user/project/" for prefix in ("/Users/", "/home/")
        ))
        code, output = self.scan_secrets()
        self.assertEqual(code, 0, output)

    def test_denylist_is_literal_case_insensitive_and_redacted(self):
        value = "Private" + "Sample.Word"
        self.commit("fixtures/data.txt", "safe\n" + value.swapcase() + "\n")
        code, output = self.scan_commits("\n" + value + "\r\n")
        self.assertEqual(code, 1, output)
        self.assertEqual(output, "fixtures/data.txt:2\n")
        self.assertNotIn(value.casefold(), output.casefold())

    def test_old_lines_and_removed_lines_do_not_match(self):
        self.commit("fixtures/data.txt", "sample-denied\n")
        self.base = self.git("rev-parse", "HEAD").strip()
        self.commit("fixtures/data.txt", "replacement\n")
        code, output = self.scan_commits("sample-denied")
        self.assertEqual(code, 0, output)

    def test_denylist_does_not_treat_terms_as_patterns(self):
        self.commit("fixtures/data.txt", "sampleXterm\n")
        code, output = self.scan_commits("sample.term")
        self.assertEqual(code, 0, output)

    def test_unicode_lines_and_quoted_filenames(self):
        self.commit('fixtures/quote".txt', "before\u2028after\nsample-denied\n")
        code, output = self.scan_commits("sample-denied")
        self.assertEqual(code, 1, output)
        self.assertEqual(output, 'fixtures/quote\\".txt:2\n')

    def test_invalid_revision_fails_without_echoing_input(self):
        code, output = self.scan_commits("sample-denied", base="private-input")
        self.assertEqual(code, 2, output)
        self.assertEqual(output, "Commit range could not be resolved.\n")

    def test_intermediate_commit_is_scanned(self):
        self.commit("fixtures/data.txt", "sample-denied\n")
        self.commit("fixtures/data.txt", "replacement\n")
        code, output = self.scan_commits("sample-denied")
        self.assertEqual(code, 1, output)
        self.assertEqual(output, "fixtures/data.txt:1\n")

    def test_missing_denylist_skips(self):
        code, output = self.scan_commits()
        self.assertEqual(code, 0, output)
        self.assertEqual(output, "PRIVACY_DENYLIST is empty; skipping denylist scan.\n")

    def test_initial_push(self):
        self.commit("fixtures/data.txt", "sample-denied\n")
        code, output = self.scan_commits("sample-denied", base="0" * 40)
        self.assertEqual(code, 1, output)

    def test_pull_request_uses_merge_base(self):
        self.git("checkout", "-b", "topic")
        self.commit("fixtures/topic.txt", "safe\n")
        self.git("checkout", "main")
        self.commit("fixtures/main.txt", "sample-denied\n")
        main = self.git("rev-parse", "HEAD").strip()
        self.git("checkout", "topic")
        code, output = self.scan_commits("sample-denied", event="pull_request", base=main)
        self.assertEqual(code, 0, output)


if __name__ == "__main__":
    unittest.main()
