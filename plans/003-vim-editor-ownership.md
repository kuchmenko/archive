# Plan 003: Give Vim ownership of editing and buffer state

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions occurs, stop and report; do not improvise. Do not update `plans/README.md`; the reviewer maintains the index.
>
> **Drift check (run first)**: run both `git diff --stat 3e25d60..HEAD -- src/lib/vim.ts src/lib/vim.test.ts src/components/MarkdownEditor.tsx src/components/MarkdownEditor.test.tsx src/App.tsx src/App.test.tsx scripts/ui-smoke.mjs` and `git status --short -- src/lib/vim.ts src/lib/vim.test.ts src/components/MarkdownEditor.tsx src/components/MarkdownEditor.test.tsx src/App.tsx src/App.test.tsx scripts/ui-smoke.mjs`.
> Both outputs must be empty. The reviewer-owned main worktree has uncommitted `plans/README.md` and `plans/003-vim-editor-ownership.md`; those are outside implementation scope and must not be copied, changed, staged, or committed by the executor.

## Status

- **Status**: DONE
- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: `plans/002-human-knowledge-workflows.md`
- **Category**: direction
- **Planned at**: commit `3e25d60`, 2026-08-09

## Why this matters

Archive uses a real CodeMirror Vim engine, but a capture-phase DOM handler currently takes core Vim keys before the engine receives them. It also destroys and recreates the sole `EditorView` whenever the active document or Read/Edit mode changes, losing undo history, marks, visual state, dot-repeat state, and editor-local Vim options. This plan makes Vim the owner of editor-focused keys, exposes Archive operations through user-approved Vim mappings, and keeps one concrete editor instance alive for each open editable document. A bounded CodeMirror viewport makes `H`, `L`, half/full-page motions, and `zz`/`zt`/`zb` operate on the viewport Vim actually measures.

## Decided product contract

- Inside CodeMirror, native Vim owns `Ctrl-V`, `Ctrl-O`, `Ctrl-N`, `H`, `L`, and `Enter`. Archive must not capture or remap them.
- Archive's normal-mode mappings are exactly:
  - `<Space><Space>` — open Explorer;
  - `<Space>n` — create a shared note;
  - `<Space>N` — create a private note;
  - `<Space>c` — open the command palette;
  - `gf` — open the Archive reference under the cursor;
  - `[b` — previous open Archive buffer;
  - `]b` — next open Archive buffer.
- Outside CodeMirror, existing `Ctrl-N`, `Ctrl-Shift-N`, and `Ctrl-O` application shortcuts remain.
- Every open user-authored daily, note, or project document owns at most one mounted `MarkdownEditor`/`EditorView`.
- Switching buffers, entering Read mode, or opening an artifact hides but does not destroy other editable buffers. Removing a buffer destroys its editor.
- Agent-authored or artifact documents never instantiate CodeMirror.
- State is preserved by retaining the concrete editor/Vim adapter instance. Do not serialize, clone, or expose package-private Vim state.
- The editor body gets a bounded internal scroll viewport for long documents. The centered title and active document shelves remain in the outer page and must remain reachable.
- System clipboard integration for Vim `"+`/`"*` registers is deferred. Removing the raw clipboard handler restores native Vim/register behavior; do not remove the installed Tauri clipboard dependency or permissions in this plan.

## Current state

- `src/components/MarkdownEditor.tsx` owns one `EditorView` and installs `vim()` before other keymaps, which is correct.
  - `appKeydown` is attached in capture phase and currently intercepts `H`/`L`, timer-based Space-Space, contextual `Enter`, application `Ctrl-N`/`Ctrl-Shift-N`/`Ctrl-O`, and `Ctrl-C`/`Ctrl-X`/`Ctrl-V`.
  - `.cm-scroller` has `overflow: visible`; the ancestor `<main>` scrolls instead. The Vim adapter only measures and scrolls `EditorView.scrollDOM`, so native viewport commands cannot represent the visible Archive page with this layout.
  - `snapshot()` persists only anchor, head, and outer-page `scrollTop`.
  - component cleanup destroys the `EditorView` and its adapter-local marks, visual state, dot-repeat metadata, and local options.
- `src/App.tsx` renders one keyed `MarkdownEditor` only for the active document in Edit mode. Read mode and every document switch unmount it.
  - one `editorRef` is used by flush, remote replacement, Explorer insertion/focus, project focus, and conflict resolution;
  - `showDocument` resets the footer to `NORMAL` regardless of the target editor's retained state;
  - `editorChanged` and the one `AutosaveController` already apply only to the active document. Preserve that ownership; do not create per-buffer autosave controllers.
