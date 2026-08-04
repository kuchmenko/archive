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

#[test]
fn built_binary_stdio_stays_clean_across_invalid_and_valid_mermaid_calls() {
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
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 5);
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| {
                tool.get("inputSchema").is_some() && tool.get("outputSchema").is_some()
            })
    );

    for (id, source, valid) in [
        (3, "flowchart TD\nA-->", false),
        (4, "sequenceDiagram\nAlice->>Bob: Hi", true),
    ] {
        send(
            &mut stdin,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": "validate_mermaid", "arguments": {"source": source}}
            }),
        );
        let result = response(&mut stdout, id);
        assert_eq!(result["result"]["structuredContent"]["valid"], valid);
        assert!(result.get("error").is_none());
    }

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "create_artifact",
                "arguments": {
                    "title": "Validated diagram",
                    "body": "```mermaid\nflowchart TD\nA-->B\n```"
                }
            }
        }),
    );
    let artifact = response(&mut stdout, 5);
    let document = &artifact["result"]["structuredContent"];
    assert!(document["body"].as_str().unwrap().contains("```mermaid"));
    let id = document["id"].as_i64().unwrap();
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {"name": "read_document", "arguments": {"id": id}}
        }),
    );
    assert_eq!(
        response(&mut stdout, 6)["result"]["structuredContent"]["body"],
        document["body"]
    );

    let observer = Connection::open(&database_path).unwrap();
    observer.busy_timeout(Duration::from_secs(5)).unwrap();
    let agent_document: i64 = observer
        .query_row(
            "SELECT document_id FROM presence WHERE actor='agent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(agent_document, id);

    for (request_id, expected_revision, expected_body) in [
        (7, 1, "first append"),
        (8, 2, "first append\n\nsecond append"),
    ] {
        let addition = if request_id == 7 {
            "first append"
        } else {
            "second append"
        };
        send(
            &mut stdin,
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "tools/call",
                "params": {
                    "name": "append_to_daily",
                    "arguments": {"day": "2026-08-04", "body": addition}
                }
            }),
        );
        let appended = response(&mut stdout, request_id);
        assert_eq!(
            appended["result"]["structuredContent"]["body"],
            expected_body
        );
        let (revision, stored_body): (i64, String) = observer
            .query_row(
                "SELECT revision, body FROM documents WHERE kind='daily' AND day='2026-08-04'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (revision, stored_body.as_str()),
            (expected_revision, expected_body)
        );
    }
    let daily_agent_count: i64 = observer
        .query_row(
            "SELECT count(*) FROM presence p JOIN documents d ON d.id=p.document_id WHERE p.actor='agent' AND d.kind='daily' AND d.day='2026-08-04'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(daily_agent_count, 1);

    drop(stdin);
    let mut remaining_stdout = String::new();
    stdout.read_to_string(&mut remaining_stdout).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(remaining_stdout.is_empty());
    assert!(output.stderr.is_empty());
    let remaining_presence: i64 = observer
        .query_row("SELECT count(*) FROM presence", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining_presence, 0);
}
