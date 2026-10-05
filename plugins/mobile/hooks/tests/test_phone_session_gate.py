#!/usr/bin/env -S uv run --quiet --script
# ABOUTME: Regression tests for the phone-session-gate hook: one Appium session per phone, only after
# ABOUTME: a claim, recorded in the claim on create and cleared on delete.
"""Regression tests for the phone-session-gate hook.

One Appium session per phone, and only after a claim: a create without a claim
or while a session is open is denied with a reason that says what to do; a
successful create is recorded in the claim, and a delete clears it so the holder
can recreate after an idle-out. Run: ./test_phone_session_gate.py
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor

HERE = Path(__file__).parent
HOOK = HERE.parent / "phone-session-gate"
CLAIM = HERE.parent.parent / "skills/ios-take-screenshot/scripts/claim-simulator.mjs"
TOOL = "mcp__plugin_mobile_appium-mcp__appium_session_management"
PHONE = "00008030-001A2B3C4D5E6F7A"
SESSION = "3f1c0a52-7d4e-4b0e-9d7a-6c1b2e3d4f50"


def hook(event: str, tool_input: dict, tmp: str, response=None, caller="caller-a", call="call-a", interrupted=False) -> dict:
    payload = {"hook_event_name": event, "tool_name": TOOL, "tool_input": tool_input,
               "session_id": caller, "tool_use_id": call}
    if event == "PostToolUseFailure":
        payload.update(error="Failed to create session: boom", is_interrupt=interrupted)
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
    config = json.loads((HERE.parent / "hooks.json").read_text())
    for event in ("PreToolUse", "PostToolUse", "PostToolUseFailure"):
        group = config["hooks"][event][0]
        for name in (TOOL, "mcp__plugin_mobile_appium_mcp__appium_session_management",
                     "mcp__appium_mcp__appium_session_management"):
            if not re.fullmatch(group["matcher"], name):
                failures.append(f"{event} does not match Appium session tool {name}")
        for name in ("mcp__plugin_mobile_xcodebuildmcp__session_set_defaults",
                     "mcp__appium_mcp__appium_screenshot",
                     "mcp__other__appium_session_management"):
            if re.fullmatch(group["matcher"], name):
                failures.append(f"{event} incorrectly matches unrelated tool {name}")

    # Execute the configured hook from an installed path containing spaces.
    with tempfile.TemporaryDirectory(prefix="mobile plugin ") as installed:
        hook_dir = Path(installed) / "hooks"
        hook_dir.mkdir()
        shutil.copy2(HOOK, hook_dir / HOOK.name)
        shutil.copy2(HERE.parent / "claim_state.py", hook_dir / "claim_state.py")
        payload = {"hook_event_name": "PreToolUse", "tool_name": TOOL,
                   "tool_input": create()}
        result = subprocess.run(
            ["/bin/sh", "-c", config["hooks"]["PreToolUse"][0]["hooks"][0]["command"]],
            input=json.dumps(payload), text=True, capture_output=True,
            env={**os.environ, "CLAUDE_PLUGIN_ROOT": installed}, check=False,
        )
        if result.returncode != 0:
            failures.append(f"installed hook path failed: {result.stderr}")
        elif decision(json.loads(result.stdout))[0] != "deny":
            failures.append("installed hook did not refuse create without a UDID")

    with tempfile.TemporaryDirectory() as tmp:
        d, why = decision(hook("PreToolUse", create(), tmp))
        if d != "deny" or "appium:udid" not in why:
            failures.append(f"create without a udid was not denied for it: {d} {why!r}")

        d, why = decision(hook("PreToolUse", create(PHONE), tmp))
        if d != "deny" or "claim-simulator.mjs" not in why:
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

        # Reclaims must preserve the existing Appium connection and its age.
        held_before = lock(tmp)
        subprocess.run([str(CLAIM), PHONE, "--run", "run-a"], check=True,
                       capture_output=True, env={**os.environ, "TMPDIR": tmp})
        if lock(tmp) != held_before:
            failures.append("reclaim erased session state or changed its age")
        for flag in ("--release",):
            result = subprocess.run([str(CLAIM), PHONE, "--run", "run-a", flag],
                                    capture_output=True, env={**os.environ, "TMPDIR": tmp})
            if result.returncode != 3 or lock(tmp) != held_before:
                failures.append(f"{flag} cleared a live phone session")
        if decision(hook("PreToolUse", {"action": "delete"}, tmp))[0] != "deny":
            failures.append("delete without explicit session id was allowed")
        if decision(hook("PreToolUse", {"action": "delete", "sessionId": SESSION}, tmp,
                         caller="caller-b"))[0] != "deny":
            failures.append("another caller could delete the live session")
        hook("PostToolUse", {"action": "delete"}, tmp,
             {"content": [{"type": "text", "text": "Active session deleted successfully."}]})
        if lock(tmp) != held_before:
            failures.append("ambiguous delete cleared a claim")
        hook("PostToolUse", {"action": "delete", "sessionId": SESSION}, tmp,
             {"content": [{"type": "text", "text": "Something else happened"}]})
        if lock(tmp) != held_before:
            failures.append("unconfirmed delete cleared a claim")
        second = Path(tmp) / "ios-screenshot-lock.other-phone.json"
        second.write_text(json.dumps({"run": "run-b", "session": "other-session"}))
        hook("PostToolUse", {"action": "delete", "sessionId": SESSION}, tmp,
             {"content": [{"type": "text", "text": f"Session {SESSION} deleted successfully."}]})
        if "session" in lock(tmp):
            failures.append(f"delete did not clear the recorded session: {lock(tmp)}")
        if json.loads(second.read_text()).get("session") != "other-session":
            failures.append("delete cleared another device session")
        if lock(tmp).get("run") != "run-a":
            failures.append(f"delete dropped the claim itself: {lock(tmp)}")

        out = hook("PreToolUse", create(PHONE), tmp)
        if out:
            failures.append(f"create after delete was not allowed: {out}")

        hook("PostToolUse", create(PHONE), tmp,
             {"content": [{"type": "text", "text": "Failed to create session: boom"}],
              "isError": True})
        if "session" in lock(tmp) or "pending" in lock(tmp):
            failures.append(f"failed create was recorded as a session: {lock(tmp)}")

        for action in ("list", "select", "detach"):
            if hook("PreToolUse", {"action": action, "sessionId": SESSION}, tmp):
                failures.append(f"{action} was gated")

    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run([str(CLAIM), PHONE, "--run", "run-a"], check=True,
                       capture_output=True, env={**os.environ, "TMPDIR": tmp})
        hook("PreToolUse", create(PHONE), tmp)
        if decision(hook("PreToolUse", create(PHONE), tmp, caller="caller-b", call="call-b"))[0] != "deny":
            failures.append("second create passed while first was in progress")
        pending = lock(tmp)
        hook("PostToolUse", create(PHONE), tmp,
             {"content": [{"type": "text", "text": f"IOS session created successfully with ID: {SESSION}"}]},
             caller="caller-b", call="call-b")
        if lock(tmp) != pending:
            failures.append("another call committed the first call's reservation")
        hook("PostToolUse", create(PHONE), tmp, {"isError": True, "content": []})
        if "pending" in lock(tmp):
            failures.append("failed create left its reservation")
        if hook("PreToolUse", create(PHONE), tmp, call="call-c"):
            failures.append("could not retry after failed create")
        hook("PostToolUse", create(PHONE), tmp, {"content": [{"type": "text", "text": "unknown result"}]}, call="call-c")
        if "pending" not in lock(tmp):
            failures.append("unknown create result incorrectly freed the phone")

    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run([str(CLAIM), PHONE, "--run", "run-a"], check=True,
                       capture_output=True, env={**os.environ, "TMPDIR": tmp})
        with ThreadPoolExecutor(max_workers=8) as executor:
            results = list(executor.map(lambda i: hook("PreToolUse", create(PHONE), tmp,
                                                      caller=f"caller-{i}", call=f"call-{i}"), range(8)))
        if sum(not result for result in results) != 1:
            failures.append("simultaneous creates did not yield exactly one reservation")
        old = lock(tmp)["pending"]
        subprocess.run([str(CLAIM), PHONE, "--run", "new", "--steal"], check=True,
                       capture_output=True, env={**os.environ, "TMPDIR": tmp})
        hook("PreToolUse", create(PHONE), tmp, caller="new-caller", call="new-call")
        current = lock(tmp)
        hook("PostToolUse", create(PHONE), tmp,
             {"content": [{"type": "text", "text": f"IOS session created successfully with ID: {SESSION}"}]},
             caller=old["caller"], call=old["call"])
        if lock(tmp) != current:
            failures.append("late create changed a new run's reservation")
        hook("PostToolUse", create(PHONE), tmp,
             [{"isError": True, "content": [{"type": "text", "text": "failed"}]}],
             caller="new-caller", call="new-call")
        if "pending" in lock(tmp):
            failures.append("nested tool error did not release its reservation")
        claim_file = Path(tmp) / f"ios-screenshot-lock.{PHONE}.json"
        claim_file.write_text("{unfinished")
        if decision(hook("PreToolUse", create(PHONE), tmp))[0] != "deny":
            failures.append("corrupt claim allowed a create")
        if claim_file.read_text() != "{unfinished":
            failures.append("corrupt claim was silently replaced")

    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run([str(CLAIM), PHONE, "--run", "run-a"], check=True,
                       capture_output=True, env={**os.environ, "TMPDIR": tmp})
        hook("PreToolUse", create(PHONE), tmp)
        hook("PostToolUseFailure", create(PHONE), tmp)
        if "pending" in lock(tmp):
            failures.append("Claude create failure left its reservation")
        hook("PreToolUse", create(PHONE), tmp, call="retry")
        hook("PostToolUseFailure", create(PHONE), tmp, call="retry", interrupted=True)
        if "pending" not in lock(tmp):
            failures.append("interrupted create incorrectly freed an unknown outcome")

    for f in failures:
        print("FAIL:", f)
    print("PASS: one Appium session per phone, only after a claim, cleared by delete"
          if not failures else f"{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