- `src/lib/buffers.ts` stores concrete open `DocumentBuffer` records and is already the lifetime list for open buffers. Do not add `EditorView`, React refs, or Vim state to these serializable records.
- `src/lib/shortcuts.ts` is a pure key classifier for the outside-editor application shortcuts. Its key definitions remain valid.
- Installed `@replit/codemirror-vim` 6.4.0 exports one global `Vim` singleton. `Vim.defineAction` and `Vim.mapCommand` mutate global registries, while every action receives the invoking adapter whose `cm6` property is the exact `EditorView`. Registrations must therefore happen once globally, and dispatch must resolve callbacks by that concrete view.
- The package stores registers, macros, search history, inserted-text replay, and its jump list globally; marks, visual state, non-insert action metadata, and local options live on each adapter. This plan preserves adapter-local state by retaining editors but does not claim per-buffer registers, insert-mode dot-repeat isolation, or a document-aware global jump list.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Install isolated dependencies | `npm ci` | exit 0; package files unchanged |
| Focused tests | `npm test -- src/lib/vim.test.ts src/components/MarkdownEditor.test.tsx src/App.test.tsx` | all pass |
| Frontend tests | `npm test` | all pass |
| Typecheck/build | `npm run build` | exit 0; existing large-bundle warning allowed |
| Browser workflow | `npm run test:ui` | exit 0 |
| Whitespace | `git diff --check` | exit 0, no output |

## Suggested executor toolkit

- Use `svelte` skills only if the codebase changes to Svelte; it is currently React, so do not load them.
- Use `better-accessibility` and `better-layout` only for checking the nested editor/outer-page viewport, focus movement, and shelf reachability. Do not redesign the canvas.
- Treat `node_modules/@replit/codemirror-vim/dist/index.d.ts` and the installed package source as the API authority. Do not infer private state shapes.

## Scope

**In scope — the only files you may modify:**

- `src/lib/vim.ts` (new)
- `src/lib/vim.test.ts` (new)
- `src/components/MarkdownEditor.tsx`
- `src/components/MarkdownEditor.test.tsx`
- `src/App.tsx`
- `src/App.test.tsx`
- `scripts/ui-smoke.mjs`

**Out of scope:**

- `package.json`, lockfiles, Cargo files, Tauri capabilities, or dependency versions
- `src/lib/buffers.ts` data shape or storing runtime editor/Vim objects in buffers
- patching, forking, or monkey-patching `@replit/codemirror-vim`
- serializing editor or Vim state across application restart or after a buffer is closed
- per-buffer Read/Edit-mode persistence
- per-buffer autosave controllers or background sync for inactive buffers
- system clipboard bridging for Vim `"+`/`"*` registers
- changing the package's global register, macro, search-history, or jump-list semantics
- an editor for agent/artifact documents or artifact forking
- a tab bar, sidebar, permanent buffer chrome, or canvas redesign
- new dependencies or code comments

## Git workflow

- Branch: `advisor/003-vim-editor-ownership`
- Commit completed implementation with a signed Conventional Commit: `feat: give Vim ownership of editing`
- Do not push, merge, or open a pull request.

## Steps

### Step 1: Prove the active editor's bounded viewport in the real browser

Change the existing CodeMirror theme in `MarkdownEditor.tsx` so short documents retain the current natural minimum while long documents use a bounded internal `.cm-scroller` with vertical overflow. Use viewport-relative CSS rather than a hard-coded pixel height. Keep the editor within the existing centered width; do not move the title or active daily/project shelves inside CodeMirror.

First make the UI smoke own its preview process reliably. Start Vite preview with strict port ownership, fail if the spawned process exits before readiness, and wait for that spawned process rather than accepting an unrelated response already listening on port 4173. Every browser verification in this plan must run `npm run build && npm run test:ui` so it cannot exercise stale `dist` output.

The browser fixture in `scripts/ui-smoke.mjs` must contain a long editable document. At this step, add probes for the currently active editor only:

