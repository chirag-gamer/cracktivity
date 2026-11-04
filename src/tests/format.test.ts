/** L1: display helpers (relativeTime/senderName/initials). */
import { describe, expect, it } from "vitest";
import { relativeTime, senderName } from "../shared/format";

describe("relativeTime", () => {
  const now = Date.parse("2026-11-04T12:00:00Z");
  it("renders now/minutes/hours compactly", () => {
    expect(relativeTime(now - 30_000, now)).toBe("now");
    expect(relativeTime(now - 5 * 60_000, now)).toBe("5m");
    expect(relativeTime(now - 3 * 3_600_000, now)).toBe("3h");
  });
  it("switches to weekday and then day-month", () => {
    expect(relativeTime(now - 2 * 86_400_000, now)).toBeTruthy();
    expect(relativeTime(now - 30 * 86_400_000, now)).toMatch(/\d{1,2}/);
  });
});

describe("senderName", () => {
  it("prefers the display name, falls back to the local part", () => {
    expect(senderName({ name: "Priya", email: "p@x.y" })).toBe("Priya");
    expect(senderName({ name: "  ", email: "priya@x.y" })).toBe("priya");
    expect(senderName(null)).toBe("(unknown)");
  });
});
