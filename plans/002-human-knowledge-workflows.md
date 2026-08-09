# Plan 002: Add human journal, reading, and agent-review workflows

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions occurs, stop and report; do not improvise. Do not update `plans/README.md`; the reviewer maintains the index.
>
> **Drift check (run first)**: `git diff --stat d7f5beb..HEAD -- src-tauri/Cargo.toml src-tauri/src/database.rs src-tauri/src/main.rs src-tauri/src/markdown.rs src/lib/archive.ts src/lib/sanitize.ts src/components/MarkdownEditor.tsx src/components/MarkdownReader.tsx src/components/MarkdownReader.test.tsx src/App.tsx src/App.test.tsx src/index.css scripts/ui-smoke.mjs`
> The expected output is empty. If it is not empty, stop and report.

## Status

- **Status**: DONE
- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: `plans/001-project-knowledge-foundation.md`
- **Category**: direction
- **Planned at**: commit `d7f5beb`, 2026-08-09
- **Implemented at**: commit `c33c282`, 2026-08-09

## Why this matters

Archive now organizes documents around projects, but the human still uses one raw editable canvas for capture, historical browsing, long-form reading, and agent review. Historical daily entries require search, agent artifacts can still be mutated through the GUI persistence path while retaining agent provenance, blocked and failed work are aggregated under one misleading label, and there is no durable cross-day review queue. This plan keeps today's editable daily as the default while adding three focused workflows: non-creating journal navigation, safe proportional reading, and explicit review of daily-attached agent work.

## Decided product model

- Startup remains today's editable daily document.
- Journal browsing moves among existing daily documents; browsing must not create empty days. `Today` is the one explicit control that may create today's canonical daily.
- User-authored daily, note, and project documents default to Edit whenever opened and may toggle to Read for the current document.
- Artifact or agent-authored documents are always rendered read-only. The Rust persistence boundary rejects GUI body replacement for them so provenance cannot remain `Agent` after a human mutation.
- Review state belongs specifically to daily-attached agent work, not every standalone agent artifact. Standalone and project-only artifacts are immutable but do not show `New` and do not enter the review queue.
- Opening does not review. Only explicit `Mark reviewed` sets the first review timestamp.
- `reviewed_at`, execution status (`completed | blocked | failed`), and agent authorship are independent facts. Marking reviewed never changes status or content.
- Cross-day review is bounded to the newest 50 unreviewed daily attachments. Pagination, search, review notes, reviewer identity, and `Mark unreviewed` are deferred.
- Rendered Markdown uses the existing `pulldown-cmark`, DOMPurify, and Mermaid renderer. No new package or rendering framework is introduced.
- Raw HTML is displayed as escaped text. Ordinary Markdown links are rendered but made inert so they cannot navigate the Tauri webview. Deliberate system-browser link opening is deferred.
- Existing Archive `[[note:ID|label]]` references remain the internal document-link syntax and become validated native buttons in reading mode.
- No permanent sidebar, dashboard, calendar, graph visualization, Vim remapping, or runner configuration is added.

## Current state

- `src-tauri/src/database.rs` owns schema version 6, migrations, document writes, daily attachments, project membership, and persistence tests.
  - `document_attachments` stores parent, artifact, and status, with one attachment per artifact; it has no review state.
  - `list_daily_attachments(day)` returns day-scoped attachment summaries.
  - `replace_document_body` currently permits GUI updates to any document kind and preserves `author`, allowing agent provenance to become misleading.
  - private `get_daily(connection, day)` already performs an exact non-creating lookup, but no adjacent-existing-daily API is exposed.
- `src-tauri/src/main.rs` contains thin Tauri wrappers and registers GUI commands. MCP tools do not need a new method for this milestone.
- `src-tauri/Cargo.toml` already pins `pulldown-cmark = 0.13.4` with default features disabled. HTML rendering requires enabling only its existing `html` feature; do not change the version or add a crate.
- `src/components/MarkdownEditor.tsx` owns CodeMirror editing, Archive reference widgets, Mermaid widgets, and the current SVG sanitizer.
- `src/App.tsx` owns document switching/autosave, the command palette, daily Agent work shelf, project shelf, dialogs, and the centered canvas.
  - daily attachments are polled only while their daily is active;
  - blocked and failed attachments are combined and displayed as `<count> blocked`;
  - every active document currently renders `MarkdownEditor`, read-only only during busy/conflict states;
  - `showDocument` and `openDocument` are the shared document-switch paths that preserve buffers and autosave.
