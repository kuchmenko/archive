import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { EditorView, ViewPlugin } from "@codemirror/view";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import {
  addDocumentToProject,
  createNote,
  createProject,
  deleteNote,
  dailyNeighbors,
  getAttachmentByArtifactId,
  getDocument,
  getOrCreateDaily,
  listDailyAttachments,
  listUnreviewedAttachments,
  markAttachmentReviewed,
  listProjectDocuments,
  removePresence,
  searchDocuments,
  syncDocument,
  updateDocument,
  updatePresence,
} from "./lib/archive";
import { readText } from "@tauri-apps/plugin-clipboard-manager";

const vimMock = vi.hoisted(() => ({
  actions: new Map<string, (adapter: { cm6: EditorView }) => void>(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    destroy: vi.fn(),
    onCloseRequested: vi.fn().mockResolvedValue(vi.fn()),
  }),
}));

vi.mock("./lib/archive", () => ({
  createNote: vi.fn(),
  createProject: vi.fn(),
  addDocumentToProject: vi.fn(),
  deleteNote: vi.fn(),
  getDocument: vi.fn(),
  getOrCreateDaily: vi.fn(),
  listDailyAttachments: vi.fn(),
  dailyNeighbors: vi.fn().mockResolvedValue({ previous: null, next: null }),
  getAttachmentByArtifactId: vi.fn().mockResolvedValue(null),
  listUnreviewedAttachments: vi.fn().mockResolvedValue([]),
  markAttachmentReviewed: vi.fn(),
  renderMarkdown: vi.fn().mockResolvedValue("<h1>Rendered</h1>"),
  renderMermaid: vi.fn(),
  listProjectDocuments: vi.fn(),
  searchDocuments: vi.fn(),
  resolveReferences: vi.fn().mockResolvedValue([]),
  updateDocument: vi.fn(),
  syncDocument: vi.fn(),
  updatePresence: vi.fn().mockResolvedValue(undefined),
  removePresence: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@replit/codemirror-vim", () => ({
  Vim: {
    defineAction: (name: string, action: (adapter: { cm6: EditorView }) => void) => vimMock.actions.set(name, action),
    map: vi.fn(),
    mapCommand: vi.fn(),
  },
  getCM: () => undefined,
  vim: () => ViewPlugin.define(() => ({})),
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: vi.fn().mockResolvedValue(""),
  writeText: vi.fn(),
}));

