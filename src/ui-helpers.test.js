import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_LOG_LIMIT,
  mergeLogLines,
  normalizePort,
  portIsValid,
  shouldStickToBottom,
} from "./ui-helpers.js";

test("normalizePort falls back when input is invalid", () => {
  assert.equal(normalizePort("abc", 7000), 7000);
  assert.equal(normalizePort("25565", 7000), 25565);
});

test("portIsValid accepts only in-range ports", () => {
  assert.equal(portIsValid("25565"), true);
  assert.equal(portIsValid("0"), false);
  assert.equal(portIsValid("70000"), false);
});

test("mergeLogLines trims to the newest limit", () => {
  const existing = Array.from({ length: DEFAULT_LOG_LIMIT - 1 }, (_, index) => `old-${index}`);
  const merged = mergeLogLines(existing, ["fresh-1", "fresh-2"]);

  assert.equal(merged.length, DEFAULT_LOG_LIMIT);
  assert.equal(merged.at(-1), "fresh-2");
  assert.equal(merged.includes("old-0"), false);
});

test("shouldStickToBottom detects near-bottom scroll state", () => {
  const nearBottom = { scrollTop: 270, clientHeight: 300, scrollHeight: 580 };
  const awayFromBottom = { scrollTop: 120, clientHeight: 300, scrollHeight: 580 };

  assert.equal(shouldStickToBottom(nearBottom), true);
  assert.equal(shouldStickToBottom(awayFromBottom), false);
});
