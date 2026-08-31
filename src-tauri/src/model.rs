use std::collections::BTreeMap;

use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteContext {
    pub idempotency_key: String,
    pub actor: String,
    pub thread: String,
    pub client: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Note,
    Observation,
    Decision,
    Idea,
    Snippet,
    Metric,
    Evidence,
}

impl RecordKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Observation => "observation",
            Self::Decision => "decision",
            Self::Idea => "idea",
            Self::Snippet => "snippet",
            Self::Metric => "metric",
            Self::Evidence => "evidence",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "note" => Some(Self::Note),
            "observation" => Some(Self::Observation),
            "decision" => Some(Self::Decision),
            "idea" => Some(Self::Idea),
            "snippet" => Some(Self::Snippet),
            "metric" => Some(Self::Metric),
            "evidence" => Some(Self::Evidence),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SnippetOrigin {
    Imported,
    Generated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetricInterval {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordPayload {
    Note {
        body: String,
    },
    Observation {
        statement: String,
        observed_at: Option<String>,
    },
    Decision {
        choice: String,
        question: Option<String>,
        rationale: Option<String>,
        decided_at: Option<String>,
    },
    Idea {
        proposal: String,
    },
    Snippet {
        language: String,
        code: String,
        origin: SnippetOrigin,
        runtime: Option<String>,
        dependencies: Option<Vec<String>>,
    },
    Metric {
        name: String,
        value: f64,
        unit: String,
        observed_at: Option<String>,
        interval: Option<MetricInterval>,
        dimensions: BTreeMap<String, String>,
        method: Option<String>,
    },
    Evidence {
        claim: String,
        action: Option<String>,
        outcome: Option<String>,
        impact: Option<String>,
    },
}

impl RecordPayload {
    pub fn kind(&self) -> RecordKind {
        match self {
            Self::Note { .. } => RecordKind::Note,
            Self::Observation { .. } => RecordKind::Observation,
            Self::Decision { .. } => RecordKind::Decision,
            Self::Idea { .. } => RecordKind::Idea,
            Self::Snippet { .. } => RecordKind::Snippet,
            Self::Metric { .. } => RecordKind::Metric,
            Self::Evidence { .. } => RecordKind::Evidence,
        }
    }

    pub fn text(&self) -> String {
        let mut values = Vec::new();
        match self {
            Self::Note { body } => values.push(body.clone()),
            Self::Observation {
                statement,
                observed_at,
            } => {
                values.push(statement.clone());
                push_option(&mut values, observed_at);
            }
            Self::Decision {
                choice,
                question,
                rationale,
                decided_at,
            } => {
                values.push(choice.clone());
                push_option(&mut values, question);
                push_option(&mut values, rationale);
                push_option(&mut values, decided_at);
            }
            Self::Idea { proposal } => values.push(proposal.clone()),
            Self::Snippet {
                language,
                code,
                origin,
                runtime,
                dependencies,
            } => {
                values.push(language.clone());
                values.push(code.clone());
                values.push(match origin {
                    SnippetOrigin::Imported => "imported".to_owned(),
                    SnippetOrigin::Generated => "generated".to_owned(),
                });
                push_option(&mut values, runtime);
                if let Some(dependencies) = dependencies {
                    values.extend(dependencies.iter().cloned());
                }
            }
            Self::Metric {
                name,
                value,
                unit,
                observed_at,
                interval,
                dimensions,
                method,
            } => {
                values.push(name.clone());
                values.push(
                    serde_json::Number::from_f64(*value)
                        .map_or_else(|| value.to_string(), |value| value.to_string()),
                );
                values.push(unit.clone());
                push_option(&mut values, observed_at);
                if let Some(interval) = interval {
                    values.push(format!("start: {}", interval.start));
                    values.push(format!("end: {}", interval.end));
                }
                values.extend(
                    dimensions
                        .iter()
                        .map(|(key, value)| format!("{key}: {value}")),
                );
                push_option(&mut values, method);
            }
            Self::Evidence {
                claim,
                action,
                outcome,
                impact,
            } => {
                values.push(claim.clone());
                push_option(&mut values, action);
                push_option(&mut values, outcome);
                push_option(&mut values, impact);
            }
        }
        values.join("\n")
    }
}

fn push_option(values: &mut Vec<String>, value: &Option<String>) {
    if let Some(value) = value {
        values.push(value.clone());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    pub identity: String,
    pub locator: Option<String>,
    pub version: Option<String>,
    pub content_hash: Option<String>,
    pub anchor: Option<String>,
    pub quote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordInput {
    pub scope_id: i64,
    pub title: String,
    pub payload: RecordPayload,
    pub sources: Vec<SourceInput>,
    pub label_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct SourceReference {
    pub id: i64,
    pub identity: String,
    pub locator: Option<String>,
    pub version: Option<String>,
    pub content_hash: Option<String>,
    pub anchor: Option<String>,
    pub quote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Provenance {
    pub server_time: String,
    pub actor: String,
    pub thread: String,
    pub client: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Scope {
    pub id: i64,
    pub name: String,
    pub created: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Label {
    pub id: i64,
    pub facet: String,
    pub key: String,
    pub canonical: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub active: bool,
    pub created: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Active,
    Superseded,
    Merged,
    Retracted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleTarget {
    Retracted,
}

impl Lifecycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Merged => "merged",
            Self::Retracted => "retracted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "superseded" => Some(Self::Superseded),
            "merged" => Some(Self::Merged),
            "retracted" => Some(Self::Retracted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Revision {
    pub revision: i64,
    pub title: String,
    pub payload: RecordPayload,
    pub reason: String,
    pub provenance: Provenance,
    pub sources: Vec<SourceReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct LifecycleTransition {
    pub id: i64,
    pub from: Option<Lifecycle>,
    pub to: Lifecycle,
    pub reason: String,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    References,
    Mentions,
    DerivedFrom,
    Supports,
    Contradicts,
    Supersedes,
    MergedInto,
    Summarizes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DirectRelationKind {
    References,
    Mentions,
    DerivedFrom,
    Supports,
    Contradicts,
    Summarizes,
}

impl DirectRelationKind {
    pub fn relation_kind(&self) -> RelationKind {
        match self {
            Self::References => RelationKind::References,
            Self::Mentions => RelationKind::Mentions,
            Self::DerivedFrom => RelationKind::DerivedFrom,
            Self::Supports => RelationKind::Supports,
            Self::Contradicts => RelationKind::Contradicts,
            Self::Summarizes => RelationKind::Summarizes,
        }
    }
}

impl RelationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::References => "references",
            Self::Mentions => "mentions",
            Self::DerivedFrom => "derived_from",
            Self::Supports => "supports",
            Self::Contradicts => "contradicts",
            Self::Supersedes => "supersedes",
            Self::MergedInto => "merged_into",
            Self::Summarizes => "summarizes",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "references" => Some(Self::References),
            "mentions" => Some(Self::Mentions),
            "derived_from" => Some(Self::DerivedFrom),
            "supports" => Some(Self::Supports),
            "contradicts" => Some(Self::Contradicts),
            "supersedes" => Some(Self::Supersedes),
            "merged_into" => Some(Self::MergedInto),
            "summarizes" => Some(Self::Summarizes),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Retraction {
    pub reason: String,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Relation {
    pub id: i64,
    pub source_record_id: i64,
    pub target_record_id: i64,
    pub kind: RelationKind,
    pub reason: String,
    pub asserted: Provenance,
    pub retracted: Option<Retraction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ImportMetadata {
    pub legacy: LegacyImportMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LegacyImportMetadata {
    pub created_at: String,
    pub updated_at: String,
    pub author: String,
    pub day: String,
    pub kind: String,
    pub visibility: String,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Record {
    pub id: i64,
    pub scope: Scope,
    pub kind: RecordKind,
    pub title: String,
    pub lifecycle: Lifecycle,
    pub current_revision: i64,
    pub created: Provenance,
    pub updated_at: String,
    pub current: Revision,
    pub history: Vec<Revision>,
    pub labels: Vec<Label>,
    pub relations: Vec<Relation>,
    pub lifecycle_history: Vec<LifecycleTransition>,
    pub import_metadata: Option<ImportMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct SearchHit {
    pub record: Record,
    pub match_explanation: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct SearchPage {
    pub records: Vec<SearchHit>,
    pub next_before_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct SemanticSearchHit {
    pub record: Record,
    pub similarity: f32,
    pub match_explanation: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct SemanticSearchResult {
    pub records: Vec<SemanticSearchHit>,
    pub model: String,
    pub model_revision: String,
    pub dimensions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecallStrategy {
    Bm25,
    Dense,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct RecallScope {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct RecallSource {
    pub id: i64,
    pub identity: String,
    pub locator: Option<String>,
    pub version: Option<String>,
    pub content_hash: Option<String>,
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct RecallScores {
    pub matched_by: Vec<RecallStrategy>,
    pub bm25_rank: Option<usize>,
    pub bm25_score: Option<f64>,
    pub dense_rank: Option<usize>,
    pub dense_similarity: Option<f32>,
    pub rrf_score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct RecallEvidence {
    pub record_id: i64,
    pub kind: RecordKind,
    pub title: String,
    pub excerpt: String,
    pub scope: RecallScope,
    pub lifecycle: Lifecycle,
    pub current_revision: i64,
    pub provenance: Provenance,
    pub sources: Vec<RecallSource>,
    pub source_count: usize,
    pub retrieval: RecallScores,
    pub match_explanation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct RecallContext {
    pub strategy: RecallStrategy,
    pub semantic_available: bool,
    pub model: Option<String>,
    pub model_revision: Option<String>,
    pub budget_bytes: usize,
    pub used_bytes: usize,
    pub records: Vec<RecallEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct EmbeddingStatus {
    pub model: String,
    pub model_revision: String,
    pub dimensions: usize,
    pub eligible_records: usize,
    pub indexed_records: usize,
    pub pending_records: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct EmbeddingSync {
    pub indexed_records: usize,
    pub status: EmbeddingStatus,
}
