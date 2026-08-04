use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use chrono::{Local, NaiveDate, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};
use serde::Serialize;

const SCHEMA_VERSION: i64 = 4;
const MAX_SESSION_ID_BYTES: usize = 128;
const PRESENCE_TTL_SECONDS: i64 = 10;
const MAX_SEARCH_QUERY_BYTES: usize = 256;
const SEARCH_RESULT_LIMIT: usize = 50;
const MAX_REFERENCE_IDS: usize = 200;
pub const MAX_BODY_BYTES: usize = 1_000_000;
pub const MAX_TITLE_BYTES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Document {
    pub id: i64,
    pub kind: String,
    pub visibility: String,
    pub author: String,
    pub day: String,
    pub created_at: String,
    pub updated_at: String,
    pub body: String,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncSnapshot {
    pub document: Option<Document>,
    pub user_count: usize,
    pub agent_present: bool,
}

pub type DocumentSummary = Document;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReferenceSummary {
    pub id: i64,
    pub kind: String,
    pub day: String,
    pub label: String,
}

#[derive(Debug)]
pub enum Error {
    InvalidDay,
    InvalidId,
    InvalidVisibility,
    InvalidTitle,
    EmptyBody,
    BodyTooLarge,
    TooManyReferenceIds,
    SearchQueryTooLarge,
    InvalidLimit,
    InvalidSessionId,
    InvalidMermaid(String),
    MissingDocument(i64),
    CannotDeleteDaily,
    WriteConflict,
    UnsupportedSchema(i64),
    Lock,
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDay => write!(formatter, "day must be a valid YYYY-MM-DD local date"),
            Self::InvalidId => write!(formatter, "document id must be positive"),
            Self::InvalidVisibility => write!(formatter, "visibility must be shared or private"),
            Self::InvalidTitle => write!(
                formatter,
                "title must be a non-empty single line of at most {MAX_TITLE_BYTES} bytes"
            ),
            Self::EmptyBody => write!(formatter, "body must not be empty"),
            Self::BodyTooLarge => write!(formatter, "body exceeds {MAX_BODY_BYTES} bytes"),
            Self::TooManyReferenceIds => write!(
                formatter,
                "cannot resolve more than {MAX_REFERENCE_IDS} reference IDs"
            ),
            Self::SearchQueryTooLarge => write!(
                formatter,
                "search query exceeds {MAX_SEARCH_QUERY_BYTES} bytes"
            ),
            Self::InvalidLimit => write!(
                formatter,
                "limit must be between 1 and {SEARCH_RESULT_LIMIT}"
            ),
            Self::InvalidSessionId => write!(
                formatter,
                "session id must be non-empty and at most {MAX_SESSION_ID_BYTES} bytes"
            ),
            Self::InvalidMermaid(message) => message.fmt(formatter),
            Self::MissingDocument(id) => write!(formatter, "document {id} does not exist"),
            Self::CannotDeleteDaily => write!(formatter, "daily documents cannot be deleted"),
            Self::WriteConflict => write!(
                formatter,
                "document changed outside this editor; reload before saving"
            ),
            Self::UnsupportedSchema(version) => write!(
                formatter,
                "database schema version {version} is not supported"
            ),
            Self::Lock => write!(formatter, "database lock is unavailable"),
            Self::Io(error) => error.fmt(formatter),
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
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub struct Database {
    connection: Mutex<Connection>,
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

    pub fn get_or_create_daily(&self, day: &str) -> Result<Document, Error> {
        validate_day(day)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO documents (kind, visibility, author, day, created_at, updated_at, body)
             VALUES ('daily', 'shared', 'user', ?1, ?2, ?2, '')
             ON CONFLICT(day) WHERE kind = 'daily' DO NOTHING",
            params![day, now()],
        )?;
        let document = get_daily(&transaction, day)?.ok_or(Error::MissingDocument(0))?;
        transaction.commit()?;
        Ok(document)
    }

    pub fn create_note(&self, day: &str, visibility: &str) -> Result<Document, Error> {
        validate_day(day)?;
        validate_visibility(visibility)?;
        let connection = self.connection()?;
        let timestamp = now();
        connection.execute("INSERT INTO documents (kind, visibility, author, day, created_at, updated_at, body) VALUES ('note', ?1, 'user', ?2, ?3, ?3, '')", params![visibility, day, timestamp])?;
        let id = connection.last_insert_rowid();
        get_document_from(&connection, id)?.ok_or(Error::MissingDocument(id))
    }

    pub fn get_document(&self, id: i64) -> Result<Document, Error> {
        validate_id(id)?;
        let connection = self.connection()?;
        get_document_from(&connection, id)?.ok_or(Error::MissingDocument(id))
    }

    #[cfg(test)]
    fn update_document_body(&self, id: i64, body: &str) -> Result<Document, Error> {
        validate_id(id)?;
        let connection = self.connection()?;
        if connection.execute(
            "UPDATE documents SET body = ?1, updated_at = ?2, revision = revision + 1 WHERE id = ?3",
            params![body, now(), id],
        )? == 0
        {
            return Err(Error::MissingDocument(id));
        }
        get_document_from(&connection, id)?.ok_or(Error::MissingDocument(id))
    }

    pub fn replace_document_body(
        &self,
        id: i64,
        expected_revision: i64,
        body: &str,
    ) -> Result<Document, Error> {
        validate_id(id)?;
        let connection = self.connection()?;
        let updated = connection
            .query_row(
                "UPDATE documents SET body = ?1, updated_at = ?2, revision = revision + 1
                 WHERE id = ?3 AND revision = ?4
                 RETURNING id, kind, visibility, author, day, created_at, updated_at, body, revision",
                params![body, now(), id, expected_revision],
                document_from_row,
            )
            .optional()?;
        if let Some(document) = updated {
            return Ok(document);
        }
        if get_document_from(&connection, id)?.is_some() {
            Err(Error::WriteConflict)
        } else {
            Err(Error::MissingDocument(id))
        }
    }

    pub fn sync_document(&self, id: i64, known_revision: i64) -> Result<SyncSnapshot, Error> {
        validate_id(id)?;
        let connection = self.connection()?;
        let document = get_document_from(&connection, id)?.ok_or(Error::MissingDocument(id))?;
        let (user_count, agent_present) = presence_snapshot(&connection, id)?;
        let document = (document.revision > known_revision).then_some(document);
        Ok(SyncSnapshot {
            document,
            user_count,
            agent_present,
        })
    }

    pub fn set_presence(
        &self,
        session_id: &str,
        actor: &str,
        document_id: i64,
    ) -> Result<(), Error> {
        validate_session_id(session_id)?;
        validate_id(document_id)?;
        if !matches!(actor, "user" | "agent") {
            return Err(Error::InvalidSessionId);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if actor == "agent" && get_shared_document_from(&transaction, document_id)?.is_none() {
            return Err(Error::MissingDocument(document_id));
        }
        if actor == "user" && get_document_from(&transaction, document_id)?.is_none() {
            return Err(Error::MissingDocument(document_id));
        }
        delete_stale_presence(&transaction)?;
        transaction.execute("INSERT INTO presence(session_id, actor, document_id, last_heartbeat) VALUES(?1, ?2, ?3, unixepoch()) ON CONFLICT(session_id) DO UPDATE SET actor=excluded.actor, document_id=excluded.document_id, last_heartbeat=excluded.last_heartbeat", params![session_id, actor, document_id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_agent_presence(&self, session_id: &str, document_id: i64) -> Result<(), Error> {
        self.set_presence(session_id, "agent", document_id)
    }

    pub fn remove_presence(&self, session_id: &str) -> Result<(), Error> {
        validate_session_id(session_id)?;
        self.connection()?
            .execute("DELETE FROM presence WHERE session_id=?1", [session_id])?;
        Ok(())
    }

    pub fn delete_note(&self, id: i64) -> Result<(), Error> {
        validate_id(id)?;
        let connection = self.connection()?;
        let kind: Option<String> = connection
            .query_row("SELECT kind FROM documents WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        match kind.as_deref() {
            None => Err(Error::MissingDocument(id)),
            Some("daily") => Err(Error::CannotDeleteDaily),
            Some("note" | "artifact") => {
                connection.execute("DELETE FROM documents WHERE id = ?1", [id])?;
                Ok(())
            }
            Some(_) => Err(Error::MissingDocument(id)),
        }
    }

    pub fn search_documents(
        &self,
        active_day: &str,
        query: &str,
    ) -> Result<Vec<DocumentSummary>, Error> {
        validate_day(active_day)?;
        validate_query(query)?;
        let pattern = format!("%{}%", escape_like(query));
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, kind, visibility, author, day, created_at, updated_at, body, revision FROM documents
             WHERE body LIKE ?2 ESCAPE '\\' OR day LIKE ?2 ESCAPE '\\'
             ORDER BY (day = ?1) DESC, abs(julianday(day) - julianday(?1)) ASC,
                      CASE kind WHEN 'daily' THEN 0 ELSE 1 END ASC, day DESC, created_at DESC, id DESC LIMIT ?3")?;
        Ok(statement
            .query_map(
                params![active_day, pattern, SEARCH_RESULT_LIMIT],
                document_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn resolve_references(&self, ids: &[i64]) -> Result<Vec<ReferenceSummary>, Error> {
        let distinct_ids = distinct_valid_ids(ids)?;
        if distinct_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", distinct_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("SELECT id, kind, day, body FROM documents WHERE id IN ({placeholders})");
        let connection = self.connection()?;
        let mut statement = connection.prepare(&sql)?;
        let rows =
            statement.query_map(params_from_iter(distinct_ids.iter()), reference_from_row)?;
        let mut summaries = rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|summary| (summary.id, summary))
            .collect::<HashMap<_, _>>();
        Ok(distinct_ids
            .into_iter()
            .filter_map(|id| summaries.remove(&id))
            .collect())
    }

    pub fn mcp_read_document(&self, id: i64) -> Result<Document, Error> {
        validate_id(id)?;
        let connection = self.connection()?;
        let document =
            get_shared_document_from(&connection, id)?.ok_or(Error::MissingDocument(id))?;
        sanitize_mcp_document(&connection, document)
    }

    pub fn mcp_search_documents(&self, query: &str, limit: usize) -> Result<Vec<Document>, Error> {
        validate_query(query)?;
        if !(1..=SEARCH_RESULT_LIMIT).contains(&limit) {
            return Err(Error::InvalidLimit);
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, kind, visibility, author, day, created_at, updated_at, body, revision FROM documents
             WHERE visibility = 'shared' ORDER BY updated_at DESC, id DESC",
        )?;
        let documents = statement
            .query_map([], document_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let documents = sanitize_mcp_documents(&connection, documents)?;
        let query = query.to_lowercase();
        let mut matches = Vec::new();
        for document in documents {
            let visible_label = if document.kind == "daily" {
                document.day.clone()
            } else {
                note_label(&document.body)
            };
            if document.body.to_lowercase().contains(&query)
                || document.day.to_lowercase().contains(&query)
                || visible_label.to_lowercase().contains(&query)
            {
                matches.push(document);
                if matches.len() == limit {
                    break;
                }
            }
        }
        Ok(matches)
    }

    pub fn mcp_create_artifact(
        &self,
        title: &str,
        body: &str,
        related_ids: &[i64],
    ) -> Result<Document, Error> {
        validate_title(title)?;
        validate_body_size(body)?;
        crate::merman::validate_markdown_fences(body).map_err(Error::InvalidMermaid)?;
        let distinct_ids = distinct_valid_ids(related_ids)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let references = resolve_shared_references(&transaction, &distinct_ids)?;
        if references.len() != distinct_ids.len() {
            let found = references.iter().map(|r| r.id).collect::<HashSet<_>>();
            let missing = distinct_ids
                .iter()
                .find(|id| !found.contains(id))
                .copied()
                .unwrap_or(0);
            return Err(Error::MissingDocument(missing));
        }
        let by_id = references
            .into_iter()
            .map(|reference| (reference.id, reference))
            .collect::<HashMap<_, _>>();
        let mut markdown = format!("# {}", title.trim());
        if !body.is_empty() {
            markdown.push_str("\n\n");
            markdown.push_str(body);
        }
        for id in distinct_ids {
            let reference = by_id.get(&id).ok_or(Error::MissingDocument(id))?;
            markdown.push_str("\n\n");
            markdown.push_str(&format!(
                "[[note:{}|{}]]",
                id,
                escape_reference_label(&reference.label)
            ));
        }
        validate_body_size(&markdown)?;
        let timestamp = now();
        let day = Local::now().date_naive().format("%Y-%m-%d").to_string();
        transaction.execute("INSERT INTO documents (kind, visibility, author, day, created_at, updated_at, body) VALUES ('artifact', 'shared', 'agent', ?1, ?2, ?2, ?3)", params![day, timestamp, markdown])?;
        let id = transaction.last_insert_rowid();
        let document = get_document_from(&transaction, id)?.ok_or(Error::MissingDocument(id))?;
        let document = sanitize_mcp_document(&transaction, document)?;
        transaction.commit()?;
        Ok(document)
    }

    pub fn mcp_append_to_daily(&self, day: &str, body: &str) -> Result<Document, Error> {
        validate_day(day)?;
        if body.trim().is_empty() {
            return Err(Error::EmptyBody);
        }
        validate_body_size(body)?;
        crate::merman::validate_markdown_fences(body).map_err(Error::InvalidMermaid)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let timestamp = now();
        let inserted = transaction.execute("INSERT INTO documents (kind, visibility, author, day, created_at, updated_at, body) VALUES ('daily', 'shared', 'user', ?1, ?2, ?2, '') ON CONFLICT(day) WHERE kind = 'daily' DO NOTHING", params![day, timestamp])?;
        let current = get_daily(&transaction, day)?.ok_or(Error::MissingDocument(0))?;
        let joined = if current.body.is_empty() {
            body.to_owned()
        } else {
            format!("{}\n\n{}", current.body, body)
        };
        validate_body_size(&joined)?;
        transaction.execute(
            "UPDATE documents SET body = ?1, updated_at = ?2, revision = revision + ?4 WHERE id = ?3",
            params![joined, now(), current.id, i64::from(inserted == 0)],
        )?;
        let document = get_document_from(&transaction, current.id)?
            .ok_or(Error::MissingDocument(current.id))?;
        let document = sanitize_mcp_document(&transaction, document)?;
        transaction.commit()?;
        Ok(document)
    }
}

fn create_v3_schema(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
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
        PRAGMA user_version = 3;"
    )
}

fn create_v2_schema(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
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
    } else if version < SCHEMA_VERSION {
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
                let current = days.last_mut().ok_or(Error::MissingDocument(id))?;
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
             DROP TABLE documents_v2;"
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
    transaction.commit()?;
    Ok(())
}

fn validate_day(day: &str) -> Result<(), Error> {
    if day.len() != 10 || NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err() {
        Err(Error::InvalidDay)
    } else {
        Ok(())
    }
}
fn validate_id(id: i64) -> Result<(), Error> {
    if id <= 0 {
        Err(Error::InvalidId)
    } else {
        Ok(())
    }
}
fn validate_visibility(value: &str) -> Result<(), Error> {
    if matches!(value, "shared" | "private") {
        Ok(())
    } else {
        Err(Error::InvalidVisibility)
    }
}
fn validate_session_id(value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > MAX_SESSION_ID_BYTES {
        Err(Error::InvalidSessionId)
    } else {
        Ok(())
    }
}
fn validate_query(query: &str) -> Result<(), Error> {
    if query.len() > MAX_SEARCH_QUERY_BYTES {
        Err(Error::SearchQueryTooLarge)
    } else {
        Ok(())
    }
}
fn validate_title(title: &str) -> Result<(), Error> {
    if title.trim().is_empty() || title.len() > MAX_TITLE_BYTES || title.contains(['\n', '\r']) {
        Err(Error::InvalidTitle)
    } else {
        Ok(())
    }
}
fn validate_body_size(body: &str) -> Result<(), Error> {
    if body.len() > MAX_BODY_BYTES {
        Err(Error::BodyTooLarge)
    } else {
        Ok(())
    }
}
fn distinct_valid_ids(ids: &[i64]) -> Result<Vec<i64>, Error> {
    if ids.len() > MAX_REFERENCE_IDS {
        return Err(Error::TooManyReferenceIds);
    }
    for &id in ids {
        validate_id(id)?;
    }
    let mut seen = HashSet::new();
    Ok(ids.iter().copied().filter(|id| seen.insert(*id)).collect())
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn escape_like(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
fn escape_reference_label(label: &str) -> String {
    label
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(']', "\\]")
        .replace(['\n', '\r'], " ")
}
fn note_label(body: &str) -> String {
    let Some(line) = body.lines().find(|line| !line.trim().is_empty()) else {
        return "Untitled note".to_owned();
    };
    let line = line.trim();
    let marker_count = line.bytes().take_while(|byte| *byte == b'#').count();
    let label = if (1..=6).contains(&marker_count)
        && line[marker_count..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
    {
        line[marker_count..].trim()
    } else {
        line
    };
    if label.is_empty() {
        "Untitled note".to_owned()
    } else {
        label.to_owned()
    }
}
fn document_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        kind: row.get(1)?,
        visibility: row.get(2)?,
        author: row.get(3)?,
        day: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        body: row.get(7)?,
        revision: row.get(8)?,
    })
}
fn delete_stale_presence(connection: &Connection) -> Result<(), Error> {
    connection.execute(
        "DELETE FROM presence WHERE last_heartbeat < unixepoch() - ?1",
        [PRESENCE_TTL_SECONDS],
    )?;
    Ok(())
}
fn presence_snapshot(connection: &Connection, id: i64) -> Result<(usize, bool), Error> {
    let (users, agents): (i64, i64) = connection.query_row(
        "SELECT count(*) FILTER (WHERE actor='user'), count(*) FILTER (WHERE actor='agent') FROM presence WHERE document_id=?1 AND last_heartbeat >= unixepoch() - ?2",
        params![id, PRESENCE_TTL_SECONDS],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((users as usize, agents > 0))
}
fn reference_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReferenceSummary> {
    let id = row.get(0)?;
    let kind: String = row.get(1)?;
    let day: String = row.get(2)?;
    let body: String = row.get(3)?;
    let label = if kind == "daily" {
        day.clone()
    } else {
        note_label(&body)
    };
    Ok(ReferenceSummary {
        id,
        kind,
        day,
        label,
    })
}
fn get_document_from(connection: &Connection, id: i64) -> Result<Option<Document>, Error> {
    Ok(connection.query_row("SELECT id, kind, visibility, author, day, created_at, updated_at, body, revision FROM documents WHERE id = ?1", [id], document_from_row).optional()?)
}
fn get_shared_document_from(connection: &Connection, id: i64) -> Result<Option<Document>, Error> {
    Ok(connection.query_row("SELECT id, kind, visibility, author, day, created_at, updated_at, body, revision FROM documents WHERE id = ?1 AND visibility = 'shared'", [id], document_from_row).optional()?)
}
fn get_daily(connection: &Connection, day: &str) -> Result<Option<Document>, Error> {
    Ok(connection.query_row("SELECT id, kind, visibility, author, day, created_at, updated_at, body, revision FROM documents WHERE kind = 'daily' AND day = ?1", [day], document_from_row).optional()?)
}
fn resolve_shared_references(
    connection: &Connection,
    ids: &[i64],
) -> Result<Vec<ReferenceSummary>, Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT id, kind, day, body FROM documents WHERE visibility = 'shared' AND id IN ({placeholders})"
    );
    let mut statement = connection.prepare(&sql)?;
    Ok(statement
        .query_map(params_from_iter(ids), reference_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

#[derive(Clone, Copy)]
struct ParsedReference {
    from: usize,
    to: usize,
    id: i64,
}

fn parse_references(body: &str) -> Vec<ParsedReference> {
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

fn sanitize_mcp_document(connection: &Connection, document: Document) -> Result<Document, Error> {
    let references = parse_references(&document.body);
    let ids = bounded_reference_ids(&references);
    let shared_ids = resolve_shared_references(connection, &ids)?
        .into_iter()
        .map(|reference| reference.id)
        .collect::<HashSet<_>>();
    Ok(sanitize_document_with_ids(
        document,
        &references,
        &shared_ids,
    ))
}

fn sanitize_mcp_documents(
    connection: &Connection,
    documents: Vec<Document>,
) -> Result<Vec<Document>, Error> {
    let parsed = documents
        .iter()
        .map(|document| parse_references(&document.body))
        .collect::<Vec<_>>();
    let mut all_ids = HashSet::new();
    for references in &parsed {
        all_ids.extend(bounded_reference_ids(references));
    }
    let all_ids = all_ids.into_iter().collect::<Vec<_>>();
    let mut shared_ids = HashSet::new();
    for ids in all_ids.chunks(MAX_REFERENCE_IDS) {
        shared_ids.extend(
            resolve_shared_references(connection, ids)?
                .into_iter()
                .map(|reference| reference.id),
        );
    }
    Ok(documents
        .into_iter()
        .zip(parsed.iter())
        .map(|(document, references)| sanitize_document_with_ids(document, references, &shared_ids))
        .collect())
}

fn bounded_reference_ids(references: &[ParsedReference]) -> Vec<i64> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for reference in references {
        if seen.contains(&reference.id) || ids.len() < MAX_REFERENCE_IDS {
            if seen.insert(reference.id) {
                ids.push(reference.id);
            }
        }
    }
    ids
}

fn sanitize_document_with_ids(
    mut document: Document,
    references: &[ParsedReference],
    shared_ids: &HashSet<i64>,
) -> Document {
    let allowed_ids = bounded_reference_ids(references)
        .into_iter()
        .collect::<HashSet<_>>();
    let mut body = String::with_capacity(document.body.len());
    let mut cursor = 0;
    for reference in references {
        body.push_str(&document.body[cursor..reference.from]);
        if allowed_ids.contains(&reference.id) && shared_ids.contains(&reference.id) {
            body.push_str(&document.body[reference.from..reference.to]);
        }
        cursor = reference.to;
    }
    body.push_str(&document.body[cursor..]);
    document.body = body;
    document
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Database {
        Database::from_connection(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn fresh_schema_is_v4_with_constraints() {
        let database = database();
        let connection = database.connection.lock().unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
        assert!(connection.execute("INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body) VALUES('daily','private','user','2026-01-01','a','a','')", []).is_err());
        assert!(connection.execute("INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body) VALUES('daily','shared','agent','2026-01-01','a','a','')", []).is_err());
    }

    #[test]
    fn migrates_v1_exactly_and_reopens_idempotently() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE entries (id INTEGER PRIMARY KEY, day TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL); INSERT INTO entries VALUES (9,'2026-08-02','2026-08-02T12:00:00Z','2026-08-05T00:00:00Z','second'),(3,'2026-08-02','2026-08-02T08:00:00Z','2026-08-02T09:00:00Z','first\n'),(7,'2026-08-02','2026-08-02T10:00:00Z','2026-08-06T00:00:00Z',''),(12,'2026-08-03','2026-08-03T08:00:00Z','2026-08-03T09:00:00Z','only'); PRAGMA user_version=1;").unwrap();
        let large = "x".repeat(600_000);
        connection.execute("INSERT INTO entries VALUES(20,'2026-08-04','2026-08-04T08:00:00Z','2026-08-04T08:00:00Z',?1)",[&large]).unwrap();
        connection.execute("INSERT INTO entries VALUES(21,'2026-08-04','2026-08-04T09:00:00Z','2026-08-04T09:00:00Z',?1)",[&large]).unwrap();
        drop(connection);
        let database = Database::open(&path).unwrap();
        let first = database.get_document(3).unwrap();
        assert_eq!(first.body, "first\n\n\nsecond");
        assert_eq!(first.created_at, "2026-08-02T08:00:00Z");
        assert_eq!(first.updated_at, "2026-08-06T00:00:00Z");
        assert_eq!(
            (&first.visibility, &first.author),
            (&"shared".to_owned(), &"user".to_owned())
        );
        assert_eq!(database.get_document(20).unwrap().body.len(), 1_200_002);
        drop(database);
        assert_eq!(
            Database::open(&path).unwrap().get_document(3).unwrap(),
            first
        );
    }

    #[test]
    fn v2_migration_preserves_rows_and_ids() {
        let mut connection = Connection::open_in_memory().unwrap();
        {
            let tx = connection.transaction().unwrap();
            create_v2_schema(&tx).unwrap();
            tx.execute(
                "INSERT INTO documents VALUES(7,'note','2020-01-01','a','b','body')",
                [],
            )
            .unwrap();
            tx.commit().unwrap();
        }
        let database = Database::from_connection(connection).unwrap();
        let row = database.get_document(7).unwrap();
        assert_eq!(
            (&row.visibility, &row.author),
            (&"shared".to_owned(), &"user".to_owned())
        );
    }

    #[test]
    fn v3_migration_assigns_revision_one() {
        let mut connection = Connection::open_in_memory().unwrap();
        {
            let tx = connection.transaction().unwrap();
            create_v3_schema(&tx).unwrap();
            tx.execute("INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body) VALUES('note','shared','user','2026-08-04','a','b','body')", []).unwrap();
            tx.commit().unwrap();
        }
        let database = Database::from_connection(connection).unwrap();
        assert_eq!(database.get_document(1).unwrap().revision, 1);
    }

    #[test]
    fn daily_is_unique_and_notes_are_standalone() {
        let d = database();
        let daily = d.get_or_create_daily("2024-02-29").unwrap();
        assert_eq!(d.get_or_create_daily("2024-02-29").unwrap(), daily);
        let note = d.create_note("2024-02-29", "shared").unwrap();
        assert_ne!(daily.id, note.id);
    }

    #[test]
    fn crud_validation_and_autoincrement() {
        let d = database();
        assert!(matches!(
            d.create_note("2024-02-30", "shared"),
            Err(Error::InvalidDay)
        ));
        assert!(matches!(
            d.create_note("2024-02-29", "no"),
            Err(Error::InvalidVisibility)
        ));
        assert!(matches!(d.get_document(0), Err(Error::InvalidId)));
        let daily = d.get_or_create_daily("2026-08-03").unwrap();
        let note = d.create_note("2026-08-03", "shared").unwrap();
        let updated = d.update_document_body(note.id, "body").unwrap();
        assert_eq!(d.get_document(note.id).unwrap(), updated);
        assert!(matches!(
            d.delete_note(daily.id),
            Err(Error::CannotDeleteDaily)
        ));
        d.delete_note(note.id).unwrap();
        assert!(d.create_note("2026-08-03", "shared").unwrap().id > note.id);
    }

    #[test]
    fn gui_search_is_literal_bounded_and_ordered_by_proximity() {
        let d = database();
        let mut active = 0;
        for day in ["2026-08-01", "2026-08-03", "2026-08-02", "2026-08-05"] {
            let x = d.get_or_create_daily(day).unwrap();
            if day == "2026-08-03" {
                active = x.id;
            }
            d.update_document_body(x.id, "needle").unwrap();
        }
        let note = d.create_note("2026-08-03", "private").unwrap();
        d.update_document_body(note.id, "needle 100%_literal")
            .unwrap();
        assert_eq!(d.search_documents("2026-08-03", "%_").unwrap().len(), 1);
        let rows = d.search_documents("2026-08-03", "needle").unwrap();
        assert_eq!((rows[0].id, rows[1].id), (active, note.id));
        assert_eq!(
            rows.into_iter().map(|x| x.day).collect::<Vec<_>>(),
            [
                "2026-08-03",
                "2026-08-03",
                "2026-08-02",
                "2026-08-05",
                "2026-08-01"
            ]
        );
        assert!(matches!(
            d.search_documents("2026-08-03", &"x".repeat(257)),
            Err(Error::SearchQueryTooLarge)
        ));
        for _ in 0..60 {
            d.create_note("2026-08-03", "shared").unwrap();
        }
        assert_eq!(d.search_documents("2026-08-03", "").unwrap().len(), 50);
    }

    #[test]
    fn resolves_distinct_references_in_input_order() {
        let d = database();
        let daily = d.get_or_create_daily("2026-08-03").unwrap();
        let note = d.create_note("2026-08-02", "private").unwrap();
        d.update_document_body(note.id, "\n ## Current title \nbody")
            .unwrap();
        let rows = d
            .resolve_references(&[note.id, i64::MAX, daily.id, note.id])
            .unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            [note.id, daily.id]
        );
        assert_eq!(rows[0].label, "Current title");
        assert!(d.resolve_references(&[]).unwrap().is_empty());
        assert!(matches!(d.resolve_references(&[0]), Err(Error::InvalidId)));
        assert!(matches!(
            d.resolve_references(&vec![1; 201]),
            Err(Error::TooManyReferenceIds)
        ));
    }

    #[test]
    fn mcp_private_and_missing_are_indistinguishable() {
        let d = database();
        let private = d.create_note("2026-08-04", "private").unwrap();
        d.update_document_body(private.id, "SECRET TITLE body [[note:998|meta]]")
            .unwrap();
        let missing = private.id + 1000;
        assert_eq!(
            d.mcp_read_document(private.id).unwrap_err().to_string(),
            d.mcp_read_document(missing)
                .unwrap_err()
                .to_string()
                .replace(&missing.to_string(), &private.id.to_string())
        );
        assert!(d.mcp_search_documents("SECRET", 50).unwrap().is_empty());
        assert!(
            d.mcp_search_documents(&private.id.to_string(), 50)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn mcp_sanitizes_private_and_missing_references_before_read_and_search() {
        let d = database();
        let shared_target = d.create_note("2026-08-04", "shared").unwrap();
        d.update_document_body(shared_target.id, "# Visible target")
            .unwrap();
        let private_target = d.create_note("2026-08-04", "private").unwrap();
        d.update_document_body(private_target.id, "# Authoritative private title")
            .unwrap();
        let source = d.create_note("2026-08-04", "shared").unwrap();
        let missing_id = private_target.id + 10_000;
        let shared_reference = format!("[[note:{}|Visible \\| label]]", shared_target.id);
        let private_reference = format!("[[note:{}|Private stored label]]", private_target.id);
        let missing_reference = format!("[[note:{missing_id}|Missing stored label]]");
        d.update_document_body(
            source.id,
            &format!("# Source\n{shared_reference}\n{private_reference}\n{missing_reference}"),
        )
        .unwrap();

        let visible = d.mcp_read_document(source.id).unwrap().body;
        assert!(visible.contains(&shared_reference));
        for secret in [
            private_target.id.to_string(),
            missing_id.to_string(),
            "Private stored label".to_owned(),
            "Missing stored label".to_owned(),
        ] {
            assert!(!visible.contains(&secret));
        }
        assert!(
            d.mcp_search_documents("Private stored label", 50)
                .unwrap()
                .is_empty()
        );
        assert!(
            d.mcp_search_documents("Missing stored label", 50)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            d.mcp_search_documents("Visible", 50).unwrap()[0].id,
            source.id
        );
        assert_eq!(
            visible.contains(&private_target.id.to_string()),
            visible.contains(&missing_id.to_string())
        );
    }

    #[test]
    fn artifact_dedupes_and_escapes_references_atomically() {
        let d = database();
        let note = d.create_note("2026-08-04", "shared").unwrap();
        d.update_document_body(note.id, "# a|b]\\c").unwrap();
        let artifact = d
            .mcp_create_artifact("Title", "Body", &[note.id, note.id])
            .unwrap();
        assert_eq!(artifact.kind, "artifact");
        assert_eq!(artifact.author, "agent");
        assert_eq!(artifact.body.matches("[[note:").count(), 1);
        assert!(artifact.body.contains("a\\|b\\]\\\\c"));
        let before = d.mcp_search_documents("Title", 50).unwrap().len();
        assert!(d.mcp_create_artifact("Other", "", &[999999]).is_err());
        assert_eq!(d.mcp_search_documents("Other", 50).unwrap().len(), 0);
        assert_eq!(d.mcp_search_documents("Title", 50).unwrap().len(), before);
    }

    #[test]
    fn mcp_body_limits_do_not_change_gui() {
        let d = database();
        let note = d.create_note("2026-08-04", "shared").unwrap();
        d.update_document_body(note.id, &"x".repeat(MAX_BODY_BYTES + 1))
            .unwrap();
        assert!(matches!(
            d.mcp_append_to_daily("2026-08-04", &"x".repeat(MAX_BODY_BYTES + 1)),
            Err(Error::BodyTooLarge)
        ));
        assert!(matches!(
            d.mcp_create_artifact("x", &"x".repeat(MAX_BODY_BYTES + 1), &[]),
            Err(Error::BodyTooLarge)
        ));
    }

    #[test]
    fn append_uses_blank_paragraph_and_preserves_daily_contract() {
        let d = database();
        let first = d.mcp_append_to_daily("2026-08-04", "one").unwrap();
        let second = d.mcp_append_to_daily("2026-08-04", "two").unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.revision, 1);
        assert_eq!(second.revision, 2);
        assert_eq!(second.body, "one\n\ntwo");
        assert_eq!(
            (&second.visibility, &second.author),
            (&"shared".to_owned(), &"user".to_owned())
        );
    }

    #[test]
    fn revisions_poll_cross_connections_and_presence_are_exact() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        let gui = Database::open(&path).unwrap();
        let other = Database::open(&path).unwrap();
        let initial = gui.get_or_create_daily("2026-08-04").unwrap();
        assert_eq!(initial.revision, 1);
        assert!(gui.sync_document(initial.id, 1).unwrap().document.is_none());
        let changed = other
            .replace_document_body(initial.id, 1, "changed")
            .unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(
            gui.sync_document(initial.id, 1)
                .unwrap()
                .document
                .unwrap()
                .body,
            "changed"
        );
        assert!(matches!(
            gui.replace_document_body(initial.id, 1, "stale"),
            Err(Error::WriteConflict)
        ));

        gui.set_presence("user-one", "user", initial.id).unwrap();
        gui.set_agent_presence("agent-one", initial.id).unwrap();
        let snapshot = gui.sync_document(initial.id, 2).unwrap();
        assert_eq!((snapshot.user_count, snapshot.agent_present), (1, true));
        assert_eq!(gui.get_document(initial.id).unwrap().revision, 2);
        let note = gui.create_note("2026-08-04", "shared").unwrap();
        gui.set_presence("user-one", "user", note.id).unwrap();
        assert_eq!(gui.sync_document(initial.id, 2).unwrap().user_count, 0);
        gui.remove_presence("agent-one").unwrap();
        assert!(!gui.sync_document(initial.id, 2).unwrap().agent_present);
    }

    #[test]
    fn stale_presence_expires_and_agents_cannot_claim_private_documents() {
        let d = database();
        let shared = d.create_note("2026-08-04", "shared").unwrap();
        let private = d.create_note("2026-08-04", "private").unwrap();
        d.set_presence("stale", "user", shared.id).unwrap();
        d.connection
            .lock()
            .unwrap()
            .execute("UPDATE presence SET last_heartbeat=unixepoch()-11", [])
            .unwrap();
        assert_eq!(d.sync_document(shared.id, 1).unwrap().user_count, 0);
        assert!(matches!(
            d.set_agent_presence("agent", private.id),
            Err(Error::MissingDocument(_))
        ));
        assert!(matches!(
            d.set_agent_presence("agent", private.id + 1000),
            Err(Error::MissingDocument(_))
        ));
        d.set_presence("private-user", "user", private.id).unwrap();
        assert_eq!(d.sync_document(private.id, 1).unwrap().user_count, 1);
    }

    #[test]
    fn invalid_mermaid_rejects_create_and_append_without_mutation() {
        let d = database();
        let invalid = "```mermaid\nflowchart TD\nA-->\n```";
        let create_error = d.mcp_create_artifact("Rejected", invalid, &[]).unwrap_err();
        assert!(create_error.to_string().contains("Mermaid block 1"));
        assert!(d.mcp_search_documents("Rejected", 50).unwrap().is_empty());

        let daily = d.mcp_append_to_daily("2026-08-04", "before").unwrap();
        let append_error = d.mcp_append_to_daily("2026-08-04", invalid).unwrap_err();
        assert!(append_error.to_string().contains("Mermaid block 1"));
        assert_eq!(d.get_document(daily.id).unwrap().body, "before");
    }

    #[test]
    fn valid_mermaid_artifact_round_trips_and_existing_daily_is_not_revalidated() {
        let d = database();
        let body = "```mermaid\nsequenceDiagram\nAlice->>Bob: Hi\n```";
        let artifact = d.mcp_create_artifact("Diagram", body, &[]).unwrap();
        assert_eq!(
            d.mcp_read_document(artifact.id).unwrap().body,
            artifact.body
        );

        let daily = d.get_or_create_daily("2026-08-04").unwrap();
        d.update_document_body(daily.id, "```mermaid\ninvalid")
            .unwrap();
        assert!(
            d.mcp_append_to_daily("2026-08-04", "plain addition")
                .is_ok()
        );
    }

    #[test]
    fn gui_replace_rejects_a_stale_body_after_mcp_append() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        let gui = Database::open(&path).unwrap();
        let mcp = Database::open(&path).unwrap();
        let daily = gui.get_or_create_daily("2026-08-04").unwrap();
        gui.update_document_body(daily.id, "user text").unwrap();
        mcp.mcp_append_to_daily("2026-08-04", "agent append")
            .unwrap();

        assert!(matches!(
            gui.replace_document_body(daily.id, 2, "user edit"),
            Err(Error::WriteConflict)
        ));
        assert_eq!(
            gui.get_document(daily.id).unwrap().body,
            "user text\n\nagent append"
        );
    }
}
