"""Exercise privacy checks against actual commits in temporary repositories."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCANNER = Path(__file__).with_name("privacy.py")


class PrivacyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.repo = Path(self.directory.name)
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
        result = subprocess.run(
            ["python3", str(SCANNER), mode], cwd=self.repo, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            env={**os.environ, "PRIVACY_EVENT": event,
                 "PRIVACY_BASE": self.base if base is None else base,
                 "PRIVACY_HEAD": self.git("rev-parse", "HEAD").strip(),
                 "PRIVACY_DENYLIST": denylist},
        )
        return result.returncode, result.stdout

    def test_home_paths_and_json_fields(self):
        for prefix in ("/Users/", "/home/"):
            for name in ("realname", "user"):
                for field in (None, "cwd", "gitBranch"):
                    for escaped in (False, True) if field else (False,):
                        with self.subTest(prefix=prefix, name=name, field=field,
                                          escaped=escaped):
                            self.base = self.git("rev-parse", "HEAD").strip()
                            value = prefix + name + "/project/"
                            encoded = value.replace("/", "\\/") if escaped else value
                            content = f'{{"{field}":"{encoded}"}}\n' if field else value + "\n"
                            self.commit("fixtures/input.json", content)
                            code, output = self.scan("gitleaks")
                            self.assertEqual(code, 1, output)
                            self.assertIn("fixtures/input.json:1", output)
                            self.assertNotIn(name, output)
                            self.assertNotIn(encoded, output)

    def test_secret_is_blocked_without_echoing_it(self):
        value = "AKIA" + "QWERTYUIOPASDFGH"
        self.commit("fixtures/key.txt", value + "\n")
        code, output = self.scan("gitleaks")
        self.assertEqual(code, 1, output)
        self.assertIn("fixtures/key.txt:1", output)
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
        code, output = self.scan("gitleaks")
        self.assertEqual(code, 0, output)

    def test_allowed_home_paths(self):
        values = [prefix + name + "/project/" for prefix in ("/Users/", "/home/")
                  for name in ("example-user", "runner")]
        for escaped in (False, True):
            with self.subTest(escaped=escaped):
                content = json.dumps([
                    {"cwd": value, "gitBranch": value} for value in values
                ])
                self.base = self.git("rev-parse", "HEAD").strip()
                self.commit("fixtures/allowed.json",
                            content.replace("/", "\\/") if escaped else content)
                code, output = self.scan("gitleaks")
                self.assertEqual(code, 0, output)

    def test_allowed_plain_home_paths(self):
        self.commit("fixtures/allowed.txt", "\n".join(
            prefix + name + "/project/" for prefix in ("/Users/", "/home/")
            for name in ("example-user", "runner")
        ))
        code, output = self.scan("gitleaks")
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
        code, output = self.scan("gitleaks", base="private-input")
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
