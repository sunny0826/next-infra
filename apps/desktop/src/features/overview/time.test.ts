import { describe, expect, it } from "vitest";

import { formatRelativeTime } from "./time";

const NOW = new Date("2026-08-09T12:00:00Z");

describe("formatRelativeTime", () => {
  it("returns the raw input for an invalid date", () => {
    expect(formatRelativeTime("not-a-date", NOW)).toBe("not-a-date");
    expect(formatRelativeTime("", NOW)).toBe("");
  });

  it("returns 刚刚 for future timestamps", () => {
    expect(formatRelativeTime("2026-08-09T12:00:01Z", NOW)).toBe("刚刚");
  });

  it("returns 刚刚 below one minute", () => {
    expect(formatRelativeTime("2026-08-09T11:59:30Z", NOW)).toBe("刚刚");
    expect(formatRelativeTime("2026-08-09T12:00:00Z", NOW)).toBe("刚刚");
  });

  it("returns minutes below one hour", () => {
    expect(formatRelativeTime("2026-08-09T11:59:00Z", NOW)).toBe("1 分钟前");
    expect(formatRelativeTime("2026-08-09T11:15:00Z", NOW)).toBe("45 分钟前");
  });

  it("returns hours below one day", () => {
    expect(formatRelativeTime("2026-08-09T11:00:00Z", NOW)).toBe("1 小时前");
    expect(formatRelativeTime("2026-08-08T13:00:00Z", NOW)).toBe("23 小时前");
  });

  it("returns days below one week", () => {
    expect(formatRelativeTime("2026-08-08T12:00:00Z", NOW)).toBe("1 天前");
    expect(formatRelativeTime("2026-08-03T12:00:00Z", NOW)).toBe("6 天前");
  });

  it("returns weeks below five weeks", () => {
    expect(formatRelativeTime("2026-08-02T12:00:00Z", NOW)).toBe("1 周前");
    expect(formatRelativeTime("2026-07-12T12:00:00Z", NOW)).toBe("4 周前");
  });

  it("returns a compact UTC date for older timestamps", () => {
    expect(formatRelativeTime("2026-07-05T12:00:00Z", NOW)).toBe("2026-07-05");
    expect(formatRelativeTime("2000-01-01T00:00:00Z", NOW)).toBe("2000-01-01");
  });

  it("defaults `now` to the current time", () => {
    expect(formatRelativeTime("not-a-date")).toBe("not-a-date");
    expect(formatRelativeTime(new Date().toISOString())).toBe("刚刚");
  });
});
