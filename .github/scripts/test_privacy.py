"""Exercise privacy checks against actual commits in temporary repositories."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCANNER = Path(__file__).with_name("privacy.py")
ROOT = SCANNER.resolve().parents[2]
RANGE = SCANNER.with_name("commit_range.py")
# Execute the workflow's actual shell command against each real repository.
step = (ROOT / ".github/workflows/test.yml").read_text().split(
    "      - name: Scan commits for secrets and home paths\n", 1
)[1].split("      - name:", 1)[0]
CI_COMMAND = "\n".join(line[10:] for line in step.splitlines()[1:])
CI_COMMAND = CI_COMMAND.replace(".github/scripts/commit_range.py", str(RANGE.resolve()))


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

    def scan(self, mode, denylist="", event="push", base=None):
        return self.run_scan(["python3", str(SCANNER), mode], denylist, event, base)

    def scan_secrets(self, event="push", base=None):
        return self.run_scan(["bash", "-eo", "pipefail", "-c", CI_COMMAND], "", event, base)

    def run_scan(self, command, denylist="", event="push", base=None):
        result = subprocess.run(
            command, cwd=self.repo, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            env={**os.environ, "PRIVACY_EVENT": event,
                 "PRIVACY_BASE": self.base if base is None else base,
                 "PRIVACY_HEAD": self.git("rev-parse", "HEAD").strip(),
                 "PRIVACY_DENYLIST": denylist},
        )
        return result.returncode, result.stdout

    def test_gitleaks_mode_is_removed(self):
        code, output = self.scan("gitleaks")
        self.assertEqual(code, 2, output)

    def test_local_denylist(self):
        (self.repo / ".privacyignore").write_text("\nSample-Denied\n")
        self.commit("fixtures/data.txt", "sample-denied\n")
        code, output = self.scan("denylist")
        self.assertEqual((code, output), (1, "fixtures/data.txt:1\n"))

    def test_environment_overrides_local_denylist(self):
        (self.repo / ".privacyignore").write_text("sample-denied\n")
        self.commit("fixtures/data.txt", "sample-denied\n")
        code, output = self.scan("denylist", "another-term")
        self.assertEqual((code, output), (0, ""))

    def test_staged_denylist_only_scans_added_staged_lines(self):
        (self.repo / ".privacyignore").write_text("Sample-Denied\n")
        self.commit("fixtures/data.txt", "sample-denied\nsafe\n")
        (self.repo / "fixtures/data.txt").write_text("sample-denied\nsafe edit\n")
        self.git("add", "--", "fixtures/data.txt")
        (self.repo / "fixtures/data.txt").write_text("sample-denied\nsample-denied\n")
        code, output = self.scan("staged")
        self.assertEqual((code, output), (0, ""))
        self.git("add", "--", "fixtures/data.txt")
        code, output = self.scan("staged")
        self.assertEqual((code, output), (1, "fixtures/data.txt:2\n"))

    def test_staged_missing_denylist_skips_without_revisions(self):
        result = subprocess.run(
            ["python3", str(SCANNER), "staged"], cwd=self.repo, text=True,
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
        code, output = self.scan("staged", "sample-denied")
        self.assertEqual((code, output), (0, ""))
        self.git("commit", "-m", "delete fixture")
        code, output = self.scan("denylist", "sample-denied")
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
        code, output = self.scan("denylist", "\n" + value + "\r\n")
        self.assertEqual(code, 1, output)
        self.assertEqual(output, "fixtures/data.txt:2\n")
        self.assertNotIn(value.casefold(), output.casefold())

    def test_old_lines_and_removed_lines_do_not_match(self):
        self.commit("fixtures/data.txt", "sample-denied\n")
        self.base = self.git("rev-parse", "HEAD").strip()
        self.commit("fixtures/data.txt", "replacement\n")
        code, output = self.scan("denylist", "sample-denied")
        self.assertEqual(code, 0, output)

    def test_denylist_does_not_treat_terms_as_patterns(self):
        self.commit("fixtures/data.txt", "sampleXterm\n")
        code, output = self.scan("denylist", "sample.term")
        self.assertEqual(code, 0, output)

    def test_unicode_lines_and_quoted_filenames(self):
        self.commit('fixtures/quote".txt', "before\u2028after\nsample-denied\n")
        code, output = self.scan("denylist", "sample-denied")
        self.assertEqual(code, 1, output)
        self.assertEqual(output, 'fixtures/quote\\".txt:2\n')

    def test_invalid_revision_fails_without_echoing_input(self):
        code, output = self.scan("denylist", "sample-denied", base="private-input")
        self.assertEqual(code, 2, output)
        self.assertEqual(output, "Privacy scan could not complete.\n")

    def test_intermediate_commit_is_scanned(self):
        self.commit("fixtures/data.txt", "sample-denied\n")
        self.commit("fixtures/data.txt", "replacement\n")
        code, output = self.scan("denylist", "sample-denied")
        self.assertEqual(code, 1, output)
        self.assertEqual(output, "fixtures/data.txt:1\n")

    def test_missing_denylist_skips(self):
        code, output = self.scan("denylist")
        self.assertEqual(code, 0, output)
        self.assertEqual(output, "PRIVACY_DENYLIST is empty; skipping denylist scan.\n")

    def test_initial_push(self):
        self.commit("fixtures/data.txt", "sample-denied\n")
        code, output = self.scan("denylist", "sample-denied", base="0" * 40)
        self.assertEqual(code, 1, output)

    def test_pull_request_uses_merge_base(self):
        self.git("checkout", "-b", "topic")
        self.commit("fixtures/topic.txt", "safe\n")
        self.git("checkout", "main")
        self.commit("fixtures/main.txt", "sample-denied\n")
        main = self.git("rev-parse", "HEAD").strip()
        self.git("checkout", "topic")
        code, output = self.scan("denylist", "sample-denied", event="pull_request", base=main)
        self.assertEqual(code, 0, output)


if __name__ == "__main__":
    unittest.main()