- `src/lib/archive.ts` mirrors Rust DTOs and contains thin Tauri invokes.
- `src/index.css` owns global Geist and dark-theme tokens; it has no proportional prose styles.
- `src/App.test.tsx` mocks every Tauri invoke and uses fake timers/deferred promises for navigation and stale-response behavior.
- `scripts/ui-smoke.mjs` supplies a real-browser Tauri mock and exercises the current editor, Explorer, Agent work, Mermaid, and project flows.
- Do not add code comments. Express contracts through concrete names, data shapes, queries, and tests.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Install isolated worktree dependencies | `npm ci` | exit 0; package files unchanged |
| Frontend build assets | `npm run build` | exit 0; existing bundle warning allowed |
| Rust format | `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | exit 0 |
| Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |
| Frontend tests | `npm test` | all pass |
| UI smoke | `npm run test:ui` | exit 0 |
| Whitespace | `git diff --check` | exit 0, no output |

Run `npm run build` before the first Rust test in an isolated worktree because Tauri test compilation embeds `dist/`.

## Suggested executor toolkit

- Use `better-accessibility`, `better-layout`, `better-writing`, `better-typography`, `better-colors`, and `better-ui` through `better-interface` when implementing and reviewing the header controls, reader, status summaries, and review dialog.
- Preserve existing shadcn/Radix CommandDialog and Tailwind patterns.
- Use the existing `App.test.tsx` deferred-promise tests as the pattern for suppressing stale async results.

## Scope

**In scope — the only files you may modify:**

- `src-tauri/Cargo.toml`
- `src-tauri/src/database.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/markdown.rs` (new)
- `src/lib/archive.ts`
- `src/lib/sanitize.ts` (new)
- `src/components/MarkdownEditor.tsx`
- `src/components/MarkdownReader.tsx` (new)
- `src/components/MarkdownReader.test.tsx` (new)
- `src/App.tsx`
- `src/App.test.tsx`
- `src/index.css`
- `scripts/ui-smoke.mjs`

**Out of scope:**

- `src-tauri/src/mcp.rs` and MCP tool schemas
- package.json, package-lock.json, Cargo.lock, dependency versions, or new dependencies
- review state for standalone/project-only artifacts
- attachment status mutation or reinterpretation
- review notes, reviewer identity, mark-unreviewed, pagination, or queue search
- external-link opening or a Tauri opener plugin
- persistent per-document/per-buffer Read/Edit mode
- editing or forking agent artifacts
- journal calendar, creating missing historical days while browsing, or a permanent timeline sidebar
- Vim mappings, CodeMirror viewport/state ownership, and runner-wide Archive adoption
- generic repositories, renderer interfaces, navigation controllers, relation APIs, or migration frameworks

## Git workflow

- Branch: `advisor/002-human-knowledge-workflows`
- Commit the completed implementation with a signed Conventional Commit: `feat: add human knowledge workflows`
- Do not push, merge, or open a pull request.

## Steps

### Step 1: Add review state and enforce artifact immutability

In `src-tauri/src/database.rs`:

1. Raise `SCHEMA_VERSION` from 6 to 7.
2. Add a v6-to-v7 migration in the existing migration chain only:
   - `ALTER TABLE document_attachments ADD COLUMN reviewed_at TEXT;`
   - set `PRAGMA user_version = 7` in the same transaction.
   - Existing rows remain `NULL` and therefore unreviewed. Do not rebuild tables or add an index.
3. Extend the attachment projection to one concrete serializable `AttachmentSummary` shape:
   - `artifact_id` (use this name consistently rather than ambiguous `id`);
   - `title`;
   - `day` from the daily parent document;
   - `status`;
   - `created_at` and `updated_at` from the artifact;
   - `reviewed_at: Option<String>`.
4. Keep `list_daily_attachments(day)` deterministic newest-created first and return the extended shape.
5. Add `list_unreviewed_attachments() -> Vec<AttachmentSummary>`:
   - only daily-parent to agent-authored artifact rows with `reviewed_at IS NULL`;
   - hard-coded `LIMIT 50`;
   - order by parent day descending, artifact creation time descending, artifact ID descending.
6. Add `get_attachment_by_artifact_id(artifact_id) -> Option<AttachmentSummary>`. `None` is normal for standalone artifacts.
7. Add `mark_attachment_reviewed(artifact_id) -> AttachmentSummary`:
   - validate positive ID;
   - update only a valid daily-attached agent artifact;
   - set the backend UTC timestamp with `reviewed_at = COALESCE(reviewed_at, now)` so the first timestamp is durable and repeated calls are idempotent;
   - return the updated projection;
   - missing, unattached, or non-agent IDs fail without mutation;
   - do not change status, document body, revision, or timestamps.
8. Add `Error::ReadOnlyDocument`. In `replace_document_body`, allow updates only when the stored document is user-authored and kind is `daily`, `note`, or `project`. Artifact or unexpected agent-authored writes return the distinct read-only error before mutation.
9. New MCP daily attachments start with `reviewed_at = NULL` through the column default and need no MCP schema change.

Add database tests for:

- a representative v6 attachment migrating to v7 with IDs/status intact and `reviewed_at IS NULL`, then reopening idempotently;
- fresh schema version 7 and the new column;
- daily and bounded cross-day ordering;
- standalone artifacts absent from the queue and returning no attachment metadata;
- explicit review preserving the first timestamp and execution status across repeated calls;
- failed review for invalid/unattached IDs without mutation;
- artifact GUI body replacement failing while body, revision, updated timestamp, and author remain exact;
- user daily/note/project replacement remaining valid.

**Verify**: `npm run build && cargo fmt --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml database::tests::` → all database tests pass.

### Step 2: Add non-creating journal adjacency

In `src-tauri/src/database.rs`, add concrete serializable shapes:

- `DailyNeighbor { id, day }`
- `DailyNeighbors { previous: Option<DailyNeighbor>, next: Option<DailyNeighbor> }`

Add `daily_neighbors(day)` that validates the day and performs exactly two indexed queries over `documents.kind = 'daily'`:

- previous: greatest `day < requested day` (`ORDER BY day DESC, id DESC LIMIT 1`);
- next: smallest `day > requested day` (`ORDER BY day ASC, id ASC LIMIT 1`).

It must never insert a daily document. Add tests covering gaps, first/last entries, a requested day with no document, deterministic IDs, invalid dates, and unchanged row count.

In `src-tauri/src/main.rs`, add and register thin GUI commands for:

- `daily_neighbors(day)`;
- `list_unreviewed_attachments()`;
- `get_attachment_by_artifact_id(artifact_id)`;
- `mark_attachment_reviewed(artifact_id)`.

Mirror all four commands and DTOs in `src/lib/archive.ts`. No MCP tools change.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml && npm run build` → Rust tests and frontend typecheck/build pass.

