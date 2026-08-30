use std::{collections::BTreeMap, path::Path};

use rusqlite::Connection;

use super::*;

fn database() -> Database {
    Database::from_connection(Connection::open_in_memory().unwrap()).unwrap()
}

fn context(key: &str) -> WriteContext {
    WriteContext {
        idempotency_key: key.to_owned(),
        actor: "agent-a".to_owned(),
        thread: "thread-a".to_owned(),
        client: "archive-tests".to_owned(),
    }
}

fn source() -> SourceInput {
    SourceInput {
        identity: "source-a".to_owned(),
        locator: Some("file:///tmp/source.txt".to_owned()),
        version: Some("v1".to_owned()),
        content_hash: Some("sha256:abc".to_owned()),
        anchor: Some("line:1".to_owned()),
        quote: Some("quoted text".to_owned()),
    }
}

fn note(scope_id: i64, label_ids: Vec<i64>, title: &str, body: &str) -> RecordInput {
    RecordInput {
        scope_id,
        title: title.to_owned(),
        payload: RecordPayload::Note {
            body: body.to_owned(),
        },
        sources: Vec::new(),
        label_ids,
    }
}

fn create_note(database: &Database, key: &str, title: &str, body: &str) -> Record {
    database
        .create_record(&note(1, vec![1], title, body), &context(key))
        .unwrap()
}

