import { spawn } from "node:child_process";
import { chromium } from "playwright-core";

const server = spawn("npm", ["exec", "vite", "preview", "--", "--host", "127.0.0.1", "--port", "4173"], {
  stdio: "ignore",
});

async function waitForServer() {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const response = await fetch("http://127.0.0.1:4173");
      if (response.ok) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("Vite preview did not start");
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

try {
  await waitForServer();
  const browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_PATH ?? "/usr/bin/chromium",
    headless: true,
  });
  const context = await browser.newContext({
    locale: "en-US",
    timezoneId: "UTC",
    viewport: { width: 960, height: 720 },
  });
  const page = await context.newPage();
  await page.clock.setFixedTime(new Date("2026-08-03T10:00:00Z"));
  await page.addInitScript(() => {
    const timestamp = "2026-08-03T10:00:00.000Z";
    const documents = [
      { id: 1, kind: "daily", visibility: "shared", author: "user", day: "2026-08-03", created_at: timestamp, updated_at: timestamp, body: "", revision: 1 },
      {
        id: 2,
        kind: "note",
        visibility: "shared",
        author: "user",
        day: "2026-08-03",
        created_at: timestamp,
        updated_at: timestamp,
        body: "# Project note\n[[note:999|Missing]]",
        revision: 1,
      },
      {
        id: 3,
        kind: "daily",
        visibility: "shared",
        author: "user",
        day: "2026-08-02",
        created_at: "2026-08-02T10:00:00.000Z",
        updated_at: "2026-08-02T10:00:00.000Z",
        body: "Nearby Sunday notes",
        revision: 1,
      },
      {
        id: 4,
        kind: "artifact",
        visibility: "shared",
        author: "agent",
        day: "2026-08-03",
        created_at: timestamp,
        updated_at: timestamp,
        body: "# Agent research artifact\nUseful findings\n```mermaid\ngraph TD\nA-->B\n```",
        revision: 1,
      },
    ];
    let nextId = 5;
    window.__archiveCalls = [];
    window.__tauriCallbacks = [];
    window.__clipboardFailure = false;
    window.__clipboardText = "clipboard";
    const presence = { user_count: 1, agent_present: false };
    const syncQueue = [];
    window.__archiveSync = {
      presence,
      queue: syncQueue,
      pushRemote(id, body, revision) {
        const document = documents.find((candidate) => candidate.id === id);
        if (!document) throw new Error(`Document ${id} does not exist`);
        Object.assign(document, { body, revision, updated_at: timestamp });
        syncQueue.push({ ...document });
      },
      setPresence(user_count, agent_present) {
        presence.user_count = user_count;
        presence.agent_present = agent_present;
      },
    };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" } },
      transformCallback: (callback) => {
        window.__tauriCallbacks.push(callback);
        return window.__tauriCallbacks.length;
      },
      unregisterCallback: () => undefined,
      invoke: async (command, args) => {
        window.__archiveCalls.push({ command, args });
        if (command === "plugin:event|listen") return 1;
        if (command === "plugin:event|unlisten") return null;
        if (command === "plugin:window|destroy") return null;
        if (command === "get_or_create_daily") {
          const existing = documents.find((document) => document.kind === "daily" && document.day === args.day);
          if (existing) return { ...existing };
          const daily = { id: nextId++, kind: "daily", visibility: "shared", author: "user", day: args.day, created_at: timestamp, updated_at: timestamp, body: "", revision: 1 };
          documents.push(daily);
          return { ...daily };
        }
        if (command === "create_note") {
          const note = { id: nextId++, kind: "note", visibility: args.visibility, author: "user", day: args.day, created_at: timestamp, updated_at: timestamp, body: "", revision: 1 };
          documents.push(note);
          return { ...note };
        }
        if (command === "get_document") {
          const document = documents.find((candidate) => candidate.id === args.id);
          if (!document) throw new Error(`Document ${args.id} does not exist`);
          return { ...document };
        }
        if (command === "update_document_body") {
          const document = documents.find((candidate) => candidate.id === args.id);
          if (!document) throw new Error(`Document ${args.id} does not exist`);
          if (args.expectedRevision !== document.revision) {
            throw new Error(`Revision conflict: expected ${args.expectedRevision}, current ${document.revision}`);
          }
          document.body = args.body;
          document.updated_at = timestamp;
          document.revision += 1;
          return { ...document };
        }
        if (command === "sync_document") {
          const queuedIndex = syncQueue.findIndex((document) => document.id === args.id && document.revision > args.knownRevision);
          const document = queuedIndex < 0 ? null : syncQueue.splice(queuedIndex, 1)[0];
          return { document, ...presence };
        }
        if (command === "update_presence") return null;
        if (command === "remove_presence") return null;
        if (command === "delete_note") {
          const index = documents.findIndex((candidate) => candidate.id === args.id);
          if (index < 0) throw new Error(`Document ${args.id} does not exist`);
          if (documents[index].kind === "daily") throw new Error("Daily documents cannot be deleted");
          documents.splice(index, 1);
          return null;
        }
        if (command === "search_documents") {
          const query = args.query.trim().toLowerCase();
          return documents
            .filter((document) => {
              const label = document.kind === "daily" ? document.day : (document.body.match(/^\s*#{1,6}\s+(.+)$/m)?.[1] ?? "Untitled note");
              return !query || label.toLowerCase().includes(query) || document.body.toLowerCase().includes(query);
            })
            .sort((left, right) => {
              const leftDistance = Math.abs(Date.parse(`${left.day}T00:00:00Z`) - Date.parse(`${args.activeDay}T00:00:00Z`));
              const rightDistance = Math.abs(Date.parse(`${right.day}T00:00:00Z`) - Date.parse(`${args.activeDay}T00:00:00Z`));
              return leftDistance - rightDistance || Number(right.kind === "daily") - Number(left.kind === "daily") || left.id - right.id;
            })
            .map((document) => ({ ...document }));
        }
        if (command === "resolve_references") {
          return [...new Set(args.ids)].flatMap((id) => {
            const document = documents.find((candidate) => candidate.id === id);
            if (!document) return [];
            const label = document.kind === "daily"
              ? new Intl.DateTimeFormat("en-US", { weekday: "long", year: "numeric", month: "long", day: "numeric", timeZone: "UTC" }).format(new Date(`${document.day}T00:00:00Z`))
              : (document.body.match(/^\s*#{1,6}\s+(.+)$/m)?.[1] ?? "Untitled note");
            return [{ id, kind: document.kind, day: document.day, label }];
          });
        }
        if (command === "render_mermaid") {
          if (args.source.includes("INVALID")) return { valid: false, diagnostics: [{ line: 1, column: 1, message: "Invalid test diagram" }] };
          return {
            valid: true,
            diagram_type: "flowchart",
            diagnostics: [],
            svg: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 40" aria-label="Test flowchart"><path d="M5 20h100" stroke="currentColor"/><path d="m105 15 10 5-10 5" fill="none" stroke="currentColor"/></svg>',
          };
        }
        if (command === "plugin:clipboard-manager|write_text") {
          if (window.__clipboardFailure) throw new Error("clipboard unavailable");
          window.__clipboardText = args.text;
          return null;
        }
        if (command === "plugin:clipboard-manager|read_text") return window.__clipboardText;
        throw new Error(`Unexpected command: ${command}`);
      },
    };
  });
  await page.goto("http://127.0.0.1:4173");
  await page.addStyleTag({ content: "*, *::before, *::after { animation: none !important; transition: none !important; }" });

  const editor = page.locator(".cm-editor");
  const content = page.locator(".cm-content");
  const explorer = page.getByRole("dialog", { name: "Explore notes" });
  const mode = (name) => page.locator("footer", { hasText: name });
  const editorBody = async () => (await content.innerText()).replace(/\n$/, "");
  const calls = (command) => page.evaluate((name) => window.__archiveCalls.filter((call) => call.command === name), command);

  async function replaceAllInVim(body) {
    await content.click();
    await page.keyboard.press("Escape");
    await page.keyboard.type("ggVGc");
    await mode("INSERT").waitFor();
    await page.keyboard.insertText(body);
    await page.keyboard.press("Escape");
    await mode("NORMAL").waitFor();
  }

  async function selectionRendering(name) {
    await page.waitForFunction(() => document.querySelectorAll(".cm-selectionLayer .cm-selectionBackground").length > 0);
    const ranges = await page.locator(".cm-selectionLayer .cm-selectionBackground").evaluateAll((elements) =>
      elements.map((element) => ({
        background: getComputedStyle(element).backgroundColor,
        width: element.getBoundingClientRect().width,
        height: element.getBoundingClientRect().height,
      })),
    );
    assert(ranges.some((range) => range.width > 0 && range.height > 0 && range.background !== "rgba(0, 0, 0, 0)"), `${name} selection is not visibly rendered`);
    return ranges;
  }

  await page.getByRole("heading", { name: "Monday, August 3, 2026" }).waitFor();
  await editor.waitFor();
  assert((await editor.count()) === 1, "Startup did not render exactly one editor");
  const startupCalls = await calls("get_or_create_daily");
  assert(startupCalls.length === 1 && startupCalls[0].args.day === "2026-08-03", "Startup did not request the canonical daily");
  await page.keyboard.press("Shift+H");
  assert((await page.getByRole("heading", { name: "Monday, August 3, 2026" }).count()) === 1, "Single-buffer H changed documents");
  assert((await calls("get_document")).length === 0, "Single-buffer H attempted to open a document");

  const cleanRemote = "shared line\nstable line";
  const cleanUpdateCount = (await calls("update_document_body")).length;
  await page.evaluate((body) => {
    window.__archiveSync.setPresence(2, true);
    window.__archiveSync.pushRemote(1, body, 2);
  }, cleanRemote);
  await page.waitForFunction((body) => document.querySelector(".cm-content")?.innerText.replace(/\n$/, "") === body, cleanRemote, { timeout: 450 });
  assert((await editorBody()) === cleanRemote, "Clean remote source was not applied exactly");
  const cleanEditorState = await page.evaluate(() => {
    const content = document.querySelector(".cm-content");
    const selection = getSelection();
    return {
      focused: document.querySelector(".cm-editor")?.classList.contains("cm-focused"),
      scrollTop: document.querySelector(".cm-scroller")?.scrollTop,
      selectionInside: !!content && !!selection?.anchorNode && content.contains(selection.anchorNode),
    };
  });
  assert(cleanEditorState.focused && cleanEditorState.selectionInside && Number.isFinite(cleanEditorState.scrollTop) && cleanEditorState.scrollTop >= 0, "Clean sync did not preserve a sensible cursor, focus, and scroll position");
  await page.waitForFunction(() => document.querySelector("footer")?.textContent?.includes("2 viewers") && document.querySelector("footer")?.textContent?.includes("✦ Agent present"), { timeout: 450 });
  await page.evaluate(() => window.__archiveSync.setPresence(1, false));
  await page.waitForFunction(() => !document.querySelector("footer")?.textContent?.includes("2 viewers") && !document.querySelector("footer")?.textContent?.includes("✦ Agent present"), { timeout: 450 });
  await page.waitForTimeout(550);
  assert((await calls("update_document_body")).length === cleanUpdateCount, "Clean remote sync produced an echo-save");

  const firstLocal = "LOCAL line\nstable line";
  const firstRemote = "REMOTE line\nstable line";
  await replaceAllInVim(firstLocal);
  const writesBeforeFirstConflict = (await calls("update_document_body")).length;
  await page.evaluate((body) => window.__archiveSync.pushRemote(1, body, 3), firstRemote);
  const conflictDialog = page.getByRole("alertdialog", { name: "Concurrent edits need your choice" });
  await conflictDialog.waitFor({ timeout: 850 });
  assert((await editorBody()) === firstLocal, "First conflict did not preserve the exact local source");
  assert((await calls("update_document_body")).length === writesBeforeFirstConflict, "Autosave wrote an unresolved first conflict");
  const dialogBox = await conflictDialog.boundingBox();
  assert(dialogBox && dialogBox.width < 600 && dialogBox.height < 360, "Conflict alert is not compact");
  await conflictDialog.getByRole("button", { name: "Use remote" }).click();
  await conflictDialog.waitFor({ state: "detached" });
  assert((await editorBody()) === firstRemote, "Use remote did not apply the exact remote source");

  const secondLocal = "MINE line\nstable line";
  const secondRemote = "THEIRS line\nstable line";
  await replaceAllInVim(secondLocal);
  const writesBeforeSecondConflict = (await calls("update_document_body")).length;
  await page.evaluate((body) => window.__archiveSync.pushRemote(1, body, 4), secondRemote);
  await conflictDialog.waitFor({ timeout: 850 });
  assert((await editorBody()) === secondLocal, "Second conflict did not preserve the exact local source");
  assert((await calls("update_document_body")).length === writesBeforeSecondConflict, "Autosave wrote an unresolved second conflict");
  await conflictDialog.getByRole("button", { name: "Keep mine" }).click();
  await conflictDialog.waitFor({ state: "detached" });
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  const keepMineWrites = await calls("update_document_body");
  assert(keepMineWrites.some((call) => call.args.id === 1 && call.args.expectedRevision === 4 && call.args.body === secondLocal), "Keep mine did not write the exact local source against the remote revision");
  assert((await editorBody()) === secondLocal, "Keep mine changed the local source");

  await page.evaluate(() => window.__archiveSync.pushRemote(1, "", 6));
  await page.waitForFunction(() => document.querySelector(".cm-content")?.innerText.replace(/\n$/, "") === "", { timeout: 450 });
  assert((await page.locator(".editor-toolbar, .editor-topbar, .editor-split, .sidebar, [role=tablist], [data-mermaid-toolbar], [data-sync-toolbar], [data-sync-status]").count()) === 0, "Sync or presence added permanent application chrome");

  await page.keyboard.press("Control+o");
  const dailyDelete = page.locator('[data-slot="command-item"]', { hasText: "Daily documents cannot be deleted" });
  await dailyDelete.waitFor();
  assert((await dailyDelete.getAttribute("data-disabled")) === "true", "Daily delete action is enabled");
  await page.keyboard.press("Escape");

  await content.click();
  await page.keyboard.press("i");
  await mode("INSERT").waitFor();
  await page.keyboard.type("  ");
  assert((await explorer.count()) === 0, "Space Space opened explorer in INSERT mode");
  assert((await editorBody()) === "  ", "Space Space did not insert ordinary spaces in INSERT mode");
  await page.keyboard.press("Escape");
  await mode("NORMAL").waitFor();
  await page.keyboard.press("v");
  await mode("VISUAL").waitFor();
  await page.keyboard.type("  ");
  assert((await explorer.count()) === 0, "Space Space opened explorer in VISUAL mode");
  await page.keyboard.press("Escape");

  await page.keyboard.type("  ");
  await explorer.waitFor();
  const search = explorer.getByPlaceholder("Search notes…");
  await page.waitForFunction(() => document.activeElement?.getAttribute("placeholder") === "Search notes…");
  await page.keyboard.press("Escape");
  await explorer.waitFor({ state: "detached" });
  await editor.waitFor({ state: "visible" });
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));

  await page.keyboard.type("  ");
  await explorer.waitFor();
  await search.fill("Project");
  const projectResult = explorer.locator('[data-slot="command-item"]', { hasText: "Project note" });
  await projectResult.waitFor();
  const projectMetadata = await projectResult.innerText();
  assert(projectMetadata.toLowerCase().includes("note") && projectMetadata.includes("2026-08-03"), "Explorer result metadata is incorrect");
  await explorer.getByText("# Project note", { exact: false }).waitFor();
  await explorer.getByText("[[note:999|Missing]]", { exact: false }).waitFor();
  await page.screenshot({ path: process.env.ARCHIVE_UI_SCREENSHOT ?? "/tmp/archive-ui-smoke.png" });
  await page.keyboard.press("Enter");
  await page.getByRole("heading", { name: "Project note" }).waitFor();
  await page.locator(".cm-reference-broken", { hasText: "Missing" }).waitFor();
  assert((await calls("resolve_references")).some((call) => call.args.ids.includes(999)), "Active references were not batch resolved");

  const createCountBefore = (await calls("create_note")).length;
  await page.keyboard.press("Control+n");
  await page.getByRole("heading", { name: "Untitled note" }).waitFor();
  assert((await calls("create_note")).length === createCountBefore + 1, "Ctrl+N did not invoke create_note exactly once");
  assert((await editor.count()) === 1, "Ctrl+N rendered more than one editor");
  await content.click();
  await page.keyboard.press("i");
  await mode("INSERT").waitFor();
  await page.keyboard.type("Draft body ");
  await page.evaluate(async () => {
    await window.__tauriCallbacks[0]({
      event: "tauri://close-requested",
      id: 1,
      payload: null,
    });
  });
  let updates = await calls("update_document_body");
  assert(updates.some((call) => call.args.id === 5 && call.args.expectedRevision === 1 && call.args.body === "Draft body "), `Standalone body did not autosave from revision 1: ${JSON.stringify(updates)}`);
  assert((await calls("plugin:window|destroy")).length === 1, "Close request did not flush before destroy");
  const closeOrder = await page.evaluate(() => [
    window.__archiveCalls.findIndex(
      (call) => call.command === "update_document_body" && call.args.id === 5,
    ),
    window.__archiveCalls.findIndex((call) => call.command === "plugin:window|destroy"),
  ]);
  assert(closeOrder[0] >= 0 && closeOrder[0] < closeOrder[1], "Window destroyed before pending body persisted");
  await page.keyboard.press("Escape");

  await page.keyboard.press("Shift+H");
  await page.getByRole("heading", { name: "Project note" }).waitFor();
  await page.waitForFunction(() => document.querySelector("footer")?.textContent?.includes("2/3"));
  await page.waitForFunction(() => document.querySelector("footer")?.textContent?.includes("NORMAL"));
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.press("Shift+L");
  await page.getByRole("heading", { name: /Draft body/ }).waitFor();
  assert((await page.locator("footer").innerText()).includes("3/3 · Draft body"), "Status bar does not show buffer position and label");
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.waitForTimeout(50);

  await page.keyboard.type("  ");
  await explorer.waitFor();
  await search.fill("Project");
  await projectResult.waitFor();
  await page.keyboard.press("Control+Enter");
  await explorer.waitFor({ state: "detached" });
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  const expectedSourceWithReference = "Draft body[[note:2|Project note]] ";
  await page.waitForTimeout(650);
  updates = await calls("update_document_body");
  assert(updates.some((call) => call.args.id === 5 && call.args.expectedRevision === 2 && call.args.body === expectedSourceWithReference), "Inserted raw reference did not reach autosave unchanged from revision 2");
  await page.locator(".cm-reference-note", { hasText: "Project note" }).waitFor();

  await page.keyboard.press("0");
  for (let index = 0; index < 12; index += 1) await page.keyboard.press("l");
  await page.locator(".cm-reference-note").waitFor({ state: "detached" });
  assert((await editorBody()).includes("[[note:2|Project note]]"), "Cursor inside reference did not reveal raw source");
  await page.keyboard.press("Enter");
  await page.getByRole("heading", { name: "Project note" }).waitFor();
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.press("j");
  await page.keyboard.press("Enter");
  await page.getByText("Could not open note: Document 999 does not exist").waitFor();
  assert((await editorBody()) === "# Project note\n[[note:999|Missing]]", "Missing reference navigation modified the project note");
  assert((await editor.count()) === 1, "Missing reference navigation replaced the editor");

  await page.keyboard.press("0");
  await page.keyboard.press("v");
  await page.keyboard.press("l");
  const focusedSelection = await selectionRendering("characterwise visual");
  await page.evaluate(() => { document.body.tabIndex = -1; document.body.focus(); });
  await editor.locator(":scope:not(.cm-focused)").waitFor();
  const inactiveSelection = await selectionRendering("inactive characterwise visual");
  assert(focusedSelection.some((range, index) => range.background !== inactiveSelection[index]?.background), "Focused and inactive selections are indistinguishable");
  await content.focus();
  await page.keyboard.press("Escape");
  await page.keyboard.press("0");
  await page.keyboard.press("Shift+V");
  const characterWidth = Math.max(...focusedSelection.map((range) => range.width));
  await page.waitForFunction((width) => [...document.querySelectorAll(".cm-selectionLayer .cm-selectionBackground")].some((range) => range.getBoundingClientRect().width > width), characterWidth * 2);
  await selectionRendering("linewise visual");
  await page.keyboard.press("Escape");

  await page.keyboard.press("Control+o");
  const noteDelete = page.locator('[data-slot="command-item"]', { hasText: "Delete note permanently…" });
  await noteDelete.waitFor();
  assert((await noteDelete.getAttribute("data-disabled")) !== "true", "Standalone delete action is disabled");
  await noteDelete.click();
  await page.getByRole("alertdialog", { name: "Permanently delete this note?" }).waitFor();
  await page.getByRole("button", { name: "Cancel" }).click();
  assert((await calls("delete_note")).length === 0, "Cancel deleted the note");
  await page.keyboard.press("Control+o");
  await noteDelete.click();
  await page.getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("heading", { name: /Draft body/ }).waitFor();
  const deletes = await calls("delete_note");
  assert(deletes.length === 1 && deletes[0].args.id === 2, "Confirmation did not delete the active standalone note exactly once");
  await page.waitForFunction(() => document.querySelector("footer")?.textContent?.includes("2/2"));
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.press("Shift+H");
  await page.getByRole("heading", { name: "Monday, August 3, 2026" }).waitFor();
  await page.keyboard.press("Control+o");
  await dailyDelete.waitFor();
  assert((await dailyDelete.getAttribute("data-disabled")) === "true", "Daily delete became available after deletion");
  await page.keyboard.press("Escape");

  await content.click();
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.type("  ");
  await explorer.waitFor();
  await search.fill("Agent research");
  const artifactResult = explorer.locator('[data-slot="command-item"]', { hasText: "Agent research artifact" });
  await artifactResult.waitFor();
  const artifactMetadata = await artifactResult.innerText();
  assert(artifactMetadata.toLowerCase().includes("artifact") && artifactMetadata.toLowerCase().includes("agent"), `Agent artifact metadata is not distinguished: ${artifactMetadata}`);
  await page.keyboard.press("Enter");
  await page.getByRole("heading", { name: "Agent research artifact" }).waitFor();
  assert((await page.locator("footer").innerText()).includes("Artifact · Agent"), "Agent artifact status is not distinguished");
  const diagram = page.locator("button.cm-mermaid");
  await diagram.locator("svg").waitFor();
  assert((await calls("render_mermaid")).some((call) => call.args.source === "graph TD\nA-->B"), "Inactive Mermaid source was not rendered lazily");
  assert((await page.locator(".editor-toolbar, .editor-topbar, .editor-split, [data-mermaid-toolbar]").count()) === 0, "Mermaid added application chrome");
  await diagram.click();
  await page.locator(".cm-mermaid-preview svg").waitFor();
  assert((await editorBody()).includes("```mermaid\ngraph TD\nA-->B\n```"), "Click did not reveal canonical Mermaid source");
  await page.keyboard.press("Escape");
  await page.keyboard.press("k");
  await diagram.waitFor();
  await page.keyboard.press("v");
  await page.keyboard.press("j");
  await page.locator(".cm-mermaid-preview").waitFor();
  assert((await editorBody()).includes("```mermaid"), "Visual selection intersecting Mermaid did not reveal source");
  await page.keyboard.press("Escape");
  await page.keyboard.press("j");
  await page.keyboard.press("i");
  await page.keyboard.type("INVALID ");
  await page.waitForTimeout(300);
  await page.locator(".cm-mermaid-diagnostic", { hasText: "Invalid test diagram" }).waitFor();
  const invalidBody = await editorBody();
  assert(invalidBody.includes("INVALID") && invalidBody.includes("```mermaid"), "Invalid Mermaid edit did not preserve raw source");
  const mermaidCalls = await calls("render_mermaid");
  assert(mermaidCalls.some((call) => call.args.source.includes("INVALID")), "Debounced active Mermaid preview did not render the edited source");
  await page.keyboard.press("Escape");

  const privateCountBefore = (await calls("create_note")).length;
  await page.keyboard.press("Control+Shift+n");
  await page.getByRole("heading", { name: "Untitled note" }).waitFor();
  const privateCalls = await calls("create_note");
  assert(privateCalls.length === privateCountBefore + 1 && privateCalls.at(-1).args.visibility === "private", "Ctrl+Shift+N did not create one private note");
  assert((await page.locator("footer").innerText()).includes("Private"), "Private active status is missing");
  await content.click();
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.type("  ");
  await explorer.waitFor();
  await search.fill("Untitled");
  const privateResult = explorer.locator('[data-slot="command-item"]', { hasText: "Untitled note" }).filter({ hasText: "Private" });
  await privateResult.waitFor();
  assert((await privateResult.innerText()).toLowerCase().includes("private"), "Private explorer metadata is missing");

  await browser.close();
} finally {
  server.kill("SIGTERM");
}
