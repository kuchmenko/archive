import type { Document } from "./archive";
import { formatLocalDay } from "./date";

export type NoteReference = {
  from: number;
  to: number;
  id: number;
  label: string;
};

export function noteTitle(body: string): string {
  const line = body.split(/\r?\n/).find((candidate) => candidate.trim());
  if (!line) return "Untitled note";
  return line.trim().replace(/^#{1,6}(?:\s+|$)/, "").trim() || "Untitled note";
}

export function documentLabel(document: Document): string {
  if (document.kind === "daily") return formatLocalDay(document.day);
  const title = noteTitle(document.body);
  return document.kind === "project" && title === "Untitled note" ? "Untitled project" : title;
}

export function escapeReferenceLabel(label: string): string {
  return label
    .replace(/\r\n?|\n/g, " ")
    .replace(/\\/g, "\\\\")
    .replace(/\|/g, "\\|")
    .replace(/\]/g, "\\]");
}

export function formatNoteReference(document: Document): string {
  return `[[note:${document.id}|${escapeReferenceLabel(documentLabel(document))}]]`;
}

export function parseNoteReferences(body: string): NoteReference[] {
  const references: NoteReference[] = [];
  for (let start = 0; start < body.length - 8; start += 1) {
    if (!body.startsWith("[[note:", start)) continue;
    let cursor = start + 7;
    const idStart = cursor;
    while (cursor < body.length && /\d/.test(body[cursor])) cursor += 1;
    if (cursor === idStart || body[cursor] !== "|") continue;
    const id = Number(body.slice(idStart, cursor));
    if (!Number.isSafeInteger(id) || id <= 0) continue;
    cursor += 1;
    let label = "";
    let closed = false;
    while (cursor < body.length) {
      const character = body[cursor];
      if (character === "\n" || character === "\r") break;
      if (character === "\\") {
        const escaped = body[cursor + 1];
        if (escaped !== "\\" && escaped !== "|" && escaped !== "]") break;
        label += escaped;
        cursor += 2;
        continue;
      }
      if (character === "]" && body[cursor + 1] === "]") {
        references.push({ from: start, to: cursor + 2, id, label });
        start = cursor + 1;
        closed = true;
        break;
      }
      if (character === "|") break;
      label += character;
      cursor += 1;
    }
    if (!closed) continue;
  }
  return references;
}

export function noteReferenceAt(body: string, position: number): NoteReference | null {
  return (
    parseNoteReferences(body).find(
      (reference) => position >= reference.from && position < reference.to,
    ) ?? null
  );
}

export function insertAt(body: string, position: number, text: string): string {
  const offset = Math.max(0, Math.min(position, body.length));
  return body.slice(0, offset) + text + body.slice(offset);
}
