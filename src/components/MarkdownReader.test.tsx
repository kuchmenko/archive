import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MarkdownReader } from "./MarkdownReader";
import { renderMarkdown, renderMermaid } from "@/lib/archive";

vi.mock("@/lib/archive", () => ({ renderMarkdown: vi.fn(), renderMermaid: vi.fn() }));

describe("MarkdownReader", () => {
  beforeEach(() => vi.clearAllMocks());

  it("sanitizes navigation and validates internal references", async () => {
    vi.mocked(renderMarkdown).mockResolvedValue('<h1>Title</h1><a href="https://example.com">Outside</a><button type="button" data-document-id="42">Inside</button><script>x</script>');
    const open = vi.fn();
    const { container } = render(<MarkdownReader documentId={1} body="text" onOpenReference={open} />);
    await act(async () => Promise.resolve());
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("a")?.hasAttribute("href")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Inside" }));
    expect(open).toHaveBeenCalledWith(42);
  });

  it("renders empty and error states", async () => {
    const { rerender } = render(<MarkdownReader documentId={1} body="" onOpenReference={vi.fn()} />);
    expect(screen.getByText("This document is empty.")).toBeTruthy();
    vi.mocked(renderMarkdown).mockRejectedValue(new Error("bad"));
    rerender(<MarkdownReader documentId={2} body="bad" onOpenReference={vi.fn()} />);
    await act(async () => Promise.resolve());
    expect(screen.getByRole("alert")).toBeTruthy();
  });

  it("enhances Mermaid with sanitized SVG", async () => {
    vi.mocked(renderMarkdown).mockResolvedValue('<pre><code class="language-mermaid">graph TD</code></pre>');
    vi.mocked(renderMermaid).mockResolvedValue({ valid: true, diagnostics: [], svg: '<svg xmlns="http://www.w3.org/2000/svg"><text>Diagram</text></svg>' });
    const { container } = render(<MarkdownReader documentId={1} body="diagram" onOpenReference={vi.fn()} />);
    await act(async () => Promise.resolve());
    await act(async () => Promise.resolve());
    expect(container.querySelector("svg")?.textContent).toBe("Diagram");
  });

  it("keeps representative prose and only valid internal markers", async () => {
    vi.mocked(renderMarkdown).mockResolvedValue('<h1>Title</h1><h2>Section</h2><p>Prose</p><ul><li>List</li></ul><table><tbody><tr><td>Cell</td></tr></tbody></table><button data-document-id="9007199254740992">Unsafe</button><button data-document-id="7" data-other="no">Safe</button>');
    const open = vi.fn();
    const { container } = render(<MarkdownReader documentId={1} body="content" onOpenReference={open} />);
    await act(async () => Promise.resolve());
    expect(container.querySelectorAll("h1, h2, p, li, table")).toHaveLength(5);
    expect(screen.getByRole("button", { name: "Safe" }).hasAttribute("data-other")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Unsafe" }));
    fireEvent.click(screen.getByRole("button", { name: "Safe" }));
    expect(open).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenCalledWith(7);
  });

  it("retains Mermaid source and reports invalid and rejected renders", async () => {
    vi.mocked(renderMarkdown).mockResolvedValue('<pre><code class="language-mermaid">INVALID source</code></pre>');
    vi.mocked(renderMermaid).mockResolvedValue({ valid: false, diagnostics: [{ message: "Invalid diagram" }] });
    const { rerender } = render(<MarkdownReader documentId={1} body="invalid" onOpenReference={vi.fn()} />);
    await act(async () => Promise.resolve());
    await act(async () => Promise.resolve());
    expect(screen.getByText("INVALID source")).toBeTruthy();
    expect(screen.getByRole("status", { name: "" }).textContent).toContain("Invalid diagram");
    vi.mocked(renderMermaid).mockRejectedValue(new Error("IPC failed"));
    rerender(<MarkdownReader documentId={2} body="rejected" onOpenReference={vi.fn()} />);
    await act(async () => Promise.resolve());
    await act(async () => Promise.resolve());
    expect(screen.getByText("Diagram could not be rendered")).toBeTruthy();
  });

  it("suppresses stale render results and cleanup after unmount", async () => {
    let resolveFirst!: (html: string) => void;
    const first = new Promise<string>((resolve) => { resolveFirst = resolve; });
    vi.mocked(renderMarkdown).mockReturnValueOnce(first).mockResolvedValueOnce("<p>New</p>");
    const { rerender, unmount } = render(<MarkdownReader documentId={1} body="old" onOpenReference={vi.fn()} />);
    rerender(<MarkdownReader documentId={2} body="new" onOpenReference={vi.fn()} />);
    await act(async () => Promise.resolve());
    expect(screen.getByText("New")).toBeTruthy();
    await act(async () => resolveFirst("<p>Old</p>"));
    expect(screen.queryByText("Old")).toBeNull();
    unmount();
    await act(async () => Promise.resolve());
  });
});
