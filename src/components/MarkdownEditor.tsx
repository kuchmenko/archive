import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { Compartment, EditorState, RangeSetBuilder } from "@codemirror/state";
import { Decoration, drawSelection, EditorView, keymap, ViewPlugin, WidgetType } from "@codemirror/view";
import { getCM, vim } from "@replit/codemirror-vim";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { noteReferenceAt, parseNoteReferences } from "@/lib/documents";
import type { ReferenceSummary } from "@/lib/archive";
import type { EditorSnapshot } from "@/lib/buffers";
import { formatLocalDay } from "@/lib/date";
import { appShortcut } from "@/lib/shortcuts";
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";

export type MarkdownEditorHandle = {
  focus: (position?: number) => void;
  insertAt: (position: number, text: string) => void;
  snapshot: () => EditorSnapshot | null;
};

export type ExplorerOrigin = {
  documentId: number;
  cursor: number;
};

type MarkdownEditorProps = {
  entryId: number;
  body: string;
  readOnly: boolean;
  onChange: (entryId: number, body: string) => void;
  onClipboardError: (message: string) => void;
  onModeChange: (mode: string) => void;
  onNewNote: () => boolean;
  onNewPrivateNote: () => boolean;
  onOpenCommands: () => boolean;
  onOpenExplorer: (origin: ExplorerOrigin) => boolean;
  onOpenReference: (id: number) => boolean;
  references: ReferenceSummary[];
  initialSnapshot?: EditorSnapshot;
  onPreviousBuffer: () => boolean;
  onNextBuffer: () => boolean;
};

class ReferenceWidget extends WidgetType {
  constructor(
    readonly id: number,
    readonly label: string,
    readonly kind: "daily" | "note" | "artifact" | "broken",
    readonly open: (id: number) => boolean,
  ) { super(); }

  eq(other: ReferenceWidget) {
    return this.id === other.id && this.label === other.label && this.kind === other.kind;
  }

  toDOM() {
    const link = document.createElement("button");
    link.type = "button";
    link.className = `cm-reference cm-reference-${this.kind}`;
    link.textContent = `${this.kind === "daily" ? "▦" : this.kind === "broken" ? "×" : "◇"} ${this.label}`;
    link.title = this.kind === "broken"
      ? `Missing note: ${this.label}`
      : `Open ${this.kind === "daily" ? "daily document" : this.kind}: ${this.label}`;
    link.setAttribute("aria-label", link.title);
    link.onclick = () => this.open(this.id);
    return link;
  }
}

export function selectionIntersectsReference(
  selection: { from: number; to: number },
  reference: { from: number; to: number },
) {
  return selection.from === selection.to
    ? selection.from >= reference.from && selection.from < reference.to
    : selection.from < reference.to && selection.to > reference.from;
}

function referenceDecorations(references: ReferenceSummary[], open: (id: number) => boolean) {
  const resolved = new Map(references.map((reference) => [reference.id, reference]));
  return ViewPlugin.fromClass(class {
    decorations;
    constructor(view: EditorView) { this.decorations = this.build(view); }
    update(update: { docChanged: boolean; selectionSet: boolean; view: EditorView }) {
      if (update.docChanged || update.selectionSet) this.decorations = this.build(update.view);
    }
    build(view: EditorView) {
      const builder = new RangeSetBuilder<Decoration>();
      for (const reference of parseNoteReferences(view.state.doc.toString())) {
        if (view.state.selection.ranges.some((range) => selectionIntersectsReference(range, reference))) continue;
        const target = resolved.get(reference.id);
        builder.add(reference.from, reference.to, Decoration.replace({
          widget: new ReferenceWidget(
            reference.id,
            target?.kind === "daily"
              ? formatLocalDay(target.day)
              : target?.label || reference.label || `Missing document ${reference.id}`,
            target?.kind ?? "broken",
            open,
          ),
        }));
      }
      return builder.finish();
    }
  }, { decorations: (plugin) => plugin.decorations });
}

