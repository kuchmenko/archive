import { describe, expect, it, vi } from "vitest";
import { AutosaveController, mergeDocumentBodies, type SaveState } from "./autosave";

describe("AutosaveController", () => {
  it("debounces changes and preserves the loaded entry identity", async () => {
    vi.useFakeTimers();
    const saves: Array<[number, number, string]> = [];
    const states: SaveState[] = [];
    const autosave = new AutosaveController(
      async (id, expectedRevision, body) => {
        saves.push([id, expectedRevision, body]);
        return { body, revision: expectedRevision + 1 };
      },
      (state) => states.push(state),
      100,
    );

    autosave.load(41, "first", 7);
    autosave.change("second");
    autosave.change("latest");
    await vi.advanceTimersByTimeAsync(100);

    expect(saves).toEqual([[41, 7, "latest"]]);
    expect(states.at(-1)).toEqual({ kind: "saved" });
    vi.useRealTimers();
  });

  it("flushes pending content before a different entry is loaded", async () => {
    const saves: Array<[number, number, string]> = [];
    const autosave = new AutosaveController(
      async (id, expectedRevision, body) => {
        saves.push([id, expectedRevision, body]);
        return { body, revision: expectedRevision + 1 };
      },
      () => undefined,
    );

    autosave.load(1, "one", 1);
    autosave.change("one edited");
    await autosave.flush();
    autosave.load(2, "two", 3);
    autosave.change("two edited");
    await autosave.flush();

    expect(saves).toEqual([
      [1, 1, "one edited"],
      [2, 3, "two edited"],
    ]);
  });

  it("surfaces save failures and keeps the content pending for retry", async () => {
    let attempts = 0;
    const states: SaveState[] = [];
    const autosave = new AutosaveController(
      async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("database busy");
        return { body: "after", revision: 2 };
      },
      (state) => states.push(state),
    );

    autosave.load(7, "before", 1);
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
      async (_id, expectedRevision, body) => {
        concurrent += 1;
        maxConcurrent = Math.max(maxConcurrent, concurrent);
        saves.push(body);
        if (body === "first") await new Promise<void>((resolve) => (releaseFirst = resolve));
        concurrent -= 1;
        return { body, revision: expectedRevision + 1 };
      },
      () => undefined,
      10,
    );

    autosave.load(9, "initial", 1);
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
    const saves: Array<[number, string]> = [];
    const autosave = new AutosaveController(
      async (_id, expectedRevision, body) => {
        saves.push([expectedRevision, body]);
        return { body, revision: expectedRevision + 1 };
      },
      () => undefined,
    );

    autosave.load(1, "loaded", 4);
    autosave.change("first");
    await autosave.flush();
    autosave.change("second");
    await autosave.flush();

    expect(saves).toEqual([
      [4, "first"],
      [5, "second"],
    ]);
  });

  it("merges disjoint line hunks and reports overlapping line edits", () => {
    expect(mergeDocumentBodies("one\ntwo\nthree", "ONE\ntwo\nthree", "one\ntwo\nTHREE"))
      .toEqual({ kind: "merged", body: "ONE\ntwo\nTHREE" });
    expect(mergeDocumentBodies("one\ntwo\nthree", "one\nLOCAL\nthree", "one\nREMOTE\nthree"))
      .toEqual({ kind: "conflict" });
  });

  it("adopts clean remote state without saving and pauses dirty persistence during conflict", async () => {
    const save = vi.fn(async (_id: number, revision: number, body: string) => ({
      body,
      revision: revision + 1,
    }));
    const autosave = new AutosaveController(save, () => undefined, 10);
    autosave.load(1, "base", 3);
    autosave.adoptRemote("remote", 4);
    await autosave.flush();
    expect(save).not.toHaveBeenCalled();
    expect(autosave.snapshot()).toMatchObject({ body: "remote", revision: 4, dirty: false });

    autosave.change("local");
    autosave.pause();
    await expect(autosave.flush()).rejects.toThrow("resolve the document conflict");
    expect(save).not.toHaveBeenCalled();
  });

  it("ignores an obsolete in-flight response and drains the rebased body", async () => {
    let resolveOld: ((document: { body: string; revision: number }) => void) | undefined;
    const saves: Array<[number, string]> = [];
    const persisted: number[] = [];
    const autosave = new AutosaveController(
      async (_id, revision, body) => {
        saves.push([revision, body]);
        if (revision === 1) {
          return new Promise((resolve) => {
            resolveOld = resolve;
          });
        }
        return { body, revision: revision + 1 };
      },
      () => undefined,
      0,
      (_id, document) => persisted.push(document.revision),
    );
    autosave.load(1, "base", 1);
    autosave.change("first local");
    const firstFlush = autosave.flush();
    await Promise.resolve();
    autosave.rebase("merged local", { body: "remote", revision: 3 });
    const mergedFlush = autosave.flush();
    resolveOld?.({ body: "first local", revision: 2 });
    await Promise.all([firstFlush, mergedFlush]);

    expect(saves).toEqual([
      [1, "first local"],
      [3, "merged local"],
    ]);
    expect(persisted).toEqual([4]);
    expect(autosave.snapshot()).toMatchObject({ revision: 4, dirty: false });
  });
});
