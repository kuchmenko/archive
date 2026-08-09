import { describe, expect, it } from "vitest";
import type { Document } from "./archive";
import {
  documentLabel,
  escapeReferenceLabel,
  formatNoteReference,
  insertAt,
  noteReferenceAt,
  noteTitle,
  parseNoteReferences,
} from "./documents";

const note = (body: string): Document => ({
  id: 42,
  kind: "note",
  visibility: "shared",
  author: "user",
  day: "2026-08-03",
  created_at: "2026-08-03T10:00:00Z",
  updated_at: "2026-08-03T10:00:00Z",
  body,
  revision: 1,
});

describe("document labels", () => {
  it("derives a standalone title from the first non-empty Markdown line", () => {
    expect(noteTitle("\n  ## Heading  \nbody")).toBe("Heading");
    expect(noteTitle("#tag")).toBe("#tag");
    expect(noteTitle("\n \n")).toBe("Untitled note");
  });

  it("uses project-specific empty labels and Markdown titles", () => {
    expect(documentLabel({ ...note(""), kind: "project" })).toBe("Untitled project");
    expect(documentLabel({ ...note("\n## Archive foundation\nDetails"), kind: "project" })).toBe("Archive foundation");
    expect(documentLabel(note(""))).toBe("Untitled note");
  });
});

describe("note references", () => {
  it("escapes reserved label characters and line breaks", () => {
    expect(escapeReferenceLabel("A | B]\\\nC")).toBe("A \\| B\\]\\\\ C");
    const reference = formatNoteReference(note("# A | B]\\\nC"));
    expect(reference).toBe("[[note:42|A \\| B\\]\\\\]]");
    expect(parseNoteReferences(reference)[0]).toMatchObject({ id: 42, label: "A | B]\\" });
  });

  it("finds a valid reference only when the cursor is inside its complete syntax", () => {
    const body = "before [[note:42|Readable]] after [[note:x|bad]]";
    const reference = parseNoteReferences(body)[0];
    expect(noteReferenceAt(body, reference.from + 4)?.id).toBe(42);
    expect(noteReferenceAt(body, reference.to)).toBeNull();
    expect(noteReferenceAt(body, 0)).toBeNull();
    expect(parseNoteReferences("[[note:42|broken]")).toEqual([]);
  });

  it("inserts at the exact bounded cursor position", () => {
    expect(insertAt("abcd", 2, "[[note:42|Title]]")).toBe("ab[[note:42|Title]]cd");
    expect(insertAt("abcd", 99, "x")).toBe("abcdx");
  });
});