### Step 3: Build the safe rendered Markdown boundary

In `src-tauri/Cargo.toml`, enable only the existing `pulldown-cmark` `html` feature; do not change its pinned version.

Create `src-tauri/src/markdown.rs` with one pure `render(markdown: &str) -> String` function and focused unit tests:

1. Parse with `pulldown-cmark` using tables, task lists, strikethrough, and footnotes. Do not enable heading attributes or generic wikilinks.
2. Convert input `Event::Html` and `Event::InlineHtml` to escaped text events so author-supplied raw HTML never reaches output as active markup.
3. Preserve Archive's exact `[[note:positive-ID|label]]` grammar, including existing escaped `\\`, `\|`, and `\]` labels. Transform only references in ordinary text events outside inline code and fenced/indented code into backend-generated native buttons:
   - `<button type="button" data-document-id="ID">escaped label</button>`;
   - validate the ID as a positive signed 64-bit integer;
   - malformed references remain ordinary escaped text.
4. Ordinary Markdown links remain links in backend output; frontend sanitization makes them inert.
5. Mermaid fences remain `<pre><code class="language-mermaid">…</code></pre>` for frontend enhancement.

In `src-tauri/src/main.rs`, register a thin `render_markdown(markdown)` Tauri command returning the HTML string. Keep it independent of document loading.

Create `src/lib/sanitize.ts` by moving the current exact SVG sanitization policy out of `MarkdownEditor.tsx` into `sanitizeSvg(svg)`. Keep SVG profiles and the existing forbidden `foreignObject`, script, href, and xlink:href rules unchanged; update the editor to import it.

Create `src/components/MarkdownReader.tsx`:

- props: `documentId`, `body`, `onOpenReference(id)`;
- invokes `render_markdown` through a typed `renderMarkdown` wrapper in `src/lib/archive.ts` whenever identity/body changes;
- suppresses stale success/error results after body/document change or unmount;
- sanitizes backend HTML with DOMPurify using an explicit HTML tag/attribute allowlist suitable for the enabled Markdown output;
- allow only the exact `data-document-id` custom marker; strip `href`, `target`, and navigation attributes from every non-internal anchor after sanitization;
- validate an internal button ID again as a positive safe integer before calling `onOpenReference`;
- render a proportional reading region with loading, empty, and rendering-error states;
- discover sanitized `pre > code.language-mermaid` blocks, invoke the existing `renderMermaid`, and replace only the corresponding still-current block with `sanitizeSvg` output;
- retain source code and show a compact diagnostic if Mermaid is invalid or rendering fails;
- do not introduce a renderer abstraction or use `dangerouslySetInnerHTML` before sanitization.

