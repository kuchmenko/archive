# Plan 001: Establish project-centered knowledge

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions occurs, stop and report; do not improvise. Do not update `plans/README.md`; the reviewer maintains it.
>
> **Drift check (run first)**: `git diff --stat d8deb0b..HEAD -- src-tauri/src/database.rs src-tauri/src/main.rs src-tauri/src/mcp.rs src-tauri/tests/mcp_stdio.rs src/lib/archive.ts src/lib/documents.ts src/lib/documents.test.ts src/App.tsx src/App.test.tsx scripts/ui-smoke.mjs`
> The expected output is empty. If it is not empty, stop and report.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `d8deb0b`, 2026-08-09

## Why this matters

Archive currently stores daily notes, human notes, and agent artifacts, but cannot organize them around the projects they concern. Agents must reconstruct context from raw search, and the human UI cannot open a project and inspect its documents. This plan adds the smallest project-centered model chosen by the user: projects are ordinary readable documents, project membership is one concrete relationship, and daily attachments remain separate because they carry execution status.

## Decided product model

- A project is a document with `kind = 'project'`, `author = 'user'`, ordinary Markdown body, revision, visibility, timestamps, and stable numeric ID.
- A project can contain many non-project documents; a document can belong to many projects.
- Nested projects are out of scope and must be rejected.
- Project membership is stored in `project_documents`; do not create a generic relation table.
- Existing `document_attachments` remains unchanged.
- Existing inline `[[note:ID|label]]` references remain unchanged.
- A private project or private member is visible to the GUI but not to MCP. MCP must treat private and missing projects identically and omit private members.
- Agents cannot create projects. Agents can associate a newly created artifact or daily attachment with one shared project.
- The human project surface remains the current focused canvas: editable project Markdown followed by a compact project-documents shelf. Do not add a permanent sidebar, dashboard, rendered reading mode, journal navigation, or Vim remapping in this plan.

## Current state

- `src-tauri/src/database.rs` owns schema, migrations, CRUD, privacy sanitization, daily attachments, and all persistence tests.
  - `SCHEMA_VERSION` is 5.
  - `documents.kind` allows only `daily`, `note`, and `artifact`.
  - `document_attachments` stores daily-to-artifact status.
  - `mcp_create_artifact` and `mcp_create_daily_attachment` already use immediate transactions.