- `scrollHeight > clientHeight` for a long active editor;
- `Ctrl-D`, `Ctrl-U`, `Ctrl-F`, and `Ctrl-B` visibly change that editor's `scrollTop` in the expected direction;
- `zz`, `zt`, and `zb` place the cursor line near the middle, top, and bottom of the editor viewport;
- ordinary outer-page scrolling can still reach the active daily/project shelf after the internal editor viewport;
- at 420×720 there is no horizontal overflow, Tab can leave CodeMirror for surrounding controls, wheel scrolling chains to the outer `<main>` after the internal scroller reaches an edge, and the shelf remains reachable through outer scrolling.

Use actual cursor geometry or line positions, not only `defaultPrevented`, to prove the motions. Keep `scrollCursorIntoPage` only if it still serves the outer page without fighting CodeMirror's internal scrolling.

`H` and `L` are still intercepted by current code at this step and hidden retained editors do not exist yet. Test native `H`/`L` after Step 3 and hidden-editor geometry after Step 4; do not claim them here.

**STOP gate**: stop and report before any mapping/state refactor if the active-editor browser checks cannot prove the viewport contract without patching the Vim package, intercepting native viewport keys, moving the full canvas into CodeMirror, making shelves unreachable, or trapping ordinary scrolling.

**Verify**: `npm run build && npm run test:ui` → strict-port preview and active-editor viewport characterization pass with current mappings otherwise unchanged.

### Step 2: Register exact-view Archive Vim actions once

Create `src/lib/vim.ts` as the concrete integration boundary between the package-global Vim command table and Archive callbacks.

Define one `ArchiveVimActions` shape containing Explorer, shared/private note, command palette, reference, and previous/next buffer actions. Keep a module-owned `WeakMap<EditorView, ArchiveVimActions>`. Export one function that registers callbacks for an exact view and returns its cleanup. The cleanup deletes only that view's entry.

Register these exact namespaced actions once:

- `archive.openExplorer`
- `archive.newSharedNote`
- `archive.newPrivateNote`
- `archive.openCommandPalette`
- `archive.openReference`
- `archive.previousBuffer`
- `archive.nextBuffer`

The package already has a complete `<Space>` mapping, so literal multi-key `<Space>…` action mappings are unreachable. Register one internal normal-mode leader alias with `Vim.map("<Space>", "<Leader>", "normal")`, then register the seven actions through `Vim.mapCommand` as `<Leader><Space>`, `<Leader>n`, `<Leader>N`, `<Leader>c`, `gf`, `[b`, and `]b`. These expose the seven approved physical key sequences. `[b` and `]b` deliberately override the package's generic bracket-symbol motions.

Each action must obtain the invoking `EditorView` from the adapter's `cm6` property, look up only that view in the WeakMap, and no-op when it is absent. Cursor/reference behavior must derive from the invoking view, never from a process-global active document or React closure.

Do not register any mapping for `Ctrl-V`, `Ctrl-O`, `Ctrl-N`, `H`, `L`, or `Enter`. Do not define Ex commands in this milestone.

Add `src/lib/vim.test.ts` by mocking the package singleton and capturing definitions. Cover:

- one `<Space>`→`<Leader>` normal-mode alias plus the exact seven internal keys, action names, and `normal` contexts;
- one registration even when multiple views register callbacks;
- two fake views route to different callback sets;
- an unregistered view is a no-op;
- cleanup removes only its view;
- registration contains none of the six native keys.

**Verify**: `npm test -- src/lib/vim.test.ts` → all integration-boundary tests pass.

### Step 3: Remove raw key ownership from MarkdownEditor

In `MarkdownEditor.tsx`:

- import and register the exact view with `src/lib/vim.ts` after constructing it;
- use the existing callback refs so mapped actions always invoke current props;
- implement `gf` by parsing `noteReferenceAt` from that invoking view's current document and cursor, then calling `onOpenReference` only for a valid reference;
- remove `appKeydown`, pending-Space state/timer, and its capture listener entirely;
- remove the in-editor import/use of `appShortcut`;
- remove `readText`/`writeText`, clipboard error handling, and `onClipboardError` from the component contract;
- keep mouse/keyboard behavior of actual reference and Mermaid buttons unchanged.

Component tests must invoke captured Vim actions rather than dispatch raw DOM keys for Archive mappings. Replace the old tests that expect capture-phase Ctrl shortcuts, H/L buffer switching, Space-Space, or reference Enter. Prove `gf` uses the current document/cursor of the invoking editor and current callback props after rerender.

In a real browser, prove:

