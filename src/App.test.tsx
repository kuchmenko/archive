import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import {
  createNote,
  getDocument,
  getOrCreateDaily,
  listDailyAttachments,
  removePresence,
  syncDocument,
  updateDocument,
  updatePresence,
} from "./lib/archive";
import { readText } from "@tauri-apps/plugin-clipboard-manager";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    destroy: vi.fn(),
    onCloseRequested: vi.fn().mockResolvedValue(vi.fn()),
  }),
}));

vi.mock("./lib/archive", () => ({
  createNote: vi.fn(),
  deleteNote: vi.fn(),
  getDocument: vi.fn(),
  getOrCreateDaily: vi.fn(),
  listDailyAttachments: vi.fn(),
  searchDocuments: vi.fn(),
  resolveReferences: vi.fn().mockResolvedValue([]),
  updateDocument: vi.fn(),
  syncDocument: vi.fn(),
  updatePresence: vi.fn().mockResolvedValue(undefined),
  removePresence: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@replit/codemirror-vim", () => ({
  getCM: () => undefined,
  vim: () => [],
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
    vi.mocked(listDailyAttachments).mockResolvedValue([]);
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

  it("opens today's canonical daily and Ctrl+N creates a standalone note", async () => {
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    expect(getOrCreateDaily).toHaveBeenCalledWith("2026-08-03");
    expect(screen.getByRole("heading", { name: "Monday, August 3, 2026" })).toBeTruthy();
    expect(screen.queryByRole("navigation")).toBeNull();

    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, key: "n" });
    await act(async () => Promise.resolve());
    expect(createNote).toHaveBeenCalledWith("2026-08-03", "shared");
    expect(screen.getByRole("heading", { name: "Untitled note" })).toBeTruthy();
  });

  it("keeps identity, document, and transient persistence in stable footer regions", async () => {
    vi.mocked(readText).mockResolvedValue("draft");
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

    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, key: "v" });
    await act(async () => Promise.resolve());
    expect(persistence.textContent).toBe("Saving…");
    expect(document.textContent).toContain("1/1 · Monday, August 3, 2026");
    expect(persistence.className).toContain("justify-self-end");
  });

  it("Ctrl+Shift+N creates and focuses a private note", async () => {
    vi.mocked(createNote).mockResolvedValue({ ...daily, id: 3, kind: "note", visibility: "private" });
    const { container } = render(<App />);
    await act(async () => Promise.resolve());

    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, shiftKey: true, key: "N" });
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

    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, key: "n" });
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
    vi.mocked(readText).mockResolvedValue("LOCAL\n");
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, key: "v" });
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
    vi.mocked(readText).mockResolvedValue("LOCAL");
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, key: "v" });
    await act(async () => Promise.resolve());
    await act(async () => vi.advanceTimersByTimeAsync(400));

    expect(screen.getByRole("alertdialog", { name: "Concurrent edits need your choice" })).toBeTruthy();
    expect(container.querySelector(".cm-content")?.textContent).toContain("LOCALbase");
    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, key: "n" });
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
    vi.mocked(readText).mockResolvedValue("LOCAL");
    const { container } = render(<App />);
    await act(async () => Promise.resolve());
    fireEvent.keyDown(container.querySelector(".cm-editor")!, { ctrlKey: true, key: "v" });
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
        id: 9,
        title: "CLOB portfolio projection writer E2E",
        status: "blocked",
        created_at: "2026-08-03T10:42:00.000Z",
        updated_at: "2026-08-03T10:42:00.000Z",
      },
      {
        id: 10,
        title: "Credential capture was too old",
        status: "failed",
        created_at: "2026-08-03T09:18:00.000Z",
        updated_at: "2026-08-03T09:18:00.000Z",
      },
    ]);
    vi.mocked(getDocument).mockResolvedValue(artifact);
    render(<App />);
    await act(async () => Promise.resolve());

    expect(listDailyAttachments).toHaveBeenCalledWith("2026-08-03");
    expect(screen.getByRole("button", { name: /Agent work · 2 subnotes/ })).toBeTruthy();
    expect(screen.getByText("2 blocked")).toBeTruthy();
    expect(screen.queryByText("CLOB portfolio projection writer E2E")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Agent work · 2 subnotes/ }));
    expect(screen.getByText("CLOB portfolio projection writer E2E")).toBeTruthy();
    expect(screen.getByText("Blocked")).toBeTruthy();
    expect(screen.getByText("Failed")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /CLOB portfolio projection writer E2E/ }));
    await act(async () => Promise.resolve());
    expect(getDocument).toHaveBeenCalledWith(9);
    expect(screen.getByRole("heading", { name: "CLOB portfolio projection writer E2E" })).toBeTruthy();
  });
});
