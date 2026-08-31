use std::{
    collections::{BTreeSet, HashSet},
    fmt, fs,
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};

use crate::model::{
    DirectRelationKind, EmbeddingStatus, Label, Lifecycle, LifecycleTransition, Provenance, Record,
    RecordInput, RecordKind, RecordPayload, Relation, RelationKind, Retraction, Revision, Scope,
    SearchHit, SearchPage, SemanticSearchHit, SemanticSearchResult, SnippetOrigin, SourceInput,
    SourceReference, WriteContext,
};

const SCHEMA_VERSION: i64 = 9;
const MAX_SEARCH_QUERY_BYTES: usize = 256;
const SEARCH_RESULT_LIMIT: usize = 50;
pub const MAX_BODY_BYTES: usize = 1_000_000;
pub const MAX_TITLE_BYTES: usize = 200;
const MAX_NAME_BYTES: usize = 200;
const MAX_PROVENANCE_BYTES: usize = 500;
const MAX_LABEL_IDS: usize = 100;
const MAX_SOURCES: usize = 100;

#[derive(Debug)]
pub enum Error {
    Invalid(String),
    MissingRecord(i64),
    MissingScope(i64),
    MissingLabel(i64),
    MissingRelation(i64),
    Conflict(String),
    UnsupportedSchema(i64),
    Lock,
    Io(std::io::Error),
    Json(serde_json::Error),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) | Self::Conflict(message) => message.fmt(formatter),
            Self::MissingRecord(id) => write!(formatter, "record {id} does not exist"),
            Self::MissingScope(id) => write!(formatter, "scope {id} does not exist"),
            Self::MissingLabel(id) => write!(formatter, "label {id} does not exist"),
            Self::MissingRelation(id) => write!(formatter, "relation {id} does not exist"),
            Self::UnsupportedSchema(version) => {
                write!(
                    formatter,
                    "database schema version {version} is not supported"
                )
            }
            Self::Lock => write!(formatter, "database lock is unavailable"),
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub struct Database {
    connection: Mutex<Connection>,
}

pub struct EmbeddingRecord {
    pub id: i64,
    pub revision: i64,
    pub title: String,
    pub payload: RecordPayload,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, Error> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, Error> {
        self.connection.lock().map_err(|_| Error::Lock)
    }