#[test]
fn fresh_schema_is_v8_and_seeds_global_inbox() {
    let database = database();
    let connection = database.connection.lock().unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 8);
    assert_eq!(
        connection
            .query_row("SELECT name FROM scopes WHERE id=1", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "global"
    );
    assert_eq!(
        connection
            .query_row("SELECT facet||':'||key FROM labels WHERE id=1", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "workflow:inbox"
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM documents", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn every_payload_kind_round_trips_with_required_sources() {
    let database = database();
    let mut dimensions = BTreeMap::new();
    dimensions.insert("host".to_owned(), "local".to_owned());
    let payloads = vec![
        (
            RecordPayload::Note {
                body: "body".to_owned(),
            },
            Vec::new(),
        ),
        (
            RecordPayload::Observation {
                statement: "observed".to_owned(),
                observed_at: Some("2026-08-30T10:00:00Z".to_owned()),
            },
            vec![source()],
        ),
        (
            RecordPayload::Decision {
                choice: "choose A".to_owned(),
                question: Some("A or B?".to_owned()),
                rationale: Some("A is direct".to_owned()),
                decided_at: Some("2026-08-30T10:00:00Z".to_owned()),
            },
            Vec::new(),
        ),
        (
            RecordPayload::Idea {
                proposal: "try A".to_owned(),
            },
            Vec::new(),
        ),
        (
            RecordPayload::Snippet {
                language: "rust".to_owned(),
                code: "fn main() {}".to_owned(),
                origin: SnippetOrigin::Imported,
                runtime: Some("stable".to_owned()),
                dependencies: Some(vec!["serde".to_owned()]),
            },
            vec![source()],
        ),
        (
            RecordPayload::Snippet {
                language: "rust".to_owned(),
                code: "fn generated() {}".to_owned(),
                origin: SnippetOrigin::Generated,
                runtime: None,
                dependencies: None,
            },
            Vec::new(),
        ),
        (
            RecordPayload::Metric {
                name: "latency".to_owned(),
                value: 12.5,
                unit: "ms".to_owned(),
                observed_at: None,
                interval: Some(crate::model::MetricInterval {
                    start: "2026-08-30T10:00:00Z".to_owned(),
                    end: "2026-08-30T11:00:00Z".to_owned(),
                }),
                dimensions,
                method: Some("wall clock".to_owned()),
            },
            vec![source()],
        ),
        (
            RecordPayload::Evidence {
                claim: "change works".to_owned(),
                action: Some("ran test".to_owned()),
                outcome: Some("passed".to_owned()),
                impact: Some("behavior verified".to_owned()),
            },
            vec![source()],
        ),
    ];
    for (index, (payload, sources)) in payloads.into_iter().enumerate() {
        let input = RecordInput {
            scope_id: 1,
            title: format!("record {index}"),
            payload: payload.clone(),
            sources: sources.clone(),
            label_ids: vec![1],
        };
        let record = database
            .create_record(&input, &context(&format!("payload-{index}")))
            .unwrap();
        assert_eq!(record.current.payload, payload);
        assert_eq!(record.current.sources.len(), sources.len());
        assert_eq!(record.kind, record.current.payload.kind());
        assert_eq!(record.labels[0].canonical, "workflow:inbox");
    }
}

#[test]
fn source_requirements_and_payload_invariants_are_enforced() {
    let database = database();
    for (index, payload) in [
        RecordPayload::Observation {
            statement: "observed".to_owned(),
            observed_at: None,
        },
        RecordPayload::Metric {
            name: "count".to_owned(),
            value: 1.0,
            unit: "items".to_owned(),
            observed_at: None,
            interval: None,
            dimensions: BTreeMap::new(),
            method: None,
        },
        RecordPayload::Evidence {
            claim: "claim".to_owned(),
            action: None,
            outcome: None,
            impact: None,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let input = RecordInput {
            scope_id: 1,
            title: "missing source".to_owned(),
            payload,
            sources: Vec::new(),
            label_ids: vec![1],
        };
        assert!(
            database
                .create_record(&input, &context(&format!("missing-source-{index}")))
                .unwrap_err()
                .to_string()
                .contains("requires at least one source")
        );
    }
    let imported = RecordInput {
        scope_id: 1,
        title: "imported".to_owned(),
        payload: RecordPayload::Snippet {
            language: "text".to_owned(),
            code: "content".to_owned(),
            origin: SnippetOrigin::Imported,
            runtime: None,
            dependencies: None,
        },
        sources: vec![SourceInput {
            content_hash: None,
            ..source()
        }],
        label_ids: vec![1],
    };
    assert!(
        database
            .create_record(&imported, &context("import-missing-hash"))
            .unwrap_err()
            .to_string()
            .contains("locator and content_hash")
    );
    let invalid_time = RecordInput {
        scope_id: 1,
        title: "invalid time".to_owned(),
        payload: RecordPayload::Observation {
            statement: "observed".to_owned(),
            observed_at: Some("yesterday".to_owned()),
        },
        sources: vec![source()],
        label_ids: vec![1],
    };
    assert!(
        database
            .create_record(&invalid_time, &context("invalid-time"))
            .unwrap_err()
            .to_string()
            .contains("RFC 3339")
    );
    let no_labels = note(1, Vec::new(), "no labels", "body");
    assert!(
        database
            .create_record(&no_labels, &context("no-labels"))
            .unwrap_err()
            .to_string()
            .contains("at least one active label")
    );
    for (key, input) in [
        ("empty-note", note(1, vec![1], "empty note", "")),
        (
            "empty-snippet",
            RecordInput {
                scope_id: 1,
                title: "empty snippet".to_owned(),
                payload: RecordPayload::Snippet {
                    language: "rust".to_owned(),
                    code: "".to_owned(),
                    origin: SnippetOrigin::Generated,
                    runtime: None,
                    dependencies: None,
                },
                sources: Vec::new(),
                label_ids: vec![1],
            },
        ),
    ] {
        assert!(
            database
                .create_record(&input, &context(key))
                .unwrap_err()
                .to_string()
                .contains("must not be empty")
        );
    }
}

#[test]
fn writes_are_transactionally_idempotent_and_keys_reject_different_requests() {
    let database = database();
    let input = note(1, vec![1], "idempotent", "same body");
    let first = database
        .create_record(&input, &context("same-create"))
        .unwrap();
    let second = database
        .create_record(&input, &context("same-create"))
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(
        database
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM records", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(
        database
            .create_record(
                &note(1, vec![1], "different", "body"),
                &context("same-create")
            )
            .unwrap_err()
            .to_string()
            .contains("different write")
    );
}

#[test]
fn revisions_are_immutable_conflict_checked_and_history_is_explicit() {
    let database = database();
    let record = create_note(&database, "revision-create", "First", "old searchable text");
    let revised = database
        .revise_record(
            record.id,
            1,
            "Second",
            &RecordPayload::Note {
                body: "new searchable text".to_owned(),
            },
            &[],
            "correct wording",
            &context("revision-two"),
        )
        .unwrap();
    assert_eq!(revised.current_revision, 2);
    assert_eq!(revised.current.reason, "correct wording");
    assert!(
        database
            .revise_record(
                record.id,
                1,
                "Third",
                &RecordPayload::Note {
                    body: "conflict".to_owned()
                },
                &[],
                "stale edit",
                &context("revision-conflict")
            )
            .unwrap_err()
            .to_string()
            .contains("current 2")
    );
    let read = database.read_record(record.id, true).unwrap();
    assert_eq!(read.history.len(), 2);
    assert_eq!(read.history[0].title, "First");
    assert_eq!(read.history[1].title, "Second");
    assert!(
        database
            .search_records(
                Some("old searchable"),
                1,
                false,
                &[],
                &[],
                &[],
                false,
                None,
                20
            )
            .unwrap()
            .records
            .is_empty()
    );
    let current = database
        .search_records(
            Some("new searchable"),
            1,
            false,
            &[],
            &[],
            &[],
            true,
            None,
            20,
        )
        .unwrap();
    assert_eq!(current.records[0].record.history.len(), 2);
}

#[test]
fn scopes_global_opt_in_filters_and_pagination_are_deterministic() {
    let database = database();
    let work = database
        .create_scope("work", &context("scope-work"))
        .unwrap();
    let global = create_note(&database, "global-record", "Global", "shared term");
    let mut work_ids = Vec::new();
    for index in 0..3 {
        let record = database
            .create_record(
                &note(work.id, vec![1], &format!("Work {index}"), "shared term"),
                &context(&format!("work-record-{index}")),
            )
            .unwrap();
        work_ids.push(record.id);
    }
    let exact = database
        .search_records(
            Some("shared term"),
            work.id,
            false,
            &[],
            &[],
            &[],
            false,
            None,
            2,
        )
        .unwrap();
    assert_eq!(exact.records.len(), 2);
    assert!(
        exact
            .records
            .iter()
            .all(|hit| hit.record.scope.id == work.id)
    );
    let second = database
        .search_records(
            Some("shared term"),
            work.id,
            false,
            &[],
            &[],
            &[],
            false,
            exact.next_before_id,
            2,
        )
        .unwrap();
    assert_eq!(second.records.len(), 1);
    let mut paged = exact
        .records
        .iter()
        .chain(&second.records)
        .map(|hit| hit.record.id)
        .collect::<Vec<_>>();
    let mut expected = work_ids;
    expected.sort_unstable_by(|left, right| right.cmp(left));
    assert_eq!(paged, expected);
    paged.clear();
    let with_global = database
        .search_records(
            Some("shared term"),
            work.id,
            true,
            &[RecordKind::Note],
            &[Lifecycle::Active],
            &[1],
            false,
            None,
            20,
        )
        .unwrap();
    assert_eq!(with_global.records.len(), 4);
    assert!(
        with_global
            .records
            .iter()
            .any(|hit| hit.record.id == global.id)
    );
    assert!(
        with_global.records[0]
            .match_explanation
            .contains(&"fts:title_or_payload".to_owned())
    );
}

#[test]
fn controlled_labels_support_alias_search_and_append_only_retraction() {
    let database = database();
    let rust = database
        .create_label(
            "topic",
            "rust",
            "Rust",
            &["rust-lang".to_owned(), "Rust Language".to_owned()],
            &context("label-rust"),
        )
        .unwrap();
    let again = database
        .create_label(
            "topic",
            "rust",
            "Ignored duplicate display",
            &["compiler".to_owned()],
            &context("label-rust-again"),
        )
        .unwrap();
    assert_eq!(rust.id, again.id);
    assert_eq!(
        database.search_labels("rust language", None, 20).unwrap()[0].id,
        rust.id
    );
    let record = create_note(&database, "labels-record", "Labels", "body");
    let labeled = database
        .add_label(
            record.id,
            rust.id,
            "classify explicitly",
            &context("label-add"),
        )
        .unwrap();
    assert_eq!(labeled.labels.len(), 2);
    let retracted = database
        .retract_label(record.id, 1, "organized", &context("label-retract-inbox"))
        .unwrap();
    assert_eq!(retracted.labels.len(), 1);
    assert_eq!(retracted.labels[0].id, rust.id);
    assert!(
        database
            .retract_label(
                record.id,
                rust.id,
                "remove final",
                &context("label-retract-final")
            )
            .unwrap_err()
            .to_string()
            .contains("retain at least one")
    );
    let connection = database.connection.lock().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM label_assertions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row("SELECT reason FROM label_retractions", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "organized"
    );
}

#[test]
fn controlled_relations_are_cross_scope_append_only_and_retractable() {
    let database = database();
    let work = database
        .create_scope("work", &context("relations-scope"))
        .unwrap();
    let source = create_note(&database, "relations-source", "Source", "body");
    let target = database
        .create_record(
            &note(work.id, vec![1], "Target", "body"),
            &context("relations-target"),
        )
        .unwrap();
    let kinds = [
        DirectRelationKind::References,
        DirectRelationKind::Mentions,
        DirectRelationKind::DerivedFrom,
        DirectRelationKind::Supports,
        DirectRelationKind::Contradicts,
        DirectRelationKind::Summarizes,
    ];
    let mut ids = Vec::new();
    for (index, kind) in kinds.iter().enumerate() {
        ids.push(
            database
                .add_relation(
                    source.id,
                    target.id,
                    kind,
                    "explicit relation",
                    &context(&format!("relation-{index}")),
                )
                .unwrap()
                .id,
        );
    }
    assert_eq!(database.list_relations(source.id, false).unwrap().len(), 6);
    let retracted = database
        .retract_relation(ids[0], "not relevant", &context("relation-retract"))
        .unwrap();
    assert!(retracted.retracted.is_some());
    assert_eq!(database.list_relations(source.id, false).unwrap().len(), 5);
    assert_eq!(database.list_relations(source.id, true).unwrap().len(), 6);
}

#[test]
fn supersede_merge_and_retract_preserve_identity_and_history() {
    let database = database();
    let old = create_note(&database, "supersede-old", "Old", "old");
    let replacement = database
        .supersede_record(
            old.id,
            &note(1, vec![1], "Replacement", "new"),
            "semantic replacement",
            &context("supersede"),
        )
        .unwrap();
    assert_ne!(old.id, replacement.id);
    assert_eq!(
        database.read_record(old.id, false).unwrap().lifecycle,
        Lifecycle::Superseded
    );
    assert!(replacement.relations.iter().any(|relation| {
        relation.kind == RelationKind::Supersedes && relation.target_record_id == old.id
    }));
    let first = create_note(&database, "merge-first", "First", "one");
    let second = create_note(&database, "merge-second", "Second", "two");
    let aggregate = database
        .merge_records(
            &[first.id, second.id],
            &note(1, vec![1], "Aggregate", "one and two"),
            "combine duplicate knowledge",
            &context("merge"),
        )
        .unwrap();
    assert_eq!(
        database.read_record(first.id, false).unwrap().lifecycle,
        Lifecycle::Merged
    );
    assert_eq!(
        database.read_record(second.id, false).unwrap().lifecycle,
        Lifecycle::Merged
    );
    assert_eq!(
        aggregate
            .relations
            .iter()
            .filter(|relation| relation.kind == RelationKind::MergedInto)
            .count(),
        2
    );
    let retracted = database
        .transition_record(
            aggregate.id,
            &Lifecycle::Retracted,
            "withdraw aggregate",
            &context("transition-retract"),
        )
        .unwrap();
    assert_eq!(retracted.lifecycle, Lifecycle::Retracted);
    assert_eq!(retracted.lifecycle_history.len(), 2);
}

fn create_v7_fixture(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE documents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, visibility TEXT NOT NULL, author TEXT NOT NULL, day TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1);
             CREATE UNIQUE INDEX documents_daily_day ON documents(day) WHERE kind='daily';
             CREATE INDEX documents_day ON documents(day);
             CREATE TABLE presence (session_id TEXT PRIMARY KEY, actor TEXT NOT NULL, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, last_heartbeat INTEGER NOT NULL);
             CREATE INDEX presence_document ON presence(document_id);
             CREATE TABLE document_attachments (parent_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, attached_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, status TEXT NOT NULL, reviewed_at TEXT, PRIMARY KEY(parent_document_id,attached_document_id), UNIQUE(attached_document_id));
             CREATE INDEX document_attachments_parent ON document_attachments(parent_document_id);
             CREATE TABLE project_documents (project_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, added_by TEXT NOT NULL, created_at TEXT NOT NULL, PRIMARY KEY(project_document_id,document_id));
             CREATE INDEX project_documents_document ON project_documents(document_id);
             INSERT INTO documents VALUES(3,'daily','shared','user','2026-08-01','daily-created','daily-updated','daily body',2);
             INSERT INTO documents VALUES(4,'note','shared','user','2026-08-02','source-created','source-updated','Visible [[note:9|Public label]] Hidden [[note:7|Secret label]]',3);
             INSERT INTO documents VALUES(7,'note','private','user','2026-08-03','private-created','private-updated','Private secret body',4);
             INSERT INTO documents VALUES(9,'note','shared','user','2026-08-04','target-created','target-updated','Public target',5);
             INSERT INTO documents VALUES(11,'artifact','shared','agent','2026-08-05','artifact-created','artifact-updated','Artifact body',6);
             INSERT INTO documents VALUES(12,'project','shared','user','2026-08-06','project-created','project-updated','Project body',7);
             INSERT INTO document_attachments VALUES(3,11,'blocked',NULL);
             INSERT INTO project_documents VALUES(12,7,'user','membership-private');
             INSERT INTO project_documents VALUES(12,9,'user','membership-public');
             PRAGMA user_version=7;",
        )
        .unwrap();
}

#[test]
fn v7_migration_preserves_legacy_data_hides_private_and_reopens_idempotently() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("archive.sqlite3");
    create_v7_fixture(&path);
    let database = Database::open(&path).unwrap();
    let source = database.read_record(4, false).unwrap();
    let RecordPayload::Note { body } = &source.current.payload else {
        panic!()
    };
    assert!(body.contains("Public label"));
    assert!(!body.contains("Secret label"));
    assert!(!body.contains("[[note:7"));
    assert_eq!(source.id, 4);
    assert_eq!(source.kind, RecordKind::Note);
    assert_eq!(source.labels[0].canonical, "workflow:inbox");
    assert_eq!(
        source.import_metadata.as_ref().unwrap()["legacy"]["revision"],
        3
    );
    assert!(matches!(
        database.read_record(7, false),
        Err(Error::MissingRecord(7))
    ));
    assert_eq!(database.list_relations(4, false).unwrap().len(), 1);
    assert_eq!(
        database.list_relations(4, false).unwrap()[0].target_record_id,
        9
    );
    assert!(
        database
            .search_records(
                Some("Secret label"),
                1,
                false,
                &[],
                &[],
                &[],
                false,
                None,
                20
            )
            .unwrap()
            .records
            .is_empty()
    );
    let connection = database.connection.lock().unwrap();
    let stored: String = connection
        .query_row(
            "SELECT payload_json FROM record_revisions WHERE record_id=4",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored.contains("Secret label"));
    assert_eq!(
        connection
            .query_row("SELECT body FROM documents WHERE id=4", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "Visible [[note:9|Public label]] Hidden [[note:7|Secret label]]"
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM record_relations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM project_documents", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM document_attachments", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
    connection.execute("DELETE FROM record_fts", []).unwrap();
    rebuild_fts(&connection).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM record_fts WHERE record_fts MATCH '\"Secret\" AND \"label\"'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM record_fts WHERE record_fts MATCH '\"Public\" AND \"label\"'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    drop(connection);
    let created = create_note(&database, "after-migration", "After", "body");
    assert!(created.id > 12);
    drop(database);
    let reopened = Database::open(&path).unwrap();
    assert_eq!(reopened.read_record(4, false).unwrap().id, 4);
    assert_eq!(
        reopened
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM records", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        7
    );
}

#[test]
fn historical_v1_migration_still_preserves_merged_body_and_id() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("archive.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE entries (id INTEGER PRIMARY KEY, day TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL); INSERT INTO entries VALUES (9,'2026-08-02','2026-08-02T12:00:00Z','2026-08-05T00:00:00Z','second'),(3,'2026-08-02','2026-08-02T08:00:00Z','2026-08-02T09:00:00Z','first'); PRAGMA user_version=1;").unwrap();
    drop(connection);
    let database = Database::open(&path).unwrap();
    let record = database.read_record(3, false).unwrap();
    let RecordPayload::Note { body } = record.current.payload else {
        panic!()
    };
    assert_eq!(body, "first\n\nsecond");
    assert_eq!(
        record.import_metadata.unwrap()["legacy"]["day"],
        "2026-08-02"
    );
    assert_eq!(
        database
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT body FROM documents WHERE id=3", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "first\n\nsecond"
    );
}