- `src-tauri/src/mcp.rs` exposes exactly five structured tools: search, read, artifact creation, daily attachment creation, and Mermaid validation.
- `src-tauri/src/main.rs` exposes GUI commands as thin Tauri wrappers over concrete `Database` methods.
- `src/lib/archive.ts` mirrors the Rust `Document` shape and contains thin Tauri invoke functions.
- `src/App.tsx` owns the focused canvas, command palette, explorer, daily attachment shelf, and document switching.
- `src/lib/documents.ts` derives labels from daily dates or the first nonempty Markdown line.
- Tests use colocated Rust unit tests, `src/App.test.tsx`, `src/lib/documents.test.ts`, and the real-browser `scripts/ui-smoke.mjs` fixture. Match these patterns.
- Do not add code comments. Express the model through names, types, schema, and tests.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Rust format | `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | exit 0 |
| Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all tests pass |
| Frontend tests | `npm test` | all tests pass |
| Build | `npm run build` | exit 0; the existing bundle-size warning is allowed |
| UI smoke | `npm run test:ui` | exit 0 |
| Whitespace | `git diff --check` | exit 0, no output |

## Suggested executor toolkit

- Use the `better-layout`, `better-writing`, and `better-accessibility` skills when adding the project shelf and command-palette states.
- Preserve existing shadcn/Radix and Tailwind patterns; do not introduce another UI library.

## Scope

**In scope — the only files you may modify:**

- `src-tauri/src/database.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/mcp.rs`
- `src-tauri/tests/mcp_stdio.rs`
- `src/lib/archive.ts`
- `src/lib/documents.ts`
- `src/lib/documents.test.ts`
- `src/components/MarkdownEditor.tsx`
- `src/App.tsx`
- `src/App.test.tsx`
- `scripts/ui-smoke.mjs`

**Out of scope:**

- Vim mappings or editor behavior beyond presenting `project` references through the existing reference widget
- rendered Markdown or a new reading-mode dependency
- journal/calendar navigation
- reviewed/unreviewed artifact state
- changing daily attachment status semantics
- backlinks or indexing inline references
- a generic graph/relation API
- embeddings, vector search, graph visualization, or external synchronization
- changing private-document behavior
- package dependencies or lockfiles

## Git workflow

- Branch: `advisor/001-project-knowledge-foundation`
- Commit the completed implementation with a signed Conventional Commit: `feat: organize knowledge by project`
- Do not push, merge, or open a pull request.

## Steps

### Step 1: Migrate documents and add concrete project membership

In `src-tauri/src/database.rs`:

1. Raise `SCHEMA_VERSION` from 5 to 6.
2. Extend the document-kind constraint to allow `project`.
3. Add a constraint that project documents are user-authored, while preserving the existing daily constraint.
4. Add `project_documents` with:
   - `project_document_id` referencing `documents(id)` with `ON DELETE CASCADE`;
   - `document_id` referencing `documents(id)` with `ON DELETE CASCADE`;
   - `added_by` constrained to `user | agent`;
   - `created_at`;
   - composite primary key `(project_document_id, document_id)`;
   - a check preventing self-membership;
   - an index supporting lookup by `document_id` in addition to the primary-key project lookup.
5. Because SQLite cannot alter the existing `documents.kind` check, implement a v5-to-v6 rebuild that preserves all document IDs, revisions, presence rows, daily attachments, timestamps, bodies, and the `documents` AUTOINCREMENT high-water mark, including IDs of rows deleted before migration. Rebuild child tables as needed so foreign keys point to the final `documents` table. Run `PRAGMA foreign_key_check` in the migration and fail rather than committing invalid references.
6. Fresh databases must finish directly at schema version 6 through the existing migration pipeline.

Add migration tests that create a representative v5 database containing a daily, a note, an artifact, presence, and an attachment; create and delete a higher-ID document before migration; reopen it; assert all rows, foreign keys, and the historical AUTOINCREMENT high-water mark survived; assert a new project receives an ID above the deleted high ID; reopen again to prove idempotence. Update the fresh-schema test to assert version 6 and both project constraints.

**Verify**: `cargo fmt --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml database::tests::` → all database tests pass.

### Step 2: Add concrete project operations and privacy behavior

In `src-tauri/src/database.rs`, add concrete methods and serializable summary types following existing patterns:

- `create_project(day, visibility) -> Document`: validates day/visibility and creates an empty user-authored project.
- `add_document_to_project(project_id, document_id, added_by)`: validates positive IDs and actor; requires the parent to be a project; requires the member to exist and not be a project; inserts idempotently in an immediate transaction.
- `list_project_documents(project_id) -> Vec<Document>`: GUI path; requires an existing project; returns member documents newest-updated first, with deterministic ID tie-break, capped at 50.
- `mcp_project_context(project_id, limit)`: requires a shared project, validates `1..=50`, returns the sanitized project plus only shared sanitized member documents in the same deterministic order.

Integrate optional project association atomically into:

- `mcp_create_artifact(..., project_id: Option<i64>)`
- `mcp_create_daily_attachment(..., project_id: Option<i64>)`

The supplied project must be a shared project. A private, missing, or non-project ID must produce the same public MCP `document not found` behavior and must leave no artifact, attachment, or membership behind. Store `added_by = 'agent'` for MCP associations and `user` for GUI associations.

Allow GUI deletion of a project using the existing delete operation; cascade only memberships, never member documents. Preserve the prohibition on deleting daily documents.

Add tests for multi-project membership, idempotent insertion, nested-project rejection, cascade semantics, deterministic limit, private project/member filtering, and atomic rollback on invalid MCP project association.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml database::tests::` → all database tests pass.

### Step 3: Expose project operations through MCP and Tauri

In `src-tauri/src/mcp.rs`:

- Add optional `project_id` to both creation argument schemas.
- Add `get_project_context({ project_id, limit? })` with default 20 and maximum 50.
- Return a structured object containing:
  - `project`: the full shared project document shape;
  - `documents`: deterministic summaries using the existing search-summary shape (`id`, `kind`, `author`, `day`, `label`, `updated_at`).
- Claim agent presence on the project after `get_project_context`, and continue claiming the created artifact after creation.
- Keep every output schema as an object.
- Update exact-tool tests from five to six and add privacy, invalid-limit, and association assertions.

In `src-tauri/src/main.rs`, add thin Tauri commands for:

- `create_project(day, visibility?)`
- `add_document_to_project(project_id, document_id)` using actor `user`
- `list_project_documents(project_id)`

Register them in the existing invoke handler.