- `Ctrl-V` enters Visual Block and does not call the Tauri clipboard read command;
- native `Ctrl-N`, `H`, `L`, and `Enter` perform Vim behavior without creating/opening Archive UI; use cursor/visible-line assertions for `H` and `L` now that their interception is gone;
- seed a same-editor jump and prove `Ctrl-O` restores its cursor; do not claim the package-global jump list is document-aware across buffers;
- Vim yank/delete/put works through Vim registers;
- the seven Archive mappings invoke exactly their decided actions.

Do not assert OS clipboard contents in CI. Browser copy permission and `"+`/`"*` register integration are not part of this contract.

**Verify**: `npm test -- src/lib/vim.test.ts src/components/MarkdownEditor.test.tsx && npm run build && npm run test:ui` → all mapping/native-key tests pass against current output.

### Step 4: Keep one editor mounted per editable open buffer

In `App.tsx`, replace the single `editorRef` and active-only editor branch with an editor-handle `Map<number, MarkdownEditorHandle>` and a pool rendered from `buffers`.

An editor is eligible only when `document.author === "user"` and `document.kind` is `daily`, `note`, or `project`. For every eligible buffer, render exactly one keyed `MarkdownEditor` wrapper carrying stable `data-document-id` and `data-editor-active` attributes for tests. Keep inactive editors mounted but hidden, read-only, unfocusable, and absent from layout. The active editor is visible only when the active document is eligible and `mode === "edit"`. Read mode renders `MarkdownReader` while its editor remains mounted and hidden. Agent/artifact buffers render no editor.

Add an `active` prop and activation method/behavior to `MarkdownEditor`:

- a hidden editor never receives application focus or drives active footer mode;
- hidden wrappers use actual layout exclusion (`hidden` or `display: none`) and inactive editors are configured read-only;
- `snapshot()` captures the outgoing editor's outer-page scroll before App changes active document or mode and retains that value in the concrete editor as well as returning it; do not read shared `<main>.scrollTop` later from child deactivation effects because effect order depends on buffer order;
- after the wrapper becomes visible, activation schedules `requestMeasure()`, then reports retained mode and focuses in the measurement write phase or a subsequent frame only after rechecking active/document identity;
- imperative `focus` and `insertAt` reject inactive editors;
- activation must not recreate state or force Normal mode.

Use callback refs to add/remove exact handles. Do not put handles, `EditorView`, or Vim objects in React state or `DocumentBuffer`.

Add App tests for exact editor counts and stable DOM identity across:

- user document A → user document B → A;
- Edit → Read → Edit;
- user document → agent artifact → user document, including hidden-editor post-visibility measurement with non-zero current geometry;
- removing a buffer.

Opening an artifact must not increase the existing editor count, and no editor wrapper may exist for that artifact's document ID. Retained user editors remain mounted and hidden. Removing one user buffer must remove exactly its editor while preserving survivors.

Pass active resolved `references` only to the matching active editor and `[]` to inactive editors. The existing references state describes only the active document and must never decorate a hidden editor for another document.

Before Step 5, extend the real-browser flow to switch from long editable document A to editable document B and back to A. After A becomes visible, prove its editor has non-zero geometry, retains its prior internal `scrollTop`, and still passes `H`/`L`, `Ctrl-D/U/F/B`, and `zz/zt/zb` geometry checks. JSDOM cannot satisfy this gate.

**STOP gate**: stop if a previously hidden editor retains stale zero-size geometry after the explicit post-visibility measurement sequence.

**Verify**: `npm test -- src/components/MarkdownEditor.test.tsx src/App.test.tsx && npm run build && npm run test:ui` → lifetime, artifact-exclusion, and post-visibility browser geometry tests pass.

### Step 5: Route every operation to the exact document handle

Add one direct `editorHandle(documentId)` lookup in `App.tsx` and replace every single-ref call site:

- `flush()` snapshots only the active document's exact handle;
- sync `applyDocument` and conflict resolution replace only the handle matching the remote/conflict document ID;
- new-project focus resolves the created project ID;
- Explorer focus restoration and insertion recheck the origin document ID before touching its handle;
- delayed focus after dialogs or switches rechecks active identity;
- deletion removes the buffer and lets callback-ref cleanup remove only that handle.

Imperative stale-operation tests must prove neither the newer active editor nor the now-hidden origin editor is focused or mutated.

Keep one active `AutosaveController`. Inactive editors are read-only and cannot emit changes; `editorChanged` continues accepting only the active ID.

