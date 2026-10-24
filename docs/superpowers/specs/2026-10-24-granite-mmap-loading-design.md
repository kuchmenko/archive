# Granite mmap loading

Date: 2026-10-24
Status: approved design, not implemented

## Problem

Every agent session starts its own `archive mcp` process over stdio. The first dense call in each process loads its own copy of the Granite model. On 2026-10-24, 7 of 12 live processes had the model loaded and used 10.58 GB RSS together, about 1.1–1.7 GB each. Loaded processes keep the model for hours or days. Archive records 1526 (measurements) and 1608 (spike).

The memory is private to each process. `GraniteModel::load` calls `commit_from_file` on `model.onnx`, so ONNX Runtime parses the protobuf and copies all weights into its own heap. The kernel cannot share those pages between processes.

## Goal

Several `archive mcp` processes share one copy of the model weights in memory, with no daemon and no change to embeddings or the MCP tools.

Success: a loaded process uses at most about 400 MB of private memory (`Pss_Anon`). The shared file pages are counted once. Embeddings stay equal to the current ones, so no reindex is needed.

## Spike results

Throwaway spike, same session settings as `src/embeddings.rs`, 4 short queries and 40 timed runs per process, PSS read from `/proc/<pid>/smaps_rollup` while the session was alive.

| Mode | 1 process | 3 processes, total PSS | p50 latency | Load time |
|---|---|---|---|---|
| Current (`commit_from_file` on `.onnx`) | 1.40 GB | 4.31 GB | 13.5–15 ms | 0.95 s |
| mmap `.ort`, prepacking off | 694 MB | 1.19 GB | 17.8–18.3 ms | 0.2 s |
| mmap `.ort`, prepacking on | 970 MB | 2.02 GB | 13.3–13.8 ms | 0.39 s |

With prepacking off, each process has about 246 MB of private heap. The mapped file adds 430 MB of resident pages, shared by all processes. Only 430 MB of the 1.25 GB file was resident, because rows of the token embedding table that no query touched stay on disk. These pages are clean file pages, so under memory pressure the kernel drops them instead of writing them to swap.

Embeddings matched the current output: maximum absolute difference 0 with prepacking on, 8.9e-08 with prepacking off.

## Design

### 1. Create `model.ort` in `archive embeddings install`

- After verifying the model and tokenizer, `install` converts `model.onnx` into `model.ort` in the same model directory. It uses `GraphOptimizationLevel::Level3` and `session.save_model_format=ORT`, the same CPU execution provider settings as runtime, and prepacking off. This took 1.8 s in the spike.
- The converter writes to a temporary file in the model directory and renames it to `model.ort`. A process that already has the old file mapped keeps the old inode. The file is never rewritten in place.
- Today `install` returns early when the model directory already exists. That branch now also creates `model.ort` when it is missing. Running `install` again on an existing installation creates the file once and does nothing on later runs.
- `manifest.json` stays unchanged, so existing installations keep passing `verify_installation`.
- The file name has no ORT version. `ort` is pinned to `=2.0.0-rc.13` and upgrades are deliberate. The README says to delete `model.ort` and run `install` again after upgrading `ort`.

### 2. Load `model.ort` through mmap

- `GraniteModel::load` opens `model.ort` with `memmap2::Mmap` and builds the session with `GraphOptimizationLevel::Disable`, prepacking off, and `commit_from_memory_directly`. That call sets `session.use_ort_model_bytes_directly` and `session.use_ort_model_bytes_for_initializers`, so the weights are read from the mapped bytes.
- `commit_from_memory_directly` returns `InMemorySession<'m>`, which borrows the mapping. The mapping is leaked with `Box::leak` to get a `&'static` slice. The model already stays loaded for the life of the process, so this does not change behaviour. If unloading after idle time is added later, this part must change.
- If `model.ort` is missing, loading fails with an error that tells the user to run `archive embeddings install`. MCP never converts the model: up to 12 processes would each write 1.25 GB at once, in the middle of a tool call.
- `is_installed()` stays a directory check. A missing `model.ort` shows up as the load error above on the first dense call, the same way a broken installation does today.
- New dependency: `memmap2`.

### 3. Prepacking off

Prepacking makes a private copy of the MatMul weights in each process. Turning it off saves about 276 MB per process (246 MB private instead of 522 MB). The cost is about 4 ms per query (18 ms instead of 13.5–15 ms), and probably a similar share for long records during `sync`. An MCP tool call already takes seconds of model round trips, so this is not noticeable. There is no separate prepacking-on mode for `embeddings backfill`.

### 4. malloc arenas: no change now

Loaded live processes hold about 750 MB in 64 MB anonymous mappings, which are glibc per-thread malloc arenas. The single-thread spike held the same data, about 608 MB, in the main heap. So the arenas hold the normal ORT heap, placed there because tools run on different tokio worker threads. They are not a separate leak. The extra ~150 MB is probably activations from long records; this has not been checked.

With mmap the heap should shrink to about 250 MB. Check after rollout: if a loaded live process stays at or below about 400 MB `Pss_Anon`, do nothing. If it is clearly higher, try `malloc_trim` after `sync` or `MALLOC_ARENA_MAX` as a separate step.

## Out of scope

- Shared daemon over a socket.
- Unloading the model after idle time.
- Smaller or quantized model.
- Per-client switch for BM25 only.

## Testing

- No test in the repository uses the real model (1.2 GB), so CI cannot run one.
- Unit test for the install flow: `model.ort` is created through a temporary file and rename, and is not recreated when it already exists. The implementation plan decides whether this uses a tiny generated ONNX graph or injects the conversion step.
- Existing `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass.
- Manual check on this workstation after `just install` and `archive embeddings install <snapshot>`, then restarting agent sessions:
  1. `Pss_Anon` and `Pss_File` of live loaded `archive mcp` processes. Expected: about 250 MB private each, model file pages counted once.
  2. `embedding_status` reports a complete index with no reindex. `semantic_search_records` returns the same top results as before for a few fixed queries.
  3. Query latency through the spike method or timing `recall_context`. Expected: about 18 ms per embedding.

## Expected result

7 loaded processes use about 2.2 GB instead of about 10 GB. This is an estimate from the spike and has not been measured on live processes.
