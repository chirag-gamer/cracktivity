import { describe, expect, it } from "vitest";

// Minimal pure logic extracted from the theme provider; keeps the L1 runner
// honest without pulling a DOM environment into the scaffold (spec §13 L1).
// The media-query behavior itself is exercised by L5 e2e in Phase 3.

function modeFromMatches(matches: boolean): "light" | "dark" {
  return matches ? "dark" : "light";
}

describe("theme mode resolution", () => {
  it("maps prefers-color-scheme dark to the dark theme", () => {
    expect(modeFromMatches(true)).toBe("dark");
  });
  it("maps no dark preference to the light theme", () => {
    expect(modeFromMatches(false)).toBe("light");
  });
});
