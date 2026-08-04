import { describe, expect, it, vi } from "vitest";
import { AutosaveController, type SaveState } from "./autosave";

describe("AutosaveController", () => {
  it("debounces changes and preserves the loaded entry identity", async () => {
    vi.useFakeTimers();
    const saves: Array<[number, string, string]> = [];
    const states: SaveState[] = [];
    const autosave = new AutosaveController(
      async (id, expectedBody, body) => {
        saves.push([id, expectedBody, body]);
      },
      (state) => states.push(state),
      100,
    );

    autosave.load(41, "first");
    autosave.change("second");
    autosave.change("latest");
    await vi.advanceTimersByTimeAsync(100);

    expect(saves).toEqual([[41, "first", "latest"]]);
    expect(states.at(-1)).toEqual({ kind: "saved" });
    vi.useRealTimers();
  });

  it("flushes pending content before a different entry is loaded", async () => {
    const saves: Array<[number, string, string]> = [];
    const autosave = new AutosaveController(
      async (id, expectedBody, body) => {
        saves.push([id, expectedBody, body]);
      },
      () => undefined,
    );

    autosave.load(1, "one");
    autosave.change("one edited");
    await autosave.flush();
    autosave.load(2, "two");
    autosave.change("two edited");
    await autosave.flush();

    expect(saves).toEqual([
      [1, "one", "one edited"],
      [2, "two", "two edited"],
    ]);
  });

  it("surfaces save failures and keeps the content pending for retry", async () => {
    let attempts = 0;
    const states: SaveState[] = [];
    const autosave = new AutosaveController(
      async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("database busy");
      },
      (state) => states.push(state),
    );

    autosave.load(7, "before");
    autosave.change("after");
    await expect(autosave.flush()).rejects.toThrow("database busy");
    expect(states.at(-1)).toEqual({ kind: "error", message: "database busy" });
    await autosave.flush();
    expect(attempts).toBe(2);
    expect(states.at(-1)).toEqual({ kind: "saved" });
  });

  it("keeps one save in flight and flushes the latest change", async () => {
    let releaseFirst: (() => void) | undefined;
    let concurrent = 0;
    let maxConcurrent = 0;
    const saves: string[] = [];
    const autosave = new AutosaveController(
      async (_id, _expectedBody, body) => {
        concurrent += 1;
        maxConcurrent = Math.max(maxConcurrent, concurrent);
        saves.push(body);
        if (body === "first") await new Promise<void>((resolve) => (releaseFirst = resolve));
        concurrent -= 1;
      },
      () => undefined,
      10,
    );

    autosave.load(9, "initial");
    autosave.change("first");
    await new Promise((resolve) => setTimeout(resolve, 15));
    autosave.change("latest");
    const flushing = autosave.flush();
    releaseFirst?.();
    await flushing;

    expect(saves).toEqual(["first", "latest"]);
    expect(maxConcurrent).toBe(1);
  });

  it("uses each successful body as the expected base for the next save", async () => {
    const saves: Array<[string, string]> = [];
    const autosave = new AutosaveController(
      async (_id, expectedBody, body) => {
        saves.push([expectedBody, body]);
      },
      () => undefined,
    );

    autosave.load(1, "loaded");
    autosave.change("first");
    await autosave.flush();
    autosave.change("second");
    await autosave.flush();

    expect(saves).toEqual([
      ["loaded", "first"],
      ["first", "second"],
    ]);
  });
});