Change Vim mode reporting to include the editor/document ID. Store the last mode per open editor in a ref or concrete map. Hidden editor events cannot overwrite the footer. Remove the unconditional `setVimMode("NORMAL")` in `showDocument`. Read/agent views show a neutral `READ` status; returning to Edit reports the retained mode from that editor.

Continue using minimal external body replacement with `Transaction.addToHistory.of(false)` and `Transaction.remote.of(true)`. Do not add inactive polling or make remote replacement an undo step.

Add deferred-operation tests proving a stale focus/insert/remote operation cannot affect a newer active editor and that mode events from a hidden editor cannot change the footer.

**Verify**: `npm test -- src/App.test.tsx src/components/MarkdownEditor.test.tsx` → exact-handle routing tests pass.

### Step 6: Prove per-buffer editor and Vim state preservation

Extend `scripts/ui-smoke.mjs` with a scoped selector for each editor wrapper; stop using an unscoped singular `.cm-editor` where multiple mounted editors now exist.

Use two editable user buffers to prove:

- exactly two editor roots exist while both are open;
- each root keeps stable DOM identity across A → B → A and Edit → Read → Edit;
- undo in A affects A's prior edit and not B;
- a mark set in A remains usable after returning;
- a non-insert repeatable action such as `x` or `dw` remains repeatable in A after returning, without performing an intervening insert edit in B; do not claim insert-mode replay is per-buffer because the installed package stores inserted-text replay globally;
- a visual/visual-block selection in A remains attached to A when switching through non-Vim UI and returning, if supported by the retained adapter without inventing state serialization;
- `[b` and `]b` navigate and wrap through the existing buffer list only in Normal mode;
- `gf` opens the reference under the invoking editor's cursor;
- opening an artifact adds no artifact editor root and does not increase the retained user-editor count;
- deleting an editable buffer removes exactly that root and leaves the surviving editor state intact.

If visual mode itself prevents the selected normal-mode buffer mapping, switch through existing mouse/command UI for that one preservation check; do not add a visual-mode buffer mapping.

**Verify**: `npm run build && npm run test:ui` → complete native-motion, mapping, viewport, and state-lifetime workflow passes against current output.

### Step 7: Keep application shortcuts outside the editor

Keep `src/lib/shortcuts.ts` unchanged. In the existing window key handler in `App.tsx`, return before classifying shortcuts when the event target is inside `.cm-editor`. Do not rely only on `defaultPrevented`; this is an explicit ownership boundary.

Tests must prove:

- outside CodeMirror, `Ctrl-N`, `Ctrl-Shift-N`, and `Ctrl-O` retain their existing behavior;
- inside CodeMirror, none invokes an application action;
- `<Space>n`, `<Space>N`, and `<Space>c` provide the corresponding editor-focused actions.

Update command-palette shortcut copy only if the current labels would falsely imply the Ctrl shortcuts override Vim while the editor is focused. Keep copy changes within `App.tsx`; add no settings surface.

**Verify**: `npm test -- src/App.test.tsx && npm run build && npm run test:ui` → inside/outside ownership tests pass against current output.

### Step 8: Run the full contract and interface gate

Run sequentially:

```bash
git diff --check
npm test
npm run build
npm run test:ui
git diff --check
git status --short
```

Compare final status with the pre-execution baseline. The only additional paths may be the seven implementation files in Scope. New untracked `src/lib/vim.ts` and `src/lib/vim.test.ts` must be included in this check. Commit only those seven files, then run `git diff --name-only 3e25d60..HEAD` and require exactly the seven paths.

Then perform a change-scoped interface check at 960×720, 420×720, and manual 200% browser zoom. Confirm focus visibility, no horizontal overflow, active/hidden editor semantics, internal/outer scrolling, and shelf reachability. Headless viewport/device-scale changes do not prove browser zoom; record the manual zoom result separately. Fix only regressions introduced by this plan and rerun affected gates.

The final name-only output may contain only the seven files listed in Scope. Package files, Rust/Tauri files, and `src/lib/buffers.ts` must remain unchanged.

## Test plan

