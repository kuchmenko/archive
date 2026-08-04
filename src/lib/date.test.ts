import { describe, expect, it } from "vitest";
import { addLocalDays, fromLocalDay, millisecondsUntilNextLocalDay, toLocalDay } from "./date";

describe("local calendar dates", () => {
  it("serializes without converting through UTC", () => {
    const local = new Date(2026, 0, 2, 23, 30);
    expect(toLocalDay(local)).toBe("2026-01-02");
    expect(toLocalDay(fromLocalDay("2026-01-02"))).toBe("2026-01-02");
  });

  it("moves across month, year, and leap-day boundaries", () => {
    expect(addLocalDays("2024-02-28", 1)).toBe("2024-02-29");
    expect(addLocalDays("2024-02-29", 1)).toBe("2024-03-01");
    expect(addLocalDays("2025-12-31", 1)).toBe("2026-01-01");
    expect(addLocalDays("2026-01-01", -1)).toBe("2025-12-31");
  });

  it("calculates the next local midnight", () => {
    expect(millisecondsUntilNextLocalDay(new Date(2026, 7, 3, 23, 59, 59, 500))).toBe(500);
  });
});
