import { describe, expect, it } from "vitest";
import { appShortcut } from "./shortcuts";

const key = (value: string, overrides: Partial<KeyboardEvent> = {}) => ({
  altKey: false,
  ctrlKey: true,
  key: value,
  metaKey: false,
  shiftKey: false,
  ...overrides,
});

describe("application shortcuts", () => {
  it("handles shared/private note creation and Ctrl+O", () => {
    expect(appShortcut(key("n"))).toBe("new-note");
    expect(appShortcut(key("N", { shiftKey: true }))).toBe("new-private-note");
    expect(appShortcut(key("O"))).toBe("open-commands");
  });

  it("does not claim modified or unrelated browser and Vim keys", () => {
    expect(appShortcut(key("o", { shiftKey: true }))).toBeNull();
    expect(appShortcut(key("o", { ctrlKey: false }))).toBeNull();
    expect(appShortcut(key("r"))).toBeNull();
  });
});