- `src/lib/vim.test.ts`: exact global registration, exact-view routing, cleanup, no native-key mappings.
- `src/components/MarkdownEditor.test.tsx`: action integration, current callback refs, `gf`, mode reporting by ID, activation/measurement, no raw capture or clipboard mutation.
- `src/App.test.tsx`: editor pool lifetime, artifact exclusion, exact-handle operations, hidden-mode suppression, Read/Edit retention, inside/outside shortcut ownership, deletion cleanup.
- `scripts/ui-smoke.mjs`: strict ownership of the preview port, real Vim native keys, exact Archive mappings, viewport geometry, two-buffer undo/marks/non-insert repeat/selection lifetime, stable editor identity, artifact exclusion, deletion, shelf accessibility, and narrow reflow.
- Existing Markdown, reference, Mermaid, autosave, conflict, journal, review, project, and MCP behavior must remain green.

## Done criteria

- [ ] No Archive raw keydown handler captures `Ctrl-V`, `Ctrl-O`, `Ctrl-N`, `H`, `L`, or editor `Enter`.
- [ ] One internal `<Space>`→`<Leader>` alias and exactly seven approved normal-mode Archive action mappings are registered through the Vim API; the seven physical user sequences work in the browser.
- [ ] Global actions route through the invoking adapter's exact `EditorView` and are cleaned up per view.
- [ ] Outside-editor Ctrl shortcuts retain existing behavior and never fire from inside CodeMirror.
- [ ] A long active editor has a measured internal viewport; `H/L`, page motions, and `zz/zt/zb` pass real-browser geometry assertions.
- [ ] Outer-page title and active shelves remain reachable at desktop, narrow viewport, and 200% zoom.
- [ ] Every open editable user buffer has exactly one retained editor; artifacts/agent documents have none.
- [ ] Buffer and Read/Edit switches preserve editor DOM identity, CM6 undo, marks, non-insert repeat state, selection, cursor, and internal scroll where those states are adapter-local; no per-buffer insert-replay claim is made.
- [ ] Hidden editors cannot receive input, autosave, focus, or footer-mode ownership.
- [ ] Remote, Explorer, conflict, focus, and deletion paths resolve the exact document handle.
- [ ] Vim register yank/delete/put works; `Ctrl-V` reaches Visual Block; no Tauri clipboard read is triggered by that key.
- [ ] `npm test`, `npm run build`, and `npm run test:ui` exit 0.
- [ ] `git diff --check` exits 0; final status adds only the seven in-scope source/test files; after the implementation commit, `git diff --name-only 3e25d60..HEAD` lists exactly those paths.
- [ ] No dependency, lockfile, Rust/Tauri, buffer data-shape, MCP, journal, review, or knowledge-model change exists.

## STOP conditions

Stop and report instead of improvising if:

- Any in-scope source has drifted from commit `3e25d60` before execution.
- Accurate viewport motions require patching/forking the Vim package, intercepting native Vim keys, or moving the whole canvas into CodeMirror.
- Nested scrolling traps ordinary page/wheel/keyboard access or makes active shelves unreachable.
- A previously hidden editor retains stale zero-size geometry after explicit measurement.
- `Vim.mapCommand` cannot be registered once without duplicate global mappings.
- An action cannot recover its invoking `EditorView` through the adapter's `cm6` property.
- Preserving editor-local state requires serializing package-private Vim internals or storing runtime editor objects in `DocumentBuffer`.
- An agent/artifact path instantiates CodeMirror.
- Inactive editors can still receive input or require separate autosave/sync controllers.
- A remote or deferred UI operation cannot be tied to one document ID and exact handle.
- Tests show CM6 undo history or adapter-local marks/dot state are shared between editors.
- The same final verification failure remains unexplained after diagnosis and one focused repair.

## Maintenance notes

- `@replit/codemirror-vim` mappings and some histories are package-global. Keep Archive action names namespaced, registration one-time, and callbacks keyed by exact `EditorView`.
- The package jump list is global and not document-aware. Restoring `Ctrl-O` means Archive no longer steals it; it does not promise cross-buffer jump navigation.
- Inserted-text replay is package-global even though non-insert action metadata is adapter-local. Do not infer per-buffer insert-mode dot-repeat isolation from retained editors.
- Retained editors consume memory proportional to open editable buffers. This is deliberate for a local single-user application and should be measured before introducing serialization or eviction.
- System clipboard registers need a separate product/API decision because Vim register operations are synchronous while the Tauri clipboard bridge is asynchronous.
- If a future tab-closing feature evicts buffers, editor state ends with buffer lifetime unless a separate supported persistence format is designed.
- Runner-wide Archive adoption remains the next milestone and must not be mixed into this editor change.
