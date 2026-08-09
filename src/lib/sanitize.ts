import DOMPurify from "dompurify";

export function sanitizeSvg(svg: string): SVGElement | null {
  const sanitized = DOMPurify.sanitize(svg, {
    USE_PROFILES: { svg: true, svgFilters: true },
    FORBID_TAGS: ["foreignObject", "script"],
    FORBID_ATTR: ["href", "xlink:href"],
  });
  const parsed = new DOMParser().parseFromString(sanitized, "image/svg+xml");
  const root = parsed.documentElement;
  if (root.localName !== "svg" || parsed.querySelector("parsererror")) return null;
  return document.importNode(root, true) as unknown as SVGElement;
}
