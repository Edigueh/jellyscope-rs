import assert from "node:assert/strict";
import test from "node:test";

import { formatLogMass } from "./mass-format.mjs";

test("formats linear stellar mass as log10 solar masses", () => {
  assert.equal(formatLogMass(3007335.42439138), "6.478");
  assert.equal(formatLogMass(489448.68901650136), "5.690");
});

test("invalid or unavailable mass displays as an em dash", () => {
  for (const value of [null, 0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
    assert.equal(formatLogMass(value), "—");
  }
});