const daily = {
  id: 1,
  kind: "daily" as const,
  visibility: "shared" as const,
  author: "user" as const,
  day: "2026-08-03",
  created_at: "2026-08-03T10:00:00.000Z",
  updated_at: "2026-08-03T10:00:00.000Z",
  body: "",
  revision: 1,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function editorView(container: HTMLElement, documentId?: number) {
  const root = documentId === undefined
    ? container.querySelector('[data-editor-active="true"] .cm-editor')
    : container.querySelector(`[data-document-id="${documentId}"] .cm-editor`);
  if (!(root instanceof HTMLElement)) throw new Error(`Editor ${documentId ?? "active"} not found`);
  const view = EditorView.findFromDOM(root);
  if (!view) throw new Error(`EditorView ${documentId ?? "active"} not found`);
  return view;
}

function invokeEditorAction(container: HTMLElement, name: string, documentId?: number) {
  const action = vimMock.actions.get(name);
  if (!action) throw new Error(`Vim action ${name} not found`);
  act(() => action({ cm6: editorView(container, documentId) }));
}

function insertLocally(container: HTMLElement, text: string, documentId?: number) {
  act(() => {
    const view = editorView(container, documentId);
    view.dispatch({ changes: { from: view.state.selection.main.head, insert: text } });
  });
}

describe("Archive document canvas", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 7, 3, 10, 0, 0));
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    Element.prototype.scrollIntoView = vi.fn();
    Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
    Range.prototype.getBoundingClientRect = () => new DOMRect();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      queueMicrotask(() => callback(0));
      return 0;
    });
    vi.mocked(getOrCreateDaily).mockResolvedValue(daily);
    vi.mocked(createNote).mockResolvedValue({ ...daily, id: 2, kind: "note" });
    vi.mocked(createProject).mockResolvedValue({ ...daily, id: 20, kind: "project" });
    vi.mocked(listDailyAttachments).mockResolvedValue([]);
    vi.mocked(listProjectDocuments).mockResolvedValue([]);
    vi.mocked(addDocumentToProject).mockResolvedValue(undefined);
    vi.mocked(getDocument).mockImplementation(async (id) => ({
      ...daily,
      id,
      kind: id === 1 ? "daily" : "artifact",
      author: id === 1 ? "user" : "agent",
      body: id === 1 ? daily.body : "# Agent run",
    }));
    vi.mocked(syncDocument).mockResolvedValue({
      document: null,
      user_count: 1,
      agent_present: false,
    });
    vi.mocked(updateDocument).mockImplementation(async (id, expectedRevision, body) => ({
      ...daily,
      id,
      body,
      revision: expectedRevision + 1,
    }));
    vi.mocked(readText).mockResolvedValue("");
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("opens today's canonical daily and the Vim note action creates a standalone note", async () => {
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    expect(getOrCreateDaily).toHaveBeenCalledWith("2026-08-03");
    expect(screen.getByRole("heading", { name: "Monday, August 3, 2026" })).toBeTruthy();
    expect(screen.queryByRole("navigation")).toBeNull();

    invokeEditorAction(container, "archive.newSharedNote");
    await act(async () => Promise.resolve());
    expect(createNote).toHaveBeenCalledWith("2026-08-03", "shared");
    expect(screen.getByRole("heading", { name: "Untitled note" })).toBeTruthy();
  });

  it("ignores a stale StrictMode startup result without resetting local edits or autosave", async () => {
    const stale = deferred<Awaited<ReturnType<typeof getOrCreateDaily>>>();
    const current = deferred<Awaited<ReturnType<typeof getOrCreateDaily>>>();
    vi.mocked(getOrCreateDaily)
      .mockReturnValueOnce(stale.promise)
      .mockReturnValueOnce(current.promise);
    const currentDaily = { ...daily, body: "current", revision: 4 };
    const { container } = render(<StrictMode><App /></StrictMode>);
    expect(getOrCreateDaily).toHaveBeenCalledTimes(2);

    await act(async () => current.resolve(currentDaily));
    insertLocally(container, "local ");
    expect(editorView(container).state.doc.toString()).toBe("local current");

    await act(async () => stale.resolve({ ...daily, body: "stale", revision: 2 }));
    expect(editorView(container).state.doc.toString()).toBe("local current");
    await act(async () => vi.advanceTimersByTimeAsync(1_000));
    expect(updateDocument).toHaveBeenCalledWith(1, 4, "local current");
  });

  it("retains exact editor identities across buffer and Read mode switches", async () => {
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    const dailyRoot = container.querySelector('[data-document-id="1"] .cm-editor');

    invokeEditorAction(container, "archive.newSharedNote");
    await act(async () => Promise.resolve());
    const noteRoot = container.querySelector('[data-document-id="2"] .cm-editor');
    expect(container.querySelectorAll(".cm-editor")).toHaveLength(2);
    expect(container.querySelector('[data-document-id="1"]')?.hasAttribute("hidden")).toBe(true);

    invokeEditorAction(container, "archive.previousBuffer");
    await act(async () => Promise.resolve());
    expect(container.querySelector('[data-document-id="1"] .cm-editor')).toBe(dailyRoot);
    expect(container.querySelector('[data-document-id="2"] .cm-editor')).toBe(noteRoot);
    fireEvent.click(screen.getByRole("button", { name: "Read" }));
    await act(async () => Promise.resolve());
    expect(container.querySelector('[data-document-id="1"] .cm-editor')).toBe(dailyRoot);
    expect(container.querySelector('[data-document-id="1"]')?.hasAttribute("hidden")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    await act(async () => Promise.resolve());
    expect(container.querySelector('[data-document-id="1"] .cm-editor')).toBe(dailyRoot);
    expect(container.querySelector('[data-document-id="1"]')?.getAttribute("data-editor-active")).toBe("true");
  });

  it("replaces a retained editor and buffer with a newer canonical Today result", async () => {
    const yesterday = { ...daily, id: 3, day: "2026-08-02", body: "yesterday" };
    const remote = { ...daily, body: "remote canonical", revision: 4 };
    vi.mocked(dailyNeighbors).mockResolvedValue({ previous: { id: 3, day: yesterday.day }, next: null });
    vi.mocked(getDocument).mockResolvedValue(yesterday);
    vi.mocked(getOrCreateDaily).mockResolvedValueOnce(daily).mockResolvedValueOnce(remote);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    const retained = editorView(container, 1);

    fireEvent.click(screen.getByRole("button", { name: /Previous daily/ }));
    await act(async () => Promise.resolve());
    const other = editorView(container, 3);
    fireEvent.click(screen.getByRole("button", { name: "Today" }));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(editorView(container, 1)).toBe(retained);
    expect(editorView(container, 3)).toBe(other);
    expect(retained.state.doc.toString()).toBe("remote canonical");
    expect(other.state.doc.toString()).toBe("yesterday");
    insertLocally(container, "!", 1);
    await act(async () => vi.advanceTimersByTimeAsync(1_000));
    expect(updateDocument).toHaveBeenCalledWith(1, 4, "!remote canonical");
  });

  it("keeps Ctrl shortcuts outside CodeMirror and routes editor actions through Vim", async () => {
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    const editor = container.querySelector('[data-editor-active="true"] .cm-content')!;

    fireEvent.keyDown(editor, { ctrlKey: true, key: "n" });
    fireEvent.keyDown(editor, { ctrlKey: true, shiftKey: true, key: "N" });
    fireEvent.keyDown(editor, { ctrlKey: true, key: "o" });
    expect(createNote).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Command Palette" })).toBeNull();

    fireEvent.keyDown(document.body, { ctrlKey: true, key: "o" });
    expect(screen.getByRole("dialog", { name: "Command Palette" })).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    invokeEditorAction(container, "archive.openCommandPalette");
    expect(screen.getByRole("dialog", { name: "Command Palette" })).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });

    fireEvent.keyDown(window, { ctrlKey: true, shiftKey: true, key: "N" });
    await act(async () => Promise.resolve());
    expect(createNote).toHaveBeenCalledWith("2026-08-03", "private");
  });

  it("toggles user documents through visible and command actions but forces artifacts to Read", async () => {
    vi.mocked(listUnreviewedAttachments).mockResolvedValue([{ artifact_id: 8, title: "Agent output", day: daily.day, status: "completed", created_at: daily.created_at, updated_at: daily.updated_at, reviewed_at: null }]);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    expect(container.querySelector(".cm-editor")).toBeTruthy();
    expect(container.querySelector("footer")?.textContent).toContain("Edit");

    fireEvent.click(screen.getByRole("button", { name: "Read" }));
    await act(async () => Promise.resolve());
    expect(screen.getByText("This document is empty.")).toBeTruthy();
    expect(container.querySelector("footer")?.textContent).toContain("Read");

    fireEvent.keyDown(document, { ctrlKey: true, key: "o" });
    fireEvent.click(screen.getByText("Edit document"));
    await act(async () => Promise.resolve());
    expect(container.querySelector(".cm-editor")).toBeTruthy();

    vi.mocked(getDocument).mockResolvedValueOnce({
      ...daily, id: 8, kind: "artifact", author: "agent", body: "# Agent output",
    });
    fireEvent.keyDown(document, { ctrlKey: true, key: "o" });
    fireEvent.click(screen.getAllByText("Review agent work").at(-1)!);
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByText("Agent output"));
    await act(async () => Promise.resolve());
    expect(container.querySelector('[data-document-id="8"] .cm-editor')).toBeNull();
    expect(container.querySelectorAll(".cm-editor")).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
  });

  it("keeps Edit when the Read flush fails", async () => {
    vi.mocked(updateDocument).mockRejectedValueOnce(new Error("disk full"));
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    insertLocally(container, "changed");
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "Read" }));
    await act(async () => Promise.resolve());
    expect(container.querySelector(".cm-editor")).toBeTruthy();
    expect(screen.getByText(/Could not save note: disk full/)).toBeTruthy();
  });

  it("navigates existing daily gaps without creation and returns through canonical Today", async () => {
    vi.mocked(dailyNeighbors).mockResolvedValueOnce({
      previous: { id: 3, day: "2026-08-01" }, next: null,
    });
    vi.mocked(getDocument).mockResolvedValueOnce({ ...daily, id: 3, day: "2026-08-01" });
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: /Previous daily/ }));
    await act(async () => Promise.resolve());
    expect(getDocument).toHaveBeenCalledWith(3);
    expect(getOrCreateDaily).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Today" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Today" }));
    await act(async () => Promise.resolve());
    expect(getOrCreateDaily).toHaveBeenLastCalledWith("2026-08-03");
    expect(container.querySelector("footer")?.textContent).toContain("Monday, August 3, 2026");
  });

  it("ignores stale daily neighbor success after switching documents", async () => {
    const pending = deferred<Awaited<ReturnType<typeof dailyNeighbors>>>();
    vi.mocked(dailyNeighbors).mockReturnValueOnce(pending.promise);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    invokeEditorAction(container, "archive.newSharedNote");
    await act(async () => Promise.resolve());
    await act(async () => pending.resolve({ previous: { id: 3, day: "2026-08-01" }, next: null }));

    expect(screen.getByRole("heading", { name: "Untitled note" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Previous daily/ })).toBeNull();
  });

  it("ignores stale daily neighbor errors after switching documents", async () => {
    const pending = deferred<Awaited<ReturnType<typeof dailyNeighbors>>>();
    vi.mocked(dailyNeighbors).mockReturnValueOnce(pending.promise);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    invokeEditorAction(container, "archive.newSharedNote");
    await act(async () => Promise.resolve());
    await act(async () => pending.reject(new Error("stale neighbor failure")));

    expect(screen.queryByText(/stale neighbor failure/)).toBeNull();
  });

  it("ignores stale attachment metadata success after switching documents", async () => {
    const pending = deferred<Awaited<ReturnType<typeof getAttachmentByArtifactId>>>();
    const row = { artifact_id: 8, title: "Agent output", day: daily.day, status: "blocked" as const, created_at: daily.created_at, updated_at: daily.updated_at, reviewed_at: null };
    vi.mocked(listUnreviewedAttachments).mockResolvedValue([row]);
    vi.mocked(getAttachmentByArtifactId).mockReturnValueOnce(pending.promise);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    fireEvent.keyDown(document, { ctrlKey: true, key: "o" });
    fireEvent.click(screen.getAllByText("Review agent work").at(-1)!);
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByText("Agent output"));
    await act(async () => Promise.resolve());

    fireEvent.keyDown(document, { ctrlKey: true, key: "n" });
    await act(async () => Promise.resolve());
    await act(async () => pending.resolve(row));

    expect(screen.getByRole("heading", { name: "Untitled note" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Mark reviewed" })).toBeNull();
    expect(container.textContent).not.toContain("Blocked");
  });

  it("ignores stale attachment metadata errors after switching documents", async () => {
    const pending = deferred<Awaited<ReturnType<typeof getAttachmentByArtifactId>>>();
    const row = { artifact_id: 8, title: "Agent output", day: daily.day, status: "completed" as const, created_at: daily.created_at, updated_at: daily.updated_at, reviewed_at: null };
    vi.mocked(listUnreviewedAttachments).mockResolvedValue([row]);
    vi.mocked(getAttachmentByArtifactId).mockReturnValueOnce(pending.promise);
    render(<App />);
    await act(async () => Promise.resolve());
    fireEvent.keyDown(document, { ctrlKey: true, key: "o" });
    fireEvent.click(screen.getAllByText("Review agent work").at(-1)!);
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByText("Agent output"));
    await act(async () => Promise.resolve());

    fireEvent.keyDown(document, { ctrlKey: true, key: "n" });
    await act(async () => Promise.resolve());
    await act(async () => pending.reject(new Error("stale metadata failure")));

    expect(screen.queryByText(/stale metadata failure/)).toBeNull();
  });

  it("invalidates a pending review load as soon as the dialog closes", async () => {
    const pending = deferred<Awaited<ReturnType<typeof listUnreviewedAttachments>>>();
    const stale = { artifact_id: 8, title: "Stale review", day: daily.day, status: "completed" as const, created_at: daily.created_at, updated_at: daily.updated_at, reviewed_at: null };
    vi.mocked(listUnreviewedAttachments).mockReturnValueOnce(pending.promise).mockResolvedValueOnce([]);
    render(<App />);
    await act(async () => Promise.resolve());
    fireEvent.keyDown(document, { ctrlKey: true, key: "o" });
    fireEvent.click(screen.getAllByText("Review agent work").at(-1)!);
    const dialog = screen.getByRole("dialog", { name: "Review agent work" });
    fireEvent.keyDown(dialog, { key: "Escape" });
    await act(async () => pending.resolve([stale]));

    fireEvent.keyDown(document, { ctrlKey: true, key: "o" });
    fireEvent.click(screen.getAllByText("Review agent work").at(-1)!);
    await act(async () => Promise.resolve());
    expect(screen.queryByText("Stale review")).toBeNull();
    expect(screen.getByText("No agent work is waiting for review.")).toBeTruthy();
  });

  it("shows truthful Agent work summaries and reviews only on explicit activation", async () => {
    const rows = [
      { artifact_id: 8, title: "Blocked run", day: daily.day, status: "blocked" as const, created_at: daily.created_at, updated_at: daily.updated_at, reviewed_at: null },
      { artifact_id: 9, title: "Failed run", day: "2026-08-02", status: "failed" as const, created_at: daily.created_at, updated_at: daily.updated_at, reviewed_at: null },
    ];
    vi.mocked(listDailyAttachments).mockResolvedValue(rows);
    vi.mocked(listUnreviewedAttachments).mockResolvedValue(rows);
    vi.mocked(getDocument).mockImplementation(async (id) => ({ ...daily, id, kind: "artifact", author: "agent", body: `# ${id === 8 ? "Blocked run" : "Failed run"}` }));
    vi.mocked(getAttachmentByArtifactId).mockImplementation(async (id) => rows.find((row) => row.artifact_id === id) ?? null);
    vi.mocked(markAttachmentReviewed).mockResolvedValue({ ...rows[0], reviewed_at: "2026-08-03T11:00:00.000Z" });
    render(<App />);
    await act(async () => Promise.resolve());
    expect(screen.getByText("1 blocked")).toBeTruthy();
    expect(screen.getByText("1 failed")).toBeTruthy();
    expect(screen.getByText("2 New")).toBeTruthy();

    fireEvent.keyDown(document, { ctrlKey: true, key: "o" });
    fireEvent.click(screen.getAllByText("Review agent work").at(-1)!);
    await act(async () => Promise.resolve());
    expect(screen.getByRole("dialog", { name: "Review agent work" })).toBeTruthy();
    expect(screen.getByText(/Sunday, August 2, 2026 · Failed · New/)).toBeTruthy();
    fireEvent.click(screen.getByText("Blocked run"));
    await act(async () => Promise.resolve());
    expect(markAttachmentReviewed).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Mark reviewed" }));
    fireEvent.click(screen.getByRole("button", { name: /Mark/ }));
    await act(async () => Promise.resolve());
    expect(markAttachmentReviewed).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/Reviewed/)).toBeTruthy();
    expect(screen.getAllByText(/Blocked/).length).toBeGreaterThan(0);
  });

  it("keeps identity, document, and transient persistence in stable footer regions", async () => {
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    const footer = container.querySelector("footer")!;
    const identity = footer.querySelector('[data-status-region="identity"]')!;
    const document = footer.querySelector('[data-status-region="document"]')!;
    const persistence = footer.querySelector('[data-status-region="persistence"]')!;
    expect(footer.className).toContain("grid-cols-[minmax(0,1fr)_minmax(0,auto)_minmax(0,1fr)]");
    expect(identity.textContent).toContain("Archive");
    expect(document.textContent).toContain("1/1 · Monday, August 3, 2026");
    expect(persistence.textContent).toBe("");

    insertLocally(container, "draft");
    await act(async () => Promise.resolve());
    expect(persistence.textContent).toBe("Saving…");
    expect(document.textContent).toContain("1/1 · Monday, August 3, 2026");
    expect(persistence.className).toContain("justify-self-end");
  });

  it("the Vim private-note action creates and focuses a private note", async () => {
    vi.mocked(createNote).mockResolvedValue({ ...daily, id: 3, kind: "note", visibility: "private" });
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    invokeEditorAction(container, "archive.newPrivateNote");
    await act(async () => Promise.resolve());

    expect(createNote).toHaveBeenCalledWith("2026-08-03", "private");
    expect(screen.getByRole("heading", { name: "Untitled note" })).toBeTruthy();
    expect(screen.getByText(/Private/, { selector: "footer *" })).toBeTruthy();
  });

  it("retries a failed midnight daily switch without exposing the wrong day", async () => {
    vi.setSystemTime(new Date(2026, 7, 3, 23, 59, 59, 900));
    const nextDaily = { ...daily, id: 3, day: "2026-08-04" };
    vi.mocked(getOrCreateDaily)
      .mockResolvedValueOnce(daily)
      .mockRejectedValueOnce(new Error("temporarily unavailable"))
      .mockResolvedValueOnce(nextDaily);
    render(<App />);
    await act(async () => Promise.resolve());

    await act(async () => {
      vi.setSystemTime(new Date(2026, 7, 4, 0, 0, 0, 100));
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(screen.getByRole("heading", { name: "Monday, August 3, 2026" })).toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(screen.getByRole("heading", { name: "Tuesday, August 4, 2026" })).toBeTruthy();
    expect(getOrCreateDaily).toHaveBeenCalledTimes(3);
  });

  it("applies clean remote revisions without echo-saving and shows live presence", async () => {
    const remote = { ...daily, body: "remote body", revision: 2 };
    vi.mocked(syncDocument)
      .mockResolvedValueOnce({ document: null, user_count: 1, agent_present: false })
      .mockResolvedValueOnce({ document: remote, user_count: 2, agent_present: true });
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    await act(async () => vi.advanceTimersByTimeAsync(400));
    expect(container.querySelector(".cm-content")?.textContent).toContain("remote body");
    expect(screen.getByText(/2 viewers.*Agent present/, { selector: "footer *" })).toBeTruthy();
    await act(async () => vi.advanceTimersByTimeAsync(700));
    expect(updateDocument).not.toHaveBeenCalled();
  });

  it("suppresses a stale poll after switching documents and cleans up timers and presence", async () => {
    let resolveOld: ((value: Awaited<ReturnType<typeof syncDocument>>) => void) | undefined;
    vi.mocked(syncDocument).mockImplementationOnce(() => new Promise((resolve) => {
      resolveOld = resolve;
    }));
    const { container, unmount } = render(<App />);
    await act(async () => Promise.resolve());

    invokeEditorAction(container, "archive.newSharedNote");
    await act(async () => Promise.resolve());
    expect(screen.getByRole("heading", { name: "Untitled note" })).toBeTruthy();
    await act(async () => resolveOld?.({
      document: { ...daily, body: "stale old body", revision: 2 },
      user_count: 4,
      agent_present: true,
    }));
    expect(container.querySelector(".cm-content")?.textContent).not.toContain("stale old body");

    const heartbeatCalls = vi.mocked(updatePresence).mock.calls.length;
    unmount();
    await act(async () => vi.advanceTimersByTimeAsync(3_000));
    expect(updatePresence).toHaveBeenCalledTimes(heartbeatCalls);
    expect(removePresence).toHaveBeenCalled();
  });

  it("three-way merges disjoint edits and saves against the remote revision", async () => {
    const base = { ...daily, body: "one\ntwo\nthree" };
    vi.mocked(getOrCreateDaily).mockResolvedValue(base);
    vi.mocked(syncDocument)
      .mockResolvedValueOnce({ document: null, user_count: 1, agent_present: false })
      .mockResolvedValueOnce({
        document: { ...base, body: "one\ntwo\nTHREE", revision: 2 },
        user_count: 1,
        agent_present: false,
      });
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    insertLocally(container, "LOCAL\n");
    await act(async () => Promise.resolve());

    await act(async () => vi.advanceTimersByTimeAsync(400));
    expect(updateDocument).toHaveBeenCalledWith(1, 2, "LOCAL\none\ntwo\nTHREE");
    expect(container.querySelector(".cm-content")?.textContent).toContain("LOCAL");
    expect(container.querySelector(".cm-content")?.textContent).toContain("THREE");
  });

  it("preserves overlapping versions, blocks switching, and resolves Keep mine", async () => {
    const base = { ...daily, body: "base" };
    vi.mocked(getOrCreateDaily).mockResolvedValue(base);
    vi.mocked(syncDocument)
      .mockResolvedValueOnce({ document: null, user_count: 1, agent_present: false })
      .mockResolvedValueOnce({
        document: { ...base, body: "REMOTEbase", revision: 2 },
        user_count: 1,
        agent_present: false,
      });
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    insertLocally(container, "LOCAL");
    await act(async () => Promise.resolve());
    await act(async () => vi.advanceTimersByTimeAsync(400));

    expect(screen.getByRole("alertdialog", { name: "Concurrent edits need your choice" })).toBeTruthy();
    expect(container.querySelector(".cm-content")?.textContent).toContain("LOCALbase");
    invokeEditorAction(container, "archive.newSharedNote");
    await act(async () => Promise.resolve());
    expect(createNote).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Keep mine" }));
    await act(async () => Promise.resolve());
    expect(updateDocument).toHaveBeenCalledWith(1, 2, "LOCALbase");
    expect(screen.queryByRole("alertdialog", { name: "Concurrent edits need your choice" })).toBeNull();
    expect(container.querySelector(".cm-content")?.textContent).toContain("LOCALbase");
  });

  it("resolves an overlapping edit with the exact remote body without saving", async () => {
    const base = { ...daily, body: "base" };
    vi.mocked(getOrCreateDaily).mockResolvedValue(base);
    vi.mocked(syncDocument)
      .mockResolvedValueOnce({ document: null, user_count: 1, agent_present: false })
      .mockResolvedValueOnce({
        document: { ...base, body: "REMOTEbase", revision: 2 },
        user_count: 1,
        agent_present: false,
      });
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    insertLocally(container, "LOCAL");
    await act(async () => Promise.resolve());
    await act(async () => vi.advanceTimersByTimeAsync(400));

    fireEvent.click(screen.getByRole("button", { name: "Use remote" }));
    expect(container.querySelector(".cm-content")?.textContent).toContain("REMOTEbase");
    expect(updateDocument).not.toHaveBeenCalled();
  });

  it("shows a collapsed agent-work shelf and opens an attachment as a document", async () => {
    const artifact = {
      ...daily,
      id: 9,
      kind: "artifact" as const,
      author: "agent" as const,
      body: "# CLOB portfolio projection writer E2E",
      revision: 1,
    };
    vi.mocked(listDailyAttachments).mockResolvedValue([
      {
        artifact_id: 9,
        title: "CLOB portfolio projection writer E2E",
        day: "2026-08-03",
        status: "blocked",
        created_at: "2026-08-03T10:42:00.000Z",
        updated_at: "2026-08-03T10:42:00.000Z",
        reviewed_at: null,
      },
      {
        artifact_id: 10,
        title: "Credential capture was too old",
        day: "2026-08-03",
        status: "failed",
        created_at: "2026-08-03T09:18:00.000Z",
        updated_at: "2026-08-03T09:18:00.000Z",
        reviewed_at: null,
      },
    ]);
    vi.mocked(getDocument).mockResolvedValue(artifact);
    render(<App />);
    await act(async () => Promise.resolve());

    expect(listDailyAttachments).toHaveBeenCalledWith("2026-08-03");
    expect(screen.getByRole("button", { name: /Agent work · 2/ })).toBeTruthy();
    expect(screen.getByText("1 blocked")).toBeTruthy();
    expect(screen.getByText("1 failed")).toBeTruthy();
    expect(screen.queryByText("CLOB portfolio projection writer E2E")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Agent work · 2/ }));
    expect(screen.getByText("CLOB portfolio projection writer E2E")).toBeTruthy();
    expect(screen.getByText("Blocked")).toBeTruthy();
    expect(screen.getByText("Failed")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /CLOB portfolio projection writer E2E/ }));
    await act(async () => Promise.resolve());
    expect(getDocument).toHaveBeenCalledWith(9);
    expect(screen.getByRole("heading", { name: "CLOB portfolio projection writer E2E" })).toBeTruthy();
  });

  it("creates and opens a project with its empty shelf and project metadata", async () => {
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    invokeEditorAction(container, "archive.openCommandPalette");
    fireEvent.click(screen.getByText("New project"));
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(createProject).toHaveBeenCalledWith("2026-08-03");
    expect(screen.getByRole("heading", { name: "Untitled project" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Project documents" })).toBeTruthy();
    expect(screen.getByText("Add notes, daily documents, or artifacts to this project.")).toBeTruthy();
    expect(screen.getByText(/Project/, { selector: "footer *" })).toBeTruthy();
    expect(document.activeElement).toBe(container.querySelector(".cm-content"));
  });

  it("selects the first visible add result, suppresses Ctrl-Enter references, adds once, and opens the member", async () => {
    const project = { ...daily, id: 20, kind: "project" as const };
    const member = { ...daily, id: 21, kind: "note" as const, body: "# Member note" };
    vi.mocked(createProject).mockResolvedValue(project);
    vi.mocked(searchDocuments).mockResolvedValue([project, member]);
    vi.mocked(listProjectDocuments).mockResolvedValueOnce([]).mockResolvedValue([member]);
    vi.mocked(getDocument).mockResolvedValue(member);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    invokeEditorAction(container, "archive.openCommandPalette");
    fireEvent.click(screen.getByText("New project"));
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "Add document" }));
    expect(screen.getByRole("dialog", { name: "Add document to project" })).toBeTruthy();
    await act(async () => vi.advanceTimersByTimeAsync(101));
    expect(screen.queryByText("Untitled project", { selector: '[data-slot="command-item"] *' })).toBeNull();
    const input = screen.getByPlaceholderText("Search documents…");
    fireEvent.keyDown(input, { ctrlKey: true, key: "Enter" });
    expect(addDocumentToProject).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Add document to project" })).toBeTruthy();
    fireEvent.keyDown(input, { key: "Enter" });
    await act(async () => Promise.resolve());
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(addDocumentToProject).toHaveBeenCalledWith(20, 21);
    expect(listProjectDocuments).toHaveBeenCalledWith(20);
    expect(screen.queryByRole("dialog", { name: "Add document to project" })).toBeNull();
    expect(document.activeElement).toBe(container.querySelector(".cm-content"));
    fireEvent.click(screen.getByRole("button", { name: /Member note/ }));
    await act(async () => Promise.resolve());
    expect(getDocument).toHaveBeenCalledWith(21);
    expect(screen.getByRole("heading", { name: "Member note" })).toBeTruthy();
  });

  it("ignores late project poll successes and errors after leaving and re-entering", async () => {
    const project = { ...daily, id: 20, kind: "project" as const };
    const stale = { ...daily, id: 21, kind: "note" as const, body: "# Stale member" };
    const fresh = { ...daily, id: 22, kind: "note" as const, body: "# Fresh member" };
    const oldSuccess = deferred<Awaited<ReturnType<typeof listProjectDocuments>>>();
    const oldError = deferred<Awaited<ReturnType<typeof listProjectDocuments>>>();
    vi.mocked(createProject).mockResolvedValue(project);
    vi.mocked(listProjectDocuments)
      .mockReturnValueOnce(oldSuccess.promise)
      .mockReturnValueOnce(oldError.promise)
      .mockResolvedValue([fresh]);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    invokeEditorAction(container, "archive.openCommandPalette");
    fireEvent.click(screen.getByText("New project"));
    await act(async () => Promise.resolve());
    await act(async () => vi.advanceTimersByTimeAsync(2_000));
    invokeEditorAction(container, "archive.newSharedNote");
    await act(async () => Promise.resolve());
    invokeEditorAction(container, "archive.previousBuffer");
    await act(async () => Promise.resolve());
    expect(screen.getByRole("button", { name: /Fresh member/ })).toBeTruthy();

    await act(async () => oldSuccess.resolve([stale]));
    await act(async () => oldError.reject(new Error("stale poll failure")));
    expect(screen.queryByRole("button", { name: /Stale member/ })).toBeNull();
    expect(screen.getByRole("button", { name: /Fresh member/ })).toBeTruthy();
    expect(screen.queryByText(/stale poll failure/)).toBeNull();
  });

  it("invalidates a pending add when Explorer closes and reopens", async () => {
    const project = { ...daily, id: 20, kind: "project" as const };
    const member = { ...daily, id: 21, kind: "note" as const, body: "# Member note" };
    const pending = deferred<void>();
    vi.mocked(createProject).mockResolvedValue(project);
    vi.mocked(searchDocuments).mockResolvedValue([project, member]);
    vi.mocked(addDocumentToProject).mockReturnValue(pending.promise);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    invokeEditorAction(container, "archive.openCommandPalette");
    fireEvent.click(screen.getByText("New project"));
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole("button", { name: "Add document" }));
    await act(async () => vi.advanceTimersByTimeAsync(101));
    const firstDialog = screen.getByRole("dialog", { name: "Add document to project" });
    const firstInput = screen.getByPlaceholderText("Search documents…");
    fireEvent.keyDown(firstInput, { key: "Enter" });
    fireEvent.keyDown(firstInput, { key: "Enter" });
    expect(addDocumentToProject).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(firstDialog, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Add document" }));
    await act(async () => vi.advanceTimersByTimeAsync(101));
    const reopenedInput = screen.getByPlaceholderText("Search documents…");
    reopenedInput.focus();

    await act(async () => pending.reject(new Error("stale add failure")));
    await act(async () => vi.advanceTimersByTimeAsync(2));
    expect(screen.getByRole("dialog", { name: "Add document to project" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Member note/ })).toBeNull();
    expect(screen.queryByText(/stale add failure/)).toBeNull();
    expect(document.activeElement).toBe(reopenedInput);
  });

  it("uses project deletion copy and stops project polling after leaving", async () => {
    vi.mocked(createProject).mockResolvedValue({ ...daily, id: 20, kind: "project" });
    vi.mocked(deleteNote).mockResolvedValue(undefined);
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    invokeEditorAction(container, "archive.openCommandPalette");
    fireEvent.click(screen.getByText("New project"));
    await act(async () => Promise.resolve());
    invokeEditorAction(container, "archive.openCommandPalette");
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByText("Delete project permanently…"));
    await act(async () => Promise.resolve());
    expect(screen.getByRole("alertdialog", { name: "Permanently delete this project?" })).toBeTruthy();
    expect(screen.getByText("This project will be removed immediately. Member documents will be retained.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Delete project" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    const callsBeforeLeaving = vi.mocked(listProjectDocuments).mock.calls.length;
    invokeEditorAction(container, "archive.newSharedNote");
    await act(async () => Promise.resolve());
    await act(async () => vi.advanceTimersByTimeAsync(4_100));
    expect(listProjectDocuments).toHaveBeenCalledTimes(callsBeforeLeaving);
  });
});
