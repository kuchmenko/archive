use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

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

    drop(stdin);
    let mut remaining_stdout = String::new();
    stdout.read_to_string(&mut remaining_stdout).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(remaining_stdout.is_empty());
    assert!(output.stderr.is_empty());
}
