export type SaveState =
  | { kind: "saved" }
  | { kind: "saving" }
  | { kind: "error"; message: string };

export class AutosaveController {
  private id: number | null = null;
  private body = "";
  private savedBody = "";
  private timer: ReturnType<typeof setTimeout> | null = null;
  private inFlight: Promise<void> | null = null;

  constructor(
    private readonly save: (id: number, expectedBody: string, body: string) => Promise<void>,
    private readonly onState: (state: SaveState) => void,
    private readonly delay = 650,
  ) {}

  load(id: number, body: string) {
    this.cancelTimer();
    this.id = id;
    this.body = body;
    this.savedBody = body;
    this.onState({ kind: "saved" });
  }

  clear() {
    this.cancelTimer();
    this.id = null;
    this.body = "";
    this.savedBody = "";
    this.onState({ kind: "saved" });
  }

  change(body: string) {
    if (this.id === null) return;
    this.body = body;
    this.onState({ kind: "saving" });
    this.cancelTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.persistLatest().catch(() => undefined);
    }, this.delay);
  }

  async flush() {
    this.cancelTimer();
    await this.persistLatest();
  }

  dispose() {
    this.cancelTimer();
  }

  private async persistLatest() {
    if (this.inFlight) return this.inFlight;

    const operation = this.drain();
    this.inFlight = operation;
    try {
      await operation;
    } finally {
      if (this.inFlight === operation) this.inFlight = null;
    }
  }

  private async drain() {
    while (this.id !== null && this.body !== this.savedBody) {
      const id = this.id;
      const body = this.body;
      const expectedBody = this.savedBody;
      this.onState({ kind: "saving" });
      try {
        await this.save(id, expectedBody, body);
        if (this.id === id) this.savedBody = body;
      } catch (error) {
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
