#!/usr/bin/env python3
"""Regression tests for the phone-session-gate hook.

One Appium session per phone, and only after a claim: a create without a claim
or while a session is open is denied with a reason that says what to do; a
successful create is recorded in the claim, and a delete clears it so the holder
can recreate after an idle-out. Run: ./test_phone_session_gate.py
"""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
HOOK = HERE.parent / "phone-session-gate"
CLAIM = HERE.parent.parent / "skills/ios-take-screenshot/scripts/claim_simulator.py"
TOOL = "mcp__plugin_build-ios-apps_appium-mcp__appium_session_management"
PHONE = "00008030-001A2B3C4D5E6F7A"
SESSION = "3f1c0a52-7d4e-4b0e-9d7a-6c1b2e3d4f50"


def hook(event: str, tool_input: dict, tmp: str, response=None) -> dict:
    payload = {"hook_event_name": event, "tool_name": TOOL, "tool_input": tool_input}
    if response is not None:
        payload["tool_response"] = response
    r = subprocess.run([str(HOOK)], input=json.dumps(payload), capture_output=True,
                       text=True, env={**os.environ, "TMPDIR": tmp}, check=False)
    if r.returncode != 0:
        raise AssertionError(f"hook exited {r.returncode}: {r.stderr}")
    return json.loads(r.stdout) if r.stdout.strip() else {}


def decision(out: dict) -> tuple[str, str]:
    h = out.get("hookSpecificOutput", {})
    return h.get("permissionDecision", ""), h.get("permissionDecisionReason", "")


def create(udid: str | None = None) -> dict:
    caps = {"platformName": "iOS", "appium:udid": udid} if udid else {"platformName": "iOS"}
    return {"action": "create", "platform": "ios", "capabilities": json.dumps(caps)}


def lock(tmp: str) -> dict:
    return json.loads((Path(tmp) / f"ios-screenshot-lock.{PHONE}.json").read_text())


def main() -> int:
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        d, why = decision(hook("PreToolUse", create(), tmp))
        if d != "deny" or "appium:udid" not in why:
            failures.append(f"create without a udid was not denied for it: {d} {why!r}")

        d, why = decision(hook("PreToolUse", create(PHONE), tmp))
        if d != "deny" or "claim_simulator.py" not in why:
            failures.append(f"create on an unclaimed phone was not sent to claim: {d} {why!r}")

        subprocess.run([str(CLAIM), PHONE, "--run", "run-a"], check=True,
                       capture_output=True, env={**os.environ, "TMPDIR": tmp})
        out = hook("PreToolUse", create(PHONE), tmp)
        if out:
            failures.append(f"create on a claimed, idle phone was not allowed: {out}")

        text = (f"IOS session created successfully with ID: {SESSION}\nPlatform: iOS\n"
                f"Automation: XCUITest\nDevice: iPhone\nActive sessions: 1")
        hook("PostToolUse", create(PHONE), tmp, {"content": [{"type": "text", "text": text}]})
        if lock(tmp).get("session") != SESSION:
            failures.append(f"successful create was not recorded in the claim: {lock(tmp)}")

        d, why = decision(hook("PreToolUse", create(PHONE), tmp))
        if d != "deny" or "run-a" not in why or "delete" not in why:
            failures.append(f"second create was not denied naming the holder and the way out: "
                            f"{d} {why!r}")

        hook("PostToolUse", {"action": "delete"}, tmp,
             {"content": [{"type": "text", "text": "Active session deleted successfully."}]})
        if "session" in lock(tmp):
            failures.append(f"delete did not clear the recorded session: {lock(tmp)}")
        if lock(tmp).get("run") != "run-a":
            failures.append(f"delete dropped the claim itself: {lock(tmp)}")

        out = hook("PreToolUse", create(PHONE), tmp)
        if out:
            failures.append(f"create after delete was not allowed: {out}")

        hook("PostToolUse", create(PHONE), tmp,
             {"content": [{"type": "text", "text": "Failed to create session: boom"}],
              "isError": True})
        if "session" in lock(tmp):
            failures.append(f"failed create was recorded as a session: {lock(tmp)}")

        for action in ("list", "select", "detach"):
            if hook("PreToolUse", {"action": action, "sessionId": SESSION}, tmp):
                failures.append(f"{action} was gated")

    for f in failures:
        print("FAIL:", f)
    print("PASS: one Appium session per phone, only after a claim, cleared by delete"
          if not failures else f"{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
