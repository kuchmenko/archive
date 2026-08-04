import { syntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";

export type MermaidBlock = {
  from: number;
  to: number;
  source: string;
};

export function mermaidBlocks(state: EditorState): MermaidBlock[] {
  const blocks: MermaidBlock[] = [];
  const cursor = syntaxTree(state).cursor();
  do {
    if (cursor.name !== "FencedCode") continue;
    const blockFrom = cursor.from;
    const blockTo = cursor.to;
    if (!cursor.firstChild()) continue;
    let info = "";
    let sourceFrom = -1;
    let sourceTo = -1;
    do {
      const childName: string = cursor.name;
      if (childName === "CodeInfo") info = state.sliceDoc(cursor.from, cursor.to).trim();
      if (childName === "CodeText") {
        if (sourceFrom < 0) sourceFrom = cursor.from;
        sourceTo = cursor.to;
      }
    } while (cursor.nextSibling());
    cursor.parent();
    if (info.split(/\s+/, 1)[0]?.toLowerCase() !== "mermaid") continue;
    blocks.push({
      from: blockFrom,
      to: blockTo,
      source: sourceFrom < 0 ? "" : state.sliceDoc(sourceFrom, sourceTo),
    });
  } while (cursor.next());
  return blocks;
}

export function mermaidBlockKey(block: MermaidBlock) {
  return `${block.from}:${block.to}:${block.source}`;
}

export function selectionIntersectsBlock(
  selection: { from: number; to: number },
  block: { from: number; to: number },
) {
  return selection.from === selection.to
    ? selection.from >= block.from && selection.from < block.to
    : selection.from < block.to && selection.to > block.from;
}
