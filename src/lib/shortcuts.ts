export type AppShortcut = "new-note" | "new-private-note" | "open-commands";

type KeyEvent = Pick<KeyboardEvent, "altKey" | "ctrlKey" | "key" | "metaKey" | "shiftKey">;

export function appShortcut(event: KeyEvent): AppShortcut | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  if (event.key.toLowerCase() === "n") return event.shiftKey ? "new-private-note" : "new-note";
  if (event.shiftKey) return null;
  if (event.key.toLowerCase() === "o") return "open-commands";
  return null;
}
