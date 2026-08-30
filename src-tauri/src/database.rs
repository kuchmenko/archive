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

const SCHEMA_VERSION: i64 = 7;
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceSummary {
    pub id: i64,
    pub label: String,
}

#[derive(Debug)]
pub enum Error {
    InvalidDay,
    InvalidId,
    InvalidTitle,
    InvalidStatus,
    BodyTooLarge,
    TooManyReferenceIds,
    SearchQueryTooLarge,
    InvalidLimit,
    InvalidMermaid(String),
    MissingDocument(i64),
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
            Self::InvalidTitle => write!(
                formatter,
                "title must be a non-empty single line of at most {MAX_TITLE_BYTES} bytes"
            ),
            Self::InvalidStatus => {
                write!(formatter, "status must be completed, blocked, or failed")
            }
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
            Self::InvalidMermaid(message) => message.fmt(formatter),
            Self::MissingDocument(id) => write!(formatter, "document {id} does not exist"),
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

    pub fn mcp_project_context(
        &self,
        project_id: i64,
        limit: usize,
    ) -> Result<(Document, Vec<Document>), Error> {
        validate_id(project_id)?;
        if !(1..=50).contains(&limit) {
            return Err(Error::InvalidLimit);
        }
        let connection = self.connection()?;
        let project = get_shared_document_from(&connection, project_id)?
            .ok_or(Error::MissingDocument(project_id))?;
        if project.kind != "project" {
            return Err(Error::MissingDocument(project_id));
        }
        let project = sanitize_mcp_document(&connection, project)?;
        let documents = project_documents(&connection, project_id, limit)?;
        Ok((project, sanitize_mcp_documents(&connection, documents)?))
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
        project_id: Option<i64>,
    ) -> Result<Document, Error> {
        validate_title(title)?;
        validate_body_size(body)?;
        crate::merman::validate_markdown_fences(body).map_err(Error::InvalidMermaid)?;
        let distinct_ids = distinct_valid_ids(related_ids)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_mcp_project(&transaction, project_id)?;
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
        associate_mcp_project(&transaction, project_id, id)?;
        let document = get_document_from(&transaction, id)?.ok_or(Error::MissingDocument(id))?;
        let document = sanitize_mcp_document(&transaction, document)?;
        transaction.commit()?;
        Ok(document)
    }

