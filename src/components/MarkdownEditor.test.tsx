import { act, render } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import { createRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  MarkdownEditor,
  selectionIntersectsReference,
  type MarkdownEditorHandle,
} from "./MarkdownEditor";

const vimMock = vi.hoisted(() => ({
  modeListener: null as ((event: { mode: string; subMode?: string }) => void) | null,
  actions: new Map<string, (adapter: unknown) => void>(),
  currentView: null as unknown,
}));
const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

vi.mock("@replit/codemirror-vim", () => ({
  Vim: {
    defineAction: (name: string, action: (adapter: unknown) => void) => vimMock.actions.set(name, action),
    map: vi.fn(),
    mapCommand: vi.fn(),
  },
  getCM: () => ({
    on: (_event: string, listener: (event: { mode: string; subMode?: string }) => void) => {
      vimMock.modeListener = listener;
    },
    off: () => {
      vimMock.modeListener = null;
    },
  }),
  vim: () => EditorView.updateListener.of((update) => { vimMock.currentView = update.view; }),
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: vi.fn(),
  writeText: vi.fn(),
}));

describe("MarkdownEditor application shortcuts", () => {
  beforeEach(() => {
    vimMock.modeListener = null;
    vimMock.currentView = null;
    Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
    Range.prototype.getBoundingClientRect = () => new DOMRect();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      queueMicrotask(() => callback(0));
      return 0;
    });
  });

  it("routes captured actions and uses current callback props", () => {
    const onNewNote = vi.fn(() => true);
    const nextOnNewNote = vi.fn(() => true);
    const onOpenExplorer = vi.fn(() => true);
    const { rerender } = render(
      <MarkdownEditor
        entryId={7}
        body=""
        active
        readOnly={false}
        onChange={vi.fn()}
        onModeChange={vi.fn()}
        onNewNote={onNewNote}
        onNewPrivateNote={vi.fn(() => true)}
        onOpenCommands={vi.fn(() => true)}
        onOpenExplorer={onOpenExplorer}
        references={[]}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
        onOpenReference={vi.fn(() => true)}
      />,
    );
    vimMock.actions.get("archive.openExplorer")?.({ cm6: vimMock.currentView });
    vimMock.actions.get("archive.newSharedNote")?.({ cm6: vimMock.currentView });
    expect(onOpenExplorer).toHaveBeenCalledWith({ documentId: 7, cursor: 0 });
    expect(onNewNote).toHaveBeenCalledOnce();
    rerender(
      <MarkdownEditor
        entryId={7}
        body=""
        active
        readOnly={false}
        onChange={vi.fn()}
        onModeChange={vi.fn()}
        onNewNote={nextOnNewNote}
        onNewPrivateNote={() => false}
        onOpenCommands={() => false}
        onOpenExplorer={() => false}
        references={[]}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
        onOpenReference={() => false}
      />,
    );
    vimMock.actions.get("archive.newSharedNote")?.({ cm6: vimMock.currentView });
    expect(onNewNote).toHaveBeenCalledOnce();
    expect(nextOnNewNote).toHaveBeenCalledOnce();
  });

  it("opens the reference at the invoking view's current document and cursor", () => {
    const editorRef = createRef<MarkdownEditorHandle>();
    const onOpenReference = vi.fn(() => true);
    render(
      <MarkdownEditor
        ref={editorRef}
        entryId={7}
        body="before [[note:9|Target]]"
        active
        readOnly={false}
        onChange={vi.fn()}
        onModeChange={vi.fn()}
        onNewNote={() => false}
        onNewPrivateNote={() => false}
        onOpenCommands={() => false}
        onOpenExplorer={() => false}
        references={[]}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
        onOpenReference={onOpenReference}
      />,
    );
    act(() => editorRef.current?.focus(12));
    vimMock.actions.get("archive.openReference")?.({ cm6: vimMock.currentView });
    expect(onOpenReference).toHaveBeenCalledWith(9);
  });

  it("reports mode with document identity and rejects inactive focus and insertion", async () => {
    const editorRef = createRef<MarkdownEditorHandle>();
    const onModeChange = vi.fn();
    const { container, rerender } = render(
      <MarkdownEditor
        ref={editorRef}
        entryId={7}
        body="body"
        active={false}
        readOnly
        onChange={vi.fn()}
        onModeChange={onModeChange}
        onNewNote={() => false}
        onNewPrivateNote={() => false}
        onOpenCommands={() => false}
        onOpenExplorer={() => false}
        references={[]}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
        onOpenReference={() => false}
      />,
    );
    act(() => {
      editorRef.current?.focus();
      editorRef.current?.insertAt(0, "stale ");
    });
    expect(container.querySelector(".cm-content")?.textContent).toContain("body");
    expect(document.activeElement).not.toBe(container.querySelector(".cm-content"));
    expect(onModeChange).not.toHaveBeenCalled();

    rerender(
      <MarkdownEditor
        ref={editorRef}
        entryId={7}
        body="body"
        active
        readOnly={false}
        onChange={vi.fn()}
        onModeChange={onModeChange}
        onNewNote={() => false}
        onNewPrivateNote={() => false}
        onOpenCommands={() => false}
        onOpenExplorer={() => false}
        references={[]}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
        onOpenReference={() => false}
      />,
    );
    await act(async () => Promise.resolve());
    expect(onModeChange).toHaveBeenCalledWith(7, "NORMAL");
  });

  it("cancels the mount frame before a disposed editor can restore selection or outer scroll", () => {
    const frames = new Map<number, FrameRequestCallback>();
    const cancel = vi.fn((id: number) => frames.delete(id));
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.set(11, callback);
      return 11;
    });
    vi.stubGlobal("cancelAnimationFrame", cancel);
    const { container, unmount } = render(
      <main>
        <MarkdownEditor
          entryId={7}
          body="body"
          active={false}
          readOnly
          onChange={vi.fn()}
          onModeChange={vi.fn()}
          onNewNote={() => false}
          onNewPrivateNote={() => false}
          onOpenCommands={() => false}
          onOpenExplorer={() => false}
          references={[]}
          initialSnapshot={{ anchor: 3, head: 3, scrollTop: 80 }}
          onPreviousBuffer={() => false}
          onNextBuffer={() => false}
          onOpenReference={() => false}
        />
      </main>,
    );
    const view = EditorView.findFromDOM(container.querySelector(".cm-editor") as HTMLElement)!;
    const frame = frames.get(11)!;
    expect(view.state.selection.main.head).toBe(0);
    unmount();
    frame(0);
    expect(cancel).toHaveBeenCalledWith(11);
    expect(container.querySelector("main")?.scrollTop ?? 0).toBe(0);
  });

  it("collapses resolved and broken references until selection intersects their source", async () => {
    const editorRef = createRef<MarkdownEditorHandle>();
    const { container } = render(
      <MarkdownEditor
        ref={editorRef}
        entryId={1}
        body="[[note:2|Stored title]] and [[note:3|Deleted title]] [[note:4|Old daily label]]"
        active
        readOnly={false}
        onChange={vi.fn()}
        onModeChange={vi.fn()}
        onNewNote={() => false}
        onNewPrivateNote={() => false}
        onOpenCommands={() => false}
        onOpenExplorer={() => false}
        onOpenReference={() => true}
        references={[
          { id: 2, kind: "note", day: "2026-08-03", label: "Current title" },
          { id: 4, kind: "daily", day: "2026-08-04", label: "2026-08-04" },
        ]}
        initialSnapshot={{ anchor: 25, head: 25, scrollTop: 0 }}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
      />,
    );

    await act(async () => Promise.resolve());
    expect(container.querySelector(".cm-reference-note")?.textContent).toContain("Current title");
    expect(container.querySelector(".cm-reference-broken")?.textContent).toContain("Deleted title");
    expect(container.querySelector(".cm-reference-daily")?.textContent).toContain("Tuesday, August 4, 2026");
    expect(container.querySelector(".cm-reference-note")?.getAttribute("aria-label")).toContain("Open note");

    act(() => editorRef.current?.focus(5));
    expect(container.querySelector(".cm-reference-note")).toBeNull();
    expect(container.querySelector(".cm-content")?.textContent).toContain("[[note:2|Stored title]]");
    expect(container.querySelector(".cm-reference-broken")).not.toBeNull();
  });

  it("keeps external document replacements out of undo history and change callbacks", async () => {
    const editorRef = createRef<MarkdownEditorHandle>();
    const onChange = vi.fn();
    const { container } = render(
      <MarkdownEditor
        ref={editorRef}
        entryId={1}
        body="local body"
        active
        readOnly={false}
        onChange={onChange}
        onModeChange={vi.fn()}
        onNewNote={() => false}
        onNewPrivateNote={() => false}
        onOpenCommands={() => false}
        onOpenExplorer={() => false}
        references={[]}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
        onOpenReference={() => false}
      />,
    );

    act(() => editorRef.current?.replaceBody("remote body"));
    container.querySelector(".cm-editor")!.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ctrlKey: true, key: "z" }),
    );
    await act(async () => Promise.resolve());
    expect(container.querySelector(".cm-content")?.textContent).toContain("remote body");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("scrolls the page container to the top when the cursor moves to document start", async () => {
    const body = `${"line\n".repeat(80)}end`;
    const ref = createRef<MarkdownEditorHandle>();
    const { container } = render(
      <main style={{ height: 200, overflow: "auto" }}>
        <div style={{ height: 80 }}>heading</div>
        <MarkdownEditor
          ref={ref}
          entryId={1}
          body={body}
          active
          readOnly={false}
          onChange={vi.fn()}
          onModeChange={vi.fn()}
          onNewNote={() => false}
          onNewPrivateNote={() => false}
          onOpenCommands={() => false}
          onOpenExplorer={() => false}
          references={[]}
          onPreviousBuffer={() => false}
          onNextBuffer={() => false}
          onOpenReference={() => false}
        />
      </main>,
    );
    const page = container.querySelector("main") as HTMLElement;
    Object.defineProperty(page, "scrollTop", {
      configurable: true,
      writable: true,
      value: 240,
    });

    await act(async () => {
      ref.current?.focus(body.length);
      await Promise.resolve();
    });
    expect(page.scrollTop).toBe(240);

    await act(async () => {
      ref.current?.focus(0);
      await Promise.resolve();
    });
    expect(page.scrollTop).toBe(0);
  });
});

