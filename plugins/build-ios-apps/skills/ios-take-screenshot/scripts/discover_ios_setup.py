#!/usr/bin/env python3
"""Report what a real iPhone needs for an Appium session, discovering what it can.

Most session capabilities do not have to be asked for or remembered: the UDID
comes from the device list, the team id from a provisioning profile, and the
WebDriverAgent bundle id from the runner already installed on the device.
Prints JSON, including a suggestedCapabilities object ready for
appium_session_management (action=create).
"""

from __future__ import annotations

import argparse
import json
import plistlib
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

PROFILE_DIRS = [
    Path.home() / "Library/Developer/Xcode/UserData/Provisioning Profiles",  # Xcode 16+
    Path.home() / "Library/MobileDevice/Provisioning Profiles",              # legacy
]
SIGNED_WDA_GLOB = ".cache/appium-mcp/wda-real/*/signed/*/Payload-resigned.ipa"
PREBUILT_WDA = Path.home() / ".appium/wda-dd"
DEFAULT_WDA_BUNDLE = "com.facebook.WebDriverAgentRunner"


def devicectl(*args: str) -> dict:
    with tempfile.NamedTemporaryFile(suffix=".json") as out:
        proc = subprocess.run(
            ["xcrun", "devicectl", *args, "--json-output", out.name],
            capture_output=True, text=True, check=False,
        )
        if proc.returncode != 0:
            return {}
        try:
            return json.loads(Path(out.name).read_text())
        except json.JSONDecodeError:
            return {}


def list_devices() -> list[dict]:
    data = devicectl("list", "devices")
    devices = []
    for d in data.get("result", {}).get("devices", []):
        hw, props = d.get("hardwareProperties", {}), d.get("deviceProperties", {})
        conn = d.get("connectionProperties", {})
        if conn.get("tunnelState") == "unavailable":
            continue
        devices.append({
            "udid": hw.get("udid"),
            "name": props.get("name"),
            "osVersion": props.get("osVersionNumber"),
            "transport": conn.get("transportType"),
            "developerModeEnabled": props.get("developerModeStatus") == "enabled",
        })
    return devices


def wda_bundle_id(udid: str) -> str | None:
    """The installed WebDriverAgent runner, if any.

    Appium wants the bundle id without the .xctrunner suffix that Xcode appends.
    """
    data = devicectl("device", "info", "apps", "--device", udid, "--include-all-apps")
    for app in data.get("result", {}).get("apps", []):
        bundle = app.get("bundleIdentifier") or ""
        if bundle.endswith(".xctrunner") and "webdriveragent" in bundle.lower().replace("-", ""):
            return bundle[: -len(".xctrunner")]
    for app in data.get("result", {}).get("apps", []):
        bundle = app.get("bundleIdentifier") or ""
        if bundle.endswith(".xctrunner"):
            return bundle[: -len(".xctrunner")]
    return None


def profiles(udid: str | None) -> list[dict]:
    found = []
    for directory in PROFILE_DIRS:
        for path in sorted(directory.glob("*.mobileprovision")) if directory.is_dir() else []:
            raw = subprocess.run(["security", "cms", "-D", "-i", str(path)],
                                 capture_output=True, check=False).stdout
            try:
                pl = plistlib.loads(raw)
            except Exception:
                continue
            expires = pl.get("ExpirationDate")
            days = (expires.replace(tzinfo=timezone.utc) - datetime.now(timezone.utc)).days if expires else None
            devices = pl.get("ProvisionedDevices") or []
            found.append({
                "name": pl.get("Name"),
                "teamId": (pl.get("TeamIdentifier") or [None])[0],
                "appId": pl.get("Entitlements", {}).get("application-identifier"),
                "expiresInDays": days,
                "coversDevice": bool(udid and udid in devices),
                "wildcard": str(pl.get("Entitlements", {}).get("application-identifier", "")).endswith(".*"),
            })
    return found


def signed_wda_ipa() -> str | None:
    """The newest WebDriverAgent that appium_prepare_ios_real_device signed.

    Preferred over a hand-built one: the tool downloads the release matching the
    driver, so there is no driver/WDA version skew to debug.
    """
    found = sorted(Path.home().glob(SIGNED_WDA_GLOB), key=lambda p: p.stat().st_mtime, reverse=True)
    return str(found[0]) if found else None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--device", help="UDID; defaults to the only connected device")
    args = ap.parse_args()

    devices = list_devices()
    udid = args.device or (devices[0]["udid"] if len(devices) == 1 else None)
    device = next((d for d in devices if d["udid"] == udid), None)

    profs = profiles(udid)
    usable = [p for p in profs if p["coversDevice"]]
    team = next((p["teamId"] for p in usable if p["wildcard"]),
                next((p["teamId"] for p in usable), None))
    wda = wda_bundle_id(udid) if udid else None

    report = {
        "devices": devices,
        "selectedDevice": device,
        "profiles": profs,
        "webDriverAgent": {
            "installed": bool(wda),
            "bundleId": wda,
            "signedIpa": signed_wda_ipa(),
            "prebuiltDerivedData": str(PREBUILT_WDA) if PREBUILT_WDA.is_dir() else None,
        },
        "ready": bool(udid and team and wda),
    }

    if report["ready"]:
        caps = {
            "appium:udid": udid,
            "appium:xcodeOrgId": team,
            "appium:xcodeSigningId": "Apple Development",
            "appium:noReset": True,
        }
        if wda != DEFAULT_WDA_BUNDLE:
            caps["appium:updatedWDABundleId"] = wda
        signed = report["webDriverAgent"]["signedIpa"]
        if signed:
            caps["appium:usePreinstalledWDA"] = True
            caps["appium:prebuiltWDAPath"] = signed
            caps["appium:wdaLaunchTimeout"] = 30000
        elif report["webDriverAgent"]["prebuiltDerivedData"]:
            caps["appium:usePrebuiltWDA"] = True
            caps["appium:derivedDataPath"] = report["webDriverAgent"]["prebuiltDerivedData"]
        if device and device.get("osVersion"):
            caps["appium:platformVersion"] = device["osVersion"]
        report["suggestedCapabilities"] = caps
    else:
        missing = []
        if not udid:
            missing.append("no single connected device; pass --device")
        if not team:
            missing.append("no provisioning profile covering this device")
        if not wda:
            missing.append("WebDriverAgent is not installed; run appium_prepare_ios_real_device")
        report["missing"] = missing

    print(json.dumps(report, indent=2))
    return 0 if report["ready"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
