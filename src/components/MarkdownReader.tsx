import DOMPurify from "dompurify";
import { renderMarkdown, renderMermaid } from "@/lib/archive";
import { sanitizeSvg } from "@/lib/sanitize";
import { useEffect, useRef, useState } from "react";

const tags = ["p", "br", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "code", "pre", "ul", "ol", "li", "strong", "em", "del", "a", "table", "thead", "tbody", "tr", "th", "td", "hr", "sup", "div", "span", "input", "button"];
const attributes = ["class", "type", "checked", "disabled", "data-document-id"];

export function MarkdownReader({ documentId, body, onOpenReference }: { documentId: number; body: string; onOpenReference: (id: number) => void }) {
  const [state, setState] = useState<{ html?: string; error?: string }>({});
  const region = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let current = true;
    setState(body.trim() ? {} : { html: "" });
    if (!body.trim()) {
      return () => {
        current = false;
      };
    }
    void renderMarkdown(body).then((raw) => {
      if (!current) return;
      const html = DOMPurify.sanitize(raw, { ALLOWED_TAGS: tags, ALLOWED_ATTR: attributes, ALLOW_DATA_ATTR: false });
      setState({ html });
    }).catch((error) => {
      if (current) setState({ error: String(error) });
    });
    return () => {
      current = false;
    };
  }, [documentId, body]);

  useEffect(() => {
    const root = region.current;
    if (!root || state.html === undefined) return;
    root.querySelectorAll("a").forEach((anchor) => {
      anchor.removeAttribute("href");
      anchor.removeAttribute("target");
      anchor.removeAttribute("rel");
    });
    let current = true;
    root.querySelectorAll<HTMLElement>("pre > code.language-mermaid").forEach((code, index) => {
      const pre = code.parentElement;
      if (!pre) return;
      void renderMermaid(code.textContent ?? "", `reader-${documentId}-${index}`).then((result) => {
        if (!current || !pre.isConnected) return;
        const svg = result.valid && result.svg ? sanitizeSvg(result.svg) : null;
        if (svg) pre.replaceChildren(svg);
        else appendDiagnostic(pre, result.diagnostics[0]?.message ?? "Diagram could not be rendered");
      }).catch(() => {
        if (current && pre.isConnected) appendDiagnostic(pre, "Diagram could not be rendered");
      });
    });
    return () => { current = false; };
  }, [documentId, state.html]);

  if (state.error) return <p role="alert" className="text-sm text-destructive">Could not render this document.</p>;
  if (state.html === undefined) return <p role="status" aria-busy="true" className="text-sm text-muted-foreground">Rendering…</p>;
  if (!state.html) return <p className="text-sm text-muted-foreground">This document is empty.</p>;
  return <div ref={region} className="archive-reader" onClick={(event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-document-id]");
    if (!button) return;
    const id = Number(button.dataset.documentId);
    if (Number.isSafeInteger(id) && id > 0) onOpenReference(id);
  }} dangerouslySetInnerHTML={{ __html: state.html }} />;
}

function appendDiagnostic(parent: HTMLElement, message: string) {
  const diagnostic = document.createElement("small");
  diagnostic.setAttribute("role", "status");
  diagnostic.textContent = message;
  parent.append(diagnostic);
}