describe("reference selection boundaries", () => {
  const reference = { from: 4, to: 12 };

  it("reveals only cursors and selections inside the source range", () => {
    expect(selectionIntersectsReference({ from: 4, to: 4 }, reference)).toBe(true);
    expect(selectionIntersectsReference({ from: 11, to: 11 }, reference)).toBe(true);
    expect(selectionIntersectsReference({ from: 12, to: 12 }, reference)).toBe(false);
    expect(selectionIntersectsReference({ from: 0, to: 4 }, reference)).toBe(false);
    expect(selectionIntersectsReference({ from: 0, to: 5 }, reference)).toBe(true);
  });
});

describe("Mermaid blocks", () => {
  const body = "before\n```mermaid title=Main\ngraph TD\nA-->B\n```\nafter";
  const props = {
    entryId: 1,
    body,
    active: true,
    readOnly: false,
    onChange: vi.fn(),
    onModeChange: vi.fn(),
    onNewNote: () => false,
    onNewPrivateNote: () => false,
    onOpenCommands: () => false,
    onOpenExplorer: () => false,
    onOpenReference: () => false,
    references: [],
    onPreviousBuffer: () => false,
    onNextBuffer: () => false,
  };

  beforeEach(() => {
    invokeMock.mockReset();
    Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
    Range.prototype.getBoundingClientRect = () => new DOMRect();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      queueMicrotask(() => callback(0));
      return 0;
    });
  });

  it("renders an inactive diagram and reveals source by click and keyboard", async () => {
    invokeMock.mockResolvedValue({ valid: true, diagram_type: "flowchart", diagnostics: [], svg: '<svg viewBox="0 0 10 10"><path d="M0 0L10 10"/></svg>' });
    const ref = createRef<MarkdownEditorHandle>();
    const { container } = render(<MarkdownEditor ref={ref} {...props} />);
    await act(async () => Promise.resolve());
    const diagram = container.querySelector("button.cm-mermaid") as HTMLButtonElement;
    expect(diagram.querySelector("svg")).not.toBeNull();
    act(() => diagram.click());
    expect(container.querySelector(".cm-mermaid-preview")).not.toBeNull();
    expect(container.querySelector(".cm-content")?.textContent).toContain("```mermaid");

    act(() => ref.current?.focus(body.length));
    const keyboardDiagram = container.querySelector("button.cm-mermaid") as HTMLButtonElement;
    keyboardDiagram.focus();
    keyboardDiagram.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(container.querySelector(".cm-mermaid-preview")).not.toBeNull();
    expect(container.querySelector(".cm-content")?.textContent).toContain("```mermaid");
  });

  it("keeps invalid source visible with a local diagnostic", async () => {
    invokeMock.mockResolvedValue({ valid: false, diagnostics: [{ line: 2, column: 3, message: "Expected arrow" }] });
    const { container } = render(<MarkdownEditor {...props} />);
    await act(async () => Promise.resolve());
    expect(container.querySelector(".cm-content")?.textContent).toContain("graph TD");
    expect(container.querySelector(".cm-mermaid-diagnostic")?.textContent).toBe("Line 2:3: Expected arrow");
  });

  it("suppresses a response after the source changes", async () => {
    let resolveFirst!: (value: unknown) => void;
    invokeMock.mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }));
    const ref = createRef<MarkdownEditorHandle>();
    const { container } = render(<MarkdownEditor ref={ref} {...props} />);
    act(() => ref.current?.insertAt(body.indexOf("A-->B") + 1, "X"));
    await act(async () => resolveFirst({ valid: true, diagnostics: [], svg: '<svg data-stale="true"/>' }));
    expect(container.querySelector('[data-stale="true"]')).toBeNull();
  });
});