Add `src/components/MarkdownReader.test.tsx` coverage for headings/prose/lists/tables, escaped raw HTML, inert external links, valid and malformed Archive references, code-contained reference exclusion, Mermaid success/failure, stale responses, empty content, and unmount cleanup.

In `src/index.css`, add restrained `.archive-reader` proportional prose styles using existing tokens and Geist:

- body text near 16px with 1.6 line-height and a readable 60–75 character measure;
- descending headings, clear list/blockquote/code/table treatment, safe wrapping, and visible internal-reference button focus;
- hide only the first rendered `h1` because the canvas already supplies the document `h1`;
- no animation and no raw colors outside existing semantic tokens.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml markdown::tests:: && npm test -- src/components/MarkdownReader.test.tsx && npm run build` → renderer tests and build pass.

### Step 4: Integrate journal and reading workflows into the canvas

In `src/App.tsx`, preserve the centered canvas and existing autosave/buffer paths:

1. Add ephemeral `edit | read` state reset inside the shared `showDocument` path:
   - user daily/note/project opens in Edit;
   - artifact or agent-authored documents open in Read and cannot toggle to Edit.
2. Render `MarkdownReader` in Read and `MarkdownEditor` in Edit. Artifact/agent documents must never instantiate an editable CodeMirror path.
3. Add a visible compact `Read`/`Edit` action in the title row for user-authored daily/note/project documents and equivalent command-palette action. The active mode appears in the footer. Switching to Read flushes autosave first; a failed flush leaves Edit active and reports the existing save error.
4. While a daily is active, load `dailyNeighbors(active.day)` with generation guards. Show native previous/next controls in the title row with accessible names containing the target formatted day; disable absent directions. Opening a neighbor uses `openDocument(id)` and therefore existing flush/buffer semantics.
5. Show `Today` only while an historical daily is active. It explicitly calls `getOrCreateDaily(todayRef.current)`, then opens through the same flush/show lifecycle. It must not appear on notes, projects, artifacts, or today's daily.
6. Do not add journal keyboard mappings in this plan.

Add focused `src/App.test.tsx` tests for default modes, read/edit transitions and failed flush, immutable artifact rendering, neighbor loading/opening across date gaps, disabled first/last navigation, non-creating browse semantics, Today creation, stale neighbor responses, footer mode, and command-palette parity.

**Verify**: `npm test && npm run build` → all frontend tests and build pass.

### Step 5: Add explicit cross-day agent review

In `src/App.tsx`:

1. Extend the active-daily shelf rows with attachment day/review metadata. Use `artifact_id` consistently.
2. Replace the combined actionable count with independent summaries:
   - blocked count includes only blocked;
   - failed count includes only failed;
   - New count includes only `reviewed_at === null`;
   - omit zero-valued summaries rather than labeling failed work as blocked.
3. Each unreviewed row shows a visible `New` text badge independent of status color.
4. Add `Review agent work` to the command palette. It opens a dedicated existing-style `CommandDialog`, fetches `listUnreviewedAttachments()` once per open with generation guards, and presents up to 50 items with title, formatted day, status, and New provenance. Empty copy: `No agent work is waiting for review.`
5. Selecting an item closes the dialog and opens its artifact through `openDocument(artifact_id)`. Opening alone does not mutate review state.
6. Whenever an artifact/agent document is active, call `getAttachmentByArtifactId` with generation guards:
   - attached artifact: show parent day, execution status, and `New` or `Reviewed` near the reader;
   - standalone artifact: show agent provenance but no review action/state.
7. For an unreviewed attachment, show a native `Mark reviewed` button. Call `markAttachmentReviewed` once, disable while pending, ignore stale responses, update active metadata and any loaded shelf/queue rows, and retain the reader/focus context. Do not alter status or close the document.
8. After review, remove the item from the open review queue and clear its New badge in the daily shelf. The reviewed artifact remains open.
9. Errors use the existing notice path and must not optimistically clear New.

Add focused tests for separate blocked/failed/New summaries, queue ordering presentation, queue empty state, open-without-review, explicit successful review, idempotent duplicate activation, standalone artifact behavior, error retention, and stale response suppression after switching documents/dialog generations.

Update `scripts/ui-smoke.mjs` mocks for every new Tauri command and schema field. Extend the real-browser flow to:

- browse previous and next existing daily entries without observing a creation call;
- return via Today;
- switch a user document between Edit and rendered Read;
- verify an agent artifact has no editable CodeMirror and renders Markdown/Mermaid;
- open Review agent work, select an item, explicitly mark it reviewed, and observe New disappear while status remains;
- verify blocked and failed summaries are distinct.

**Verify**: `npm test && npm run build && npm run test:ui` → all frontend, build, and browser gates pass.

### Step 6: Final contract and interface verification

Run every repository gate sequentially so Cargo does not race Vite while Tauri embeds `dist/`:

```bash
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
npm test
npm run test:ui
git diff --check
```

Then perform a quick change-scoped `better-interface` review of journal controls, rendered reading, agent status summaries, review dialog, Mark reviewed, keyboard/focus behavior, loading/empty/error states, and 320px/200% reflow. Fix only findings introduced by this plan and rerun affected gates. Existing deferred low-priority typography/motion issues outside the changed surfaces remain out of scope.

## Test plan

- Rust migration/domain: v6→v7 preservation, fresh v7, explicit/idempotent review, queue bounds/order, immutable artifacts, user-write regression, non-creating journal adjacency.
- Rust renderer: CommonMark extensions, raw HTML escaping, exact Archive references, code exclusion, malformed IDs, Mermaid fence preservation.
- Reader component: sanitization, inert external links, internal navigation, async Mermaid, stale/unmount behavior, empty/error states.
- App: mode defaults/toggle, autosave boundary, historical daily navigation/Today, separate status counts, queue, explicit review, standalone artifacts, stale async operations.
- Real browser: complete journal→read→agent-review path plus existing editor/Explorer/project regressions.

## Done criteria

- [x] Schema version is 7; v6 attachments migrate unreviewed without data loss.
- [x] Agent/Artifact GUI writes are rejected in Rust; user daily/note/project writes remain valid.
- [x] Journal previous/next queries never create rows; Today remains explicit canonical creation.
- [x] User documents can toggle Read/Edit; agent/artifact documents are rendered read-only only.
- [x] Rendered Markdown escapes raw HTML, sanitizes output, prevents external navigation, preserves valid Archive references, and enhances Mermaid safely.
- [x] Daily status summaries distinguish blocked, failed, and New.
- [x] Cross-day review is bounded to daily-attached unreviewed work and opening never reviews.
- [x] Mark reviewed preserves the first timestamp and does not change status/content.
- [x] Standalone/project-only artifacts remain immutable and outside the review queue.
- [x] Existing project, daily attachment, MCP, autosave, conflict, Explorer, reference, Mermaid, and Vim tests remain green.
- [x] All final verification commands exit 0.
- [x] `git diff --name-only d7f5beb..c33c282` contains only the thirteen in-scope files.
- [x] No package lock, Cargo lock, dependency version, MCP schema, or Vim mapping changed.

## STOP conditions

Stop and report instead of improvising if:

- Any drift-check path changed after `d7f5beb` before execution.
- Enabling the existing `pulldown-cmark` HTML feature changes Cargo.lock or requires another crate/version.
- Safe Markdown rendering requires a new frontend or Rust dependency.
- Enforcing artifact immutability requires changing MCP artifact creation or MCP tool schemas.
- Review state cannot remain owned by the unique daily attachment without changing standalone artifact semantics.
- Journal navigation requires creating missing historical days or a schema beyond version 7.
- The complete workflow requires Vim mapping changes, a permanent navigation surface, or edits outside the in-scope files.
- After the step's required implementation and tests are complete, the same final verification failure remains unexplained after diagnosis and a focused fix. Intermediate compile/test failures encountered while completing the specified work are not STOP conditions by themselves.

## Maintenance notes

- Review is intentionally attachment-owned. If standalone artifacts later join the review queue, move the concept deliberately rather than duplicating review state.
- External links are intentionally inert until an explicit system-browser opening policy is chosen.
- Read/Edit mode is session-ephemeral and resets on open; per-buffer persistence belongs with the later Vim/editor-state milestone.
- The review queue is hard-bounded at 50 with no index or pagination because this is a local single-user database. Measure before adding either.
- A future reviewer should scrutinize the HTML/SVG sanitization boundary, backend immutability check, review timestamp idempotence, and stale async suppression first.