    pub fn mcp_create_daily_attachment(
        &self,
        day: &str,
        title: &str,
        body: &str,
        status: Option<&str>,
        project_id: Option<i64>,
    ) -> Result<Document, Error> {
        validate_day(day)?;
        validate_title(title)?;
        validate_body_size(body)?;
        let status = status.unwrap_or("completed");
        validate_status(status)?;
        crate::merman::validate_markdown_fences(body).map_err(Error::InvalidMermaid)?;
        let mut markdown = format!("# {}", title.trim());
        if !body.is_empty() {
            markdown.push_str("\n\n");
            markdown.push_str(body);
        }
        validate_body_size(&markdown)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_mcp_project(&transaction, project_id)?;
        let timestamp = now();
        transaction.execute(
            "INSERT INTO documents (kind, visibility, author, day, created_at, updated_at, body)
             VALUES ('daily', 'shared', 'user', ?1, ?2, ?2, '')
             ON CONFLICT(day) WHERE kind = 'daily' DO NOTHING",
            params![day, timestamp],
        )?;
        let daily = get_daily(&transaction, day)?.ok_or(Error::MissingDocument(0))?;
        transaction.execute(
            "INSERT INTO documents (kind, visibility, author, day, created_at, updated_at, body)
             VALUES ('artifact', 'shared', 'agent', ?1, ?2, ?2, ?3)",
            params![day, timestamp, markdown],
        )?;
        let artifact_id = transaction.last_insert_rowid();
        associate_mcp_project(&transaction, project_id, artifact_id)?;
        transaction.execute(
            "INSERT INTO document_attachments (parent_document_id, attached_document_id, status)
             VALUES (?1, ?2, ?3)",
            params![daily.id, artifact_id, status],
        )?;
        let document = get_document_from(&transaction, artifact_id)?
            .ok_or(Error::MissingDocument(artifact_id))?;
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
             PRAGMA user_version = 6;"
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
fn validate_status(status: &str) -> Result<(), Error> {
    if matches!(status, "completed" | "blocked" | "failed") {
        Ok(())
    } else {
        Err(Error::InvalidStatus)
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
    Ok(ReferenceSummary { id, label })
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
fn project_documents(
    connection: &Connection,
    project_id: i64,
    limit: usize,
) -> Result<Vec<Document>, Error> {
    let mut statement = connection.prepare(
        "SELECT d.id, d.kind, d.visibility, d.author, d.day, d.created_at, d.updated_at, d.body, d.revision
         FROM project_documents pd JOIN documents d ON d.id = pd.document_id
         WHERE pd.project_document_id = ?1 AND d.visibility = 'shared'
         ORDER BY d.updated_at DESC, d.id DESC LIMIT ?2",
    )?;
    Ok(statement
        .query_map(params![project_id, limit], document_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}
fn validate_mcp_project(connection: &Connection, project_id: Option<i64>) -> Result<(), Error> {
    let Some(id) = project_id else {
        return Ok(());
    };
    validate_id(id)?;
    let project = get_shared_document_from(connection, id)?.ok_or(Error::MissingDocument(id))?;
    if project.kind != "project" {
        return Err(Error::MissingDocument(id));
    }
    Ok(())
}
fn associate_mcp_project(
    connection: &Connection,
    project_id: Option<i64>,
    document_id: i64,
) -> Result<(), Error> {
    if let Some(project_id) = project_id {
        connection.execute("INSERT INTO project_documents(project_document_id, document_id, added_by, created_at) VALUES(?1, ?2, 'agent', ?3)", params![project_id, document_id, now()])?;
    }
    Ok(())
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

    fn insert_document(
        database: &Database,
        kind: &str,
        visibility: &str,
        author: &str,
        day: &str,
        body: &str,
    ) -> Document {
        let connection = database.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body)
             VALUES(?1,?2,?3,?4,'a','a',?5)",
                params![kind, visibility, author, day, body],
            )
            .unwrap();
        let id = connection.last_insert_rowid();
        get_document_from(&connection, id).unwrap().unwrap()
    }

    fn document(database: &Database, id: i64) -> Document {
        get_document_from(&database.connection.lock().unwrap(), id)
            .unwrap()
            .unwrap()
    }

    fn add_to_project(database: &Database, project_id: i64, document_id: i64) {
        database
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO project_documents(project_document_id,document_id,added_by,created_at)
             VALUES(?1,?2,'user','a')",
                [project_id, document_id],
            )
            .unwrap();
    }

    #[test]
    fn fresh_schema_is_v7_with_constraints() {
        let database = database();
        let connection = database.connection.lock().unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 7);
        assert!(connection.execute("INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body) VALUES('daily','private','user','2026-01-01','a','a','')", []).is_err());
        assert!(connection.execute("INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body) VALUES('daily','shared','agent','2026-01-01','a','a','')", []).is_err());
        assert!(connection.execute("INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body) VALUES('project','shared','agent','2026-01-01','a','a','')", []).is_err());
        connection.execute("INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body) VALUES('project','shared','user','2026-01-01','a','a','')", []).unwrap();
        let table: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='document_attachments'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(table.contains("completed"));
        assert!(table.contains("blocked"));
        assert!(table.contains("failed"));
        assert!(table.contains("reviewed_at"));
    }

    #[test]
    fn v4_migration_adds_document_attachments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        {
            let mut connection = Connection::open(&path).unwrap();
            {
                let tx = connection.transaction().unwrap();
                create_v3_schema(&tx).unwrap();
                tx.execute(
                    "INSERT INTO documents(kind,visibility,author,day,created_at,updated_at,body)
                     VALUES('daily','shared','user','2026-08-04','a','b','user body')",
                    [],
                )
                .unwrap();
                tx.execute_batch(
                    "ALTER TABLE documents ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
                     CREATE TABLE presence (
                        session_id TEXT PRIMARY KEY,
                        actor TEXT NOT NULL CHECK(actor IN ('user', 'agent')),
                        document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                        last_heartbeat INTEGER NOT NULL
                     );
                     PRAGMA user_version = 4;",
                )
                .unwrap();
                tx.commit().unwrap();
            }
        }
        let database = Database::open(&path).unwrap();
        let version: i64 = database
            .connection
            .lock()
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 7);
        assert_eq!(document(&database, 1).body, "user body");
        assert_eq!(
            database
                .connection
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM document_attachments", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }

    #[test]
    fn v5_migration_preserves_documents_children_ids_and_reopens() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE documents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL CHECK(kind IN ('daily','note','artifact')), visibility TEXT NOT NULL CHECK(visibility IN ('shared','private')), author TEXT NOT NULL CHECK(author IN ('user','agent')), day TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1, CHECK(kind <> 'daily' OR (visibility='shared' AND author='user')));
             CREATE UNIQUE INDEX documents_daily_day ON documents(day) WHERE kind='daily';
             CREATE INDEX documents_day ON documents(day);
             CREATE TABLE presence (session_id TEXT PRIMARY KEY, actor TEXT NOT NULL CHECK(actor IN ('user','agent')), document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, last_heartbeat INTEGER NOT NULL);
             CREATE INDEX presence_document ON presence(document_id);
             CREATE TABLE document_attachments (parent_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, attached_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, status TEXT NOT NULL CHECK(status IN ('completed','blocked','failed')), PRIMARY KEY(parent_document_id,attached_document_id), UNIQUE(attached_document_id));
             CREATE INDEX document_attachments_parent ON document_attachments(parent_document_id);
             INSERT INTO documents VALUES(3,'daily','shared','user','2026-08-04','a','b','daily',4);
             INSERT INTO documents VALUES(7,'note','private','user','2026-08-04','c','d','note',2);
             INSERT INTO documents VALUES(11,'artifact','shared','agent','2026-08-04','e','f','artifact',3);
             INSERT INTO documents VALUES(50,'note','shared','user','2026-08-04','g','h','deleted',1);
             DELETE FROM documents WHERE id=50;
             INSERT INTO presence VALUES('session','user',7,123);
             INSERT INTO document_attachments VALUES(3,11,'blocked');
             PRAGMA user_version=5;"
        ).unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        assert_eq!(document(&database, 3).body, "daily");
        assert_eq!(document(&database, 7).revision, 2);
        assert_eq!(document(&database, 11).author, "agent");
        let connection = database.connection.lock().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='documents'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            50
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM presence WHERE session_id='session' AND document_id=7",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(connection.query_row("SELECT count(*) FROM document_attachments WHERE parent_document_id=3 AND attached_document_id=11 AND status='blocked'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        drop(connection);
        let project = insert_document(&database, "project", "shared", "user", "2026-08-04", "");
        assert!(project.id > 50);
        drop(database);
        let reopened = Database::open(&path).unwrap();
        assert_eq!(document(&reopened, project.id), project);
        assert_eq!(
            reopened.connection.lock().unwrap().query_row(
                "SELECT attached_document_id FROM document_attachments WHERE parent_document_id=3",
                [],
                |row| row.get::<_, i64>(0),
            ).unwrap(),
            11
        );
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
        let first = document(&database, 3);
        assert_eq!(first.body, "first\n\n\nsecond");
        assert_eq!(first.created_at, "2026-08-02T08:00:00Z");
        assert_eq!(first.updated_at, "2026-08-06T00:00:00Z");
        assert_eq!(
            (&first.visibility, &first.author),
            (&"shared".to_owned(), &"user".to_owned())
        );
        assert_eq!(document(&database, 20).body.len(), 1_200_002);
        drop(database);
        assert_eq!(document(&Database::open(&path).unwrap(), 3), first);
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
        let row = document(&database, 7);
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
        assert_eq!(document(&database, 1).revision, 1);
    }

    #[test]
    fn mcp_project_context_is_private_safe_sanitized_and_bounded() {
        let d = database();
        let private = insert_document(
            &d,
            "note",
            "private",
            "user",
            "2026-08-04",
            "# Secret member",
        );
        let shared = insert_document(
            &d,
            "note",
            "shared",
            "user",
            "2026-08-04",
            &format!("# Shared\n[[note:{}|Secret label]]", private.id),
        );
        let project = insert_document(
            &d,
            "project",
            "shared",
            "user",
            "2026-08-04",
            &format!("# Project\n[[note:{}|Private project label]]", private.id),
        );
        add_to_project(&d, project.id, shared.id);
        add_to_project(&d, project.id, private.id);
        let (visible_project, members) = d.mcp_project_context(project.id, 1).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].id, shared.id);
        for secret in [
            private.id.to_string(),
            "Secret label".into(),
            "Private project label".into(),
        ] {
            assert!(!visible_project.body.contains(&secret));
            assert!(!members[0].body.contains(&secret));
        }
        assert!(matches!(
            d.mcp_project_context(project.id, 0),
            Err(Error::InvalidLimit)
        ));
        assert!(matches!(
            d.mcp_project_context(project.id, 51),
            Err(Error::InvalidLimit)
        ));
        let private_project = insert_document(&d, "project", "private", "user", "2026-08-04", "");
        assert!(matches!(
            d.mcp_project_context(private_project.id, 20),
            Err(Error::MissingDocument(_))
        ));
        assert!(matches!(
            d.mcp_project_context(9999, 20),
            Err(Error::MissingDocument(_))
        ));
    }

    #[test]
    fn invalid_mcp_project_associations_roll_back_every_related_row() {
        let d = database();
        let private = insert_document(&d, "project", "private", "user", "2026-08-04", "");
        let note = insert_document(&d, "note", "shared", "user", "2026-08-04", "");
        for project_id in [Some(private.id), Some(note.id), Some(9999)] {
            assert!(matches!(
                d.mcp_create_artifact("Rejected", "body", &[], project_id),
                Err(Error::MissingDocument(_))
            ));
            assert!(matches!(
                d.mcp_create_daily_attachment("2026-08-04", "Rejected", "body", None, project_id),
                Err(Error::MissingDocument(_))
            ));
        }
        let connection = d.connection.lock().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM documents WHERE body LIKE '# Rejected%'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM document_attachments", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM project_documents", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn mcp_private_and_missing_are_indistinguishable() {
        let d = database();
        let private = insert_document(
            &d,
            "note",
            "private",
            "user",
            "2026-08-04",
            "SECRET TITLE body [[note:998|meta]]",
        );
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
        let shared_target = insert_document(
            &d,
            "note",
            "shared",
            "user",
            "2026-08-04",
            "# Visible target",
        );
        let private_target = insert_document(
            &d,
            "note",
            "private",
            "user",
            "2026-08-04",
            "# Authoritative private title",
        );
        let missing_id = private_target.id + 10_000;
        let shared_reference = format!("[[note:{}|Visible \\| label]]", shared_target.id);
        let private_reference = format!("[[note:{}|Private stored label]]", private_target.id);
        let missing_reference = format!("[[note:{missing_id}|Missing stored label]]");
        let source = insert_document(
            &d,
            "note",
            "shared",
            "user",
            "2026-08-04",
            &format!("# Source\n{shared_reference}\n{private_reference}\n{missing_reference}"),
        );

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
        let note = insert_document(&d, "note", "shared", "user", "2026-08-04", "# a|b]\\c");
        let artifact = d
            .mcp_create_artifact("Title", "Body", &[note.id, note.id], None)
            .unwrap();
        assert_eq!(artifact.kind, "artifact");
        assert_eq!(artifact.author, "agent");
        assert_eq!(artifact.body.matches("[[note:").count(), 1);
        assert!(artifact.body.contains("a\\|b\\]\\\\c"));
        let before = d.mcp_search_documents("Title", 50).unwrap().len();
        assert!(d.mcp_create_artifact("Other", "", &[999999], None).is_err());
        assert_eq!(d.mcp_search_documents("Other", 50).unwrap().len(), 0);
        assert_eq!(d.mcp_search_documents("Title", 50).unwrap().len(), before);
    }

    #[test]
    fn mcp_body_limits_are_enforced() {
        let d = database();
        assert!(matches!(
            d.mcp_create_daily_attachment(
                "2026-08-04",
                "x",
                &"x".repeat(MAX_BODY_BYTES + 1),
                None,
                None
            ),
            Err(Error::BodyTooLarge)
        ));
        assert!(matches!(
            d.mcp_create_artifact("x", &"x".repeat(MAX_BODY_BYTES + 1), &[], None),
            Err(Error::BodyTooLarge)
        ));
    }

    #[test]
    fn daily_attachment_leaves_daily_body_untouched() {
        let d = database();
        let daily = insert_document(&d, "daily", "shared", "user", "2026-08-04", "user thoughts");
        let first = d
            .mcp_create_daily_attachment("2026-08-04", "Run A", "details a", Some("blocked"), None)
            .unwrap();
        let second = d
            .mcp_create_daily_attachment("2026-08-04", "Run B", "details b", Some("failed"), None)
            .unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(first.kind, "artifact");
        assert_eq!(first.author, "agent");
        assert_eq!(first.body, "# Run A\n\ndetails a");
        assert_eq!(document(&d, daily.id).body, "user thoughts");
        assert_eq!(document(&d, daily.id).revision, 1);
        let connection = d.connection.lock().unwrap();
        let mut statement = connection
            .prepare(
                "SELECT attached_document_id,status FROM document_attachments
             WHERE parent_document_id=?1 ORDER BY attached_document_id",
            )
            .unwrap();
        let attachments = statement
            .query_map([daily.id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            attachments,
            vec![(first.id, "blocked".into()), (second.id, "failed".into())]
        );
        drop(statement);
        drop(connection);
        assert!(matches!(
            d.mcp_create_daily_attachment("2026-08-04", "Bad", "x", Some("running"), None),
            Err(Error::InvalidStatus)
        ));
    }

    #[test]
    fn v6_attachment_migrates_unreviewed_and_reopens() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(
            "CREATE TABLE documents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, visibility TEXT NOT NULL, author TEXT NOT NULL, day TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1);
             CREATE UNIQUE INDEX documents_daily_day ON documents(day) WHERE kind='daily';
             CREATE INDEX documents_day ON documents(day);
             CREATE TABLE presence (session_id TEXT PRIMARY KEY, actor TEXT NOT NULL, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, last_heartbeat INTEGER NOT NULL);
             CREATE INDEX presence_document ON presence(document_id);
             CREATE TABLE document_attachments (parent_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, attached_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, status TEXT NOT NULL, PRIMARY KEY(parent_document_id,attached_document_id), UNIQUE(attached_document_id));
             CREATE INDEX document_attachments_parent ON document_attachments(parent_document_id);
             CREATE TABLE project_documents (project_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, added_by TEXT NOT NULL, created_at TEXT NOT NULL, PRIMARY KEY(project_document_id,document_id));
             CREATE INDEX project_documents_document ON project_documents(document_id);
             INSERT INTO documents VALUES(3,'daily','shared','user','2026-08-01','a','b','daily',2);
             INSERT INTO documents VALUES(8,'artifact','shared','agent','2026-08-01','c','d','# Work',4);
             INSERT INTO document_attachments VALUES(3,8,'blocked');
             PRAGMA user_version=6;",
        ).unwrap();
        drop(connection);
        let database = Database::open(&path).unwrap();
        assert_eq!(
            database
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT attached_document_id,status,reviewed_at FROM document_attachments",
                    [],
                    |row| Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?
                    )),
                )
                .unwrap(),
            (8, "blocked".to_owned(), None)
        );
        drop(database);
        let reopened = Database::open(&path).unwrap();
        assert_eq!(
            reopened.connection.lock().unwrap().query_row(
                "SELECT attached_document_id FROM document_attachments WHERE reviewed_at IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            ).unwrap(),
            8
        );
    }

    #[test]
    fn invalid_mermaid_rejects_create_and_attachment_without_mutation() {
        let d = database();
        let invalid = "```mermaid\nflowchart TD\nA-->\n```";
        let create_error = d
            .mcp_create_artifact("Rejected", invalid, &[], None)
            .unwrap_err();
        assert!(create_error.to_string().contains("Mermaid block 1"));
        assert!(d.mcp_search_documents("Rejected", 50).unwrap().is_empty());

        let daily = insert_document(&d, "daily", "shared", "user", "2026-08-04", "before");
        let attach_error = d
            .mcp_create_daily_attachment("2026-08-04", "Rejected", invalid, None, None)
            .unwrap_err();
        assert!(attach_error.to_string().contains("Mermaid block 1"));
        assert_eq!(document(&d, daily.id).body, "before");
        assert_eq!(
            d.connection
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM document_attachments", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }

    #[test]
    fn valid_mermaid_artifact_round_trips_and_existing_daily_is_not_revalidated() {
        let d = database();
        let body = "```mermaid\nsequenceDiagram\nAlice->>Bob: Hi\n```";
        let artifact = d.mcp_create_artifact("Diagram", body, &[], None).unwrap();
        assert_eq!(
            d.mcp_read_document(artifact.id).unwrap().body,
            artifact.body
        );

        let daily = insert_document(
            &d,
            "daily",
            "shared",
            "user",
            "2026-08-04",
            "```mermaid\ninvalid",
        );
        assert!(
            d.mcp_create_daily_attachment("2026-08-04", "Plain", "plain addition", None, None)
                .is_ok()
        );
        assert_eq!(document(&d, daily.id).body, "```mermaid\ninvalid");
    }
}