    pub fn create_scope(&self, name: &str, context: &WriteContext) -> Result<Scope, Error> {
        validate_context(context)?;
        let name = validate_scope_name(name)?;
        let request = json!({"name": name, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "create_scope", &request)? {
            return scope_from(&transaction, id)?.ok_or(Error::MissingScope(id));
        }
        let timestamp = now();
        transaction.execute(
            "INSERT INTO scopes(name,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(name) DO NOTHING",
            params![
                name,
                timestamp,
                context.actor,
                context.thread,
                context.client,
                context.idempotency_key
            ],
        )?;
        let id: i64 = transaction.query_row(
            "SELECT id FROM scopes WHERE name=?1 COLLATE NOCASE",
            [name],
            |row| row.get(0),
        )?;
        store_write(
            &transaction,
            context,
            "create_scope",
            &request,
            id,
            &timestamp,
        )?;
        let scope = scope_from(&transaction, id)?.ok_or(Error::MissingScope(id))?;
        transaction.commit()?;
        Ok(scope)
    }

    pub fn list_scopes(&self) -> Result<Vec<Scope>, Error> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id,name,created_at,actor,thread,client FROM scopes ORDER BY name COLLATE NOCASE,id",
        )?;
        Ok(statement
            .query_map([], scope_from_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create_label(
        &self,
        facet: &str,
        key: &str,
        display_name: &str,
        aliases: &[String],
        context: &WriteContext,
    ) -> Result<Label, Error> {
        validate_context(context)?;
        let facet = validate_label_part("facet", facet)?;
        let key = validate_label_part("key", key)?;
        let display_name = validate_single_line("display_name", display_name, MAX_NAME_BYTES)?;
        let aliases = validate_aliases(aliases)?;
        let request = json!({"facet": facet, "key": key, "display_name": display_name, "aliases": aliases, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "create_label", &request)? {
            return label_from(&transaction, id)?.ok_or(Error::MissingLabel(id));
        }
        let timestamp = now();
        transaction.execute(
            "INSERT INTO labels(facet,key,display_name,active,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,?2,?3,1,?4,?5,?6,?7,?8)
             ON CONFLICT(facet,key) DO NOTHING",
            params![
                facet,
                key,
                display_name,
                timestamp,
                context.actor,
                context.thread,
                context.client,
                context.idempotency_key
            ],
        )?;
        let id: i64 = transaction.query_row(
            "SELECT id FROM labels WHERE facet=?1 AND key=?2",
            params![facet, key],
            |row| row.get(0),
        )?;
        for alias in aliases {
            transaction.execute(
                "INSERT INTO label_aliases(label_id,alias) VALUES(?1,?2)
                 ON CONFLICT(alias) DO NOTHING",
                params![id, alias],
            )?;
            let owner: i64 = transaction.query_row(
                "SELECT label_id FROM label_aliases WHERE alias=?1 COLLATE NOCASE",
                [&alias],
                |row| row.get(0),
            )?;
            if owner != id {
                return Err(Error::Conflict(format!(
                    "label alias {alias} belongs to another label"
                )));
            }
        }
        store_write(
            &transaction,
            context,
            "create_label",
            &request,
            id,
            &timestamp,
        )?;
        let label = label_from(&transaction, id)?.ok_or(Error::MissingLabel(id))?;
        transaction.commit()?;
        Ok(label)
    }

    pub fn search_labels(
        &self,
        query: &str,
        facet: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Label>, Error> {
        validate_query(query)?;
        validate_limit(limit)?;
        let facet = facet
            .map(|value| validate_label_part("facet", value))
            .transpose()?;
        let pattern = format!("%{}%", query.trim().to_lowercase());
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT l.id FROM labels l
             LEFT JOIN label_aliases a ON a.label_id=l.id
             WHERE l.active=1 AND (?1 IS NULL OR l.facet=?1)
             AND (?2='%%' OR lower(l.facet||':'||l.key) LIKE ?2 OR lower(l.display_name) LIKE ?2 OR lower(a.alias) LIKE ?2)
             ORDER BY l.facet,l.key,l.id LIMIT ?3",
        )?;
        let ids = statement
            .query_map(params![facet, pattern, limit], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| label_from(&connection, id)?.ok_or(Error::MissingLabel(id)))
            .collect()
    }

    pub fn create_record(
        &self,
        input: &RecordInput,
        context: &WriteContext,
    ) -> Result<Record, Error> {
        validate_context(context)?;
        validate_record_input(input)?;
        let request = json!({"record": input, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "create_record", &request)? {
            return record_from(&transaction, id, false)?.ok_or(Error::MissingRecord(id));
        }
        let timestamp = now();
        let id = insert_record(&transaction, input, context, "record created", &timestamp)?;
        store_write(
            &transaction,
            context,
            "create_record",
            &request,
            id,
            &timestamp,
        )?;
        let record = record_from(&transaction, id, false)?.ok_or(Error::MissingRecord(id))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn read_record(&self, id: i64, include_history: bool) -> Result<Record, Error> {
        validate_id("record_id", id)?;
        let connection = self.connection()?;
        record_from(&connection, id, include_history)?.ok_or(Error::MissingRecord(id))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_records(
        &self,
        query: Option<&str>,
        scope_id: i64,
        include_global: bool,
        kinds: &[RecordKind],
        lifecycles: &[Lifecycle],
        label_ids: &[i64],
        include_history: bool,
        before_id: Option<i64>,
        limit: usize,
    ) -> Result<SearchPage, Error> {
        validate_id("scope_id", scope_id)?;
        validate_limit(limit)?;
        if let Some(query) = query {
            validate_query(query)?;
        }
        let label_ids = distinct_ids("label_ids", label_ids, MAX_LABEL_IDS)?;
        if let Some(before_id) = before_id {
            validate_id("before_id", before_id)?;
        }
        let connection = self.connection()?;
        if scope_from(&connection, scope_id)?.is_none() {
            return Err(Error::MissingScope(scope_id));
        }
        for id in &label_ids {
            if label_from(&connection, *id)?.is_none() {
                return Err(Error::MissingLabel(*id));
            }
        }
        let fts = query.and_then(fts_query);
        let lifecycle_values = if lifecycles.is_empty() {
            vec![Lifecycle::Active]
        } else {
            lifecycles.to_vec()
        };
        let mut sql = String::from("SELECT r.id FROM records r JOIN scopes s ON s.id=r.scope_id ");
        if fts.is_some() {
            sql.push_str("JOIN record_fts ON record_fts.record_id=r.id ");
        }
        sql.push_str("WHERE r.readable=1 AND (r.scope_id=?1 OR (?2=1 AND s.name='global')) ");
        if fts.is_some() {
            sql.push_str("AND record_fts MATCH ?3 ");
        }
        let first_dynamic = if fts.is_some() { 4 } else { 3 };
        let mut values: Vec<rusqlite::types::Value> = vec![
            scope_id.into(),
            if include_global { 1_i64 } else { 0_i64 }.into(),
        ];
        if let Some(fts) = &fts {
            values.push(fts.clone().into());
        }
        let mut next = first_dynamic;
        sql.push_str("AND r.lifecycle IN (");
        push_placeholders(&mut sql, lifecycle_values.len(), &mut next);
        sql.push_str(") ");
        values.extend(
            lifecycle_values
                .iter()
                .map(|value| value.as_str().to_owned().into()),
        );
        if !kinds.is_empty() {
            sql.push_str("AND r.kind IN (");
            push_placeholders(&mut sql, kinds.len(), &mut next);
            sql.push_str(") ");
            values.extend(kinds.iter().map(|value| value.as_str().to_owned().into()));
        }
        for label_id in &label_ids {
            sql.push_str(&format!(
                "AND EXISTS(SELECT 1 FROM label_assertions la WHERE la.record_id=r.id AND la.label_id=?{next} AND NOT EXISTS(SELECT 1 FROM label_retractions lr WHERE lr.assertion_id=la.id)) "
            ));
            values.push((*label_id).into());
            next += 1;
        }
        if let Some(before_id) = before_id {
            sql.push_str(&format!("AND r.id<?{next} "));
            values.push(before_id.into());
            next += 1;
        }
        sql.push_str(&format!("ORDER BY r.id DESC LIMIT ?{next}"));
        values.push(((limit + 1) as i64).into());
        let mut statement = connection.prepare(&sql)?;
        let mut ids = statement
            .query_map(rusqlite::params_from_iter(values), |row| {
                row.get::<_, i64>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let next_before_id = if ids.len() > limit {
            ids.pop();
            ids.last().copied()
        } else {
            None
        };
        let mut explanation = Vec::new();
        if fts.is_some() {
            explanation.push("fts:title_or_payload".to_owned());
        }
        explanation.push(if include_global {
            "scope:exact_or_global".to_owned()
        } else {
            "scope:exact".to_owned()
        });
        if !kinds.is_empty() {
            explanation.push("kind".to_owned());
        }
        explanation.push("lifecycle".to_owned());
        if !label_ids.is_empty() {
            explanation.push("labels:all".to_owned());
        }
        let records = ids
            .into_iter()
            .map(|id| {
                Ok(SearchHit {
                    record: record_from(&connection, id, include_history)?
                        .ok_or(Error::MissingRecord(id))?,
                    match_explanation: explanation.clone(),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(SearchPage {
            records,
            next_before_id,
        })
    }

    pub fn embedding_status(
        &self,
        model: &str,
        model_revision: &str,
        dimensions: usize,
    ) -> Result<EmbeddingStatus, Error> {
        validate_embedding_model(model, model_revision, dimensions)?;
        let connection = self.connection()?;
        embedding_status_from(&connection, model, model_revision, dimensions)
    }

    pub fn pending_embedding_records(
        &self,
        model: &str,
        model_revision: &str,
        dimensions: usize,
    ) -> Result<Vec<EmbeddingRecord>, Error> {
        validate_embedding_model(model, model_revision, dimensions)?;
        let connection = self.connection()?;
        let rows = {
            let mut statement = connection.prepare(
                "SELECT r.id,r.current_revision,r.title,rr.payload_json,r.import_metadata IS NOT NULL
                 FROM records r
                 JOIN record_revisions rr
                   ON rr.record_id=r.id AND rr.revision=r.current_revision
                 LEFT JOIN record_embeddings e
                   ON e.record_id=r.id AND e.model=?1 AND e.model_revision=?2
                 WHERE r.readable=1
                   AND (e.record_id IS NULL OR e.revision<>r.current_revision OR e.dimensions<>?3)
                 ORDER BY r.id",
            )?;
            statement
                .query_map(params![model, model_revision, dimensions as i64], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, bool>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        rows.into_iter()
            .map(|(id, revision, title, payload_json, imported)| {
                let mut payload: RecordPayload = serde_json::from_str(&payload_json)?;
                if imported {
                    sanitize_imported_payload(&connection, &mut payload)?;
                }
                Ok(EmbeddingRecord {
                    id,
                    revision,
                    title,
                    payload,
                })
            })
            .collect()
    }

    pub fn store_embedding(
        &self,
        record_id: i64,
        revision: i64,
        model: &str,
        model_revision: &str,
        dimensions: usize,
        embedding: &[f32],
    ) -> Result<bool, Error> {
        validate_id("record_id", record_id)?;
        validate_id("revision", revision)?;
        validate_embedding_model(model, model_revision, dimensions)?;
        validate_embedding(embedding, dimensions)?;
        let vector = embedding
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        let current_revision: Option<i64> = transaction
            .query_row(
                "SELECT current_revision FROM records WHERE id=?1 AND readable=1",
                [record_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(current_revision) = current_revision else {
            return Err(Error::MissingRecord(record_id));
        };
        if current_revision != revision {
            return Ok(false);
        }
        transaction.execute(
            "INSERT INTO record_embeddings(record_id,revision,model,model_revision,dimensions,vector,embedded_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(record_id,model,model_revision) DO UPDATE SET
               revision=excluded.revision,
               dimensions=excluded.dimensions,
               vector=excluded.vector,
               embedded_at=excluded.embedded_at",
            params![
                record_id,
                revision,
                model,
                model_revision,
                dimensions as i64,
                vector,
                now()
            ],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn semantic_search_records(
        &self,
        query_embedding: &[f32],
        model: &str,
        model_revision: &str,
        dimensions: usize,
        scope_id: i64,
        include_global: bool,
        kinds: &[RecordKind],
        lifecycles: &[Lifecycle],
        label_ids: &[i64],
        include_history: bool,
        limit: usize,
    ) -> Result<SemanticSearchResult, Error> {
        validate_embedding_model(model, model_revision, dimensions)?;
        validate_embedding(query_embedding, dimensions)?;
        validate_id("scope_id", scope_id)?;
        validate_limit(limit)?;
        let label_ids = distinct_ids("label_ids", label_ids, MAX_LABEL_IDS)?;
        let connection = self.connection()?;
        if scope_from(&connection, scope_id)?.is_none() {
            return Err(Error::MissingScope(scope_id));
        }
        for id in &label_ids {
            if label_from(&connection, *id)?.is_none() {
                return Err(Error::MissingLabel(*id));
            }
        }
        let status = embedding_status_from(&connection, model, model_revision, dimensions)?;
        if status.pending_records != 0 {
            return Err(Error::Conflict(format!(
                "embedding index has {} pending records; run sync_embeddings",
                status.pending_records
            )));
        }
        let lifecycle_values = if lifecycles.is_empty() {
            vec![Lifecycle::Active]
        } else {
            lifecycles.to_vec()
        };
        let mut sql = String::from(
            "SELECT r.id,e.vector FROM record_embeddings e
             JOIN records r ON r.id=e.record_id AND r.current_revision=e.revision
             JOIN scopes s ON s.id=r.scope_id
             WHERE e.model=?1 AND e.model_revision=?2 AND e.dimensions=?3
               AND r.readable=1 AND (r.scope_id=?4 OR (?5=1 AND s.name='global')) ",
        );
        let mut values: Vec<rusqlite::types::Value> = vec![
            model.to_owned().into(),
            model_revision.to_owned().into(),
            (dimensions as i64).into(),
            scope_id.into(),
            if include_global { 1_i64 } else { 0_i64 }.into(),
        ];
        let mut next = 6;
        sql.push_str("AND r.lifecycle IN (");
        push_placeholders(&mut sql, lifecycle_values.len(), &mut next);
        sql.push_str(") ");
        values.extend(
            lifecycle_values
                .iter()
                .map(|value| value.as_str().to_owned().into()),
        );
        if !kinds.is_empty() {
            sql.push_str("AND r.kind IN (");
            push_placeholders(&mut sql, kinds.len(), &mut next);
            sql.push_str(") ");
            values.extend(kinds.iter().map(|value| value.as_str().to_owned().into()));
        }
        for label_id in &label_ids {
            sql.push_str(&format!(
                "AND EXISTS(SELECT 1 FROM label_assertions la WHERE la.record_id=r.id AND la.label_id=?{next} AND NOT EXISTS(SELECT 1 FROM label_retractions lr WHERE lr.assertion_id=la.id)) "
            ));
            values.push((*label_id).into());
            next += 1;
        }
        sql.push_str("ORDER BY r.id");
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(values), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut scored = rows
            .into_iter()
            .map(|(id, bytes)| {
                let vector = embedding_from_bytes(&bytes, dimensions)?;
                let similarity = query_embedding
                    .iter()
                    .zip(vector)
                    .map(|(left, right)| left * right)
                    .sum::<f32>();
                Ok((id, similarity))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        scored.sort_by(|(left_id, left_score), (right_id, right_score)| {
            right_score
                .total_cmp(left_score)
                .then_with(|| right_id.cmp(left_id))
        });
        scored.truncate(limit);
        let mut explanation = vec![
            "semantic:cosine".to_owned(),
            format!("model:{model}@{model_revision}"),
            if include_global {
                "scope:exact_or_global".to_owned()
            } else {
                "scope:exact".to_owned()
            },
        ];
        if !kinds.is_empty() {
            explanation.push("kind".to_owned());
        }
        explanation.push("lifecycle".to_owned());
        if !label_ids.is_empty() {
            explanation.push("labels:all".to_owned());
        }
        let records = scored
            .into_iter()
            .map(|(id, similarity)| {
                Ok(SemanticSearchHit {
                    record: record_from(&connection, id, include_history)?
                        .ok_or(Error::MissingRecord(id))?,
                    similarity,
                    match_explanation: explanation.clone(),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(SemanticSearchResult {
            records,
            model: model.to_owned(),
            model_revision: model_revision.to_owned(),
            dimensions,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn revise_record(
        &self,
        record_id: i64,
        expected_revision: i64,
        title: &str,
        payload: &RecordPayload,
        sources: &[SourceInput],
        reason: &str,
        context: &WriteContext,
    ) -> Result<Record, Error> {
        validate_context(context)?;
        validate_id("record_id", record_id)?;
        validate_id("expected_revision", expected_revision)?;
        validate_title(title)?;
        validate_reason(reason)?;
        validate_payload(payload, sources)?;
        let request = json!({"record_id": record_id, "expected_revision": expected_revision, "title": title, "payload": payload, "sources": sources, "reason": reason, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "revise_record", &request)? {
            return record_from(&transaction, id, false)?.ok_or(Error::MissingRecord(id));
        }
        let (kind, current_revision) = readable_record_state(&transaction, record_id)?;
        if current_revision != expected_revision {
            return Err(Error::Conflict(format!(
                "revision conflict: expected {expected_revision}, current {current_revision}"
            )));
        }
        if kind != payload.kind() {
            return Err(Error::Invalid(
                "record kind cannot change during revision".to_owned(),
            ));
        }
        let timestamp = now();
        let revision = current_revision + 1;
        let payload_json = serde_json::to_string(payload)?;
        transaction.execute(
            "INSERT INTO record_revisions(record_id,revision,title,payload_json,reason,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![record_id,revision,title.trim(),payload_json,reason.trim(),timestamp,context.actor,context.thread,context.client,context.idempotency_key],
        )?;
        insert_sources(&transaction, record_id, revision, sources)?;
        transaction.execute(
            "UPDATE records SET title=?1,current_revision=?2,updated_at=?3 WHERE id=?4",
            params![title.trim(), revision, timestamp, record_id],
        )?;
        update_fts(&transaction, record_id, title.trim(), &payload_json)?;
        store_write(
            &transaction,
            context,
            "revise_record",
            &request,
            record_id,
            &timestamp,
        )?;
        let record =
            record_from(&transaction, record_id, false)?.ok_or(Error::MissingRecord(record_id))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn add_label(
        &self,
        record_id: i64,
        label_id: i64,
        reason: &str,
        context: &WriteContext,
    ) -> Result<Record, Error> {
        validate_context(context)?;
        validate_id("record_id", record_id)?;
        validate_id("label_id", label_id)?;
        validate_reason(reason)?;
        let request = json!({"record_id": record_id, "label_id": label_id, "reason": reason, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "add_label", &request)? {
            return record_from(&transaction, id, false)?.ok_or(Error::MissingRecord(id));
        }
        readable_record_state(&transaction, record_id)?;
        ensure_active_label(&transaction, label_id)?;
        let active: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM label_assertions la WHERE la.record_id=?1 AND la.label_id=?2
             AND NOT EXISTS(SELECT 1 FROM label_retractions lr WHERE lr.assertion_id=la.id))",
            params![record_id, label_id],
            |row| row.get(0),
        )?;
        if active {
            return Err(Error::Conflict(
                "label is already active on record".to_owned(),
            ));
        }
        let timestamp = now();
        transaction.execute(
            "INSERT INTO label_assertions(record_id,label_id,reason,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![record_id,label_id,reason.trim(),timestamp,context.actor,context.thread,context.client,context.idempotency_key],
        )?;
        store_write(
            &transaction,
            context,
            "add_label",
            &request,
            record_id,
            &timestamp,
        )?;
        let record =
            record_from(&transaction, record_id, false)?.ok_or(Error::MissingRecord(record_id))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn retract_label(
        &self,
        record_id: i64,
        label_id: i64,
        reason: &str,
        context: &WriteContext,
    ) -> Result<Record, Error> {
        validate_context(context)?;
        validate_id("record_id", record_id)?;
        validate_id("label_id", label_id)?;
        validate_reason(reason)?;
        let request = json!({"record_id": record_id, "label_id": label_id, "reason": reason, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "retract_label", &request)? {
            return record_from(&transaction, id, false)?.ok_or(Error::MissingRecord(id));
        }
        readable_record_state(&transaction, record_id)?;
        let assertion_id: i64 = transaction
            .query_row(
                "SELECT la.id FROM label_assertions la WHERE la.record_id=?1 AND la.label_id=?2
                 AND NOT EXISTS(SELECT 1 FROM label_retractions lr WHERE lr.assertion_id=la.id)
                 ORDER BY la.id DESC LIMIT 1",
                params![record_id, label_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::Conflict("label is not active on record".to_owned()))?;
        let count: i64 = transaction.query_row(
            "SELECT count(*) FROM label_assertions la WHERE la.record_id=?1
             AND NOT EXISTS(SELECT 1 FROM label_retractions lr WHERE lr.assertion_id=la.id)",
            [record_id],
            |row| row.get(0),
        )?;
        if count <= 1 {
            return Err(Error::Conflict(
                "a record must retain at least one active label".to_owned(),
            ));
        }
        let timestamp = now();
        transaction.execute(
            "INSERT INTO label_retractions(assertion_id,reason,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![assertion_id,reason.trim(),timestamp,context.actor,context.thread,context.client,context.idempotency_key],
        )?;
        store_write(
            &transaction,
            context,
            "retract_label",
            &request,
            record_id,
            &timestamp,
        )?;
        let record =
            record_from(&transaction, record_id, false)?.ok_or(Error::MissingRecord(record_id))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn add_relation(
        &self,
        source_record_id: i64,
        target_record_id: i64,
        kind: &DirectRelationKind,
        reason: &str,
        context: &WriteContext,
    ) -> Result<Relation, Error> {
        validate_context(context)?;
        validate_id("source_record_id", source_record_id)?;
        validate_id("target_record_id", target_record_id)?;
        if source_record_id == target_record_id {
            return Err(Error::Invalid(
                "a relation cannot target its source".to_owned(),
            ));
        }
        validate_reason(reason)?;
        let request = json!({"source_record_id": source_record_id, "target_record_id": target_record_id, "kind": kind, "reason": reason, "context": context});
        let kind = kind.relation_kind();
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "add_relation", &request)? {
            return relation_from(&transaction, id, true)?.ok_or(Error::MissingRelation(id));
        }
        readable_record_state(&transaction, source_record_id)?;
        readable_record_state(&transaction, target_record_id)?;
        ensure_relation_absent(&transaction, source_record_id, target_record_id, &kind)?;
        let timestamp = now();
        let id = insert_relation(
            &transaction,
            source_record_id,
            target_record_id,
            &kind,
            reason,
            context,
            &timestamp,
        )?;
        store_write(
            &transaction,
            context,
            "add_relation",
            &request,
            id,
            &timestamp,
        )?;
        let relation = relation_from(&transaction, id, true)?.ok_or(Error::MissingRelation(id))?;
        transaction.commit()?;
        Ok(relation)
    }

    pub fn retract_relation(
        &self,
        relation_id: i64,
        reason: &str,
        context: &WriteContext,
    ) -> Result<Relation, Error> {
        validate_context(context)?;
        validate_id("relation_id", relation_id)?;
        validate_reason(reason)?;
        let request = json!({"relation_id": relation_id, "reason": reason, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "retract_relation", &request)? {
            return relation_from(&transaction, id, true)?.ok_or(Error::MissingRelation(id));
        }
        let relation = relation_from(&transaction, relation_id, true)?
            .ok_or(Error::MissingRelation(relation_id))?;
        readable_record_state(&transaction, relation.source_record_id)?;
        readable_record_state(&transaction, relation.target_record_id)?;
        if relation.retracted.is_some() {
            return Err(Error::Conflict("relation is already retracted".to_owned()));
        }
        let timestamp = now();
        transaction.execute(
            "INSERT INTO relation_retractions(relation_id,reason,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![relation_id,reason.trim(),timestamp,context.actor,context.thread,context.client,context.idempotency_key],
        )?;
        store_write(
            &transaction,
            context,
            "retract_relation",
            &request,
            relation_id,
            &timestamp,
        )?;
        let relation = relation_from(&transaction, relation_id, true)?
            .ok_or(Error::MissingRelation(relation_id))?;
        transaction.commit()?;
        Ok(relation)
    }

    pub fn list_relations(
        &self,
        record_id: i64,
        include_retracted: bool,
    ) -> Result<Vec<Relation>, Error> {
        validate_id("record_id", record_id)?;
        let connection = self.connection()?;
        readable_record_state(&connection, record_id)?;
        relations_for(&connection, record_id, include_retracted)
    }

    pub fn transition_record(
        &self,
        record_id: i64,
        to: &Lifecycle,
        reason: &str,
        context: &WriteContext,
    ) -> Result<Record, Error> {
        validate_context(context)?;
        validate_id("record_id", record_id)?;
        validate_reason(reason)?;
        if *to != Lifecycle::Retracted {
            return Err(Error::Invalid(
                "use supersede_record or merge_records for that lifecycle".to_owned(),
            ));
        }
        let request =
            json!({"record_id": record_id, "to": to, "reason": reason, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "transition_record", &request)? {
            return record_from(&transaction, id, false)?.ok_or(Error::MissingRecord(id));
        }
        let timestamp = now();
        transition(&transaction, record_id, to, reason, context, &timestamp)?;
        store_write(
            &transaction,
            context,
            "transition_record",
            &request,
            record_id,
            &timestamp,
        )?;
        let record =
            record_from(&transaction, record_id, false)?.ok_or(Error::MissingRecord(record_id))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn supersede_record(
        &self,
        record_id: i64,
        replacement: &RecordInput,
        reason: &str,
        context: &WriteContext,
    ) -> Result<Record, Error> {
        validate_context(context)?;
        validate_id("record_id", record_id)?;
        validate_record_input(replacement)?;
        validate_reason(reason)?;
        let request = json!({"record_id": record_id, "replacement": replacement, "reason": reason, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "supersede_record", &request)? {
            return record_from(&transaction, id, false)?.ok_or(Error::MissingRecord(id));
        }
        let (lifecycle, _) = readable_record_lifecycle(&transaction, record_id)?;
        if lifecycle != Lifecycle::Active {
            return Err(Error::Conflict(
                "only an active record can be superseded".to_owned(),
            ));
        }
        let timestamp = now();
        let replacement_id = insert_record(&transaction, replacement, context, reason, &timestamp)?;
        insert_relation(
            &transaction,
            replacement_id,
            record_id,
            &RelationKind::Supersedes,
            reason,
            context,
            &timestamp,
        )?;
        transition(
            &transaction,
            record_id,
            &Lifecycle::Superseded,
            reason,
            context,
            &timestamp,
        )?;
        store_write(
            &transaction,
            context,
            "supersede_record",
            &request,
            replacement_id,
            &timestamp,
        )?;
        let record = record_from(&transaction, replacement_id, false)?
            .ok_or(Error::MissingRecord(replacement_id))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn merge_records(
        &self,
        record_ids: &[i64],
        aggregate: &RecordInput,
        reason: &str,
        context: &WriteContext,
    ) -> Result<Record, Error> {
        validate_context(context)?;
        let record_ids = distinct_ids("record_ids", record_ids, MAX_LABEL_IDS)?;
        if record_ids.len() < 2 {
            return Err(Error::Invalid(
                "merge requires at least two records".to_owned(),
            ));
        }
        validate_record_input(aggregate)?;
        validate_reason(reason)?;
        let request = json!({"record_ids": record_ids, "aggregate": aggregate, "reason": reason, "context": context});
        let mut connection = self.connection()?;
        let transaction = immediate(&mut connection)?;
        if let Some(id) = previous_result(&transaction, context, "merge_records", &request)? {
            return record_from(&transaction, id, false)?.ok_or(Error::MissingRecord(id));
        }
        for id in &record_ids {
            let (lifecycle, _) = readable_record_lifecycle(&transaction, *id)?;
            if lifecycle != Lifecycle::Active {
                return Err(Error::Conflict(
                    "only active records can be merged".to_owned(),
                ));
            }
        }
        let timestamp = now();
        let aggregate_id = insert_record(&transaction, aggregate, context, reason, &timestamp)?;
        if record_ids.contains(&aggregate_id) {
            return Err(Error::Invalid("aggregate must be a new record".to_owned()));
        }
        for id in record_ids {
            insert_relation(
                &transaction,
                id,
                aggregate_id,
                &RelationKind::MergedInto,
                reason,
                context,
                &timestamp,
            )?;
            transition(
                &transaction,
                id,
                &Lifecycle::Merged,
                reason,
                context,
                &timestamp,
            )?;
        }
        store_write(
            &transaction,
            context,
            "merge_records",
            &request,
            aggregate_id,
            &timestamp,
        )?;
        let record = record_from(&transaction, aggregate_id, false)?
            .ok_or(Error::MissingRecord(aggregate_id))?;
        transaction.commit()?;
        Ok(record)
    }
}

fn immediate(connection: &mut Connection) -> Result<Transaction<'_>, Error> {
    Ok(connection.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

fn insert_record(
    transaction: &Transaction<'_>,
    input: &RecordInput,
    context: &WriteContext,
    reason: &str,
    timestamp: &str,
) -> Result<i64, Error> {
    if scope_from(transaction, input.scope_id)?.is_none() {
        return Err(Error::MissingScope(input.scope_id));
    }
    let label_ids = distinct_ids("label_ids", &input.label_ids, MAX_LABEL_IDS)?;
    if label_ids.is_empty() {
        return Err(Error::Invalid(
            "record requires at least one active label".to_owned(),
        ));
    }
    for id in &label_ids {
        ensure_active_label(transaction, *id)?;
    }
    let kind = input.payload.kind();
    let payload_json = serde_json::to_string(&input.payload)?;
    transaction.execute(
        "INSERT INTO records(scope_id,kind,title,lifecycle,current_revision,readable,created_at,updated_at,actor,thread,client,idempotency_key,import_metadata)
         VALUES(?1,?2,?3,'active',1,1,?4,?4,?5,?6,?7,?8,NULL)",
        params![input.scope_id,kind.as_str(),input.title.trim(),timestamp,context.actor,context.thread,context.client,context.idempotency_key],
    )?;
    let id = transaction.last_insert_rowid();
    transaction.execute(
        "INSERT INTO record_revisions(record_id,revision,title,payload_json,reason,created_at,actor,thread,client,idempotency_key)
         VALUES(?1,1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![id,input.title.trim(),payload_json,reason.trim(),timestamp,context.actor,context.thread,context.client,context.idempotency_key],
    )?;
    insert_sources(transaction, id, 1, &input.sources)?;
    for label_id in label_ids {
        transaction.execute(
            "INSERT INTO label_assertions(record_id,label_id,reason,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,?2,'record created',?3,?4,?5,?6,?7)",
            params![id,label_id,timestamp,context.actor,context.thread,context.client,format!("{}:label:{label_id}",context.idempotency_key)],
        )?;
    }
    transaction.execute(
        "INSERT INTO lifecycle_transitions(record_id,from_state,to_state,reason,created_at,actor,thread,client,idempotency_key)
         VALUES(?1,NULL,'active','record created',?2,?3,?4,?5,?6)",
        params![id,timestamp,context.actor,context.thread,context.client,format!("{}:lifecycle",context.idempotency_key)],
    )?;
    update_fts(transaction, id, input.title.trim(), &payload_json)?;
    Ok(id)
}

fn insert_sources(
    transaction: &Transaction<'_>,
    record_id: i64,
    revision: i64,
    sources: &[SourceInput],
) -> Result<(), Error> {
    for (position, source) in sources.iter().enumerate() {
        transaction.execute(
            "INSERT INTO source_references(record_id,revision,position,identity,locator,version,content_hash,anchor,quote)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![record_id,revision,position as i64,source.identity.trim(),trim_option(&source.locator),trim_option(&source.version),trim_option(&source.content_hash),trim_option(&source.anchor),source.quote],
        )?;
    }
    Ok(())
}

fn transition(
    transaction: &Transaction<'_>,
    record_id: i64,
    to: &Lifecycle,
    reason: &str,
    context: &WriteContext,
    timestamp: &str,
) -> Result<(), Error> {
    let (from, _) = readable_record_lifecycle(transaction, record_id)?;
    if from != Lifecycle::Active {
        return Err(Error::Conflict(
            "only an active record can transition".to_owned(),
        ));
    }
    transaction.execute(
        "UPDATE records SET lifecycle=?1,updated_at=?2 WHERE id=?3",
        params![to.as_str(), timestamp, record_id],
    )?;
    transaction.execute(
        "INSERT INTO lifecycle_transitions(record_id,from_state,to_state,reason,created_at,actor,thread,client,idempotency_key)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![record_id,from.as_str(),to.as_str(),reason.trim(),timestamp,context.actor,context.thread,context.client,format!("{}:lifecycle:{record_id}",context.idempotency_key)],
    )?;
    Ok(())
}

fn insert_relation(
    transaction: &Transaction<'_>,
    source_record_id: i64,
    target_record_id: i64,
    kind: &RelationKind,
    reason: &str,
    context: &WriteContext,
    timestamp: &str,
) -> Result<i64, Error> {
    ensure_relation_absent(transaction, source_record_id, target_record_id, kind)?;
    transaction.execute(
        "INSERT INTO record_relations(source_record_id,target_record_id,kind,reason,created_at,actor,thread,client,idempotency_key)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![source_record_id,target_record_id,kind.as_str(),reason.trim(),timestamp,context.actor,context.thread,context.client,format!("{}:relation:{source_record_id}:{target_record_id}:{}",context.idempotency_key,kind.as_str())],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn ensure_relation_absent(
    connection: &Connection,
    source_record_id: i64,
    target_record_id: i64,
    kind: &RelationKind,
) -> Result<(), Error> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM record_relations rr
         WHERE rr.source_record_id=?1 AND rr.target_record_id=?2 AND rr.kind=?3
         AND NOT EXISTS(SELECT 1 FROM relation_retractions rt WHERE rt.relation_id=rr.id))",
        params![source_record_id, target_record_id, kind.as_str()],
        |row| row.get(0),
    )?;
    if exists {
        Err(Error::Conflict("relation is already active".to_owned()))
    } else {
        Ok(())
    }
}

fn previous_result(
    connection: &Connection,
    context: &WriteContext,
    operation: &str,
    request: &Value,
) -> Result<Option<i64>, Error> {
    let stored: Option<(String, String, i64)> = connection
        .query_row(
            "SELECT operation,request_json,result_id FROM write_keys WHERE key=?1",
            [&context.idempotency_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((stored_operation, stored_request, result_id)) = stored else {
        return Ok(None);
    };
    let request_json = serde_json::to_string(request)?;
    if stored_operation == operation && stored_request == request_json {
        Ok(Some(result_id))
    } else {
        Err(Error::Conflict(
            "idempotency key was already used for a different write".to_owned(),
        ))
    }
}

fn store_write(
    connection: &Connection,
    context: &WriteContext,
    operation: &str,
    request: &Value,
    result_id: i64,
    timestamp: &str,
) -> Result<(), Error> {
    connection.execute(
        "INSERT INTO write_keys(key,operation,request_json,result_id,created_at,actor,thread,client)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![context.idempotency_key,operation,serde_json::to_string(request)?,result_id,timestamp,context.actor,context.thread,context.client],
    )?;
    Ok(())
}

fn update_fts(
    connection: &Connection,
    record_id: i64,
    title: &str,
    payload_json: &str,
) -> Result<(), Error> {
    let imported: bool = connection.query_row(
        "SELECT import_metadata IS NOT NULL FROM records WHERE id=?1",
        [record_id],
        |row| row.get(0),
    )?;
    let payload_json = if imported {
        let mut payload: RecordPayload = serde_json::from_str(payload_json)?;
        sanitize_imported_payload(connection, &mut payload)?;
        serde_json::to_string(&payload)?
    } else {
        payload_json.to_owned()
    };
    connection.execute("DELETE FROM record_fts WHERE record_id=?1", [record_id])?;
    connection.execute(
        "INSERT INTO record_fts(record_id,title,payload) VALUES(?1,?2,?3)",
        params![record_id, title, payload_json],
    )?;
    Ok(())
}

fn rebuild_fts(connection: &Connection) -> Result<(), Error> {
    let rows = {
        let mut statement = connection.prepare(
            "SELECT r.id,r.title,rr.payload_json,r.import_metadata IS NOT NULL
             FROM records r JOIN record_revisions rr
             ON rr.record_id=r.id AND rr.revision=r.current_revision
             WHERE r.readable=1 ORDER BY r.id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    connection.execute("DELETE FROM record_fts", [])?;
    for (id, title, payload_json, imported) in rows {
        let payload_json = if imported {
            let mut payload: RecordPayload = serde_json::from_str(&payload_json)?;
            sanitize_imported_payload(connection, &mut payload)?;
            serde_json::to_string(&payload)?
        } else {
            payload_json
        };
        connection.execute(
            "INSERT INTO record_fts(record_id,title,payload) VALUES(?1,?2,?3)",
            params![id, title, payload_json],
        )?;
    }
    Ok(())
}

fn scope_from(connection: &Connection, id: i64) -> Result<Option<Scope>, Error> {
    Ok(connection
        .query_row(
            "SELECT id,name,created_at,actor,thread,client FROM scopes WHERE id=?1",
            [id],
            scope_from_row,
        )
        .optional()?)
}

fn scope_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Scope> {
    Ok(Scope {
        id: row.get(0)?,
        name: row.get(1)?,
        created: Provenance {
            server_time: row.get(2)?,
            actor: row.get(3)?,
            thread: row.get(4)?,
            client: row.get(5)?,
        },
    })
}

fn label_from(connection: &Connection, id: i64) -> Result<Option<Label>, Error> {
    let base: Option<(i64, String, String, String, bool, Provenance)> = connection
        .query_row(
            "SELECT id,facet,key,display_name,active,created_at,actor,thread,client FROM labels WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    Provenance {
                        server_time: row.get(5)?,
                        actor: row.get(6)?,
                        thread: row.get(7)?,
                        client: row.get(8)?,
                    },
                ))
            },
        )
        .optional()?;
    let Some((id, facet, key, display_name, active, created)) = base else {
        return Ok(None);
    };
    let mut statement =
        connection.prepare("SELECT alias FROM label_aliases WHERE label_id=?1 ORDER BY alias")?;
    let aliases = statement
        .query_map([id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Label {
        id,
        canonical: format!("{facet}:{key}"),
        facet,
        key,
        display_name,
        aliases,
        active,
        created,
    }))
}

#[allow(clippy::type_complexity)]
fn record_from(
    connection: &Connection,
    id: i64,
    include_history: bool,
) -> Result<Option<Record>, Error> {
    let base: Option<(
        i64,
        i64,
        String,
        String,
        String,
        i64,
        Provenance,
        String,
        Option<String>,
    )> = connection
        .query_row(
            "SELECT id,scope_id,kind,title,lifecycle,current_revision,created_at,actor,thread,client,updated_at,import_metadata
             FROM records WHERE id=?1 AND readable=1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    Provenance {
                        server_time: row.get(6)?,
                        actor: row.get(7)?,
                        thread: row.get(8)?,
                        client: row.get(9)?,
                    },
                    row.get(10)?,
                    row.get(11)?,
                ))
            },
        )
        .optional()?;
    let Some((
        id,
        scope_id,
        kind,
        title,
        lifecycle,
        current_revision,
        created,
        updated_at,
        import_json,
    )) = base
    else {
        return Ok(None);
    };
    let imported = import_json.is_some();
    let current = revision_from(connection, id, current_revision, imported)?
        .ok_or(Error::MissingRecord(id))?;
    let history = if include_history {
        let mut statement = connection.prepare(
            "SELECT revision FROM record_revisions WHERE record_id=?1 ORDER BY revision",
        )?;
        let revisions = statement
            .query_map([id], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        revisions
            .into_iter()
            .map(|revision| {
                revision_from(connection, id, revision, imported)?.ok_or(Error::MissingRecord(id))
            })
            .collect::<Result<Vec<_>, Error>>()?
    } else {
        Vec::new()
    };
    let scope = scope_from(connection, scope_id)?.ok_or(Error::MissingScope(scope_id))?;
    let kind = RecordKind::parse(&kind)
        .ok_or_else(|| Error::Invalid("stored record kind is invalid".to_owned()))?;
    let lifecycle = Lifecycle::parse(&lifecycle)
        .ok_or_else(|| Error::Invalid("stored lifecycle is invalid".to_owned()))?;
    Ok(Some(Record {
        id,
        scope,
        kind,
        title,
        lifecycle,
        current_revision,
        created,
        updated_at,
        current,
        history,
        labels: labels_for_record(connection, id)?,
        relations: relations_for(connection, id, false)?,
        lifecycle_history: lifecycle_for_record(connection, id)?,
        import_metadata: import_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
    }))
}

fn revision_from(
    connection: &Connection,
    record_id: i64,
    revision: i64,
    sanitize_import: bool,
) -> Result<Option<Revision>, Error> {
    let row: Option<(String, String, String, Provenance)> = connection
        .query_row(
            "SELECT title,payload_json,reason,created_at,actor,thread,client FROM record_revisions
             WHERE record_id=?1 AND revision=?2",
            params![record_id, revision],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    Provenance {
                        server_time: row.get(3)?,
                        actor: row.get(4)?,
                        thread: row.get(5)?,
                        client: row.get(6)?,
                    },
                ))
            },
        )
        .optional()?;
    let Some((title, payload_json, reason, provenance)) = row else {
        return Ok(None);
    };
    let mut payload: RecordPayload = serde_json::from_str(&payload_json)?;
    if sanitize_import {
        sanitize_imported_payload(connection, &mut payload)?;
    }
    let mut statement = connection.prepare(
        "SELECT id,identity,locator,version,content_hash,anchor,quote FROM source_references
         WHERE record_id=?1 AND revision=?2 ORDER BY position,id",
    )?;
    let sources = statement
        .query_map(params![record_id, revision], |row| {
            Ok(SourceReference {
                id: row.get(0)?,
                identity: row.get(1)?,
                locator: row.get(2)?,
                version: row.get(3)?,
                content_hash: row.get(4)?,
                anchor: row.get(5)?,
                quote: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Revision {
        revision,
        title,
        payload,
        reason,
        provenance,
        sources,
    }))
}

fn labels_for_record(connection: &Connection, record_id: i64) -> Result<Vec<Label>, Error> {
    let mut statement = connection.prepare(
        "SELECT la.label_id FROM label_assertions la JOIN labels l ON l.id=la.label_id
         WHERE la.record_id=?1 AND l.active=1
         AND NOT EXISTS(SELECT 1 FROM label_retractions lr WHERE lr.assertion_id=la.id)
         ORDER BY l.facet,l.key,l.id",
    )?;
    let ids = statement
        .query_map([record_id], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|id| label_from(connection, id)?.ok_or(Error::MissingLabel(id)))
        .collect()
}

fn lifecycle_for_record(
    connection: &Connection,
    record_id: i64,
) -> Result<Vec<LifecycleTransition>, Error> {
    let mut statement = connection.prepare(
        "SELECT id,from_state,to_state,reason,created_at,actor,thread,client
         FROM lifecycle_transitions WHERE record_id=?1 ORDER BY id",
    )?;
    statement
        .query_map([record_id], |row| {
            let from: Option<String> = row.get(1)?;
            let to: String = row.get(2)?;
            Ok((
                row.get::<_, i64>(0)?,
                from,
                to,
                row.get::<_, String>(3)?,
                Provenance {
                    server_time: row.get(4)?,
                    actor: row.get(5)?,
                    thread: row.get(6)?,
                    client: row.get(7)?,
                },
            ))
        })?
        .map(|row| {
            let (id, from, to, reason, provenance) = row?;
            let from = match from {
                Some(value) => Some(
                    Lifecycle::parse(&value)
                        .ok_or_else(|| Error::Invalid("stored lifecycle is invalid".to_owned()))?,
                ),
                None => None,
            };
            Ok(LifecycleTransition {
                id,
                from,
                to: Lifecycle::parse(&to)
                    .ok_or_else(|| Error::Invalid("stored lifecycle is invalid".to_owned()))?,
                reason,
                provenance,
            })
        })
        .collect::<Result<Vec<_>, Error>>()
}

fn relations_for(
    connection: &Connection,
    record_id: i64,
    include_retracted: bool,
) -> Result<Vec<Relation>, Error> {
    let mut statement = connection.prepare(
        "SELECT rr.id FROM record_relations rr
         JOIN records source ON source.id=rr.source_record_id AND source.readable=1
         JOIN records target ON target.id=rr.target_record_id AND target.readable=1
         WHERE (rr.source_record_id=?1 OR rr.target_record_id=?1)
         AND (?2=1 OR NOT EXISTS(SELECT 1 FROM relation_retractions rt WHERE rt.relation_id=rr.id))
         ORDER BY rr.id",
    )?;
    let ids = statement
        .query_map(params![record_id, include_retracted], |row| {
            row.get::<_, i64>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|id| {
            relation_from(connection, id, include_retracted)?.ok_or(Error::MissingRelation(id))
        })
        .collect()
}

#[allow(clippy::type_complexity)]
fn relation_from(
    connection: &Connection,
    id: i64,
    include_retracted: bool,
) -> Result<Option<Relation>, Error> {
    let row: Option<(
        i64,
        i64,
        String,
        String,
        Provenance,
        Option<(String, Provenance)>,
    )> = connection
        .query_row(
            "SELECT rr.source_record_id,rr.target_record_id,rr.kind,rr.reason,
                    rr.created_at,rr.actor,rr.thread,rr.client,
                    rt.reason,rt.created_at,rt.actor,rt.thread,rt.client
             FROM record_relations rr
             JOIN records source ON source.id=rr.source_record_id AND source.readable=1
             JOIN records target ON target.id=rr.target_record_id AND target.readable=1
             LEFT JOIN relation_retractions rt ON rt.relation_id=rr.id
             WHERE rr.id=?1",
            [id],
            |row| {
                let retraction_reason: Option<String> = row.get(8)?;
                let retracted = retraction_reason
                    .map(|reason| {
                        Ok::<_, rusqlite::Error>((
                            reason,
                            Provenance {
                                server_time: row.get(9)?,
                                actor: row.get(10)?,
                                thread: row.get(11)?,
                                client: row.get(12)?,
                            },
                        ))
                    })
                    .transpose()?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    Provenance {
                        server_time: row.get(4)?,
                        actor: row.get(5)?,
                        thread: row.get(6)?,
                        client: row.get(7)?,
                    },
                    retracted,
                ))
            },
        )
        .optional()?;
    let Some((source_record_id, target_record_id, kind, reason, asserted, retracted)) = row else {
        return Ok(None);
    };
    if retracted.is_some() && !include_retracted {
        return Ok(None);
    }
    Ok(Some(Relation {
        id,
        source_record_id,
        target_record_id,
        kind: RelationKind::parse(&kind)
            .ok_or_else(|| Error::Invalid("stored relation kind is invalid".to_owned()))?,
        reason,
        asserted,
        retracted: retracted.map(|(reason, provenance)| Retraction { reason, provenance }),
    }))
}

fn readable_record_state(
    connection: &Connection,
    record_id: i64,
) -> Result<(RecordKind, i64), Error> {
    let row: Option<(String, i64)> = connection
        .query_row(
            "SELECT kind,current_revision FROM records WHERE id=?1 AND readable=1",
            [record_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (kind, revision) = row.ok_or(Error::MissingRecord(record_id))?;
    Ok((
        RecordKind::parse(&kind)
            .ok_or_else(|| Error::Invalid("stored record kind is invalid".to_owned()))?,
        revision,
    ))
}

fn readable_record_lifecycle(
    connection: &Connection,
    record_id: i64,
) -> Result<(Lifecycle, i64), Error> {
    let row: Option<(String, i64)> = connection
        .query_row(
            "SELECT lifecycle,current_revision FROM records WHERE id=?1 AND readable=1",
            [record_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (lifecycle, revision) = row.ok_or(Error::MissingRecord(record_id))?;
    Ok((
        Lifecycle::parse(&lifecycle)
            .ok_or_else(|| Error::Invalid("stored lifecycle is invalid".to_owned()))?,
        revision,
    ))
}

fn ensure_active_label(connection: &Connection, label_id: i64) -> Result<(), Error> {
    let active: Option<bool> = connection
        .query_row("SELECT active FROM labels WHERE id=?1", [label_id], |row| {
            row.get(0)
        })
        .optional()?;
    match active {
        Some(true) => Ok(()),
        _ => Err(Error::MissingLabel(label_id)),
    }
}

fn validate_record_input(input: &RecordInput) -> Result<(), Error> {
    validate_id("scope_id", input.scope_id)?;
    validate_title(&input.title)?;
    if input.label_ids.is_empty() {
        return Err(Error::Invalid(
            "record requires at least one active label".to_owned(),
        ));
    }
    distinct_ids("label_ids", &input.label_ids, MAX_LABEL_IDS)?;
    validate_payload(&input.payload, &input.sources)
}

fn validate_payload(payload: &RecordPayload, sources: &[SourceInput]) -> Result<(), Error> {
    if sources.len() > MAX_SOURCES {
        return Err(Error::Invalid(format!(
            "sources cannot contain more than {MAX_SOURCES} items"
        )));
    }
    for source in sources {
        validate_source(source)?;
    }
    match payload {
        RecordPayload::Note { body } => validate_text("body", body, true, true)?,
        RecordPayload::Observation {
            statement,
            observed_at,
        } => {
            validate_text("statement", statement, true, true)?;
            validate_timestamp_option("observed_at", observed_at)?;
            require_sources("observation", sources)?;
        }
        RecordPayload::Decision {
            choice,
            question,
            rationale,
            decided_at,
        } => {
            validate_text("choice", choice, true, true)?;
            validate_optional_text("question", question, true)?;
            validate_optional_text("rationale", rationale, true)?;
            validate_timestamp_option("decided_at", decided_at)?;
        }
        RecordPayload::Idea { proposal } => validate_text("proposal", proposal, true, true)?,
        RecordPayload::Snippet {
            language,
            code,
            origin,
            runtime,
            dependencies,
        } => {
            validate_single_line("language", language, MAX_NAME_BYTES)?;
            validate_text("code", code, true, false)?;
            if let Some(runtime) = runtime {
                validate_single_line("runtime", runtime, MAX_NAME_BYTES)?;
            }
            if let Some(dependencies) = dependencies {
                if dependencies.len() > 100 {
                    return Err(Error::Invalid(
                        "dependencies cannot contain more than 100 items".to_owned(),
                    ));
                }
                for dependency in dependencies {
                    validate_single_line("dependency", dependency, MAX_NAME_BYTES)?;
                }
            }
            if *origin == SnippetOrigin::Imported
                && !sources.iter().any(|source| {
                    trim_option(&source.locator).is_some()
                        && trim_option(&source.content_hash).is_some()
                })
            {
                return Err(Error::Invalid(
                    "imported snippet requires a source with locator and content_hash".to_owned(),
                ));
            }
        }
        RecordPayload::Metric {
            name,
            value,
            unit,
            observed_at,
            interval,
            dimensions,
            method,
        } => {
            validate_single_line("name", name, MAX_NAME_BYTES)?;
            validate_single_line("unit", unit, MAX_NAME_BYTES)?;
            if !value.is_finite() {
                return Err(Error::Invalid("metric value must be finite".to_owned()));
            }
            if observed_at.is_some() && interval.is_some() {
                return Err(Error::Invalid(
                    "metric cannot have both observed_at and interval".to_owned(),
                ));
            }
            validate_timestamp_option("observed_at", observed_at)?;
            if let Some(interval) = interval {
                let start = validate_timestamp("interval.start", &interval.start)?;
                let end = validate_timestamp("interval.end", &interval.end)?;
                if end < start {
                    return Err(Error::Invalid(
                        "metric interval end must not precede start".to_owned(),
                    ));
                }
            }
            if dimensions.len() > 100 {
                return Err(Error::Invalid(
                    "dimensions cannot contain more than 100 items".to_owned(),
                ));
            }
            for (key, value) in dimensions {
                validate_single_line("dimension key", key, MAX_NAME_BYTES)?;
                validate_single_line("dimension value", value, MAX_NAME_BYTES)?;
            }
            validate_optional_text("method", method, true)?;
            require_sources("metric", sources)?;
        }
        RecordPayload::Evidence {
            claim,
            action,
            outcome,
            impact,
        } => {
            validate_text("claim", claim, true, true)?;
            validate_optional_text("action", action, true)?;
            validate_optional_text("outcome", outcome, true)?;
            validate_optional_text("impact", impact, true)?;
            require_sources("evidence", sources)?;
        }
    }
    Ok(())
}

fn validate_source(source: &SourceInput) -> Result<(), Error> {
    validate_single_line("source identity", &source.identity, MAX_NAME_BYTES)?;
    for (name, value) in [
        ("source locator", &source.locator),
        ("source version", &source.version),
        ("source content_hash", &source.content_hash),
        ("source anchor", &source.anchor),
    ] {
        if let Some(value) = value {
            validate_single_line(name, value, MAX_NAME_BYTES)?;
        }
    }
    if let Some(quote) = &source.quote {
        validate_text("source quote", quote, false, true)?;
    }
    Ok(())
}

fn require_sources(kind: &str, sources: &[SourceInput]) -> Result<(), Error> {
    if sources.is_empty() {
        Err(Error::Invalid(format!(
            "{kind} requires at least one source"
        )))
    } else {
        Ok(())
    }
}

fn validate_text(
    name: &str,
    value: &str,
    nonempty: bool,
    validate_mermaid: bool,
) -> Result<(), Error> {
    if value.len() > MAX_BODY_BYTES {
        return Err(Error::Invalid(format!(
            "{name} exceeds {MAX_BODY_BYTES} bytes"
        )));
    }
    if nonempty && value.trim().is_empty() {
        return Err(Error::Invalid(format!("{name} must not be empty")));
    }
    if validate_mermaid {
        crate::merman::validate_markdown_fences(value).map_err(Error::Invalid)?;
    }
    Ok(())
}

fn validate_optional_text(
    name: &str,
    value: &Option<String>,
    validate_mermaid: bool,
) -> Result<(), Error> {
    if let Some(value) = value {
        validate_text(name, value, true, validate_mermaid)?;
    }
    Ok(())
}

fn validate_title(title: &str) -> Result<(), Error> {
    validate_single_line("title", title, MAX_TITLE_BYTES).map(|_| ())
}

fn validate_reason(reason: &str) -> Result<(), Error> {
    validate_single_line("reason", reason, MAX_NAME_BYTES).map(|_| ())
}

fn validate_single_line<'a>(
    name: &str,
    value: &'a str,
    max_bytes: usize,
) -> Result<&'a str, Error> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes || value.contains(['\n', '\r']) {
        Err(Error::Invalid(format!(
            "{name} must be a non-empty single line of at most {max_bytes} bytes"
        )))
    } else {
        Ok(value)
    }
}

fn validate_scope_name(name: &str) -> Result<&str, Error> {
    let name = validate_single_line("scope name", name, MAX_NAME_BYTES)?;
    if name.eq_ignore_ascii_case("global") {
        Ok("global")
    } else {
        Ok(name)
    }
}

fn validate_label_part<'a>(name: &str, value: &'a str) -> Result<&'a str, Error> {
    let value = validate_single_line(name, value, 64)?;
    if value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
    }) {
        Ok(value)
    } else {
        Err(Error::Invalid(format!(
            "{name} must contain only lowercase ASCII letters, digits, underscore, or hyphen"
        )))
    }
}

fn validate_aliases(aliases: &[String]) -> Result<Vec<String>, Error> {
    if aliases.len() > 100 {
        return Err(Error::Invalid(
            "aliases cannot contain more than 100 items".to_owned(),
        ));
    }
    let mut values = BTreeSet::new();
    for alias in aliases {
        values.insert(validate_single_line("alias", alias, MAX_NAME_BYTES)?.to_lowercase());
    }
    Ok(values.into_iter().collect())
}

fn validate_context(context: &WriteContext) -> Result<(), Error> {
    validate_single_line("idempotency_key", &context.idempotency_key, MAX_NAME_BYTES)?;
    validate_single_line("actor", &context.actor, MAX_PROVENANCE_BYTES)?;
    validate_single_line("thread", &context.thread, MAX_PROVENANCE_BYTES)?;
    validate_single_line("client", &context.client, MAX_PROVENANCE_BYTES)?;
    Ok(())
}

fn validate_embedding_model(
    model: &str,
    model_revision: &str,
    dimensions: usize,
) -> Result<(), Error> {
    validate_single_line("embedding model", model, MAX_PROVENANCE_BYTES)?;
    validate_single_line(
        "embedding model revision",
        model_revision,
        MAX_PROVENANCE_BYTES,
    )?;
    if dimensions == 0 || dimensions > 4096 {
        return Err(Error::Invalid(
            "embedding dimensions must be between 1 and 4096".to_owned(),
        ));
    }
    Ok(())
}

fn validate_embedding(embedding: &[f32], dimensions: usize) -> Result<(), Error> {
    if embedding.len() != dimensions || embedding.iter().any(|value| !value.is_finite()) {
        return Err(Error::Invalid(
            "embedding has invalid dimensions or values".to_owned(),
        ));
    }
    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if (norm - 1.0).abs() > 0.001 {
        return Err(Error::Invalid("embedding must be L2-normalized".to_owned()));
    }
    Ok(())
}

fn embedding_from_bytes(bytes: &[u8], dimensions: usize) -> Result<Vec<f32>, Error> {
    if bytes.len() != dimensions * size_of::<f32>() {
        return Err(Error::Invalid(
            "stored embedding has invalid dimensions".to_owned(),
        ));
    }
    let embedding = bytes
        .chunks_exact(size_of::<f32>())
        .map(|chunk| {
            let bytes: [u8; size_of::<f32>()] = chunk.try_into().map_err(|_| {
                Error::Invalid("stored embedding has invalid dimensions".to_owned())
            })?;
            Ok(f32::from_le_bytes(bytes))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    validate_embedding(&embedding, dimensions)?;
    Ok(embedding)
}

fn embedding_status_from(
    connection: &Connection,
    model: &str,
    model_revision: &str,
    dimensions: usize,
) -> Result<EmbeddingStatus, Error> {
    let eligible_records: i64 =
        connection.query_row("SELECT count(*) FROM records WHERE readable=1", [], |row| {
            row.get(0)
        })?;
    let indexed_records: i64 = connection.query_row(
        "SELECT count(*)
         FROM records r
         JOIN record_embeddings e
           ON e.record_id=r.id AND e.revision=r.current_revision
         WHERE r.readable=1 AND e.model=?1 AND e.model_revision=?2 AND e.dimensions=?3",
        params![model, model_revision, dimensions as i64],
        |row| row.get(0),
    )?;
    Ok(EmbeddingStatus {
        model: model.to_owned(),
        model_revision: model_revision.to_owned(),
        dimensions,
        eligible_records: eligible_records as usize,
        indexed_records: indexed_records as usize,
        pending_records: (eligible_records - indexed_records) as usize,
    })
}

fn validate_id(name: &str, id: i64) -> Result<(), Error> {
    if id > 0 {
        Ok(())
    } else {
        Err(Error::Invalid(format!("{name} must be positive")))
    }
}

fn validate_limit(limit: usize) -> Result<(), Error> {
    if (1..=SEARCH_RESULT_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "limit must be between 1 and {SEARCH_RESULT_LIMIT}"
        )))
    }
}

fn validate_query(query: &str) -> Result<(), Error> {
    if query.len() <= MAX_SEARCH_QUERY_BYTES {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "search query exceeds {MAX_SEARCH_QUERY_BYTES} bytes"
        )))
    }
}

fn validate_timestamp(name: &str, value: &str) -> Result<DateTime<chrono::FixedOffset>, Error> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|_| Error::Invalid(format!("{name} must be an RFC 3339 timestamp")))
}

fn validate_timestamp_option(name: &str, value: &Option<String>) -> Result<(), Error> {
    if let Some(value) = value {
        validate_timestamp(name, value)?;
    }
    Ok(())
}

fn distinct_ids(name: &str, ids: &[i64], max: usize) -> Result<Vec<i64>, Error> {
    if ids.len() > max {
        return Err(Error::Invalid(format!(
            "{name} cannot contain more than {max} items"
        )));
    }
    let mut values = Vec::new();
    let mut seen = HashSet::new();
    for id in ids {
        validate_id(name, *id)?;
        if seen.insert(*id) {
            values.push(*id);
        }
    }
    Ok(values)
}

fn trim_option(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn fts_query(query: &str) -> Option<String> {
    let terms = query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

fn push_placeholders(sql: &mut String, count: usize, next: &mut usize) {
    for index in 0..count {
        if index != 0 {
            sql.push(',');
        }
        sql.push('?');
        sql.push_str(&next.to_string());
        *next += 1;
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn create_v3_schema(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL CHECK (kind IN ('daily', 'note', 'artifact')),
            visibility TEXT NOT NULL CHECK (visibility IN ('shared', 'private')),
            author TEXT NOT NULL CHECK (author IN ('user', 'agent')),
            day TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL,
            CHECK (kind <> 'daily' OR (visibility = 'shared' AND author = 'user'))
        );
        CREATE UNIQUE INDEX documents_daily_day ON documents(day) WHERE kind = 'daily';
        CREATE INDEX documents_day ON documents(day);
        PRAGMA user_version = 3;",
    )
}

fn create_v2_schema(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch("CREATE TABLE documents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL CHECK (kind IN ('daily', 'note')), day TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL); CREATE UNIQUE INDEX documents_daily_day ON documents(day) WHERE kind = 'daily'; CREATE INDEX documents_day ON documents(day); PRAGMA user_version = 2;")
}

fn migrate(connection: &mut Connection) -> Result<(), Error> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(Error::UnsupportedSchema(version));
    }
    if version == 0 {
        create_v3_schema(&transaction)?;
    } else if version == 1 || version == 2 {
        if version == 1 {
            create_v2_schema(&transaction)?;
            let migrated = {
                let mut statement = transaction.prepare("SELECT id, day, created_at, updated_at, body FROM entries ORDER BY day ASC, created_at ASC, id ASC")?;
                statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let mut days: Vec<(i64, String, String, String, Vec<String>)> = Vec::new();
            for (id, day, created_at, updated_at, body) in migrated {
                if days.last().is_none_or(|item| item.1 != day) {
                    days.push((id, day, created_at, updated_at.clone(), Vec::new()));
                }
                let current = days.last_mut().ok_or(Error::MissingRecord(id))?;
                if updated_at > current.3 {
                    current.3 = updated_at;
                }
                if !body.is_empty() {
                    current.4.push(body);
                }
            }
            for (id, day, created_at, updated_at, bodies) in days {
                transaction.execute("INSERT INTO documents (id, kind, day, created_at, updated_at, body) VALUES (?1, 'daily', ?2, ?3, ?4, ?5)", params![id, day, created_at, updated_at, bodies.join("\n\n")])?;
            }
            transaction.execute_batch("DROP TABLE entries; PRAGMA user_version = 2;")?;
        }
        transaction.execute_batch(
            "ALTER TABLE documents RENAME TO documents_v2;
             DROP INDEX documents_daily_day; DROP INDEX documents_day;",
        )?;
        create_v3_schema(&transaction)?;
        transaction.execute_batch(
            "INSERT INTO documents (id, kind, visibility, author, day, created_at, updated_at, body)
             SELECT id, kind, 'shared', 'user', day, created_at, updated_at, body FROM documents_v2;
             DROP TABLE documents_v2;",
        )?;
    }
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 3 {
        transaction.execute_batch(
            "ALTER TABLE documents ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
             CREATE TABLE presence (
                session_id TEXT PRIMARY KEY,
                actor TEXT NOT NULL CHECK(actor IN ('user', 'agent')),
                document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                last_heartbeat INTEGER NOT NULL
             );
             CREATE INDEX presence_document ON presence(document_id);
             PRAGMA user_version = 4;",
        )?;
    }
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 4 {
        transaction.execute_batch(
            "CREATE TABLE document_attachments (
                parent_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                attached_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                status TEXT NOT NULL CHECK (status IN ('completed', 'blocked', 'failed')),
                PRIMARY KEY (parent_document_id, attached_document_id),
                UNIQUE (attached_document_id)
             );
             CREATE INDEX document_attachments_parent ON document_attachments(parent_document_id);
             PRAGMA user_version = 5;",
        )?;
    }
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 5 {
        let documents_sequence: i64 = transaction
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'documents'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        transaction.execute_batch(
            "CREATE TEMP TABLE presence_v5 AS SELECT * FROM presence;
             CREATE TEMP TABLE attachments_v5 AS SELECT * FROM document_attachments;
             DROP TABLE presence;
             DROP TABLE document_attachments;
             ALTER TABLE documents RENAME TO documents_v5;
             DROP INDEX documents_daily_day;
             DROP INDEX documents_day;
             CREATE TABLE documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL CHECK (kind IN ('daily', 'note', 'artifact', 'project')),
                visibility TEXT NOT NULL CHECK (visibility IN ('shared', 'private')),
                author TEXT NOT NULL CHECK (author IN ('user', 'agent')),
                day TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL,
                revision INTEGER NOT NULL DEFAULT 1,
                CHECK (kind <> 'daily' OR (visibility = 'shared' AND author = 'user')),
                CHECK (kind <> 'project' OR author = 'user')
             );
             INSERT INTO documents SELECT * FROM documents_v5;
             DROP TABLE documents_v5;
             CREATE UNIQUE INDEX documents_daily_day ON documents(day) WHERE kind = 'daily';
             CREATE INDEX documents_day ON documents(day);
             CREATE TABLE presence (session_id TEXT PRIMARY KEY, actor TEXT NOT NULL CHECK(actor IN ('user', 'agent')), document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, last_heartbeat INTEGER NOT NULL);
             INSERT INTO presence SELECT * FROM presence_v5;
             DROP TABLE presence_v5;
             CREATE INDEX presence_document ON presence(document_id);
             CREATE TABLE document_attachments (parent_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, attached_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, status TEXT NOT NULL CHECK (status IN ('completed', 'blocked', 'failed')), PRIMARY KEY (parent_document_id, attached_document_id), UNIQUE (attached_document_id));
             INSERT INTO document_attachments SELECT * FROM attachments_v5;
             DROP TABLE attachments_v5;
             CREATE INDEX document_attachments_parent ON document_attachments(parent_document_id);
             CREATE TABLE project_documents (project_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, added_by TEXT NOT NULL CHECK(added_by IN ('user', 'agent')), created_at TEXT NOT NULL, PRIMARY KEY(project_document_id, document_id), CHECK(project_document_id <> document_id));
             CREATE INDEX project_documents_document ON project_documents(document_id);
             PRAGMA user_version = 6;",
        )?;
        let sequence_updated = transaction.execute(
            "UPDATE sqlite_sequence SET seq = max(seq, ?1) WHERE name = 'documents'",
            [documents_sequence],
        )?;
        if sequence_updated == 0 {
            transaction.execute(
                "INSERT INTO sqlite_sequence(name, seq) VALUES('documents', ?1)",
                [documents_sequence],
            )?;
        }
        let violations: i64 =
            transaction.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
        if violations != 0 {
            return Err(Error::Sqlite(rusqlite::Error::ExecuteReturnedResults));
        }
    }
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 6 {
        transaction.execute_batch(
            "ALTER TABLE document_attachments ADD COLUMN reviewed_at TEXT;
             PRAGMA user_version = 7;",
        )?;
    }
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 7 {
        migrate_v8(&transaction)?;
    }
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 8 {
        migrate_v9(&transaction)?;
    }
    transaction.commit()?;
    Ok(())
}

fn migrate_v8(transaction: &Transaction<'_>) -> Result<(), Error> {
    transaction.execute_batch(
        "CREATE TABLE scopes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE COLLATE NOCASE,
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE
         );
         CREATE TABLE records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_id INTEGER NOT NULL REFERENCES scopes(id),
            kind TEXT NOT NULL CHECK(kind IN ('note','observation','decision','idea','snippet','metric','evidence')),
            title TEXT NOT NULL,
            lifecycle TEXT NOT NULL CHECK(lifecycle IN ('active','superseded','merged','retracted')),
            current_revision INTEGER NOT NULL CHECK(current_revision > 0),
            readable INTEGER NOT NULL CHECK(readable IN (0,1)),
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
            actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE,
            import_metadata TEXT
         );
         CREATE INDEX records_scope_lifecycle_id ON records(scope_id,lifecycle,id DESC);
         CREATE TABLE record_revisions (
            record_id INTEGER NOT NULL REFERENCES records(id),
            revision INTEGER NOT NULL CHECK(revision > 0),
            title TEXT NOT NULL, payload_json TEXT NOT NULL, reason TEXT NOT NULL,
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE,
            PRIMARY KEY(record_id,revision)
         );
         CREATE TABLE source_references (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            record_id INTEGER NOT NULL,
            revision INTEGER NOT NULL,
            position INTEGER NOT NULL,
            identity TEXT NOT NULL, locator TEXT, version TEXT, content_hash TEXT, anchor TEXT, quote TEXT,
            FOREIGN KEY(record_id,revision) REFERENCES record_revisions(record_id,revision),
            UNIQUE(record_id,revision,position)
         );
         CREATE TABLE labels (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            facet TEXT NOT NULL, key TEXT NOT NULL, display_name TEXT NOT NULL,
            active INTEGER NOT NULL CHECK(active IN (0,1)),
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE,
            UNIQUE(facet,key)
         );
         CREATE TABLE label_aliases (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            label_id INTEGER NOT NULL REFERENCES labels(id),
            alias TEXT NOT NULL UNIQUE COLLATE NOCASE
         );
         CREATE TABLE label_assertions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            record_id INTEGER NOT NULL REFERENCES records(id),
            label_id INTEGER NOT NULL REFERENCES labels(id),
            reason TEXT NOT NULL,
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE
         );
         CREATE INDEX label_assertions_record ON label_assertions(record_id,label_id,id);
         CREATE TABLE label_retractions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            assertion_id INTEGER NOT NULL UNIQUE REFERENCES label_assertions(id),
            reason TEXT NOT NULL,
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE
         );
         CREATE TABLE record_relations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_record_id INTEGER NOT NULL REFERENCES records(id),
            target_record_id INTEGER NOT NULL REFERENCES records(id),
            kind TEXT NOT NULL CHECK(kind IN ('references','mentions','derived_from','supports','contradicts','supersedes','merged_into','summarizes')),
            reason TEXT NOT NULL,
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE,
            CHECK(source_record_id <> target_record_id)
         );
         CREATE INDEX record_relations_source ON record_relations(source_record_id,id);
         CREATE INDEX record_relations_target ON record_relations(target_record_id,id);
         CREATE TABLE relation_retractions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            relation_id INTEGER NOT NULL UNIQUE REFERENCES record_relations(id),
            reason TEXT NOT NULL,
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE
         );
         CREATE TABLE lifecycle_transitions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            record_id INTEGER NOT NULL REFERENCES records(id),
            from_state TEXT CHECK(from_state IS NULL OR from_state IN ('active','superseded','merged','retracted')),
            to_state TEXT NOT NULL CHECK(to_state IN ('active','superseded','merged','retracted')),
            reason TEXT NOT NULL,
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE
         );
         CREATE INDEX lifecycle_transitions_record ON lifecycle_transitions(record_id,id);
         CREATE TABLE write_keys (
            key TEXT PRIMARY KEY,
            operation TEXT NOT NULL, request_json TEXT NOT NULL, result_id INTEGER NOT NULL,
            created_at TEXT NOT NULL, actor TEXT NOT NULL, thread TEXT NOT NULL, client TEXT NOT NULL
         );
         CREATE VIRTUAL TABLE record_fts USING fts5(record_id UNINDEXED,title,payload);
         PRAGMA user_version = 8;",
    )?;
    let timestamp = now();
    transaction.execute(
        "INSERT INTO scopes(name,created_at,actor,thread,client,idempotency_key)
         VALUES('global',?1,'archive','schema-v8','archive','schema-v8:scope:global')",
        [&timestamp],
    )?;
    let global_scope = transaction.last_insert_rowid();
    transaction.execute(
        "INSERT INTO labels(facet,key,display_name,active,created_at,actor,thread,client,idempotency_key)
         VALUES('workflow','inbox','Inbox',1,?1,'archive','schema-v8','archive','schema-v8:label:workflow:inbox')",
        [&timestamp],
    )?;
    let inbox_label = transaction.last_insert_rowid();
    let legacy = {
        let mut statement = transaction.prepare(
            "SELECT id,kind,visibility,author,day,created_at,updated_at,body,revision FROM documents ORDER BY id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (id, kind, visibility, author, day, created_at, updated_at, body, revision) in &legacy {
        let title = format!("Imported legacy {kind} {id}");
        let payload = RecordPayload::Note { body: body.clone() };
        let payload_json = serde_json::to_string(&payload)?;
        let metadata = serde_json::to_string(&json!({
            "legacy": {
                "created_at": created_at,
                "updated_at": updated_at,
                "author": author,
                "day": day,
                "kind": kind,
                "visibility": visibility,
                "revision": revision
            }
        }))?;
        let key = format!("schema-v8:legacy-record:{id}");
        transaction.execute(
            "INSERT INTO records(id,scope_id,kind,title,lifecycle,current_revision,readable,created_at,updated_at,actor,thread,client,idempotency_key,import_metadata)
             VALUES(?1,?2,'note',?3,'active',1,?4,?5,?5,'archive','schema-v8','archive',?6,?7)",
            params![id,global_scope,title,if visibility == "shared" { 1 } else { 0 },timestamp,key,metadata],
        )?;
        transaction.execute(
            "INSERT INTO record_revisions(record_id,revision,title,payload_json,reason,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,1,?2,?3,'legacy import',?4,'archive','schema-v8','archive',?5)",
            params![id,title,payload_json,timestamp,format!("{key}:revision")],
        )?;
        transaction.execute(
            "INSERT INTO label_assertions(record_id,label_id,reason,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,?2,'legacy import fallback',?3,'archive','schema-v8','archive',?4)",
            params![id,inbox_label,timestamp,format!("{key}:label")],
        )?;
        transaction.execute(
            "INSERT INTO lifecycle_transitions(record_id,from_state,to_state,reason,created_at,actor,thread,client,idempotency_key)
             VALUES(?1,NULL,'active','legacy import',?2,'archive','schema-v8','archive',?3)",
            params![id,timestamp,format!("{key}:lifecycle")],
        )?;
    }
    for (source_id, _, _, _, _, _, _, body, _) in &legacy {
        let mut seen = HashSet::new();
        for target_id in parsed_reference_ids(body) {
            if target_id != *source_id
                && legacy.iter().any(|row| row.0 == target_id)
                && seen.insert(target_id)
            {
                transaction.execute(
                    "INSERT INTO record_relations(source_record_id,target_record_id,kind,reason,created_at,actor,thread,client,idempotency_key)
                     VALUES(?1,?2,'references','legacy note link',?3,'archive','schema-v8','archive',?4)",
                    params![source_id,target_id,timestamp,format!("schema-v8:legacy-reference:{source_id}:{target_id}")],
                )?;
            }
        }
    }
    let maximum = legacy.iter().map(|row| row.0).max().unwrap_or(0);
    if maximum > 0 {
        transaction.execute(
            "UPDATE sqlite_sequence SET seq=max(seq,?1) WHERE name='records'",
            [maximum],
        )?;
    }
    rebuild_fts(transaction)?;
    Ok(())
}

fn migrate_v9(transaction: &Transaction<'_>) -> Result<(), Error> {
    transaction.execute_batch(
        "CREATE TABLE record_embeddings (
            record_id INTEGER NOT NULL,
            revision INTEGER NOT NULL,
            model TEXT NOT NULL,
            model_revision TEXT NOT NULL,
            dimensions INTEGER NOT NULL CHECK(dimensions > 0),
            vector BLOB NOT NULL CHECK(length(vector) = dimensions * 4),
            embedded_at TEXT NOT NULL,
            PRIMARY KEY(record_id,model,model_revision),
            FOREIGN KEY(record_id,revision) REFERENCES record_revisions(record_id,revision)
         );
         CREATE INDEX record_embeddings_model_revision
           ON record_embeddings(model,model_revision,dimensions,record_id);
         PRAGMA user_version = 9;",
    )?;
    Ok(())
}

#[derive(Clone, Copy)]
struct ParsedReference {
    from: usize,
    to: usize,
    id: i64,
}

fn parsed_references(body: &str) -> Vec<ParsedReference> {
    let bytes = body.as_bytes();
    let mut references = Vec::new();
    let mut start = 0;
    while start + 8 < bytes.len() {
        if !body[start..].starts_with("[[note:") {
            start += body[start..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let mut cursor = start + 7;
        let id_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        let id = body[id_start..cursor].parse::<i64>().ok();
        if cursor == id_start || bytes.get(cursor) != Some(&b'|') || id.is_none_or(|id| id <= 0) {
            start += 1;
            continue;
        }
        cursor += 1;
        let mut closed = false;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'\n' | b'\r' | b'|' => break,
                b'\\' => {
                    if !matches!(bytes.get(cursor + 1), Some(b'\\' | b'|' | b']')) {
                        break;
                    }
                    cursor += 2;
                }
                b']' if bytes.get(cursor + 1) == Some(&b']') => {
                    references.push(ParsedReference {
                        from: start,
                        to: cursor + 2,
                        id: id.unwrap(),
                    });
                    start = cursor + 2;
                    closed = true;
                    break;
                }
                _ => cursor += 1,
            }
        }
        if !closed {
            start += 1;
        }
    }
    references
}

fn parsed_reference_ids(body: &str) -> Vec<i64> {
    parsed_references(body)
        .into_iter()
        .map(|reference| reference.id)
        .collect()
}

fn sanitize_legacy_body(body: &str, readable_ids: &HashSet<i64>) -> String {
    let references = parsed_references(body);
    let mut visible = String::with_capacity(body.len());
    let mut cursor = 0;
    for reference in references {
        visible.push_str(&body[cursor..reference.from]);
        if readable_ids.contains(&reference.id) {
            visible.push_str(&body[reference.from..reference.to]);
        }
        cursor = reference.to;
    }
    visible.push_str(&body[cursor..]);
    visible
}

fn sanitize_imported_payload(
    connection: &Connection,
    payload: &mut RecordPayload,
) -> Result<(), Error> {
    let RecordPayload::Note { body } = payload else {
        return Ok(());
    };
    let ids = parsed_reference_ids(body);
    if ids.is_empty() {
        return Ok(());
    }
    let mut readable = HashSet::new();
    for id in ids {
        let visible: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE id=?1 AND readable=1)",
            [id],
            |row| row.get(0),
        )?;
        if visible {
            readable.insert(id);
        }
    }
    *body = sanitize_legacy_body(body, &readable);
    Ok(())
}

#[cfg(test)]
mod tests;
