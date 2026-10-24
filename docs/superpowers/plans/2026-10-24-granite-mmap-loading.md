# Granite mmap Loading Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Several `archive mcp` processes share one copy of the Granite weights in memory by loading an offline-optimized `model.ort` through mmap.

**Architecture:** `archive embeddings install` converts `model.onnx` into `model.ort` once, through a temporary file and `rename`. `GraniteModel::load` maps `model.ort` read-only and builds the ONNX Runtime session with `commit_from_memory_directly`, so weights are read from shared page-cache pages. All code stays in `src/embeddings.rs`.

**Tech Stack:** Rust 2024, `ort = "=2.0.0-rc.13"`, new `memmap2 = "0.9"`, `tempfile` (already a dev-dependency).

**Spec:** `docs/superpowers/specs/2026-10-24-granite-mmap-loading-design.md`

## Global Constraints

- `ort` stays pinned at `=2.0.0-rc.13`.
- Conversion uses `GraphOptimizationLevel::Level3` and config entry `session.save_model_format` = `ORT`.
- Loading uses `GraphOptimizationLevel::Disable` and `commit_from_memory_directly`.
- Prepacking is off for both conversion and loading (`with_prepacking(false)`).
- Both keep today's settings: CPU execution provider with arena allocator off and `error_on_failure()`, `with_inter_threads(1)`, `with_memory_pattern(false)`.
- `manifest.json` format and `verify_installation` stay unchanged.
- MCP never converts the model. A missing `model.ort` is an error that names `archive embeddings install`.
- `model.ort` is never rewritten in place: it is created by `rename` only, and an existing file is left alone.
- No change to malloc arenas, model unloading, or MCP tools.
- `just check` (fmt, test, clippy with `-D warnings`) passes after every task.
- Conventional Commits, signed (`git commit -S`).

## Review Focus

1. Existing installation, new binary, `install` not run again: the first dense call fails with a message that names `archive embeddings install`, not a raw ORT "no such file" error. Test: `missing_ort_model_reports_install_command` (Task 1).
2. Conversion fails (bad or truncated `model.onnx`): no `model.ort` and no temporary file is left behind, so the next `install` tries again. Test: `failed_conversion_leaves_no_files` (Task 1).
3. `install` runs again while MCP processes have `model.ort` mapped: the file is not touched. Test: `ort_model_is_written_once_and_loads_from_memory` checks bytes and modification time after a second call (Task 1).
4. Fresh install: the model directory appears with `model.ort` already in it, never without it. Covered by converting inside the temporary install directory before its `rename` (Task 2, Step 3). No automated test, because `install` checks the real 1.2 GB model's hashes.
5. `ort` upgrade with an old `model.ort`: the README tells the user to delete `model.ort` and run `install` again (Task 2). No code.

---

### Task 1: Convert to `.ort` and load through mmap

