import { describe, expect, it } from "vitest";
import type { Document } from "./archive";
import { addBuffer, adjacentBufferId, removeBuffer } from "./buffers";

const document = (id: number): Document => ({
  id, kind: "note", visibility: "shared", author: "user", day: "2026-08-03", created_at: "", updated_at: "", body: `note ${id}`,
});

describe("document buffers", () => {
  it("keeps insertion order and deduplicates documents", () => {
    const buffers = addBuffer(addBuffer(addBuffer([], document(1)), document(2)), document(1));
    expect(buffers.map((buffer) => buffer.document.id)).toEqual([1, 2]);
  });

  it("preserves the buffered body, save state, selection, and scroll when reopened", () => {
    const [first] = addBuffer([], document(1));
    const buffered = {
      ...first,
      document: { ...first.document, body: "unsaved session body" },
      saveState: { kind: "saving" as const },
      editor: { anchor: 3, head: 8, scrollTop: 240 },
    };
    const [reopened] = addBuffer([buffered], { ...document(1), body: "older database body" });
    expect(reopened.document.body).toBe("unsaved session body");
    expect(reopened.saveState).toEqual({ kind: "saving" });
    expect(reopened.editor).toEqual({ anchor: 3, head: 8, scrollTop: 240 });
  });

  it("wraps navigation and chooses the nearest survivor", () => {
    const buffers = [document(1), document(2), document(3)].reduce(addBuffer, []);
    expect(adjacentBufferId(buffers, 1, -1)).toBe(3);
    expect(adjacentBufferId(buffers, 3, 1)).toBe(1);
    expect(removeBuffer(buffers, 2).nextId).toBe(3);
    expect(removeBuffer(buffers, 3).nextId).toBe(2);
  });
});
