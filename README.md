# Archive

**Durable local memory for coding agents.**

Archive gives agents a small, structured knowledge base they can carry across sessions. It stores decisions, observations, evidence, snippets, and other useful context in SQLite, then makes that knowledge available through [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) tools.

Use Archive when chat history is too temporary, but a shared document or hosted memory service is too broad. An agent can save a decision with its source, recall a few relevant excerpts later, and open the full record only when needed.

Archive is:

- **Local:** one process and one SQLite database on your machine
- **Agent-friendly:** typed MCP tools with bounded recall results
- **Traceable:** sources, labels, revisions, and lifecycle history stay attached to records
- **Private by default:** no HTTP server, remote sync, telemetry, or hosted service
- **Useful without a model:** full-text search and BM25 recall work out of the box

Archive does not copy chat history automatically or decide what an agent should remember. You choose when to read and write knowledge through your agent's instructions.

## Quick start

You need a current stable Rust toolchain.

```sh
git clone https://github.com/kuchmenko/archive.git
cd archive
cargo install --locked --path .
```

This installs `archive` in Cargo's binary directory, usually `~/.cargo/bin`. Make sure that directory is on your `PATH`.

### Connect an MCP client

Add Archive as a local stdio server in your MCP client:

```json
{
  "mcpServers": {
    "archive": {
      "command": "archive",
      "args": ["mcp"]
    }
  }
}
```

The exact configuration file depends on your client. Use the absolute path to the executable if the client does not inherit your shell's `PATH`.

The client starts `archive mcp` when it needs the server. On first use, Archive creates a `global` scope and a `workflow:inbox` label.

### Give your agent a memory policy

Connecting the tools does not tell an agent when to use them. Add instructions like these to your agent configuration:

```text
Use Archive for knowledge that will be useful in future sessions, such as
decisions, verified observations, reusable snippets, and evidence. Recall
relevant context before starting related work. Save only explicit, durable
knowledge; do not archive routine chat or unverified guesses. Use read_record
when a recall excerpt needs its full context or revision history.
```

## First workflow

An agent can complete a useful write-and-recall flow with four tools:

1. Call `list_scopes` and `search_labels` with `{"query":"workflow:inbox"}`.
2. Save knowledge with `create_record`, using the returned scope and label IDs.
3. In a later session, call `recall_context` with a plain-language query.
4. Call `read_record` for the complete record when an excerpt is relevant.

A minimal `create_record` request looks like this:

```json
{
  "record": {
    "scope_id": 1,
    "title": "Use transactions for multi-table imports",
    "payload": {
      "kind": "decision",
      "choice": "Commit the imported records and relations in one transaction.",
      "rationale": "A failed import must not leave partial knowledge behind."
    },
    "sources": [],
    "label_ids": [1]
  },
  "context": {
    "idempotency_key": "import-transaction-decision",
    "actor": "coding-agent",
    "thread": "task-123",
    "client": "mcp-client"
  }
}
```

Recall related knowledge later:

```json
{
  "query": "How should multi-table imports handle partial failures?",
  "scope_id": 1,
  "include_global": true,
  "max_bytes": 4000
}
```

Use caller-stable idempotency keys for writes. Retrying the same logical write with the same key is safe.

## Privacy and storage

Archive keeps its canonical data in `archive.sqlite3` under the platform data directory `dev.kuchmenko.archive`. On Linux, set `XDG_DATA_HOME` to choose a different data root.

The MCP server communicates only through stdin and stdout. It has no network listener, account system, or remote backup. Files remain subject to your operating system's permissions and backup settings, so protect the data directory as you would any other local private data.

Embeddings are optional and local. Archive does not download a model or send record content to a model provider. Without a model, `recall_context` uses SQLite full-text search and BM25 ordering.

## Record model

Every record has a title, one scope, at least one active controlled label, a lifecycle state, a server timestamp, and caller-provided write context. Content uses one of seven typed payloads:

- `note`: free-form knowledge
- `observation`: something observed, with a required source
- `decision`: a choice with optional question, rationale, and date
- `idea`: a proposal
- `snippet`: imported or generated code with language and origin metadata
- `metric`: a measured value with a required source
- `evidence`: a sourced claim with optional action, outcome, and impact

Sources can include an identity, locator, version, content hash, anchor, and quote. Corrections append immutable revisions. Records can also be superseded, merged, retracted, labeled, and related without erasing their history.

Scopes separate bodies of knowledge. The seeded `global` scope can hold context that applies everywhere. Labels are stable `facet:key` concepts with display names and aliases.

## MCP tools

- Scopes: `create_scope`, `list_scopes`
- Labels: `create_label`, `search_labels`, `add_label`, `retract_label`
- Records: `create_record`, `search_records`, `recall_context`, `read_record`, `revise_record`
- Embeddings: `embedding_status`, `sync_embeddings`, `semantic_search_records`
- Relations: `add_relation`, `retract_relation`, `list_relations`
- Lifecycle: `transition_record`, `supersede_record`, `merge_records`
- Validation: `validate_mermaid`

Write tools require this context:

```json
{
  "idempotency_key": "caller-stable-key",
  "actor": "agent-name",
  "thread": "thread-id",
  "client": "client-name"
}
```

`search_records` provides deterministic full-text search and filters. `recall_context` returns 3–5 compact, relevant excerpts within a configurable 4,000–32,000 byte budget when enough records match. `read_record` returns exact content and optional revision history.

## Optional semantic search

Archive supports the official FP32 ONNX export of [`ibm-granite/granite-embedding-311m-multilingual-r2`](https://huggingface.co/ibm-granite/granite-embedding-311m-multilingual-r2) at revision `44399559930365213510b1ee2eb15ded83374f0e`. It does not download the model. Install an existing Hugging Face snapshot explicitly:

```sh
snapshot="$HOME/.cache/huggingface/hub/models--ibm-granite--granite-embedding-311m-multilingual-r2/snapshots/44399559930365213510b1ee2eb15ded83374f0e"
archive embeddings install "$snapshot"
archive embeddings status
archive embeddings backfill
```

The installer verifies exact model and tokenizer sizes and SHA-256 hashes. It hard-links files when possible and copies them otherwise.

Archive stores one 768-dimensional vector for each readable record revision. The index is derived data and can be rebuilt. Semantic search is available only when the index is complete, so it never returns silently partial results. `recall_context` uses semantic ordering when the complete index is available and BM25 otherwise.

## Existing databases

Archive applies SQLite migrations at startup. The current schema keeps migrations from the earlier daily-notes application so existing databases can be opened without losing legacy data.

Legacy documents become typed `note` records while their original metadata and tables remain intact. Legacy private records stay unreadable through MCP and are excluded from full-text and embedding indexes.

## Development

Build and run from the repository without installing:

```sh
cargo build --release
cargo run -- mcp
```

Run all checks:

```sh
just check
```