Extend `src-tauri/tests/mcp_stdio.rs` to verify all six tools over the built binary. Create a shared project directly through the observer database, create a project-associated artifact through MCP, retrieve project context, and assert the persisted membership. Also verify a private member is absent from MCP context.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml` → all Rust unit and stdio integration tests pass.

### Step 4: Mirror project types and labels in the frontend

In `src/lib/archive.ts`:

- Extend `DocumentKind` with `project`.
- Add typed invoke functions for `createProject`, `addDocumentToProject`, and `listProjectDocuments`.

In `src/lib/documents.ts`:

- Return `Untitled project` for an empty project body; preserve `Untitled note` for notes and artifacts.
- Continue deriving a nonempty project title from its first nonempty Markdown line.

In `src/components/MarkdownEditor.tsx`, extend only the existing reference-widget kind handling so resolved project references typecheck and render through the current document-reference behavior. Do not change Vim mappings or introduce new editor behavior.

Update `src/lib/documents.test.ts` accordingly.

**Verify**: `npm test -- src/lib/documents.test.ts && npm run build` → tests and typecheck/build pass.

### Step 5: Add the minimal human project workflow

In `src/App.tsx`, preserve the centered canvas and implement:

1. A `New project` command alongside new note/private note. Creating it uses the current local day, opens the empty project, and focuses the editor so the user can type its Markdown title/body.
2. A project-documents shelf below the editor whenever the active document is a project. Unlike daily Agent Work, show it even when empty so the project has an obvious organization affordance.
3. The shelf lists current member documents with label, kind, day, and author provenance. Selecting a member uses the existing `openDocument` flow.
4. A visible `Add document` action in the project shelf and an equivalent command-palette action when a project is active.
5. The action opens the existing Explorer in a distinct `add-to-project` purpose. Search and preview remain unchanged; selecting a non-project document adds it idempotently, closes the explorer, refreshes the shelf, and returns focus to the project editor. Projects must be excluded as candidates and selection must be derived from the filtered candidate list so keyboard Enter adds the first visible document. Do not overload reference insertion: `Ctrl-Enter` must insert references only in ordinary open mode, while the explorer hint and Enter behavior clearly reflect the current purpose.
6. Poll project documents while a project is active using the same restrained local-process pattern as daily attachments so newly created MCP artifacts appear without reopening the project. Clean up the timer on document change and ignore late success/error responses from an inactive polling generation. Likewise, an in-flight add must not update another project's shelf, close a subsequently opened Explorer, or steal focus after its originating add flow is no longer current.
7. Explorer and footer metadata must display `Project` for project documents.
8. Project deletion uses the existing confirmation flow, with project-specific copy that says member documents are retained.

Keep existing daily, note, artifact, conflict, autosave, buffer, and reference behavior unchanged.

Add focused `src/App.test.tsx` coverage for:

- creating and opening a project;
- empty project shelf visibility;
- entering add-to-project Explorer mode, adding a selected note, refreshing the shelf, and restoring focus;
- keyboard selection from a result list beginning with an excluded project, plus suppression of reference insertion in add mode;
- opening a listed member;
- project metadata and project-specific deletion copy;
- polling cleanup and ignored late responses when leaving a project.

Update `scripts/ui-smoke.mjs` mocks for every new Tauri command. Extend the smoke flow to create a project, add an existing note, observe the project shelf, open the member, and confirm the Explorer still performs ordinary open/reference behavior outside add mode.

**Verify**: `npm test && npm run build && npm run test:ui` → all frontend tests, build, and UI smoke pass.

### Step 6: Final contract verification

Run every repository gate and inspect the diff for scope and vocabulary consistency.

**Verify**:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
npm test
npm run build
npm run test:ui
git diff --check
```

Every command must exit 0. The existing Vite bundle-size warning is allowed; no new warnings should be introduced.

## Test plan

- Rust migration tests: v5 preservation, v6 fresh schema, idempotent reopen, foreign-key integrity.
- Rust domain tests: creation, memberships, multiple projects, nested rejection, cascade, ordering/limit, privacy, atomic rollback.
- MCP unit and stdio tests: six structured tools, project context, project-associated artifacts, private omission, invalid project parity.
- Frontend unit tests: project labels, project creation, membership Explorer mode, shelf refresh/opening, delete copy, timer cleanup.
- Real browser smoke: create project, add existing note, observe/open membership, preserve normal Explorer behavior.

## Done criteria

- [ ] Schema version is 6 and v5 data migrates without loss.
- [ ] Projects reuse the existing document lifecycle and cannot be agent-authored.
- [ ] `project_documents` is concrete, many-to-many, idempotent, and rejects nested projects.
- [ ] MCP exposes exactly six structured tools including bounded project context.
- [ ] MCP project context and creation associations never expose private documents.
- [ ] GUI can create a project, add documents through Explorer, and browse project members.
- [ ] Existing daily, artifact, search, references, autosave, conflict, and Vim tests remain green.
- [ ] All final verification commands exit 0.
- [ ] `git diff --name-only d8deb0b..HEAD` contains only the eleven in-scope source/test files.
- [ ] No package or lockfile changed.

## STOP conditions

Stop and report instead of improvising if:

- The drift check is nonempty.
- Preserving foreign keys in the v5-to-v6 rebuild requires disabling foreign-key enforcement outside the migration transaction.
- The project workflow requires changing `MarkdownEditor.tsx`, Vim mappings, or adding a Markdown-rendering dependency.
- MCP privacy cannot keep private and missing project IDs indistinguishable.
- An implementation requires a generic relationship abstraction rather than `project_documents`.
- Any final verification command fails twice after one focused correction.
- Any file outside the in-scope list must change.

## Maintenance notes

- A future backlinks milestone may index inline references, but must not silently reinterpret project membership or daily attachments.
- A future reading-mode milestone should change presentation, not create a second project body.
- If project metadata grows beyond what a document body and current columns express, revisit it from demonstrated needs rather than adding speculative fields now.
- Review migration ordering, foreign-key targets after table renames, MCP privacy filtering, and Explorer purpose transitions particularly carefully.
