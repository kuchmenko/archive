import type { EditorView } from "@codemirror/view";
import { Vim } from "@replit/codemirror-vim";

export type ArchiveVimActions = {
  openExplorer: () => void;
  newSharedNote: () => void;
  newPrivateNote: () => void;
  openCommandPalette: () => void;
  openReference: () => void;
  previousBuffer: () => void;
  nextBuffer: () => void;
};

const actionsByView = new WeakMap<EditorView, ArchiveVimActions>();
let registered = false;

const mappings = [
  ["<Leader><Space>", "archive.openExplorer", "openExplorer"],
  ["<Leader>n", "archive.newSharedNote", "newSharedNote"],
  ["<Leader>N", "archive.newPrivateNote", "newPrivateNote"],
  ["<Leader>c", "archive.openCommandPalette", "openCommandPalette"],
  ["gf", "archive.openReference", "openReference"],
  ["[b", "archive.previousBuffer", "previousBuffer"],
  ["]b", "archive.nextBuffer", "nextBuffer"],
] as const;

function registerActions() {
  if (registered) return;
  registered = true;
  Vim.map("<Space>", "<Leader>", "normal");
  for (const [keys, name, action] of mappings) {
    Vim.defineAction(name, (adapter) => {
      const view = (adapter as typeof adapter & { cm6?: EditorView }).cm6;
      if (view) actionsByView.get(view)?.[action]();
    });
    Vim.mapCommand(keys, "action", name, {}, { context: "normal" });
  }
}

export function registerArchiveVimActions(view: EditorView, actions: ArchiveVimActions) {
  registerActions();
  actionsByView.set(view, actions);
  return () => actionsByView.delete(view);
}