export const MarkdownEditor = forwardRef<MarkdownEditorHandle, MarkdownEditorProps>(function MarkdownEditor(
  {
    entryId,
    body,
    readOnly,
    onChange,
    onClipboardError,
    onModeChange,
    onNewNote,
    onNewPrivateNote,
    onOpenCommands,
    onOpenExplorer,
    onOpenReference,
    references,
    initialSnapshot,
    onPreviousBuffer,
    onNextBuffer,
  },
  ref,
) {
  const mount = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const readOnlyCompartment = useRef(new Compartment()).current;
  const referencesCompartment = useRef(new Compartment()).current;
  const onChangeRef = useRef(onChange);
  const onClipboardErrorRef = useRef(onClipboardError);
  const onModeChangeRef = useRef(onModeChange);
  const onNewNoteRef = useRef(onNewNote);
  const onNewPrivateNoteRef = useRef(onNewPrivateNote);
  const onOpenCommandsRef = useRef(onOpenCommands);
  const onOpenExplorerRef = useRef(onOpenExplorer);
  const onOpenReferenceRef = useRef(onOpenReference);
  const onPreviousBufferRef = useRef(onPreviousBuffer);
  const onNextBufferRef = useRef(onNextBuffer);
  onChangeRef.current = onChange;
  onClipboardErrorRef.current = onClipboardError;
  onModeChangeRef.current = onModeChange;
  onNewNoteRef.current = onNewNote;
  onNewPrivateNoteRef.current = onNewPrivateNote;
  onOpenCommandsRef.current = onOpenCommands;
  onOpenExplorerRef.current = onOpenExplorer;
  onOpenReferenceRef.current = onOpenReference;
  onPreviousBufferRef.current = onPreviousBuffer;
  onNextBufferRef.current = onNextBuffer;

  useImperativeHandle(ref, () => ({
    focus(position) {
      const view = viewRef.current;
      if (!view) return;
      if (position !== undefined) {
        const cursor = Math.max(0, Math.min(position, view.state.doc.length));
        view.dispatch({ selection: { anchor: cursor } });
      }
      view.focus();
    },
    insertAt(position, text) {
      const view = viewRef.current;
      if (!view) return;
      const cursor = Math.max(0, Math.min(position, view.state.doc.length));
      view.dispatch({
        changes: { from: cursor, insert: text },
        selection: { anchor: cursor + text.length },
      });
      view.focus();
    },
    snapshot() {
      const view = viewRef.current;
      if (!view) return null;
      const { anchor, head } = view.state.selection.main;
      return { anchor, head, scrollTop: mount.current?.closest("main")?.scrollTop ?? 0 };
    },
  }));

  useEffect(() => {
    if (!mount.current) return;
    let disposed = false;
    let vimMode = "normal";
    let pendingSpace = false;
    let spaceTimer: ReturnType<typeof setTimeout> | null = null;

    const clipboardError = (error: unknown) => {
      if (disposed) return;
      onClipboardErrorRef.current(error instanceof Error ? error.message : String(error));
    };
    const view = new EditorView({
      parent: mount.current,
      state: EditorState.create({
        doc: body,
        extensions: [
          vim(),
          history(),
          drawSelection(),
          readOnlyCompartment.of(EditorState.readOnly.of(readOnly)),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          markdown(),
          referencesCompartment.of(referenceDecorations(references, (id) => onOpenReferenceRef.current(id))),
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onChangeRef.current(entryId, update.state.doc.toString());
          }),
          EditorView.theme({
            "&": { minHeight: "160px", backgroundColor: "transparent", color: "#ece8df" },
            ".cm-scroller": {
              overflow: "visible",
              fontFamily: '"Iosevka", "JetBrains Mono", "SFMono-Regular", Consolas, monospace',
              fontSize: "16px",
              lineHeight: "1.75",
            },
            ".cm-content": { caretColor: "#e9a95b", padding: "8px 0 48px" },
            ".cm-line": { padding: "0" },
            ".cm-cursor, .cm-dropCursor": { borderLeftColor: "#e9a95b" },
            ".cm-selectionBackground": {
              backgroundColor: "#394253",
            },
            "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
              backgroundColor: "#59677d",
            },
            "&:not(.cm-focused) > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
              backgroundColor: "#394253",
            },
            "&.cm-focused": { outline: "none" },
            ".cm-reference": { color: "#c6bda9", textDecoration: "underline", textDecorationColor: "#5d6675", textUnderlineOffset: "3px", cursor: "pointer" },
            ".cm-reference-daily": { color: "#b8c7d9" },
            ".cm-reference-broken": { color: "#bf7777", textDecorationStyle: "dotted" },
          }),
        ],
      }),
    });
    viewRef.current = view;
    const appKeydown = (event: KeyboardEvent) => {
      if (
        vimMode === "normal" && !event.ctrlKey && !event.metaKey && !event.altKey && event.shiftKey &&
        (event.key === "H" || event.key === "L")
      ) {
        event.key === "H" ? onPreviousBufferRef.current() : onNextBufferRef.current();
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (
        vimMode === "normal" &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey &&
        !event.shiftKey &&
        event.key === " "
      ) {
        event.preventDefault();
        event.stopPropagation();
        if (pendingSpace) {
          pendingSpace = false;
          if (spaceTimer !== null) clearTimeout(spaceTimer);
          spaceTimer = null;
          onOpenExplorerRef.current({ documentId: entryId, cursor: view.state.selection.main.head });
        } else {
          pendingSpace = true;
          spaceTimer = setTimeout(() => {
            pendingSpace = false;
            spaceTimer = null;
          }, 500);
        }
        return;
      }
      if (pendingSpace) {
        pendingSpace = false;
        if (spaceTimer !== null) clearTimeout(spaceTimer);
        spaceTimer = null;
      }

      if (
        vimMode === "normal" &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey &&
        event.key === "Enter"
      ) {
        const reference = noteReferenceAt(view.state.doc.toString(), view.state.selection.main.head);
        if (reference && onOpenReferenceRef.current(reference.id)) {
          event.preventDefault();
          event.stopPropagation();
          return;
        }
      }

      const shortcut = appShortcut(event);
      const handled =
        shortcut === "new-note"
          ? onNewNoteRef.current()
          : shortcut === "new-private-note"
            ? onNewPrivateNoteRef.current()
            : shortcut === "open-commands"
              ? onOpenCommandsRef.current()
              : false;
      if (handled) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }

      if ((!event.ctrlKey && !event.metaKey) || event.altKey) return;
      const key = event.key.toLowerCase();
      const selection = view.state.selection.main;
      if ((key === "c" || key === "x") && selection.empty) return;
      if (key !== "c" && key !== "x" && key !== "v") return;
      event.preventDefault();
      event.stopPropagation();

      if (key === "c") {
        void writeText(view.state.sliceDoc(selection.from, selection.to)).catch(clipboardError);
      } else if (key === "x") {
        const { from, to } = selection;
        const selected = view.state.sliceDoc(from, to);
        const originalDocument = view.state.doc;
        void writeText(selected)
          .then(() => {
            if (disposed) return;
            if (view.state.doc !== originalDocument) {
              throw new Error("Selection changed; cut was canceled");
            }
            view.dispatch({ changes: { from, to, insert: "" } });
          })
          .catch(clipboardError);
      } else {
        void readText()
          .then((text) => {
            if (!disposed) view.dispatch(view.state.replaceSelection(text));
          })
          .catch(clipboardError);
      }
    };
    view.dom.addEventListener("keydown", appKeydown, { capture: true });
    const cm = getCM(view);
    const modeChanged = (event: { mode: string; subMode?: string }) => {
      vimMode = event.mode.toLowerCase();
      pendingSpace = false;
      if (spaceTimer !== null) clearTimeout(spaceTimer);
      spaceTimer = null;
      const subMode = event.subMode ? ` ${event.subMode}` : "";
      onModeChangeRef.current(`${event.mode}${subMode}`.toUpperCase());
    };
    cm?.on("vim-mode-change", modeChanged);

    onModeChangeRef.current("NORMAL");
    requestAnimationFrame(() => {
      if (initialSnapshot) {
        const anchor = Math.min(initialSnapshot.anchor, view.state.doc.length);
        const head = Math.min(initialSnapshot.head, view.state.doc.length);
        view.dispatch({ selection: { anchor, head } });
        const scrollContainer = mount.current?.closest("main");
        if (scrollContainer) scrollContainer.scrollTop = initialSnapshot.scrollTop;
      }
      view.focus();
    });
    return () => {
      disposed = true;
      viewRef.current = null;
      if (spaceTimer !== null) clearTimeout(spaceTimer);
      view.dom.removeEventListener("keydown", appKeydown, { capture: true });
      cm?.off("vim-mode-change", modeChanged);
      view.destroy();
    };
  }, []);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: readOnlyCompartment.reconfigure(EditorState.readOnly.of(readOnly)),
    });
  }, [readOnly, readOnlyCompartment]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: referencesCompartment.reconfigure(
        referenceDecorations(references, (id) => onOpenReferenceRef.current(id)),
      ),
    });
  }, [references, referencesCompartment]);

  return <div ref={mount} className="w-full" aria-label="Note body" />;
});
