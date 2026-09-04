use std::{path::Path, sync::Arc};

use rmcp::{
    ServiceExt,
    handler::server::wrapper::{Json, Parameters},
    schemars, tool, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};

use crate::database::{DEFAULT_RECALL_BUDGET_BYTES, Database, Error};
use crate::embeddings::{self, DIMENSIONS, Embeddings, MODEL, MODEL_REVISION};
use crate::merman::{self, MermaidResult};
use crate::model::{
    DirectRelationKind, EmbeddingStatus, EmbeddingSync, Label, Lifecycle, LifecycleTarget,
    RecallContext, Record, RecordInput, RecordKind, RecordPayload, Relation, Scope, SearchPage,
    SemanticSearchResult, SourceInput, WriteContext,
};

#[derive(Clone)]
pub struct ArchiveMcp {
    database: Arc<Database>,
    embeddings: Arc<Embeddings>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateScopeArgs {
    name: String,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateLabelArgs {
    facet: String,
    key: String,
    display_name: String,
    aliases: Vec<String>,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchLabelsArgs {
    query: String,
    facet: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateRecordArgs {
    record: RecordInput,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchRecordsArgs {
    query: Option<String>,
    scope_id: i64,
    include_global: Option<bool>,
    kinds: Option<Vec<RecordKind>>,
    lifecycles: Option<Vec<Lifecycle>>,
    label_ids: Option<Vec<i64>>,
    include_history: Option<bool>,
    before_id: Option<i64>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SemanticSearchRecordsArgs {
    query: String,
    scope_id: i64,
    include_global: Option<bool>,
    kinds: Option<Vec<RecordKind>>,
    lifecycles: Option<Vec<Lifecycle>>,
    label_ids: Option<Vec<i64>>,
    include_history: Option<bool>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct RecallContextArgs {
    query: String,
    scope_id: i64,
    include_global: Option<bool>,
    kinds: Option<Vec<RecordKind>>,
    lifecycles: Option<Vec<Lifecycle>>,
    label_ids: Option<Vec<i64>>,
    max_bytes: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadRecordArgs {
    record_id: i64,
    include_history: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReviseRecordArgs {
    record_id: i64,
    expected_revision: i64,
    title: String,
    payload: RecordPayload,
    sources: Vec<SourceInput>,
    reason: String,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct LabelMutationArgs {
    record_id: i64,
    label_id: i64,
    reason: String,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddRelationArgs {
    source_record_id: i64,
    target_record_id: i64,
    kind: DirectRelationKind,
    reason: String,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct RetractRelationArgs {
    relation_id: i64,
    reason: String,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListRelationsArgs {
    record_id: i64,
    include_retracted: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct TransitionRecordArgs {
    record_id: i64,
    to: LifecycleTarget,
    reason: String,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SupersedeRecordArgs {
    record_id: i64,
    replacement: RecordInput,
    reason: String,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct MergeRecordsArgs {
    record_ids: Vec<i64>,
    aggregate: RecordInput,
    reason: String,
    context: WriteContext,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ValidateMermaidArgs {
    source: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ScopesResult {
    scopes: Vec<Scope>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct LabelsResult {
    labels: Vec<Label>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct RelationsResult {
    relations: Vec<Relation>,
}

fn tool_error(error: Error) -> String {
    match error {
        Error::MissingRecord(_) => "record not found".to_owned(),
        Error::MissingRelation(_) => "relation not found".to_owned(),
        error => error.to_string(),
    }
}

fn embedding_tool_error(error: embeddings::Error) -> String {
    match error {
        embeddings::Error::Database(error) => tool_error(error),
        error => error.to_string(),
    }
}

#[tool_router(server_handler)]
impl ArchiveMcp {
    pub fn new(database: Database, data_directory: &Path) -> Self {
        Self {
            database: Arc::new(database),
            embeddings: Arc::new(Embeddings::new(data_directory)),
        }
    }

    fn finish_record_write(&self, record: Result<Record, Error>) -> Result<Json<Record>, String> {
        let record = record.map_err(tool_error)?;
        if self.embeddings.is_installed() {
            let sync = self
                .embeddings
                .sync(&self.database)
                .map_err(embedding_tool_error)?;
            if sync.status.pending_records != 0 {
                return Err(format!(
                    "embedding index still has {} pending records",
                    sync.status.pending_records
                ));
            }
        }
        Ok(Json(record))
    }

    #[tool(description = "Create or return one named Archive scope")]
    fn create_scope(
        &self,
        Parameters(args): Parameters<CreateScopeArgs>,
    ) -> Result<Json<Scope>, String> {
        self.database
            .create_scope(&args.name, &args.context)
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "List Archive scopes in deterministic name order")]
    fn list_scopes(&self) -> Result<Json<ScopesResult>, String> {
        self.database
            .list_scopes()
            .map(|scopes| Json(ScopesResult { scopes }))
            .map_err(tool_error)
    }

    #[tool(description = "Explicitly create or return a controlled faceted label")]
    fn create_label(
        &self,
        Parameters(args): Parameters<CreateLabelArgs>,
    ) -> Result<Json<Label>, String> {
        self.database
            .create_label(
                &args.facet,
                &args.key,
                &args.display_name,
                &args.aliases,
                &args.context,
            )
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "Search active labels by canonical key, display name, or alias")]
    fn search_labels(
        &self,
        Parameters(args): Parameters<SearchLabelsArgs>,
    ) -> Result<Json<LabelsResult>, String> {
        self.database
            .search_labels(&args.query, args.facet.as_deref(), args.limit.unwrap_or(20))
            .map(|labels| Json(LabelsResult { labels }))
            .map_err(tool_error)
    }

    #[tool(description = "Create one validated typed Archive record transactionally")]
    fn create_record(
        &self,
        Parameters(args): Parameters<CreateRecordArgs>,
    ) -> Result<Json<Record>, String> {
        self.finish_record_write(self.database.create_record(&args.record, &args.context))
    }

    #[tool(
        description = "Search current readable record revisions with deterministic filters and pagination"
    )]
    fn search_records(
        &self,
        Parameters(args): Parameters<SearchRecordsArgs>,
    ) -> Result<Json<SearchPage>, String> {
        self.database
            .search_records(
                args.query.as_deref(),
                args.scope_id,
                args.include_global.unwrap_or(false),
                args.kinds.as_deref().unwrap_or(&[]),
                args.lifecycles.as_deref().unwrap_or(&[]),
                args.label_ids.as_deref().unwrap_or(&[]),
                args.include_history.unwrap_or(false),
                args.before_id,
                args.limit.unwrap_or(20),
            )
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        description = "Search complete current Archive embeddings by semantic similarity with the same scope, lifecycle, kind, and label eligibility rules as deterministic search"
    )]
    fn semantic_search_records(
        &self,
        Parameters(args): Parameters<SemanticSearchRecordsArgs>,
    ) -> Result<Json<SemanticSearchResult>, String> {
        let embedding = self
            .embeddings
            .embed_query(&args.query)
            .map_err(embedding_tool_error)?;
        self.database
            .semantic_search_records(
                &embedding,
                MODEL,
                MODEL_REVISION,
                DIMENSIONS,
                args.scope_id,
                args.include_global.unwrap_or(false),
                args.kinds.as_deref().unwrap_or(&[]),
                args.lifecycles.as_deref().unwrap_or(&[]),
                args.label_ids.as_deref().unwrap_or(&[]),
                args.include_history.unwrap_or(false),
                args.limit.unwrap_or(20),
            )
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        description = "Recall 3–5 bounded evidence excerpts from durable Archive knowledge. Uses dense ordering when the complete local embedding index is available and BM25 otherwise; read_record remains the exact full-record follow-up."
    )]
    fn recall_context(
        &self,
        Parameters(args): Parameters<RecallContextArgs>,
    ) -> Result<Json<RecallContext>, String> {
        let semantic_available = self.embeddings.is_installed()
            && self
                .embeddings
                .status(&self.database)
                .map_err(embedding_tool_error)?
                .pending_records
                == 0;
        let embedding = semantic_available
            .then(|| self.embeddings.embed_query(&args.query))
            .transpose()
            .map_err(embedding_tool_error)?;
        self.database
            .recall_context(
                &args.query,
                embedding.as_deref(),
                MODEL,
                MODEL_REVISION,
                DIMENSIONS,
                args.scope_id,
                args.include_global.unwrap_or(false),
                args.kinds.as_deref().unwrap_or(&[]),
                args.lifecycles.as_deref().unwrap_or(&[]),
                args.label_ids.as_deref().unwrap_or(&[]),
                args.max_bytes.unwrap_or(DEFAULT_RECALL_BUDGET_BYTES),
            )
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "Report coverage of the selected local whole-record embedding index")]
    fn embedding_status(&self) -> Result<Json<EmbeddingStatus>, String> {
        self.embeddings
            .status(&self.database)
            .map(Json)
            .map_err(embedding_tool_error)
    }

    #[tool(
        description = "Generate selected local whole-record embeddings for all readable records that are missing or stale"
    )]
    fn sync_embeddings(&self) -> Result<Json<EmbeddingSync>, String> {
        self.embeddings
            .sync(&self.database)
            .map(Json)
            .map_err(embedding_tool_error)
    }

    #[tool(
        description = "Read one record by exact ID, optionally including immutable revision history"
    )]
    fn read_record(
        &self,
        Parameters(args): Parameters<ReadRecordArgs>,
    ) -> Result<Json<Record>, String> {
        self.database
            .read_record(args.record_id, args.include_history.unwrap_or(false))
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "Create an immutable correction revision with optimistic concurrency")]
    fn revise_record(
        &self,
        Parameters(args): Parameters<ReviseRecordArgs>,
    ) -> Result<Json<Record>, String> {
        self.finish_record_write(self.database.revise_record(
            args.record_id,
            args.expected_revision,
            &args.title,
            &args.payload,
            &args.sources,
            &args.reason,
            &args.context,
        ))
    }

    #[tool(description = "Append an active label assertion to a record")]
    fn add_label(
        &self,
        Parameters(args): Parameters<LabelMutationArgs>,
    ) -> Result<Json<Record>, String> {
        self.database
            .add_label(args.record_id, args.label_id, &args.reason, &args.context)
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "Append a label retraction while retaining at least one active label")]
    fn retract_label(
        &self,
        Parameters(args): Parameters<LabelMutationArgs>,
    ) -> Result<Json<Record>, String> {
        self.database
            .retract_label(args.record_id, args.label_id, &args.reason, &args.context)
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "Append one controlled cross-scope record relation")]
    fn add_relation(
        &self,
        Parameters(args): Parameters<AddRelationArgs>,
    ) -> Result<Json<Relation>, String> {
        self.database
            .add_relation(
                args.source_record_id,
                args.target_record_id,
                &args.kind,
                &args.reason,
                &args.context,
            )
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "Append a retraction for one relation assertion")]
    fn retract_relation(
        &self,
        Parameters(args): Parameters<RetractRelationArgs>,
    ) -> Result<Json<Relation>, String> {
        self.database
            .retract_relation(args.relation_id, &args.reason, &args.context)
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "List deterministic incoming and outgoing relations for one exact record")]
    fn list_relations(
        &self,
        Parameters(args): Parameters<ListRelationsArgs>,
    ) -> Result<Json<RelationsResult>, String> {
        self.database
            .list_relations(args.record_id, args.include_retracted.unwrap_or(false))
            .map(|relations| Json(RelationsResult { relations }))
            .map_err(tool_error)
    }

    #[tool(description = "Retract an active record with lifecycle history")]
    fn transition_record(
        &self,
        Parameters(args): Parameters<TransitionRecordArgs>,
    ) -> Result<Json<Record>, String> {
        let to = match args.to {
            LifecycleTarget::Retracted => Lifecycle::Retracted,
        };
        self.database
            .transition_record(args.record_id, &to, &args.reason, &args.context)
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "Create a semantic replacement and atomically supersede the old record")]
    fn supersede_record(
        &self,
        Parameters(args): Parameters<SupersedeRecordArgs>,
    ) -> Result<Json<Record>, String> {
        self.finish_record_write(self.database.supersede_record(
            args.record_id,
            &args.replacement,
            &args.reason,
            &args.context,
        ))
    }

    #[tool(description = "Create an aggregate and atomically mark all input records merged")]
    fn merge_records(
        &self,
        Parameters(args): Parameters<MergeRecordsArgs>,
    ) -> Result<Json<Record>, String> {
        self.finish_record_write(self.database.merge_records(
            &args.record_ids,
            &args.aggregate,
            &args.reason,
            &args.context,
        ))
    }

    #[tool(description = "Validate Mermaid source and return its type and structured diagnostics")]
    fn validate_mermaid(
        &self,
        Parameters(args): Parameters<ValidateMermaidArgs>,
    ) -> Json<MermaidResult> {
        Json(merman::validate(&args.source))
    }
}

pub async fn run(
    database: Database,
    data_directory: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    ArchiveMcp::new(database, data_directory)
        .serve(stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
