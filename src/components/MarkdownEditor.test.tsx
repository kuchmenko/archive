import { act, render } from "@testing-library/react";
import { createRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  MarkdownEditor,
  selectionIntersectsReference,
  type MarkdownEditorHandle,
} from "./MarkdownEditor";

const vimMock = vi.hoisted(() => ({
  modeListener: null as ((event: { mode: string; subMode?: string }) => void) | null,
}));
const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

vi.mock("@replit/codemirror-vim", () => ({
  getCM: () => ({
    on: (_event: string, listener: (event: { mode: string; subMode?: string }) => void) => {
      vimMock.modeListener = listener;
    },
    off: () => {
      vimMock.modeListener = null;
    },
  }),
  vim: () => [],
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: vi.fn(),
  writeText: vi.fn(),
}));

describe("MarkdownEditor application shortcuts", () => {
  beforeEach(() => {
    vimMock.modeListener = null;
    Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
    Range.prototype.getBoundingClientRect = () => new DOMRect();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      queueMicrotask(() => callback(0));
      return 0;
    });
  });

  it("handles Ctrl+N and Ctrl+O before editor keymaps", () => {
    const onNewNote = vi.fn(() => true);
    const onOpenCommands = vi.fn(() => true);
    const { container } = render(
      <MarkdownEditor
        entryId={1}
        body=""
        readOnly={false}
        onChange={vi.fn()}
        onClipboardError={vi.fn()}
        onModeChange={vi.fn()}
        onNewNote={onNewNote}
        onNewPrivateNote={vi.fn(() => true)}
        onOpenCommands={onOpenCommands}
        onOpenExplorer={vi.fn(() => true)}
        references={[]}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
        onOpenReference={vi.fn(() => true)}
      />,
    );
    const editor = container.querySelector(".cm-editor");
    expect(editor).not.toBeNull();

    const newNote = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      ctrlKey: true,
      key: "n",
    });
    const commands = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      ctrlKey: true,
      key: "o",
    });
    editor!.dispatchEvent(newNote);
    editor!.dispatchEvent(commands);

    expect(onNewNote).toHaveBeenCalledOnce();
    expect(onOpenCommands).toHaveBeenCalledOnce();
    expect(newNote.defaultPrevented).toBe(true);
    expect(commands.defaultPrevented).toBe(true);
  });

  it("leaves unhandled shortcuts available to the editor", () => {
    const { container } = render(
      <MarkdownEditor
        entryId={1}
        body=""
        readOnly={false}
        onChange={vi.fn()}
        onClipboardError={vi.fn()}
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
    const event = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      ctrlKey: true,
      key: "n",
    });
    container.querySelector(".cm-editor")!.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });

  it("gates Space Space and reference Enter to NORMAL mode", () => {
    const editorRef = createRef<MarkdownEditorHandle>();
    const onOpenExplorer = vi.fn(() => true);
    const onOpenReference = vi.fn(() => true);
    const { container } = render(
      <MarkdownEditor
        ref={editorRef}
        entryId={7}
        body="before [[note:9|Target]]"
        readOnly={false}
        onChange={vi.fn()}
        onClipboardError={vi.fn()}
        onModeChange={vi.fn()}
        onNewNote={() => false}
        onNewPrivateNote={() => false}
        onOpenCommands={() => false}
        onOpenExplorer={onOpenExplorer}
        references={[]}
        onPreviousBuffer={() => false}
        onNextBuffer={() => false}
        onOpenReference={onOpenReference}
      />,
    );
    const editor = container.querySelector(".cm-editor")!;
    const space = () =>
      editor.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: " " }),
      );

    space();
    space();
    expect(onOpenExplorer).toHaveBeenCalledWith({ documentId: 7, cursor: 0 });

    act(() => vimMock.modeListener?.({ mode: "insert" }));
    space();
    space();
    expect(onOpenExplorer).toHaveBeenCalledOnce();

    act(() => vimMock.modeListener?.({ mode: "visual" }));
    space();
    space();
    expect(onOpenExplorer).toHaveBeenCalledOnce();

    act(() => vimMock.modeListener?.({ mode: "normal" }));
    act(() => editorRef.current?.focus(12));
    editor.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Enter" }),
    );
    expect(onOpenReference).toHaveBeenCalledWith(9);

    act(() => vimMock.modeListener?.({ mode: "insert" }));
    editor.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Enter" }),
    );
    expect(onOpenReference).toHaveBeenCalledOnce();
  });

  it("collapses resolved and broken references until selection intersects their source", async () => {
    const editorRef = createRef<MarkdownEditorHandle>();
    const { container } = render(
      <MarkdownEditor
        ref={editorRef}
        entryId={1}
        body="[[note:2|Stored title]] and [[note:3|Deleted title]] [[note:4|Old daily label]]"
        readOnly={false}
        onChange={vi.fn()}
        onClipboardError={vi.fn()}
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

  it("reserves NORMAL H and L for buffer navigation without handling them in other modes", () => {
    const previous = vi.fn(() => false);
    const next = vi.fn(() => false);
    const { container } = render(
      <MarkdownEditor
        entryId={1}
        body="body"
        readOnly={false}
        onChange={vi.fn()}
        onClipboardError={vi.fn()}
        onModeChange={vi.fn()}
        onNewNote={() => false}
        onNewPrivateNote={() => false}
        onOpenCommands={() => false}
        onOpenExplorer={() => false}
        onOpenReference={() => false}
        references={[]}
        onPreviousBuffer={previous}
        onNextBuffer={next}
      />,
    );
    const editor = container.querySelector(".cm-editor")!;
    const h = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, shiftKey: true, key: "H" });
    const l = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, shiftKey: true, key: "L" });
    editor.dispatchEvent(h);
    editor.dispatchEvent(l);
    expect(previous).toHaveBeenCalledOnce();
    expect(next).toHaveBeenCalledOnce();
    expect(h.defaultPrevented).toBe(true);
    expect(l.defaultPrevented).toBe(true);

    act(() => vimMock.modeListener?.({ mode: "insert" }));
    const insertH = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, shiftKey: true, key: "H" });
    editor.dispatchEvent(insertH);
    expect(previous).toHaveBeenCalledOnce();
    expect(insertH.defaultPrevented).toBe(false);
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
    readOnly: false,
    onChange: vi.fn(),
    onClipboardError: vi.fn(),
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
