use std::sync::Arc;

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
}

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
        Self {
            database: Arc::new(database),
        }
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
            .map(|document| Json(document.into()))
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
            .map(|document| Json(document.into()))
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
            .map(|document| Json(document.into()))
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
    ArchiveMcp::new(database)
        .serve(stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;
    use rusqlite::{Connection, params};

    fn server() -> ArchiveMcp {
        ArchiveMcp::new(Database::open(tempfile::NamedTempFile::new().unwrap().path()).unwrap())
    }

    fn server_with_observer() -> (ArchiveMcp, Connection, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        let archive = ArchiveMcp::new(Database::open(&path).unwrap());
        let observer = Connection::open(path).unwrap();
        (archive, observer, directory)
    }

    fn insert_document(connection: &Connection, kind: &str, visibility: &str, body: &str) -> i64 {
        connection
            .execute(
                "INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body)
             VALUES(?1,?2,'user','2026-08-04','a','a',?3)",
                params![kind, visibility, body],
            )
            .unwrap();
        connection.last_insert_rowid()
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
        let (archive, observer, _directory) = server_with_observer();
        let project_id = insert_document(&observer, "project", "shared", "# Project");
        let private_id = insert_document(&observer, "note", "private", "# Private");
        observer
            .execute(
                "INSERT INTO project_documents(project_document_id,document_id,added_by,created_at)
             VALUES(?1,?2,'user','a')",
                [project_id, private_id],
            )
            .unwrap();
        let artifact = archive
            .create_artifact(Parameters(CreateArtifactArgs {
                title: "Project artifact".into(),
                body: "body".into(),
                related_document_ids: None,
                project_id: Some(project_id),
            }))
            .unwrap()
            .0;
        let context = archive
            .get_project_context(Parameters(ProjectContextArgs {
                project_id,
                limit: Some(20),
            }))
            .unwrap()
            .0;
        assert_eq!(context.project.id, project_id);
        assert_eq!(context.documents.len(), 1);
        assert_eq!(context.documents[0].id, artifact.id);
        assert_eq!(
            observer
                .query_row(
                    "SELECT count(*) FROM project_documents WHERE project_document_id=?1",
                    [project_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
        assert_eq!(
            archive
                .get_project_context(Parameters(ProjectContextArgs {
                    project_id,
                    limit: Some(0)
                }))
                .err()
                .unwrap(),
            "limit must be between 1 and 50"
        );
        let private_project_id = insert_document(&observer, "project", "private", "");
        assert_eq!(
            archive
                .get_project_context(Parameters(ProjectContextArgs {
                    project_id: private_project_id,
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
        let (archive, observer, _directory) = server_with_observer();
        let private_id = insert_document(&observer, "note", "private", "");
        let private_error = archive
            .read_document(Parameters(ReadArgs { id: private_id }))
            .err()
            .expect("private document must not be readable");
        let missing_error = archive
            .read_document(Parameters(ReadArgs {
                id: private_id + 1000,
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
