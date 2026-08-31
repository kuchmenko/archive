use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use rusqlite::Connection;
use serde_json::{Value, json};

fn send(stdin: &mut impl Write, message: Value) {
    writeln!(stdin, "{message}").unwrap();
    stdin.flush().unwrap();
}

fn response(reader: &mut impl BufRead, id: i64) -> Value {
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        let message: Value = serde_json::from_str(&line).unwrap();
        if message["id"] == id {
            return message;
        }
    }
}

fn context(key: &str) -> Value {
    json!({
        "idempotency_key": key,
        "actor": "stdio-agent",
        "thread": "stdio-thread",
        "client": "archive-integration"
    })
}

fn call(
    stdin: &mut impl Write,
    reader: &mut impl BufRead,
    id: i64,
    name: &str,
    arguments: Value,
) -> Value {
    send(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }),
    );
    response(reader, id)
}

#[test]
fn mcp_stdio_built_binary_runs_representative_record_flow_with_clean_framing() {
    let state = tempfile::tempdir().unwrap();
    let database_path = state
        .path()
        .join("dev.kuchmenko.archive")
        .join("archive.sqlite3");
    let mut child = Command::new(env!("CARGO_BIN_EXE_archive"))
        .arg("mcp")
        .env("XDG_DATA_HOME", state.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "archive-test", "version": "1"}
            }
        }),
    );
    assert!(response(&mut stdout, 1)["result"]["serverInfo"]["name"].is_string());
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    );
    let tools = response(&mut stdout, 2);
    let tools = tools["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 20);
    assert!(tools.iter().all(|tool| {
        tool.get("inputSchema").is_some() && tool["outputSchema"]["type"] == "object"
    }));
    assert!(!tools.iter().any(|tool| tool["name"] == "search_documents"));

    let work = call(
        &mut stdin,
        &mut stdout,
        3,
        "create_scope",
        json!({"name": "work", "context": context("stdio-scope")}),
    );
    let work_id = work["result"]["structuredContent"]["id"].as_i64().unwrap();
    let label = call(
        &mut stdin,
        &mut stdout,
        4,
        "create_label",
        json!({
            "facet": "topic",
            "key": "rust",
            "display_name": "Rust",
            "aliases": ["rust-lang"],
            "context": context("stdio-label")
        }),
    );
    let label_id = label["result"]["structuredContent"]["id"].as_i64().unwrap();
    let created = call(
        &mut stdin,
        &mut stdout,
        5,
        "create_record",
        json!({
            "record": {
                "scope_id": work_id,
                "title": "Stdio record",
                "payload": {"kind": "note", "body": "initial local knowledge"},
                "sources": [],
                "label_ids": [1]
            },
            "context": context("stdio-create")
        }),
    );
    let record_id = created["result"]["structuredContent"]["id"]
        .as_i64()
        .unwrap();
    assert_eq!(
        created["result"]["structuredContent"]["current"]["payload"]["kind"],
        "note"
    );
    let duplicate = call(
        &mut stdin,
        &mut stdout,
        6,
        "create_record",
        json!({
            "record": {
                "scope_id": work_id,
                "title": "Stdio record",
                "payload": {"kind": "note", "body": "initial local knowledge"},
                "sources": [],
                "label_ids": [1]
            },
            "context": context("stdio-create")
        }),
    );
    assert_eq!(duplicate["result"]["structuredContent"]["id"], record_id);
    let revised = call(
        &mut stdin,
        &mut stdout,
        7,
        "revise_record",
        json!({
            "record_id": record_id,
            "expected_revision": 1,
            "title": "Stdio record corrected",
            "payload": {"kind": "note", "body": "corrected local knowledge"},
            "sources": [],
            "reason": "correct wording",
            "context": context("stdio-revise")
        }),
    );
    assert_eq!(
        revised["result"]["structuredContent"]["current_revision"],
        2
    );
    let labeled = call(
        &mut stdin,
        &mut stdout,
        8,
        "add_label",
        json!({
            "record_id": record_id,
            "label_id": label_id,
            "reason": "organized by topic",
            "context": context("stdio-add-label")
        }),
    );
    assert_eq!(
        labeled["result"]["structuredContent"]["labels"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let organized = call(
        &mut stdin,
        &mut stdout,
        9,
        "retract_label",
        json!({
            "record_id": record_id,
            "label_id": 1,
            "reason": "left inbox",
            "context": context("stdio-retract-label")
        }),
    );
    assert_eq!(
        organized["result"]["structuredContent"]["labels"][0]["canonical"],
        "topic:rust"
    );
    let global = call(
        &mut stdin,
        &mut stdout,
        10,
        "create_record",
        json!({
            "record": {
                "scope_id": 1,
                "title": "Global reference",
                "payload": {"kind": "idea", "proposal": "reuse locally"},
                "sources": [],
                "label_ids": [1]
            },
            "context": context("stdio-global")
        }),
    );
    let global_id = global["result"]["structuredContent"]["id"]
        .as_i64()
        .unwrap();
    let relation = call(
        &mut stdin,
        &mut stdout,
        11,
        "add_relation",
        json!({
            "source_record_id": record_id,
            "target_record_id": global_id,
            "kind": "references",
            "reason": "explicit cross-scope reference",
            "context": context("stdio-relation")
        }),
    );
    assert_eq!(
        relation["result"]["structuredContent"]["kind"],
        "references"
    );
    let searched = call(
        &mut stdin,
        &mut stdout,
        12,
        "search_records",
        json!({
            "scope_id": work_id,
            "query": "corrected knowledge",
            "label_ids": [label_id],
            "include_history": true
        }),
    );
    let hit = &searched["result"]["structuredContent"]["records"][0];
    assert_eq!(hit["record"]["id"], record_id);
    assert_eq!(hit["record"]["history"].as_array().unwrap().len(), 2);
    assert!(
        hit["match_explanation"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "fts:title_or_payload")
    );
    let read = call(
        &mut stdin,
        &mut stdout,
        13,
        "read_record",
        json!({"record_id": record_id, "include_history": true}),
    );
    assert_eq!(
        read["result"]["structuredContent"]["relations"][0]["target_record_id"],
        global_id
    );
    let mermaid = call(
        &mut stdin,
        &mut stdout,
        14,
        "validate_mermaid",
        json!({"source": "flowchart TD\nA-->B"}),
    );
    assert_eq!(mermaid["result"]["structuredContent"]["valid"], true);
    let embeddings = call(&mut stdin, &mut stdout, 15, "embedding_status", json!({}));
    assert_eq!(
        embeddings["result"]["structuredContent"]["eligible_records"],
        2
    );
    assert_eq!(
        embeddings["result"]["structuredContent"]["pending_records"],
        2
    );

    let observer = Connection::open(&database_path).unwrap();
    observer.busy_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        observer
            .query_row(
                "SELECT current_revision FROM records WHERE id=?1",
                [record_id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(
        observer
            .query_row("SELECT count(*) FROM label_retractions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
    assert_eq!(
        observer
            .query_row("SELECT count(*) FROM record_relations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );

    drop(stdin);
    let mut remaining_stdout = String::new();
    stdout.read_to_string(&mut remaining_stdout).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(remaining_stdout.is_empty());
    assert!(output.stderr.is_empty());
}
