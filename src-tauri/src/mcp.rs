use std::{
    process,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use rmcp::{
    ServiceExt,
    handler::server::wrapper::{Json, Parameters},
    schemars, tool, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};

use crate::database::{Database, Document, Error};
use crate::merman::{self, MermaidResult};

#[derive(Clone)]
pub struct ArchiveMcp {
    database: Arc<Database>,
    session_id: Arc<String>,
    active_document: Arc<Mutex<Option<i64>>>,
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    query: String,
    limit: Option<usize>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReadArgs {
    id: i64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateArtifactArgs {
    title: String,
    body: String,
    related_document_ids: Option<Vec<i64>>,
    project_id: Option<i64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateDailyAttachmentArgs {
    day: String,
    title: String,
    body: String,
    /// One of: completed, blocked, failed. Defaults to completed.
    status: Option<String>,
    project_id: Option<i64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProjectContextArgs {
    project_id: i64,
    limit: Option<usize>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ValidateMermaidArgs {
    source: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DocumentSummary {
    id: i64,
    kind: String,
    author: String,
    day: String,
    label: String,
    updated_at: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SearchDocumentsResult {
    documents: Vec<DocumentSummary>,
}
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ProjectContextResult {
    project: SharedDocument,
    documents: Vec<DocumentSummary>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SharedDocument {
    id: i64,
    kind: String,
    author: String,
    day: String,
    created_at: String,
    updated_at: String,
    body: String,
}

fn tool_error(error: Error) -> String {
    if matches!(error, Error::MissingDocument(_)) {
        "document not found".to_owned()
    } else {
        error.to_string()
    }
}

fn label(document: &Document) -> String {
    if document.kind == "daily" {
        return document.day.clone();
    }
    let line = document
        .body
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    let value = if (1..=6).contains(&hashes)
        && line[hashes..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
    {
        line[hashes..].trim()
    } else {
        line
    };
    if value.is_empty() {
        "Untitled note".to_owned()
    } else {
        value.to_owned()
    }
}

impl From<Document> for SharedDocument {
    fn from(document: Document) -> Self {
        Self {
            id: document.id,
            kind: document.kind,
            author: document.author,
            day: document.day,
            created_at: document.created_at,
            updated_at: document.updated_at,
            body: document.body,
        }
    }
}

#[tool_router(server_handler)]
impl ArchiveMcp {
    pub fn new(database: Database) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            database: Arc::new(database),
            session_id: Arc::new(format!(
                "mcp-{}-{nanos}-{}",
                process::id(),
                SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
            )),
            active_document: Arc::new(Mutex::new(None)),
        }
    }

    fn claim(&self, document_id: i64) {
        if let Ok(mut active_document) = self.active_document.lock() {
            *active_document = Some(document_id);
        }
        let _ = self
            .database
            .set_agent_presence(&self.session_id, document_id);
    }

    #[tool(description = "Search shared Archive documents and return deterministic summaries")]
    fn search_documents(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<Json<SearchDocumentsResult>, String> {
        self.database
            .mcp_search_documents(&args.query, args.limit.unwrap_or(20))
            .map(|documents| {
                Json(SearchDocumentsResult {
                    documents: documents
                        .into_iter()
                        .map(|document| DocumentSummary {
                            label: label(&document),
                            id: document.id,
                            kind: document.kind.clone(),
                            author: document.author,
                            day: document.day,
                            updated_at: document.updated_at,
                        })
                        .collect(),
                })
            })
            .map_err(tool_error)
    }

    #[tool(description = "Read one shared Archive document by its positive ID")]
    fn read_document(
        &self,
        Parameters(args): Parameters<ReadArgs>,
    ) -> Result<Json<SharedDocument>, String> {
        self.database
            .mcp_read_document(args.id)
            .map_err(tool_error)
            .map(|document| {
                self.claim(document.id);
                Json(document.into())
            })
    }

    #[tool(
        description = "Create a shared agent-authored artifact with optional links to shared documents"
    )]
    fn create_artifact(
        &self,
        Parameters(args): Parameters<CreateArtifactArgs>,
    ) -> Result<Json<SharedDocument>, String> {
        self.database
            .mcp_create_artifact(
                &args.title,
                &args.body,
                args.related_document_ids.as_deref().unwrap_or(&[]),
                args.project_id,
            )
            .map_err(tool_error)
            .map(|document| {
                self.claim(document.id);
                Json(document.into())
            })
    }

    #[tool(
        description = "Create an agent-authored artifact attached to the daily note for an explicit YYYY-MM-DD day without mutating the daily body"
    )]
    fn create_daily_attachment(
        &self,
        Parameters(args): Parameters<CreateDailyAttachmentArgs>,
    ) -> Result<Json<SharedDocument>, String> {
        self.database
            .mcp_create_daily_attachment(
                &args.day,
                &args.title,
                &args.body,
                args.status.as_deref(),
                args.project_id,
            )
            .map_err(tool_error)
            .map(|document| {
                self.claim(document.id);
                Json(document.into())
            })
    }

    #[tool(description = "Read bounded shared context for one shared Archive project")]
    fn get_project_context(
        &self,
        Parameters(args): Parameters<ProjectContextArgs>,
    ) -> Result<Json<ProjectContextResult>, String> {
        self.database
            .mcp_project_context(args.project_id, args.limit.unwrap_or(20))
            .map_err(tool_error)
            .map(|(project, documents)| {
                self.claim(project.id);
                Json(ProjectContextResult {
                    project: project.into(),
                    documents: documents
                        .into_iter()
                        .map(|document| DocumentSummary {
                            label: label(&document),
                            id: document.id,
                            kind: document.kind,
                            author: document.author,
                            day: document.day,
                            updated_at: document.updated_at,
                        })
                        .collect(),
                })
            })
    }

    #[tool(description = "Validate Mermaid source and return its type and structured diagnostics")]
    fn validate_mermaid(
        &self,
        Parameters(args): Parameters<ValidateMermaidArgs>,
    ) -> Json<MermaidResult> {
        Json(merman::validate(&args.source))
    }
}

pub async fn run(database: Database) -> Result<(), Box<dyn std::error::Error>> {
    let archive = ArchiveMcp::new(database);
    let service = archive.clone().serve(stdio()).await?;
    let heartbeat_archive = archive.clone();
    let heartbeat = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
            let id = heartbeat_archive
                .active_document
                .lock()
                .ok()
                .and_then(|value| *value);
            if let Some(id) = id {
                let _ = heartbeat_archive
                    .database
                    .set_agent_presence(&heartbeat_archive.session_id, id);
            }
        }
    });
    let result = service.waiting().await;
    heartbeat.abort();
    let _ = heartbeat.await;
    archive.database.remove_presence(&archive.session_id)?;
    result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;

    fn server() -> ArchiveMcp {
        ArchiveMcp::new(Database::open(tempfile::NamedTempFile::new().unwrap().path()).unwrap())
    }

    #[test]
    fn generated_router_has_exactly_six_structured_tools() {
        let tools = ArchiveMcp::tool_router().list_all();
        assert_eq!(tools.len(), 6);
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_ref())
                .collect::<std::collections::BTreeSet<_>>(),
            [
                "create_artifact",
                "create_daily_attachment",
                "get_project_context",
                "read_document",
                "search_documents",
                "validate_mermaid"
            ]
            .into_iter()
            .collect()
        );
        assert!(tools.iter().all(|tool| tool.output_schema.is_some()));
        for name in ["create_artifact", "create_daily_attachment"] {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            assert!(tool.input_schema["properties"]["project_id"].is_object());
        }
        let context = tools
            .iter()
            .find(|tool| tool.name == "get_project_context")
            .unwrap();
        assert!(context.input_schema["properties"]["project_id"].is_object());
        assert!(context.input_schema["properties"]["limit"].is_object());
    }

    #[test]
    fn project_context_enforces_privacy_limits_and_associations() {
        let archive = server();
        let project = archive
            .database
            .create_project("2026-08-04", "shared")
            .unwrap();
        let private = archive
            .database
            .create_note("2026-08-04", "private")
            .unwrap();
        archive
            .database
            .add_document_to_project(project.id, private.id, "user")
            .unwrap();
        let artifact = archive
            .create_artifact(Parameters(CreateArtifactArgs {
                title: "Project artifact".into(),
                body: "body".into(),
                related_document_ids: None,
                project_id: Some(project.id),
            }))
            .unwrap()
            .0;
        let context = archive
            .get_project_context(Parameters(ProjectContextArgs {
                project_id: project.id,
                limit: Some(20),
            }))
            .unwrap()
            .0;
        assert_eq!(context.project.id, project.id);
        assert_eq!(context.documents.len(), 1);
        assert_eq!(context.documents[0].id, artifact.id);
        assert_eq!(
            archive
                .database
                .list_project_documents(project.id)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            archive
                .get_project_context(Parameters(ProjectContextArgs {
                    project_id: project.id,
                    limit: Some(0)
                }))
                .err()
                .unwrap(),
            "limit must be between 1 and 50"
        );
        let private_project = archive
            .database
            .create_project("2026-08-04", "private")
            .unwrap();
        assert_eq!(
            archive
                .get_project_context(Parameters(ProjectContextArgs {
                    project_id: private_project.id,
                    limit: None
                }))
                .err()
                .unwrap(),
            "document not found"
        );
        assert_eq!(
            archive
                .get_project_context(Parameters(ProjectContextArgs {
                    project_id: 9999,
                    limit: None
                }))
                .err()
                .unwrap(),
            "document not found"
        );
    }

    #[test]
    fn private_and_missing_errors_are_exactly_identical() {
        let archive = server();
        let private = archive
            .database
            .create_note("2026-08-04", "private")
            .unwrap();
        let private_error = archive
            .read_document(Parameters(ReadArgs { id: private.id }))
            .err()
            .expect("private document must not be readable");
        let missing_error = archive
            .read_document(Parameters(ReadArgs {
                id: private.id + 1000,
            }))
            .err()
            .expect("missing document must not be readable");
        assert_eq!(private_error, missing_error);
        assert_eq!(private_error, "document not found");
    }

    #[tokio::test]
    async fn duplex_initialization_framing_dispatch_and_malformed_arguments() {
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
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
        let invalid = client
            .call_tool(
                CallToolRequestParams::new("validate_mermaid").with_arguments(
                    serde_json::json!({"source":"flowchart TD\nA-->"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(invalid.structured_content.unwrap()["valid"], false);
        let valid = client
            .call_tool(
                CallToolRequestParams::new("validate_mermaid").with_arguments(
                    serde_json::json!({"source":"flowchart TD\nA-->B"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(valid.structured_content.unwrap()["valid"], true);
        let result = client
            .call_tool(
                CallToolRequestParams::new("create_daily_attachment").with_arguments(
                    serde_json::json!({
                        "day":"2026-08-04",
                        "title":"Run",
                        "body":"hello",
                        "status":"blocked"
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await
            .unwrap();
        let content = result.structured_content.unwrap();
        assert_eq!(content["kind"], "artifact");
        assert_eq!(content["author"], "agent");
        assert_eq!(content["body"], "# Run\n\nhello");
        let malformed = client
            .call_tool(
                CallToolRequestParams::new("read_document").with_arguments(
                    serde_json::json!({"id":"wrong"})
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
}
