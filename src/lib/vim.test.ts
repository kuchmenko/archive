import { beforeEach, describe, expect, it, vi } from "vitest";

const vimMock = vi.hoisted(() => ({
  actions: new Map<string, (adapter: unknown) => void>(),
  defineAction: vi.fn((name: string, action: (adapter: unknown) => void) => vimMock.actions.set(name, action)),
  map: vi.fn(),
  mapCommand: vi.fn(),
}));

vi.mock("@replit/codemirror-vim", () => ({ Vim: vimMock }));

describe("Archive Vim actions", () => {
  beforeEach(() => {
    vi.resetModules();
    vimMock.actions.clear();
    vimMock.defineAction.mockClear();
    vimMock.map.mockClear();
    vimMock.mapCommand.mockClear();
  });

  it("registers the exact normal-mode mappings once without native keys", async () => {
    const { registerArchiveVimActions } = await import("./vim");
    registerArchiveVimActions({} as never, {} as never);
    registerArchiveVimActions({} as never, {} as never);

    expect(vimMock.map.mock.calls).toEqual([["<Space>", "<Leader>", "normal"]]);
    expect(vimMock.mapCommand.mock.calls).toEqual([
      ["<Leader><Space>", "action", "archive.openExplorer", {}, { context: "normal" }],
      ["<Leader>n", "action", "archive.newSharedNote", {}, { context: "normal" }],
      ["<Leader>N", "action", "archive.newPrivateNote", {}, { context: "normal" }],
      ["<Leader>c", "action", "archive.openCommandPalette", {}, { context: "normal" }],
      ["gf", "action", "archive.openReference", {}, { context: "normal" }],
      ["[b", "action", "archive.previousBuffer", {}, { context: "normal" }],
      ["]b", "action", "archive.nextBuffer", {}, { context: "normal" }],
    ]);
    expect(vimMock.defineAction).toHaveBeenCalledTimes(7);
    expect(JSON.stringify([...vimMock.map.mock.calls, ...vimMock.mapCommand.mock.calls])).not.toMatch(/Ctrl-V|Ctrl-O|Ctrl-N|"H"|"L"|Enter/);
  });

  it("routes by exact view and cleanup removes only that view", async () => {
    const { registerArchiveVimActions } = await import("./vim");
    const firstView = {} as never;
    const secondView = {} as never;
    const first = { openExplorer: vi.fn() };
    const second = { openExplorer: vi.fn() };
    const complete = (actions: typeof first) => ({
      ...actions,
      newSharedNote: vi.fn(), newPrivateNote: vi.fn(), openCommandPalette: vi.fn(),
      openReference: vi.fn(), previousBuffer: vi.fn(), nextBuffer: vi.fn(),
    });
    const cleanupFirst = registerArchiveVimActions(firstView, complete(first));
    registerArchiveVimActions(secondView, complete(second));
    const action = vimMock.actions.get("archive.openExplorer")!;

    action({ cm6: firstView });
    action({ cm6: secondView });
    action({ cm6: {} });
    expect(first.openExplorer).toHaveBeenCalledOnce();
    expect(second.openExplorer).toHaveBeenCalledOnce();

    cleanupFirst();
    action({ cm6: firstView });
    action({ cm6: secondView });
    expect(first.openExplorer).toHaveBeenCalledOnce();
    expect(second.openExplorer).toHaveBeenCalledTimes(2);
  });
});
