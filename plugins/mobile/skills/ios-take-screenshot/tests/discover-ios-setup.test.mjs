// ABOUTME: Regression tests for the simulator branch of discover-ios-setup.mjs: the report names
// ABOUTME: its pick in full and says the right thing when the requested simulator is not booted.

import { test, describe } from "node:test";
import assert from "node:assert/strict";

import { deps, simulatorReport } from "../scripts/discover-ios-setup.mjs";

const BOOTED = [
  { udid: "AAAA", name: "iPhone 16 Pro", osVersion: "18.3", state: "Booted" },
  { udid: "BBBB", name: "iPhone 16", osVersion: "18.3", state: "Booted" },
];

describe("the simulator report names its pick and explains every refusal", () => {
  test("session defaults carry the name with the id, so a stale name does not survive", () => {
    deps.listSimulators = () => [...BOOTED];
    const r = simulatorReport("AAAA");
    assert.equal(r.ready, true);
    assert.deepEqual(r.sessionDefaults, { simulatorId: "AAAA", simulatorName: "iPhone 16 Pro" });
    assert.deepEqual(r.selectedSimulator, BOOTED[0]);
  });

  test("a shutdown UDID is reported as not booted", () => {
    deps.listSimulators = () => [...BOOTED];
    const r = simulatorReport("CCCC");
    assert.equal(r.ready, false);
    assert.equal(r.selectedSimulator, null);
    assert.ok(r.missing[0].includes("CCCC"), r.missing[0]);
    assert.ok(r.missing[0].includes("not booted"), r.missing[0]);
  });

  test("two booted and none requested asks for --device", () => {
    deps.listSimulators = () => [...BOOTED];
    const r = simulatorReport(null);
    assert.equal(r.ready, false);
    assert.ok(r.missing[0].includes("--device"), r.missing[0]);
  });

  test("a single booted simulator is selected", () => {
    deps.listSimulators = () => BOOTED.slice(0, 1);
    const r = simulatorReport(null);
    assert.equal(r.ready, true);
    assert.equal(r.selectedSimulator.udid, "AAAA");
  });

  test("no booted simulator says how to boot one", () => {
    deps.listSimulators = () => [];
    const r = simulatorReport(null);
    assert.equal(r.ready, false);
    assert.ok(r.missing[0].includes("xcrun simctl boot"), r.missing[0]);
  });
});