**Files:**
- Modify: `Cargo.toml` (add `memmap2 = "0.9"` to `[dependencies]`)
- Create: `tests/fixtures/mul_1.onnx` (copy of ONNX Runtime's `onnxruntime/datasets/mul_1.onnx`, 130 bytes, MIT; on this machine at `~/.local/lib/python3.13/site-packages/onnxruntime/datasets/mul_1.onnx`)
- Modify: `src/embeddings.rs` (`GraniteModel`, `GraniteModel::load`, new private functions, test module)

**Interfaces:**
- Consumes: nothing new.
- Produces, all private to `src/embeddings.rs`:
  - `fn session_builder(level: GraphOptimizationLevel) -> Result<SessionBuilder, Error>`: today's shared session settings plus prepacking off, at the given level.
  - `fn write_ort_model(onnx: &Path, ort: &Path) -> Result<(), Error>`: returns `Ok(())` without touching anything when `ort` exists. Otherwise converts into `<ort>.<pid>.tmp` in the same directory and renames it to `ort`. Removes the temporary file on any error.
  - `fn load_ort_session(path: &Path) -> Result<InMemorySession<'static>, Error>`: errors with `Error::Invalid` when `path` is missing. Otherwise maps the file, leaks the `Mmap` with `Box::leak`, and commits with `commit_from_memory_directly`.
  - `GraniteModel.session` changes type to `InMemorySession<'static>`.

Fixture behaviour, checked in the spike: input `X` is f32 `[3, 2]`, output `Y` is `X * W` with an initializer `W`. With `X` all ones, `Y` is `[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]` with shape `[3, 2]`. The converted `.ort` is 1,480 bytes.

- [ ] **Step 1: Add the fixture and write the failing tests** in the existing `mod tests` of `src/embeddings.rs`. Load the fixture with `include_bytes!("../tests/fixtures/mul_1.onnx")`, write it into a `tempfile::tempdir()`, and add a one-line comment naming its origin (ONNX Runtime test dataset, MIT).

```rust
#[test]
fn ort_model_is_written_once_and_loads_from_memory() {
    // write mul_1.onnx to dir/model.onnx, then:
    write_ort_model(&onnx, &ort).unwrap();
    let bytes = fs::read(&ort).unwrap();
    let modified = fs::metadata(&ort).unwrap().modified().unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2); // model.onnx, model.ort; no *.tmp left

    write_ort_model(&onnx, &ort).unwrap();
    assert_eq!(fs::read(&ort).unwrap(), bytes);
    assert_eq!(fs::metadata(&ort).unwrap().modified().unwrap(), modified);

    let mut session = load_ort_session(&ort).unwrap();
    let x = [1.0_f32; 6];
    let outputs = session
        .run(ort::inputs! { "X" => TensorRef::from_array_view(([3_usize, 2], x.as_slice())).unwrap() })
        .unwrap();
    let (shape, y) = outputs["Y"].try_extract_tensor::<f32>().unwrap();
    assert_eq!(shape.as_ref(), [3, 2]);
    assert_eq!(y, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
}

#[test]
fn missing_ort_model_reports_install_command() {
    let error = load_ort_session(&dir.path().join("model.ort")).err().unwrap();
    assert!(matches!(error, Error::Invalid(_)));
    assert!(error.to_string().contains("archive embeddings install"));
}

#[test]
fn failed_conversion_leaves_no_files() {
    // write b"not an onnx model" to dir/model.onnx, then:
    assert!(write_ort_model(&onnx, &ort).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1); // only model.onnx
}
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test --lib embeddings`
Expected: compile error, `write_ort_model` and `load_ort_session` not found.

- [ ] **Step 3: Implement** `session_builder`, `write_ort_model` and `load_ort_session` in `src/embeddings.rs`, add `memmap2` to `Cargo.toml`, and change `GraniteModel`:
  - `GraniteModel::load(directory)` keeps `verify_installation` and the tokenizer load, then calls `load_ort_session(&directory.join("model.ort"))`. `embed` is unchanged; `InMemorySession` derefs to `Session`.
  - `write_ort_model` builds with `session_builder(Level3)`, adds `with_config_entry("session.save_model_format", "ORT")` and `with_optimized_model_path(&temporary)`, calls `commit_from_file(onnx)`, drops the session, then `fs::rename(&temporary, ort)`.
  - The missing-file message: `"embedding model.ort is missing; run archive embeddings install <model-snapshot>"`.
  - `memmap2::Mmap::map` is `unsafe`. Add a `// SAFETY:` comment: `model.ort` is only ever replaced by `rename`, never written in place, so mapped bytes do not change.

- [ ] **Step 4: Run the tests and see them pass**

Run: `just check`
Expected: the three new tests and all existing tests pass; fmt and clippy clean.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock tests/fixtures/mul_1.onnx src/embeddings.rs
git commit -S -m "feat: load Granite from mmap .ort model"
```

### Task 2: Create `model.ort` during install

**Files:**
- Modify: `src/embeddings.rs` (`Embeddings::install`)
- Modify: `README.md` ("Optional semantic search" section)

**Interfaces:**
- Consumes: `write_ort_model(onnx: &Path, ort: &Path) -> Result<(), Error>` from Task 1.
- Produces: nothing new for other code. `archive embeddings install <snapshot>` now leaves `model.ort` in the model directory.

- [ ] **Step 1: Existing installation branch.** In the `if destination.exists()` branch, after both `verify_file` calls and before `return Ok(destination)`, call `write_ort_model(&destination.join("model.onnx"), &destination.join("model.ort"))?`.

- [ ] **Step 2: Fresh installation branch.** After writing `manifest.json` into `temporary` and before `fs::rename(&temporary, &destination)`, call `write_ort_model(&temporary.join("model.onnx"), &temporary.join("model.ort"))?`. The model directory then appears together with `model.ort`.

- [ ] **Step 3: README.** After the sentence about hard links in "Optional semantic search", add plain-English text saying that:
  - `install` also writes an optimized `model.ort`, and Archive loads that file through mmap so several MCP processes share one copy of the weights;
  - existing installations must run `archive embeddings install "$snapshot"` once;
  - after upgrading the `ort` dependency, delete `model.ort` and run `install` again.

- [ ] **Step 4: Run checks**

Run: `just check`
Expected: all tests pass; fmt and clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src/embeddings.rs README.md
git commit -S -m "feat: write model.ort during embeddings install"
```

### Task 3: Roll out and measure on this workstation

No code. This checks the spec's success criteria on live processes. Agent sessions must be restarted by the user, so this task needs the user.

- [ ] **Step 0: Save baseline search results with the old binary.** Through any running Archive MCP, run 3 fixed `semantic_search_records` queries in scope `global` ("Granite memory per process", "ONNX runtime CPU decision", "automatic embedding on record write") and save the top 5 record IDs of each.

- [ ] **Step 1: Install the binary and create `model.ort`**

Run: `just install`, then `archive embeddings install "$HOME/.cache/huggingface/hub/models--ibm-granite--granite-embedding-311m-multilingual-r2/snapshots/44399559930365213510b1ee2eb15ded83374f0e"`
Expected: JSON with the model directory. `model.ort` in that directory is 1,247,639,640 bytes (the spike's size; a small difference is fine).

- [ ] **Step 2: Check the index without reindexing**

Run: `archive embeddings status`
Expected: `pending_records` is 0. In a restarted session, run the Step 0 queries again. Expected: same top 5 IDs in the same order.

- [ ] **Step 3: Measure memory after the user restarts agent sessions** and each has made a dense call.

Run: for each `archive mcp` PID, read `Pss_Anon` and `Pss_File` from `/proc/<pid>/smaps_rollup`.
Expected: about 250 MB `Pss_Anon` per loaded process, at most about 400 MB. If clearly higher, open the malloc arena follow-up from the spec; do not change code in this plan.

- [ ] **Step 4: Record the result in Archive.** Add an evidence record in scope `global` with the measured numbers, linked to records 1526 and 1608. If the result supports the spec, revise 1526 to point at the new loading path.
