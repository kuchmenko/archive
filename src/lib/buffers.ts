import type { Document } from "./archive";
import type { SaveState } from "./autosave";

export type EditorSnapshot = {
  anchor: number;
  head: number;
  scrollTop: number;
};

export type DocumentBuffer = {
  document: Document;
  saveState: SaveState;
  editor: EditorSnapshot;
};

export const emptyEditorSnapshot: EditorSnapshot = { anchor: 0, head: 0, scrollTop: 0 };

export function addBuffer(buffers: DocumentBuffer[], document: Document): DocumentBuffer[] {
  const index = buffers.findIndex((buffer) => buffer.document.id === document.id);
  if (index >= 0) {
    return buffers.map((buffer, position) =>
      position === index ? { ...buffer, document: { ...document, body: buffer.document.body } } : buffer,
    );
  }
  return [...buffers, { document, saveState: { kind: "saved" }, editor: emptyEditorSnapshot }];
}

export function removeBuffer(buffers: DocumentBuffer[], id: number): {
  buffers: DocumentBuffer[];
  nextId: number | null;
} {
  const index = buffers.findIndex((buffer) => buffer.document.id === id);
  if (index < 0) return { buffers, nextId: null };
  const next = buffers.filter((buffer) => buffer.document.id !== id);
  return { buffers: next, nextId: next[Math.min(index, next.length - 1)]?.document.id ?? null };
}

export function adjacentBufferId(
  buffers: DocumentBuffer[],
  activeId: number,
  direction: -1 | 1,
): number | null {
  if (buffers.length < 2) return null;
  const index = buffers.findIndex((buffer) => buffer.document.id === activeId);
  if (index < 0) return null;
  return buffers[(index + direction + buffers.length) % buffers.length].document.id;
}
