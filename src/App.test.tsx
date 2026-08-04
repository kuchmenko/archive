import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { createNote, getOrCreateDaily } from "./lib/archive";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    destroy: vi.fn(),
    onCloseRequested: vi.fn().mockResolvedValue(vi.fn()),
  }),
}));

vi.mock("./lib/archive", () => ({
  createNote: vi.fn(),
  deleteNote: vi.fn(),
  getDocument: vi.fn(),
  getOrCreateDaily: vi.fn(),
  searchDocuments: vi.fn(),
  resolveReferences: vi.fn().mockResolvedValue([]),
  updateDocument: vi.fn(),
}));

vi.mock("@replit/codemirror-vim", () => ({
  getCM: () => undefined,
  vim: () => [],
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: vi.fn(),
  writeText: vi.fn(),
}));

const daily = {
  id: 1,
  kind: "daily" as const,
  visibility: "shared" as const,
  author: "user" as const,
  day: "2026-08-03",
  created_at: "2026-08-03T10:00:00.000Z",
  updated_at: "2026-08-03T10:00:00.000Z",
  body: "",
};

describe("Archive document canvas", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 7, 3, 10, 0, 0));
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    Element.prototype.scrollIntoView = vi.fn();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      queueMicrotask(() => callback(0));
      return 0;
    });
    vi.mocked(getOrCreateDaily).mockResolvedValue(daily);
    vi.mocked(createNote).mockResolvedValue({ ...daily, id: 2, kind: "note" });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("opens today's canonical daily and Ctrl+N creates a standalone note", async () => {
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    expect(getOrCreateDaily).toHaveBeenCalledWith("2026-08-03");
    expect(screen.getByRole("heading", { name: "Monday, August 3, 2026" })).toBeTruthy();
    expect(screen.queryByRole("navigation")).toBeNull();

    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, key: "n" });
    await act(async () => Promise.resolve());
    expect(createNote).toHaveBeenCalledWith("2026-08-03", "shared");
    expect(screen.getByRole("heading", { name: "Untitled note" })).toBeTruthy();
  });

  it("Ctrl+Shift+N creates and focuses a private note", async () => {
    vi.mocked(createNote).mockResolvedValue({ ...daily, id: 3, kind: "note", visibility: "private" });
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, shiftKey: true, key: "N" });
    await act(async () => Promise.resolve());

    expect(createNote).toHaveBeenCalledWith("2026-08-03", "private");
    expect(screen.getByRole("heading", { name: "Untitled note" })).toBeTruthy();
    expect(screen.getByText(/Private/, { selector: "footer *" })).toBeTruthy();
  });

  it("retries a failed midnight daily switch without exposing the wrong day", async () => {
    vi.setSystemTime(new Date(2026, 7, 3, 23, 59, 59, 900));
    const nextDaily = { ...daily, id: 3, day: "2026-08-04" };
    vi.mocked(getOrCreateDaily)
      .mockResolvedValueOnce(daily)
      .mockRejectedValueOnce(new Error("temporarily unavailable"))
      .mockResolvedValueOnce(nextDaily);
    render(<App />);
    await act(async () => Promise.resolve());

    await act(async () => {
      vi.setSystemTime(new Date(2026, 7, 4, 0, 0, 0, 100));
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(screen.getByRole("heading", { name: "Monday, August 3, 2026" })).toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(screen.getByRole("heading", { name: "Tuesday, August 4, 2026" })).toBeTruthy();
    expect(getOrCreateDaily).toHaveBeenCalledTimes(3);
  });
});
