# Archive

Archive is a local-only agent knowledge archive. One Rust process stores text and structured JSON in one local SQLite database and exposes typed tools over MCP stdio. It has no HTTP transport, network service, authentication, sync, embeddings, LLM extraction, graph database, binary storage, or GUI.

## Build and run

```sh
cargo build --release --manifest-path src-tauri/Cargo.toml
cargo run --manifest-path src-tauri/Cargo.toml -- mcp
```

The release executable is `src-tauri/target/release/archive`. `archive mcp` stores SQLite data at the platform data directory under `dev.kuchmenko.archive/archive.sqlite3`. On Linux, `XDG_DATA_HOME` selects a different data root.

## Record model

Every record has a title, one primary scope, at least one active controlled label, lifecycle state, server timestamp, and caller-provided actor, thread, client, and idempotency key. Content is a closed tagged payload:

- `note`: `body`
- `observation`: `statement`, optional `observed_at`; requires a source
- `decision`: `choice`, optional `question`, `rationale`, and `decided_at`
- `idea`: `proposal`
- `snippet`: `language`, `code`, `origin` (`imported` or `generated`), optional `runtime` and `dependencies`; imported snippets require a source locator and content hash
- `metric`: `name`, numeric `value`, `unit`, optional `observed_at` or interval, dimensions, and method; requires a source
- `evidence`: `claim`, optional `action`, `outcome`, and `impact`; requires a source

Sources can carry identity, locator, version, content hash, anchor, and quote. Corrections append immutable revisions with an expected revision and reason. Semantic replacements use `supersede_record`; merges use `merge_records` so input identities and lifecycle histories remain intact.

The seeded scope is `global`. The seeded fallback label is `workflow:inbox`. Labels are explicit stable `facet:key` concepts with display names and aliases. Direct relation creation accepts `references`, `mentions`, `derived_from`, `supports`, `contradicts`, and `summarizes`. `supersedes` and `merged_into` are created only by their lifecycle operations.

## MCP tools

- Scopes: `create_scope`, `list_scopes`
- Labels: `create_label`, `search_labels`, `add_label`, `retract_label`
- Records: `create_record`, `search_records`, `read_record`, `revise_record`
- Relations: `add_relation`, `retract_relation`, `list_relations`
- Lifecycle: `transition_record`, `supersede_record`, `merge_records`
- Validation: `validate_mermaid`

Write tools require a context object:

```json
{
  "idempotency_key": "caller-stable-key",
  "actor": "agent-name",
  "thread": "thread-id",
  "client": "client-name"
}
```

A minimal `create_record` argument is:

```json
{
  "record": {
    "scope_id": 1,
    "title": "Local design note",
    "payload": { "kind": "note", "body": "The explicit knowledge." },
    "sources": [],
    "label_ids": [1]
  },
  "context": {
    "idempotency_key": "design-note-1",
    "actor": "agent-name",
    "thread": "thread-id",
    "client": "client-name"
  }
}
```

Search defaults to the exact requested scope, active lifecycle, and current revisions. `include_global` adds the global scope. Results use descending record IDs as a stable order and `next_before_id` for pagination. FTS5 searches current readable titles and payloads; deterministic kind, lifecycle, and label filters are applied with a match explanation. `include_history` returns revision history for matching current records; historical revisions are not separate FTS matches.

## Migration

Schema version 8 adds the typed record model in one transaction while retaining all version 1–7 migrations and every legacy table. Each legacy document becomes a global `note` record with the same numeric ID and exact stored body, plus `workflow:inbox`. Legacy timestamps, author, day, kind, visibility, and revision are retained in import metadata. Existing project memberships and daily attachments stay in their legacy tables.

Valid legacy `[[note:id|label]]` links become `references` relations. Legacy private records are imported as unreadable compatibility data and are omitted from MCP reads, relation results, and FTS. Links to private records remain unchanged in canonical imported bodies but are removed from agent-facing payloads and the rebuildable FTS index.

## Verify

```sh
just fmt
just test
just mcp-stdio
just clippy
```
