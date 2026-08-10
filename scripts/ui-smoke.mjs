import { spawn } from "node:child_process";
import { chromium } from "playwright-core";

const server = spawn("npm", ["exec", "vite", "preview", "--", "--host", "127.0.0.1", "--port", "4173", "--strictPort"], {
  stdio: ["ignore", "pipe", "pipe"],
});
let serverExit = null;
let serverOutput = "";
server.stdout.on("data", (chunk) => { serverOutput += chunk; });
server.stderr.on("data", (chunk) => { serverOutput += chunk; });
server.once("exit", (code, signal) => { serverExit = { code, signal }; });

async function waitForServer() {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    if (serverExit) throw new Error(`Vite preview exited before readiness (${serverExit.code ?? serverExit.signal})\n${serverOutput}`);
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
      { id: 1, kind: "daily", visibility: "shared", author: "user", day: "2026-08-03", created_at: timestamp, updated_at: timestamp, body: Array.from({ length: 140 }, (_, index) => `viewport line ${String(index + 1).padStart(3, "0")}`).join("\n"), revision: 1 },
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
        id: 100,
        kind: "daily",
        visibility: "shared",
        author: "user",
        day: "2026-08-04",
        created_at: "2026-08-04T10:00:00.000Z",
        updated_at: "2026-08-04T10:00:00.000Z",
        body: "Tuesday notes",
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
        body: `# Agent research artifact\n${Array.from({ length: 140 }, (_, index) => `artifact line ${String(index + 1).padStart(3, "0")}`).join("\n")}\n\`\`\`mermaid\ngraph TD\nA-->B\n\`\`\``,
        revision: 1,
      },
      {
        id: 50,
        kind: "artifact",
        visibility: "shared",
        author: "agent",
        day: "2026-08-03",
        created_at: timestamp,
        updated_at: timestamp,
        body: "# Failed agent run\nFailure details",
        revision: 1,
      },
      {
        id: 51,
        kind: "artifact",
        visibility: "shared",
        author: "agent",
        day: "2026-08-03",
        created_at: timestamp,
        updated_at: timestamp,
        body: "# Completed agent run\nCompleted details",
        revision: 1,
      },
    ];
    let nextId = 5;
    const projectDocuments = new Map();
    const attachments = [
      { artifact_id: 4, title: "Agent research artifact", day: "2026-08-03", status: "blocked", created_at: timestamp, updated_at: timestamp, reviewed_at: null },
      { artifact_id: 50, title: "Failed agent run", day: "2026-08-03", status: "failed", created_at: timestamp, updated_at: timestamp, reviewed_at: timestamp },
      { artifact_id: 51, title: "Completed agent run", day: "2026-08-03", status: "completed", created_at: timestamp, updated_at: timestamp, reviewed_at: null },
    ];
    window.__archiveCalls = [];
    window.__tauriCallbacks = [];
    window.__clipboardFailure = false;
    window.__clipboardText = "clipboard";
    const presence = { user_count: 1, agent_present: false };
    const syncQueue = [];
    window.__archiveSync = {
      presence,
      queue: syncQueue,
      revision(id) {
        return documents.find((candidate) => candidate.id === id)?.revision;
      },
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
        if (command === "create_project") {
          const project = { id: nextId++, kind: "project", visibility: args.visibility, author: "user", day: args.day, created_at: timestamp, updated_at: timestamp, body: "", revision: 1 };
          documents.push(project);
          projectDocuments.set(project.id, []);
          return { ...project };
        }
        if (command === "add_document_to_project") {
          const members = projectDocuments.get(args.projectId);
          const document = documents.find((candidate) => candidate.id === args.documentId);
          if (!members || !document || document.kind === "project") throw new Error("Document does not exist");
          if (!members.includes(args.documentId)) members.push(args.documentId);
          return null;
        }
        if (command === "list_project_documents") {
          return (projectDocuments.get(args.projectId) ?? []).map((id) => ({ ...documents.find((document) => document.id === id) }));
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
          projectDocuments.delete(documents[index].id);
          for (const members of projectDocuments.values()) {
            const memberIndex = members.indexOf(args.id);
            if (memberIndex >= 0) members.splice(memberIndex, 1);
          }
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
        if (command === "daily_neighbors") {
          const days = documents.filter((document) => document.kind === "daily").sort((left, right) => left.day.localeCompare(right.day));
          const previous = [...days].reverse().find((document) => document.day < args.day);
          const next = days.find((document) => document.day > args.day);
          return {
            previous: previous ? { id: previous.id, day: previous.day } : null,
            next: next ? { id: next.id, day: next.day } : null,
          };
        }
        if (command === "list_daily_attachments") return attachments.filter((item) => item.day === args.day).map((item) => ({ ...item }));
        if (command === "list_unreviewed_attachments") return attachments.filter((item) => item.reviewed_at === null).map((item) => ({ ...item }));
        if (command === "get_attachment_by_artifact_id") return attachments.find((item) => item.artifact_id === args.artifactId) ?? null;
        if (command === "mark_attachment_reviewed") {
          const attachment = attachments.find((item) => item.artifact_id === args.artifactId);
          if (!attachment) throw new Error("Attachment does not exist");
          attachment.reviewed_at ??= timestamp;
          return { ...attachment };
        }
        if (command === "render_markdown") {
          const escaped = args.markdown.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
          return escaped.replace(/^# (.+)$/gm, "<h1>$1</h1>").replace(/```mermaid\n([\s\S]*?)```/g, '<pre><code class="language-mermaid">$1</code></pre>').replaceAll("\n", "<br>");
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

  const editor = page.locator('[data-editor-active="true"] .cm-editor');
  const content = page.locator('[data-editor-active="true"] .cm-content');
  const retainedEditors = page.locator("[data-document-id] .cm-editor");
  const explorer = page.getByRole("dialog", { name: "Explore notes" });
  const mode = (name) => page.locator("footer", { hasText: name });
  const editorBody = async () => (await content.innerText()).replace(/\n$/, "");
  const calls = (command) => page.evaluate((name) => window.__archiveCalls.filter((call) => call.command === name), command);
  const cursorLine = () => editor.evaluate((root) => {
    const cursor = root.querySelector(".cm-vimCursorLayer .cm-fat-cursor.cm-cursor-primary")?.getBoundingClientRect();
    if (!cursor) return null;
    const y = cursor.top + cursor.height / 2;
    return [...root.querySelectorAll(".cm-line")].find((line) => {
      const box = line.getBoundingClientRect();
      return y >= box.top && y <= box.bottom;
    })?.textContent ?? null;
  });

  async function leader(key) {
    await page.keyboard.press("Space");
    await page.keyboard.press(key);
  }

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
  const scroller = editor.locator(".cm-scroller");
  const viewport = await scroller.evaluate((element) => ({ scrollHeight: element.scrollHeight, clientHeight: element.clientHeight }));
  assert(viewport.scrollHeight > viewport.clientHeight && viewport.clientHeight > 0, "Long active editor does not have a bounded viewport");
  await content.click();
  await page.keyboard.press("Escape");
  const clipboardReadsBeforeVisualBlock = (await calls("plugin:clipboard-manager|read_text")).length;
  await page.keyboard.press("Control+v");
  await mode("VISUAL BLOCK").waitFor();
  assert((await calls("plugin:clipboard-manager|read_text")).length === clipboardReadsBeforeVisualBlock, "Ctrl-V read the Tauri clipboard instead of entering Visual Block");
  await page.keyboard.press("Escape");
  await page.keyboard.type("gg10j");
  const jumpTarget = await cursorLine();
  await page.keyboard.press("Shift+G");
  await page.keyboard.press("Control+o");
  assert((await cursorLine()) === jumpTarget, "Same-editor Ctrl-O did not restore the seeded jump cursor");
  const createCallsBeforeNativeKeys = (await calls("create_note")).length;
  const commandCallsBeforeNativeKeys = (await calls("search_documents")).length;
  const lineBeforeCtrlN = await cursorLine();
  await page.keyboard.press("Control+n");
  const lineAfterCtrlN = await cursorLine();
  assert(lineAfterCtrlN !== lineBeforeCtrlN, "Ctrl-N did not perform native Vim cursor movement");
  await page.keyboard.press("Enter");
  assert((await cursorLine()) !== lineAfterCtrlN, "Enter did not perform native Vim line movement");
  assert((await calls("create_note")).length === createCallsBeforeNativeKeys && (await calls("search_documents")).length === commandCallsBeforeNativeKeys, "Native Ctrl-N or Enter triggered an Archive action");
  await page.keyboard.type("gg60j");
  const scrollBeforeDown = await scroller.evaluate((element) => element.scrollTop);
  await page.keyboard.press("Control+d");
  await page.waitForTimeout(30);
  const scrollAfterDown = await scroller.evaluate((element) => element.scrollTop);
  assert(scrollAfterDown > scrollBeforeDown, "Ctrl-D did not move the active editor viewport down");
  await page.keyboard.press("Control+u");
  await page.waitForTimeout(30);
  const scrollAfterUp = await scroller.evaluate((element) => element.scrollTop);
  assert(scrollAfterUp < scrollAfterDown, "Ctrl-U did not move the active editor viewport up");
  await page.keyboard.press("Control+f");
  await page.waitForTimeout(30);
  const scrollAfterForward = await scroller.evaluate((element) => element.scrollTop);
  assert(scrollAfterForward > scrollAfterUp, "Ctrl-F did not move the active editor viewport down");
  await page.keyboard.press("Control+b");
  await page.waitForTimeout(30);
  const scrollAfterBack = await scroller.evaluate((element) => element.scrollTop);
  assert(scrollAfterBack < scrollAfterForward, "Ctrl-B did not move the active editor viewport up");

  async function cursorViewportGeometry() {
    return editor.evaluate((root) => {
      const cursorElement = root.querySelector(".cm-vimCursorLayer .cm-fat-cursor.cm-cursor-primary");
      const scrollerElement = root.querySelector(".cm-scroller");
      if (!cursorElement || !scrollerElement) return null;
      const cursor = cursorElement.getBoundingClientRect();
      const scroll = scrollerElement.getBoundingClientRect();
      return {
        cursorWidth: cursor.width,
        cursorHeight: cursor.height,
        scrollerWidth: scroll.width,
        scrollerHeight: scroll.height,
        ratio: (cursor.top + cursor.height / 2 - scroll.top) / scroll.height,
      };
    });
  }
  async function waitForCursorBand(min, max) {
    await page.waitForFunction(({ min, max }) => {
      const cursorElement = document.querySelector('[data-editor-active="true"] .cm-vimCursorLayer .cm-fat-cursor.cm-cursor-primary');
      const scrollerElement = document.querySelector('[data-editor-active="true"] .cm-scroller');
      if (!cursorElement || !scrollerElement) return false;
      const cursor = cursorElement.getBoundingClientRect();
      const scroll = scrollerElement.getBoundingClientRect();
      if (cursor.width <= 0 || cursor.height <= 0 || scroll.width <= 0 || scroll.height <= 0) return false;
      const ratio = (cursor.top + cursor.height / 2 - scroll.top) / scroll.height;
      return ratio > min && ratio < max;
    }, { min, max });
  }
  const initialCursorGeometry = await cursorViewportGeometry();
  assert(initialCursorGeometry !== null, "Visible primary Vim cursor or active editor scroller does not exist");
  assert(initialCursorGeometry.cursorWidth > 0 && initialCursorGeometry.cursorHeight > 0 && initialCursorGeometry.scrollerWidth > 0 && initialCursorGeometry.scrollerHeight > 0, "Visible primary Vim cursor or active editor scroller has non-positive geometry");
  await page.keyboard.press("Shift+H");
  await waitForCursorBand(-Number.EPSILON, 0.25);
  const highLine = await cursorLine();
  await page.keyboard.press("Shift+L");
  await waitForCursorBand(0.75, 1 + Number.EPSILON);
  const lowLine = await cursorLine();
  assert(highLine !== lowLine, "H and L did not move between actual high and low visible lines");
  await page.keyboard.type("zz");
  await waitForCursorBand(0.35, 0.65);
  const centeredCursor = (await cursorViewportGeometry()).ratio;
  assert(centeredCursor > 0.35 && centeredCursor < 0.65, `zz did not center the cursor (${centeredCursor})`);
  await page.keyboard.type("zt");
  await waitForCursorBand(-Number.EPSILON, 0.2);
  const topCursor = (await cursorViewportGeometry()).ratio;
  assert(topCursor >= 0 && topCursor < 0.2, `zt did not place the cursor near the top (${topCursor})`);
  await page.keyboard.type("zb");
  await waitForCursorBand(0.8, 1 + Number.EPSILON);
  const bottomCursor = (await cursorViewportGeometry()).ratio;
  assert(bottomCursor > 0.8 && bottomCursor <= 1, `zb did not place the cursor near the bottom (${bottomCursor})`);

  const main = page.locator("main");
  await main.evaluate((element) => { element.scrollTop = element.scrollHeight; });
  await page.getByRole("button", { name: /Agent work · 3/ }).waitFor({ state: "visible" });
  assert(await page.getByRole("button", { name: /Agent work · 3/ }).isVisible(), "Outer scrolling cannot reach the active daily shelf");
  const footer = page.locator("footer");
  const identityStatus = footer.locator('[data-status-region="identity"]');
  const documentStatus = footer.locator('[data-status-region="document"]');
  const persistenceStatus = footer.locator('[data-status-region="persistence"]');
  const footerBox = await footer.boundingBox();
  const documentBox = await documentStatus.boundingBox();
  assert(footerBox && documentBox, "Status geometry is unavailable");
  assert(Math.abs(documentBox.x + documentBox.width / 2 - (footerBox.x + footerBox.width / 2)) < 1, "Document status is not centered against the full window");
  assert((await identityStatus.innerText()).includes("ARCHIVE") && (await documentStatus.innerText()).includes("1/1 · Monday, August 3, 2026"), "Stable status regions contain the wrong information");
  assert((await persistenceStatus.innerText()) === "", "Persistence status is not initially empty");
  await scroller.evaluate((element) => { element.scrollTop = Math.floor(element.scrollHeight / 3); });
  const dailyEditorRoot = await page.locator('[data-document-id="1"] .cm-editor').elementHandle();
  await content.click();
  await page.keyboard.press("Escape");
  await page.keyboard.type("ggma");
  const beforeRetainedAction = await editorBody();
  await page.keyboard.press("x");
  const afterRetainedAction = await editorBody();
  assert(afterRetainedAction !== beforeRetainedAction, "Seed non-insert action did not change A");
  const retainedScroll = await scroller.evaluate((element) => element.scrollTop);
  await leader("Space");
  await explorer.waitFor();
  await explorer.getByPlaceholder("Search notes…").fill("Project");
  await explorer.locator('[data-slot="command-item"]', { hasText: "Project note" }).waitFor();
  await page.keyboard.press("Enter");
  await page.getByRole("heading", { name: "Project note" }).waitFor();
  await content.click();
  await page.keyboard.press("Escape");
  await page.keyboard.type("[b");
  await page.getByRole("heading", { name: "Monday, August 3, 2026" }).waitFor();
  await editor.waitFor();
  assert(await dailyEditorRoot.evaluate((root) => root === document.querySelector('[data-document-id="1"] .cm-editor')), "A editor DOM identity changed across A to B to A");
  const reactivatedGeometry = await editor.boundingBox();
  assert(reactivatedGeometry && reactivatedGeometry.width > 0 && reactivatedGeometry.height > 0, "Reactivated A editor has zero geometry");
  assert((await scroller.evaluate((element) => element.scrollTop)) === retainedScroll, "Reactivated A editor lost internal scroll");
  const inactiveB = page.locator('[data-document-id="2"] .cm-content');
  const inactiveBody = await inactiveB.innerText();
  await inactiveB.evaluate((element) => element.focus());
  await page.keyboard.press("x");
  assert((await inactiveB.innerText()) === inactiveBody && !await inactiveB.evaluate((element) => element === document.activeElement), "Inactive retained B accepted browser focus or input");
  await content.focus();
  await page.keyboard.press(".");
  const afterRetainedRepeat = await editorBody();
  assert(afterRetainedRepeat !== afterRetainedAction, "A lost its non-insert repeat action across A to B to A");
  await page.keyboard.press("u");
  assert((await editorBody()) === afterRetainedAction, "A undo did not remain scoped to the retained A editor");
  await page.keyboard.type("'a");
  assert((await cursorLine())?.startsWith("iewport line 001"), "A mark did not survive A to B to A");
  await page.keyboard.press("u");
  assert((await editorBody()) === beforeRetainedAction, "A retained undo history did not restore its original body");
  await page.keyboard.press("Shift+H");
  await page.keyboard.press("Shift+L");
  for (const key of ["Control+d", "Control+u", "Control+f", "Control+b"]) await page.keyboard.press(key);
  for (const command of ["zz", "zt", "zb"]) await page.keyboard.type(command);
  const postReactivationGeometry = await cursorViewportGeometry();
  assert(postReactivationGeometry && postReactivationGeometry.cursorWidth > 0 && postReactivationGeometry.cursorHeight > 0 && postReactivationGeometry.scrollerWidth > 0 && postReactivationGeometry.scrollerHeight > 0, "Reactivated A viewport motions lost measured geometry");
  await page.setViewportSize({ width: 420, height: 720 });
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), "Active editor overflows horizontally at 420px");
  await scroller.evaluate((element) => { element.scrollTop = element.scrollHeight; });
  await content.focus();
  await scroller.evaluate((element) => { element.scrollTop = element.scrollHeight; });
  let leftEditor = false;
  for (let index = 0; index < 20 && !leftEditor; index += 1) {
    await page.keyboard.press("Tab");
    leftEditor = await page.evaluate(() => !document.activeElement?.closest(".cm-editor"));
  }
  assert(leftEditor, "Tab cannot leave CodeMirror for surrounding controls");
  await main.evaluate((element) => { element.scrollTop = 0; });
  await scroller.hover();
  const outerBeforeWheel = await main.evaluate((element) => element.scrollTop);
  await page.mouse.wheel(0, 800);
  await page.waitForTimeout(50);
  const outerAfterWheel = await main.evaluate((element) => element.scrollTop);
  assert(outerAfterWheel > outerBeforeWheel, "Wheel scrolling did not chain to the outer page at the editor edge");
  await main.evaluate((element) => { element.scrollTop = element.scrollHeight; });
  assert(await page.getByRole("button", { name: /Agent work · 3/ }).isVisible(), "Active shelf is unreachable at 420px");
  const narrowStatus = await documentStatus.evaluate((element) => ({
    overflow: getComputedStyle(element).overflow,
    whiteSpace: getComputedStyle(element).whiteSpace,
    textOverflow: getComputedStyle(element).textOverflow,
    footerOverflow: element.closest("footer").scrollWidth - element.closest("footer").clientWidth,
  }));
  const narrowFooterBox = await footer.boundingBox();
  const narrowDocumentBox = await documentStatus.boundingBox();
  assert(narrowFooterBox && narrowDocumentBox && Math.abs(narrowDocumentBox.x + narrowDocumentBox.width / 2 - (narrowFooterBox.x + narrowFooterBox.width / 2)) < 1, "Narrow document status lost full-window centering");
  assert(narrowStatus.overflow === "hidden" && narrowStatus.whiteSpace === "nowrap" && narrowStatus.textOverflow === "ellipsis" && narrowStatus.footerOverflow <= 0, "Narrow status does not truncate without overflowing");
  await page.setViewportSize({ width: 960, height: 720 });
  await content.focus();
  await page.keyboard.press("Escape");
  const startupCalls = await calls("get_or_create_daily");
  assert(startupCalls.length === 1 && startupCalls[0].args.day === "2026-08-03", "Startup did not request the canonical daily");
  const documentCallsBeforeH = (await calls("get_document")).length;
  await page.keyboard.press("Shift+H");
  assert((await page.getByRole("heading", { name: "Monday, August 3, 2026" }).count()) === 1, "H changed documents");
  assert((await calls("get_document")).length === documentCallsBeforeH, "H attempted to open a document");
  await page.getByRole("button", { name: "Read" }).click();
  await page.locator(".archive-reader").waitFor();
  await page.getByRole("button", { name: "Edit" }).click();
  await editor.waitFor();

  const cleanRemote = "shared line\nstable line";
  const cleanUpdateCount = (await calls("update_document_body")).length;
  const cleanRemoteRevision = await page.evaluate(() => window.__archiveSync.revision(1) + 1);
  await page.evaluate(({ body, revision }) => {
    window.__archiveSync.setPresence(2, true);
    window.__archiveSync.pushRemote(1, body, revision);
  }, { body: cleanRemote, revision: cleanRemoteRevision });
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
  await page.evaluate(({ body, revision }) => window.__archiveSync.pushRemote(1, body, revision), { body: firstRemote, revision: cleanRemoteRevision + 1 });
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
  await page.evaluate(({ body, revision }) => window.__archiveSync.pushRemote(1, body, revision), { body: secondRemote, revision: cleanRemoteRevision + 2 });
  await conflictDialog.waitFor({ timeout: 850 });
  assert((await editorBody()) === secondLocal, "Second conflict did not preserve the exact local source");
  assert((await calls("update_document_body")).length === writesBeforeSecondConflict, "Autosave wrote an unresolved second conflict");
  await conflictDialog.getByRole("button", { name: "Keep mine" }).click();
  await conflictDialog.waitFor({ state: "detached" });
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  const keepMineWrites = await calls("update_document_body");
  assert(keepMineWrites.some((call) => call.args.id === 1 && call.args.expectedRevision === cleanRemoteRevision + 2 && call.args.body === secondLocal), "Keep mine did not write the exact local source against the remote revision");
  assert((await editorBody()) === secondLocal, "Keep mine changed the local source");

  await page.evaluate((revision) => window.__archiveSync.pushRemote(1, "", revision), cleanRemoteRevision + 4);
  await page.waitForFunction(() => document.querySelector(".cm-content")?.innerText.replace(/\n$/, "") === "", { timeout: 450 });
  assert((await page.locator(".editor-toolbar, .editor-topbar, .editor-split, .sidebar, [role=tablist], [data-mermaid-toolbar], [data-sync-toolbar], [data-sync-status]").count()) === 0, "Sync or presence added permanent application chrome");

  await leader("c");
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
  await page.waitForTimeout(10);
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
  await page.getByRole("button", { name: "Read" }).click();
  await page.locator(".archive-reader").waitFor();
  assert((await editor.count()) === 0 && (await retainedEditors.count()) === 2, "Read mode did not retain exactly the open user editors while hiding the active one");
  await page.getByRole("button", { name: "Edit" }).click();
  await editor.waitFor();
  assert((await page.locator(".archive-reader").count()) === 0, "Edit mode left the user document reader mounted");

  const createCountBefore = (await calls("create_note")).length;
  await content.click();
  await leader("n");
  await page.getByRole("heading", { name: "Untitled note" }).waitFor();
  assert((await calls("create_note")).length === createCountBefore + 1, "Space n did not invoke create_note exactly once");
  assert((await editor.count()) === 1, "Space n rendered more than one editor");
  await content.click();
  await page.keyboard.press("i");
  await mode("INSERT").waitFor();
  await page.keyboard.type("Draft body ");
  const savingFooterBox = await footer.boundingBox();
  const savingDocumentBox = await documentStatus.boundingBox();
  const savingPersistenceBox = await persistenceStatus.boundingBox();
  assert((await persistenceStatus.textContent()) === "Saving…", "Transient save status is not in the persistence region");
  assert(savingFooterBox && savingDocumentBox && savingPersistenceBox, "Saving status geometry is unavailable");
  assert(Math.abs(savingDocumentBox.x + savingDocumentBox.width / 2 - (savingFooterBox.x + savingFooterBox.width / 2)) < 1, "Saving status shifted the centered document information");
  assert(savingPersistenceBox.x + savingPersistenceBox.width > savingFooterBox.x + savingFooterBox.width / 2 && savingPersistenceBox.x + savingPersistenceBox.width <= savingFooterBox.x + savingFooterBox.width, "Saving status is not bottom-right aligned");
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

  await page.keyboard.type("[b");
  await page.getByRole("heading", { name: "Project note" }).waitFor();
  await page.waitForFunction(() => document.querySelector("footer")?.textContent?.includes("2/3"));
  await page.waitForFunction(() => document.querySelector("footer")?.textContent?.includes("NORMAL"));
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.type("]b");
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
  await page.waitForFunction((body) => window.__archiveCalls.some((call) =>
    call.command === "update_document_body" && call.args.id === 5 && call.args.expectedRevision === 2 && call.args.body === body
  ), expectedSourceWithReference, { timeout: 2_000 });
  updates = await calls("update_document_body");
  assert(updates.some((call) => call.args.id === 5 && call.args.expectedRevision === 2 && call.args.body === expectedSourceWithReference), `Inserted raw reference did not reach autosave unchanged from revision 2: ${JSON.stringify(updates.filter((call) => call.args.id === 5))}`);
  await page.locator(".cm-reference-note", { hasText: "Project note" }).waitFor();

  await page.keyboard.press("0");
  for (let index = 0; index < 12; index += 1) await page.keyboard.press("l");
  await page.locator(".cm-reference-note").waitFor({ state: "detached" });
  assert((await editorBody()).includes("[[note:2|Project note]]"), "Cursor inside reference did not reveal raw source");
  await page.keyboard.type("gf");
  await page.getByRole("heading", { name: "Project note" }).waitFor();
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.press("j");
  await page.keyboard.type("gf");
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

  const survivorRoot = await page.locator('[data-document-id="5"] .cm-editor').elementHandle();
  const editorsBeforeDeletion = await retainedEditors.count();
  await leader("c");
  const noteDelete = page.locator('[data-slot="command-item"]', { hasText: "Delete note permanently…" });
  await noteDelete.waitFor();
  assert((await noteDelete.getAttribute("data-disabled")) !== "true", "Standalone delete action is disabled");
  await noteDelete.click();
  await page.getByRole("alertdialog", { name: "Permanently delete this note?" }).waitFor();
  await page.getByRole("button", { name: "Cancel" }).click();
  assert((await calls("delete_note")).length === 0, "Cancel deleted the note");
  await content.click();
  await leader("c");
  await noteDelete.click();
  await page.getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("heading", { name: /Draft body/ }).waitFor();
  const deletes = await calls("delete_note");
  assert(deletes.length === 1 && deletes[0].args.id === 2, "Confirmation did not delete the active standalone note exactly once");
  assert((await retainedEditors.count()) === editorsBeforeDeletion - 1, "Deletion did not remove exactly one editor");
  assert(await survivorRoot.evaluate((root) => root === document.querySelector('[data-document-id="5"] .cm-editor')), "Deletion replaced a surviving editor DOM identity");
  await page.waitForFunction(() => document.querySelector("footer")?.textContent?.includes("2/2"));
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.type("[b");
  await page.getByRole("heading", { name: "Monday, August 3, 2026" }).waitFor();
  await content.click();
  await leader("c");
  await dailyDelete.waitFor();
  assert((await dailyDelete.getAttribute("data-disabled")) === "true", "Daily delete became available after deletion");
  await page.keyboard.press("Escape");

  const dailyLookupsBeforeBrowsing = (await calls("get_or_create_daily")).length;
  await page.getByRole("button", { name: /Previous daily/ }).click();
  await page.getByRole("heading", { name: "Sunday, August 2, 2026" }).waitFor();
  await page.getByRole("button", { name: /Next daily/ }).click();
  await page.getByRole("heading", { name: "Monday, August 3, 2026" }).waitFor();
  await page.getByRole("button", { name: /Next daily/ }).click();
  await page.getByRole("heading", { name: "Tuesday, August 4, 2026" }).waitFor();
  assert((await calls("get_or_create_daily")).length === dailyLookupsBeforeBrowsing, "Existing daily browsing called get_or_create_daily");
  await page.getByRole("button", { name: "Today" }).click();
  await page.getByRole("heading", { name: "Monday, August 3, 2026" }).waitFor();
  const dailyLookupsAfterToday = await calls("get_or_create_daily");
  assert(dailyLookupsAfterToday.length === dailyLookupsBeforeBrowsing + 1 && dailyLookupsAfterToday.at(-1).args.day === "2026-08-03", "Today was not the explicit canonical daily lookup");
  const shelf = page.getByRole("button", { name: /Agent work · 3/ });
  await shelf.waitFor();
  const shelfSummary = await shelf.innerText();
  assert(shelfSummary.includes("1 blocked") && shelfSummary.includes("1 failed") && shelfSummary.includes("2 New"), `Agent shelf summaries are not independent: ${shelfSummary}`);
  await shelf.click();
  const blockedShelfRow = await page.getByRole("button", { name: /Agent research artifact/ }).innerText();
  const failedShelfRow = await page.getByRole("button", { name: /Failed agent run/ }).innerText();
  const newShelfRow = await page.getByRole("button", { name: /Completed agent run/ }).innerText();
  assert(blockedShelfRow.toLowerCase().includes("blocked") && failedShelfRow.toLowerCase().includes("failed") && newShelfRow.toLowerCase().includes("new"), `Agent shelf rows do not distinguish blocked, failed, and New states: ${blockedShelfRow} | ${failedShelfRow} | ${newShelfRow}`);

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
  assert((await page.locator('[data-document-id="4"] .cm-editor').count()) === 0, "Agent artifact instantiated CodeMirror");
  const reader = page.locator(".archive-reader");
  await reader.locator("svg").waitFor();
  assert((await calls("render_mermaid")).length > 0, "Reader Mermaid source was not rendered");
  assert((await page.locator(".editor-toolbar, .editor-topbar, .editor-split, [data-mermaid-toolbar]").count()) === 0, "Mermaid added application chrome");
  assert((await page.getByText("· New").count()) === 1, "Unreviewed artifact provenance is missing");
  const artifactMain = page.locator("main");
  await artifactMain.evaluate((element) => { element.scrollTop = 0; });
  const readScroll = async (key) => {
    const before = await artifactMain.evaluate((element) => element.scrollTop);
    await page.keyboard.press(key);
    await page.waitForTimeout(20);
    return { before, after: await artifactMain.evaluate((element) => element.scrollTop) };
  };
  const lineDown = await readScroll("j");
  assert(lineDown.after > 0, "Read j did not scroll the outer canvas down");
  assert((await readScroll("k")).after < lineDown.after, "Read k did not scroll the outer canvas up");
  await page.keyboard.press("Shift+G");
  const artifactBottom = await artifactMain.evaluate((element) => element.scrollTop);
  assert(artifactBottom > 0, "Read G did not reach the outer document bottom");
  await page.keyboard.press("g");
  await page.evaluate(() => window.__archiveSync.setPresence(2, true));
  await page.waitForTimeout(450);
  await page.keyboard.press("g");
  assert((await artifactMain.evaluate((element) => element.scrollTop)) === 0, "Read gg did not reach the outer document top");
  for (const [down, up, name] of [["Control+d", "Control+u", "half-page"], ["Control+f", "Control+b", "full-page"]]) {
    const movedDown = await readScroll(down);
    assert(movedDown.after > movedDown.before, `Read ${name} down did not scroll the outer canvas`);
    const movedUp = await readScroll(up);
    assert(movedUp.after < movedUp.before, `Read ${name} up did not scroll the outer canvas`);
  }
  const artifactBufferStatus = await documentStatus.innerText();
  await page.keyboard.press("Shift+H");
  await page.waitForFunction((status) => document.querySelector('[data-status-region="document"]')?.textContent !== status, artifactBufferStatus);
  assert((await page.getByRole("button", { name: "Edit" }).count()) === 1, "Read H did not preserve Read on a user buffer");
  await page.keyboard.press("Shift+L");
  await page.getByRole("heading", { name: "Agent research artifact" }).waitFor();
  assert((await page.locator('[data-document-id="4"] .cm-editor').count()) === 0, "Read H/L instantiated artifact CodeMirror");
  await page.keyboard.press("Shift+H");
  const editButton = page.getByRole("button", { name: "Edit" });
  await editButton.focus();
  await page.keyboard.press("Space");
  await page.getByRole("button", { name: "Read" }).waitFor();
  await page.getByRole("button", { name: "Read" }).click();
  await page.keyboard.press("Shift+L");
  await page.getByRole("heading", { name: "Agent research artifact" }).waitFor();
  await leader("Space");
  await explorer.waitFor();
  const dialogCreateBefore = (await calls("create_note")).length;
  await search.press("Control+n");
  await search.press("Control+o");
  await explorer.press("Control+n");
  await explorer.press("Control+o");
  assert((await calls("create_note")).length === dialogCreateBefore && (await page.getByRole("dialog").count()) === 1, "Dialog input or surface leaked Ctrl-N/Ctrl-O to application shortcuts");
  await page.keyboard.press("Escape");
  await leader("c");
  await page.getByRole("dialog", { name: "Command Palette" }).waitFor();
  await page.keyboard.press("Escape");
  const readCreateBefore = (await calls("create_note")).length;
  await leader("n");
  await page.getByRole("heading", { name: "Untitled note" }).waitFor();
  assert((await calls("create_note")).length === readCreateBefore + 1 && (await calls("create_note")).at(-1).args.visibility === "shared", "Read Space n did not create a shared note");
  await page.getByRole("button", { name: "Read" }).click();
  await page.keyboard.press("Shift+H");
  await page.getByRole("heading", { name: "Agent research artifact" }).waitFor();
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await page.keyboard.press("Space");
  await page.evaluate(() => document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "N", shiftKey: true, bubbles: true, cancelable: true })));
  await page.getByRole("heading", { name: "Untitled note" }).waitFor();
  assert((await calls("create_note")).at(-1).args.visibility === "private", "Read Space N did not create a private note");
  await page.getByRole("button", { name: "Read" }).click();
  const privateReadStatus = await documentStatus.innerText();
  await page.keyboard.press("Shift+H");
  await page.waitForFunction((status) => document.querySelector('[data-status-region="document"]')?.textContent !== status, privateReadStatus);
  for (let index = 0; index < 10 && (await page.getByRole("heading", { name: "Agent research artifact" }).count()) === 0; index += 1) {
    const status = await documentStatus.innerText();
    await page.keyboard.press("Shift+H");
    await page.waitForFunction((previous) => document.querySelector('[data-status-region="document"]')?.textContent !== previous, status);
  }
  await page.getByRole("heading", { name: "Agent research artifact" }).waitFor();
  const markArtifactReviewed = page.getByRole("button", { name: "Mark reviewed" });
  await markArtifactReviewed.scrollIntoViewIfNeeded();
  await page.keyboard.press("f");
  assert((await page.locator("[data-read-hint]").count()) > 0, "Read f did not show hints for the artifact");
  await artifactMain.evaluate((element) => { element.scrollTop -= 80; });
  await page.locator("[data-read-hint]").first().waitFor({ state: "detached" });
  await markArtifactReviewed.scrollIntoViewIfNeeded();
  await page.keyboard.press("f");
  const markHintKey = await page.locator("[data-read-hint]").evaluateAll((badges, button) => {
    const target = button.getBoundingClientRect();
    const badge = badges.find((candidate) => {
      const rect = candidate.getBoundingClientRect();
      return Math.abs(rect.top - (target.top + 2)) < 2 && Math.abs(rect.left - (target.left + 2)) < 2;
    });
    return badge?.getAttribute("data-read-hint") ?? null;
  }, await markArtifactReviewed.elementHandle());
  assert(markHintKey, "Could not identify the Mark reviewed hint by geometry");
  await page.keyboard.type(markHintKey);
  await page.getByText("· Reviewed").waitFor();
  let reviewCalls = await calls("mark_attachment_reviewed");
  assert(reviewCalls.length === 1 && reviewCalls[0].args.artifactId === 4, "Read f did not mark artifact 4 reviewed exactly once");
  await page.keyboard.press("Control+o");
  await page.locator('[data-slot="command-item"]', { hasText: "Review agent work" }).click();
  const reviewDialog = page.getByRole("dialog", { name: "Review agent work" });
  await reviewDialog.waitFor();
  await reviewDialog.locator('[data-slot="command-item"]', { hasText: "Completed agent run" }).click();
  assert((await calls("mark_attachment_reviewed")).length === 1, "Opening agent work marked it reviewed");
  await page.getByRole("button", { name: "Mark reviewed" }).click();
  await page.getByText("· Reviewed").waitFor();
  reviewCalls = await calls("mark_attachment_reviewed");
  assert(reviewCalls.length === 2 && reviewCalls[1].args.artifactId === 51, "Mark reviewed did not mutate attachment 51 exactly once");
  assert((await page.locator("section").innerText()).includes("Completed"), "Review changed execution status");
  assert((await page.getByText("· New").count()) === 0, "Reviewed attachment kept its New indicator");

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
  const privateResult = explorer.locator('[data-slot="command-item"]', { hasText: "Untitled note" }).filter({ hasText: "Private" }).last();
  await privateResult.waitFor();
  assert((await privateResult.innerText()).toLowerCase().includes("private"), "Private explorer metadata is missing");
  await page.keyboard.press("Escape");

  await content.click();
  await leader("c");
  await page.locator('[data-slot="command-item"]', { hasText: "New project" }).click();
  await page.getByRole("heading", { name: "Untitled project" }).waitFor();
  await page.getByRole("heading", { name: "Project documents" }).waitFor();
  assert((await calls("create_project")).length === 1, "New project did not invoke create_project");
  await page.getByRole("button", { name: "Add document" }).click();
  const addExplorer = page.getByRole("dialog", { name: "Add document to project" });
  await addExplorer.waitFor();
  await addExplorer.getByPlaceholder("Search documents…").fill("Untitled");
  const addResult = addExplorer.locator('[data-slot="command-item"]', { hasText: "Untitled note" }).filter({ hasText: "Private" }).last();
  await addResult.waitFor();
  await addResult.click();
  await addExplorer.waitFor({ state: "detached" });
  await page.getByRole("button", { name: /Untitled note/ }).waitFor();
  assert((await calls("add_document_to_project")).length === 1, "Project member was not persisted through add_document_to_project");
  await page.getByRole("button", { name: /Untitled note/ }).click();
  await page.getByRole("heading", { name: "Untitled note" }).waitFor();
  await page.waitForFunction(() => document.querySelector(".cm-editor")?.classList.contains("cm-focused"));
  await page.keyboard.type("  ");
  await explorer.waitFor();
  assert((await explorer.innerText()).includes("Enter open") && (await explorer.innerText()).includes("Ctrl Enter reference"), "Ordinary Explorer behavior was not restored after adding a project member");
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 320, height: 720 });
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), "Changed canvas overflows at 320px");
  const readButton = page.getByRole("button", { name: "Read" });
  await page.evaluate(() => { document.body.tabIndex = -1; document.body.focus(); });
  for (let index = 0; index < 20 && !await readButton.evaluate((element) => element === document.activeElement); index += 1) {
    await page.keyboard.press("Tab");
  }
  assert(await readButton.evaluate((element) => element === document.activeElement), "Keyboard did not focus the Read/Edit control");
  const focusedOutline = await readButton.evaluate((element) => getComputedStyle(element).outlineStyle);
  assert(focusedOutline !== "none", "Read/Edit control has no visible keyboard focus indicator");

  await browser.close();
} finally {
  server.kill("SIGTERM");
}
