use rmcp::model::CallToolRequestParams;
use rusqlite::Connection;
use serde_json::json;

use super::*;

fn server() -> ArchiveMcp {
    ArchiveMcp::new(Database::open(tempfile::NamedTempFile::new().unwrap().path()).unwrap())
}

#[test]
fn generated_router_has_exact_structured_record_tools_and_closed_payload_schema() {
    let tools = ArchiveMcp::tool_router().list_all();
    let names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names,
        [
            "add_label",
            "add_relation",
            "create_label",
            "create_record",
            "create_scope",
            "list_relations",
            "list_scopes",
            "merge_records",
            "read_record",
            "retract_label",
            "retract_relation",
            "revise_record",
            "search_labels",
            "search_records",
            "supersede_record",
            "transition_record",
            "validate_mermaid",
        ]
        .into_iter()
        .collect()
    );
    assert!(tools.iter().all(|tool| tool.output_schema.is_some()));
    for removed in [
        "create_artifact",
        "create_daily_attachment",
        "get_project_context",
        "read_document",
        "search_documents",
    ] {
        assert!(!names.contains(removed));
    }
    let create = tools
        .iter()
        .find(|tool| tool.name == "create_record")
        .unwrap();
    let schema = serde_json::to_string(&create.input_schema).unwrap();
    for kind in [
        "note",
        "observation",
        "decision",
        "idea",
        "snippet",
        "metric",
        "evidence",
    ] {
        assert!(schema.contains(&format!("\"{kind}\"")));
    }
    assert!(schema.contains("\"additionalProperties\":false"));
    assert!(schema.contains("idempotency_key"));
    assert!(schema.contains("label_ids"));
    assert!(schema.contains("sources"));

    let add_relation = tools
        .iter()
        .find(|tool| tool.name == "add_relation")
        .unwrap();
    let schema = serde_json::to_string(&add_relation.input_schema).unwrap();
    for kind in [
        "references",
        "mentions",
        "derived_from",
        "supports",
        "contradicts",
        "summarizes",
    ] {
        assert!(schema.contains(&format!("\"{kind}\"")));
    }
    assert!(!schema.contains("supersedes"));
    assert!(!schema.contains("merged_into"));

    let transition = tools
        .iter()
        .find(|tool| tool.name == "transition_record")
        .unwrap();
    let schema = serde_json::to_string(&transition.input_schema).unwrap();
    assert!(schema.contains("\"retracted\""));
    assert!(!schema.contains("\"active\""));
    assert!(!schema.contains("\"superseded\""));
    assert!(!schema.contains("\"merged\""));
}

#[test]
fn hidden_and_missing_record_errors_are_identical() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("archive.sqlite3");
    let archive = ArchiveMcp::new(Database::open(&path).unwrap());
    let observer = Connection::open(path).unwrap();
    observer
        .execute(
            "INSERT INTO records(id,scope_id,kind,title,lifecycle,current_revision,readable,created_at,updated_at,actor,thread,client,idempotency_key)
             VALUES(50,1,'note','Private','active',1,0,'a','a','a','a','a','private-test')",
            [],
        )
        .unwrap();
    let hidden = archive
        .read_record(Parameters(ReadRecordArgs {
            record_id: 50,
            include_history: None,
        }))
        .err()
        .unwrap();
    let missing = archive
        .read_record(Parameters(ReadRecordArgs {
            record_id: 51,
            include_history: None,
        }))
        .err()
        .unwrap();
    assert_eq!(hidden, "record not found");
    assert_eq!(hidden, missing);
}

#[tokio::test]
async fn duplex_dispatches_typed_create_search_read_and_rejects_unknown_fields() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        server()
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap()
    });
    let client = ().serve(client_transport).await.unwrap();
    let created = client
        .call_tool(
            CallToolRequestParams::new("create_record").with_arguments(
                json!({
                    "record": {
                        "scope_id": 1,
                        "title": "Duplex note",
                        "payload": {"kind": "note", "body": "typed body"},
                        "sources": [],
                        "label_ids": [1]
                    },
                    "context": {
                        "idempotency_key": "duplex-create",
                        "actor": "agent",
                        "thread": "duplex",
                        "client": "test"
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .unwrap();
    let created = created.structured_content.unwrap();
    assert_eq!(created["current"]["payload"]["kind"], "note");
    let id = created["id"].as_i64().unwrap();
    let searched = client
        .call_tool(
            CallToolRequestParams::new("search_records").with_arguments(
                json!({"scope_id": 1, "query": "typed body"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(searched["records"][0]["record"]["id"], id);
    let read = client
        .call_tool(
            CallToolRequestParams::new("read_record").with_arguments(
                json!({"record_id": id, "include_history": true})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(read["history"].as_array().unwrap().len(), 1);
    let malformed = client
        .call_tool(
            CallToolRequestParams::new("read_record").with_arguments(
                json!({"record_id": id, "unknown": true})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(malformed.is_error, Some(true));
    client.cancel().await.unwrap();
    task.await.unwrap();
}
