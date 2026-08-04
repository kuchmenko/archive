import { diff3Merge } from "node-diff3";

export type SaveState =
  | { kind: "saved" }
  | { kind: "saving" }
  | { kind: "error"; message: string };

export type PersistedDocument = {
  body: string;
  revision: number;
  updated_at?: string;
};

export type MergeResult =
  | { kind: "merged"; body: string }
  | { kind: "conflict" };

export function mergeDocumentBodies(base: string, local: string, remote: string): MergeResult {
  const regions = diff3Merge(local, base, remote, {
    excludeFalseConflicts: true,
    stringSeparator: /(\r?\n)/,
  });
  if (regions.some((region) => region.conflict)) return { kind: "conflict" };
  return {
    kind: "merged",
    body: regions.flatMap((region) => region.ok ?? []).join(""),
  };
}

export class AutosaveController {
  private id: number | null = null;
  private body = "";
  private savedBody = "";
  private savedRevision = 0;
  private paused = false;
  private basis = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private inFlight: Promise<void> | null = null;

  constructor(
    private readonly save: (id: number, expectedRevision: number, body: string) => Promise<PersistedDocument>,
    private readonly onState: (state: SaveState) => void,
    private readonly delay = 650,
    private readonly onPersisted?: (id: number, document: PersistedDocument) => void,
  ) {}

  load(id: number, body: string, revision: number) {
    this.cancelTimer();
    this.id = id;
    this.body = body;
    this.savedBody = body;
    this.savedRevision = revision;
    this.paused = false;
    this.basis += 1;
    this.onState({ kind: "saved" });
  }

  clear() {
    this.cancelTimer();
    this.id = null;
    this.body = "";
    this.savedBody = "";
    this.savedRevision = 0;
    this.paused = false;
    this.basis += 1;
    this.onState({ kind: "saved" });
  }

  change(body: string) {
    if (this.id === null) return;
    this.body = body;
    this.onState({ kind: "saving" });
    this.cancelTimer();
    if (this.paused) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.persistLatest().catch(() => undefined);
    }, this.delay);
  }

  async flush() {
    this.cancelTimer();
    if (this.paused) throw new Error("resolve the document conflict before continuing");
    await this.persistLatest();
  }

  snapshot() {
    return {
      id: this.id,
      body: this.body,
      persistedBody: this.savedBody,
      revision: this.savedRevision,
      dirty: this.body !== this.savedBody,
      paused: this.paused,
    };
  }

  adoptRemote(body: string, revision: number) {
    this.cancelTimer();
    this.body = body;
    this.savedBody = body;
    this.savedRevision = revision;
    this.basis += 1;
    this.onState({ kind: "saved" });
  }

  rebase(body: string, remote: PersistedDocument) {
    this.cancelTimer();
    this.body = body;
    this.savedBody = remote.body;
    this.savedRevision = remote.revision;
    this.basis += 1;
    this.onState({ kind: body === remote.body ? "saved" : "saving" });
  }

  pause() {
    this.paused = true;
    this.basis += 1;
    this.cancelTimer();
  }

  resume() {
    this.paused = false;
  }

  dispose() {
    this.cancelTimer();
  }

  private async persistLatest(): Promise<void> {
    if (this.inFlight) {
      await this.inFlight;
      if (!this.paused && this.id !== null && this.body !== this.savedBody) {
        return this.persistLatest();
      }
      return;
    }

    const operation = this.drain();
    this.inFlight = operation;
    try {
      await operation;
    } finally {
      if (this.inFlight === operation) this.inFlight = null;
    }
  }

  private async drain() {
    while (!this.paused && this.id !== null && this.body !== this.savedBody) {
      const id = this.id;
      const body = this.body;
      const expectedRevision = this.savedRevision;
      const basis = this.basis;
      this.onState({ kind: "saving" });
      try {
        const persisted = await this.save(id, expectedRevision, body);
        if (this.id === id && this.basis === basis) {
          this.savedBody = persisted.body;
          this.savedRevision = persisted.revision;
          this.onPersisted?.(id, persisted);
        }
      } catch (error) {
        if (this.id !== id || this.basis !== basis) continue;
        const message = error instanceof Error ? error.message : String(error);
        this.onState({ kind: "error", message });
        throw error;
      }
    }

    if (this.id !== null) this.onState({ kind: "saved" });
  }

  private cancelTimer() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
}
