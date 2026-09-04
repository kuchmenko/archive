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
fn fresh_schema_is_v9_and_seeds_global_inbox() {
    let database = database();
    let connection = database.connection.lock().unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 9);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='record_embeddings'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
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
fn v8_migration_adds_the_derived_embedding_index_without_changing_records() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("archive.sqlite3");
    let id = {
        let database = Database::open(&path).unwrap();
        create_note(&database, "v8-record", "Preserved", "body").id
    };
    let connection = Connection::open(&path).unwrap();
    connection
        .execute("DROP TABLE record_embeddings", [])
        .unwrap();
    connection.pragma_update(None, "user_version", 8).unwrap();
    drop(connection);
    let database = Database::open(&path).unwrap();
    assert_eq!(database.read_record(id, false).unwrap().title, "Preserved");
    let connection = database.connection.lock().unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 9);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM record_embeddings", [], |row| {
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
fn bm25_recall_uses_or_matches_relevance_order_and_deterministic_ties() {
    let database = database();
    let partial = create_note(&database, "bm25-partial", "Partial", "alpha only");
    let relevant = create_note(
        &database,
        "bm25-relevant",
        "Relevant",
        "alpha beta beta beta",
    );
    let strict = database
        .search_records(Some("alpha beta"), 1, false, &[], &[], &[], false, None, 20)
        .unwrap();
    assert_eq!(strict.records.len(), 1);
    assert_eq!(strict.records[0].record.id, relevant.id);
    let ranked = database
        .bm25_search_records("alpha beta", 1, false, &[], &[], &[], 20)
        .unwrap();
    assert_eq!(ranked[0].record.id, relevant.id);
    assert!(ranked.iter().any(|hit| hit.record.id == partial.id));

    let older = create_note(&database, "bm25-tie-old", "Tie", "equal terms");
    let newer = create_note(&database, "bm25-tie-new", "Tie", "equal terms");
    let tied = database
        .bm25_search_records("equal terms", 1, false, &[], &[], &[], 20)
        .unwrap();
    assert_eq!(
        tied.iter()
            .take(2)
            .map(|hit| hit.record.id)
            .collect::<Vec<_>>(),
        vec![newer.id, older.id]
    );
}

#[test]
fn recall_filtering_matches_search_and_excludes_unreadable_records() {
    let database = database();
    let work = database
        .create_scope("work", &context("recall-filter-work"))
        .unwrap();
    let other = database
        .create_scope("other", &context("recall-filter-other"))
        .unwrap();
    let rust = database
        .create_label(
            "topic",
            "rust",
            "Rust",
            &[],
            &context("recall-filter-label"),
        )
        .unwrap();
    let eligible = database
        .create_record(
            &note(work.id, vec![rust.id], "Eligible", "filter needle"),
            &context("recall-filter-eligible"),
        )
        .unwrap();
    let global = database
        .create_record(
            &note(1, vec![rust.id], "Global", "filter needle"),
            &context("recall-filter-global"),
        )
        .unwrap();
    let other_scope = database
        .create_record(
            &note(other.id, vec![rust.id], "Other", "filter needle"),
            &context("recall-filter-other-record"),
        )
        .unwrap();
    let other_kind = database
        .create_record(
            &RecordInput {
                scope_id: work.id,
                title: "Idea".to_owned(),
                payload: RecordPayload::Idea {
                    proposal: "filter needle".to_owned(),
                },
                sources: Vec::new(),
                label_ids: vec![rust.id],
            },
            &context("recall-filter-kind"),
        )
        .unwrap();
    let wrong_label = database
        .create_record(
            &note(work.id, vec![1], "Inbox", "filter needle"),
            &context("recall-filter-inbox"),
        )
        .unwrap();
    let retracted = database
        .create_record(
            &note(work.id, vec![rust.id], "Retracted", "filter needle"),
            &context("recall-filter-retracted"),
        )
        .unwrap();
    database
        .transition_record(
            retracted.id,
            &Lifecycle::Retracted,
            "exclude from active recall",
            &context("recall-filter-transition"),
        )
        .unwrap();
    {
        let connection = database.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO records(id,scope_id,kind,title,lifecycle,current_revision,readable,created_at,updated_at,actor,thread,client,idempotency_key)
                 VALUES(900,?1,'note','Hidden','active',1,0,'now','now','agent','hidden','test','recall-hidden')",
                [work.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO record_revisions(record_id,revision,title,payload_json,reason,created_at,actor,thread,client,idempotency_key)
                 VALUES(900,1,'Hidden','{\"kind\":\"note\",\"body\":\"filter needle\"}','hidden','now','agent','hidden','test','recall-hidden-revision')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO record_fts(record_id,title,payload) VALUES(900,'Hidden','filter needle')",
                [],
            )
            .unwrap();
    }
    let filtered = database
        .recall_context(
            "filter needle",
            None,
            "unused",
            "unused",
            2,
            work.id,
            false,
            &[RecordKind::Note],
            &[],
            &[rust.id],
            DEFAULT_RECALL_BUDGET_BYTES,
        )
        .unwrap();
    assert_eq!(
        filtered
            .records
            .iter()
            .map(|record| record.record_id)
            .collect::<Vec<_>>(),
        vec![eligible.id]
    );
    let with_global = database
        .recall_context(
            "filter needle",
            None,
            "unused",
            "unused",
            2,
            work.id,
            true,
            &[RecordKind::Note],
            &[],
            &[rust.id],
            DEFAULT_RECALL_BUDGET_BYTES,
        )
        .unwrap();
    let ids = with_global
        .records
        .iter()
        .map(|record| record.record_id)
        .collect::<Vec<_>>();
    assert!(ids.contains(&eligible.id));
    assert!(ids.contains(&global.id));
    for excluded in [
        other_scope.id,
        other_kind.id,
        wrong_label.id,
        retracted.id,
        900,
    ] {
        assert!(!ids.contains(&excluded));
    }
}

#[test]
fn recall_deduplicates_dense_and_bm25_candidates_and_uses_dense_ordering() {
    let database = database();
    let records = [
        ("first", "no lexical match", [0.8, 0.6]),
        ("second", "dense needle", [1.0, 0.0]),
        ("third", "another body", [0.6, 0.8]),
        ("fourth", "another body", [0.6, 0.8]),
        ("fifth", "needle lexical only", [0.0, 1.0]),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (title, body, vector))| {
        let record = create_note(&database, &format!("recall-dense-{index}"), title, body);
        (record, vector)
    })
    .collect::<Vec<_>>();
    for (record, vector) in &records {
        database
            .store_embedding(record.id, 1, "test/recall", "v1", 2, vector)
            .unwrap();
    }
    let result = database
        .recall_context(
            "needle",
            Some(&[1.0, 0.0]),
            "test/recall",
            "v1",
            2,
            1,
            false,
            &[],
            &[],
            &[],
            DEFAULT_RECALL_BUDGET_BYTES,
        )
        .unwrap();
    assert_eq!(result.strategy, RecallStrategy::Dense);
    assert!(result.semantic_available);
    assert_eq!(result.records.len(), 5);
    assert_eq!(result.records[0].record_id, records[1].0.id);
    assert_eq!(result.records[1].record_id, records[0].0.id);
    assert_eq!(result.records[2].record_id, records[3].0.id);
    assert_eq!(result.records[3].record_id, records[2].0.id);
    assert_eq!(
        result
            .records
            .iter()
            .filter(|record| record.record_id == records[1].0.id)
            .count(),
        1
    );
    assert_eq!(
        result.records[0].retrieval.matched_by,
        vec![RecallStrategy::Bm25, RecallStrategy::Dense]
    );
    assert!(result.records[0].retrieval.rrf_score.unwrap() > 1.0 / 61.0);
}

#[test]
fn recall_returns_bounded_excerpts_with_current_sources_and_provenance() {
    let database = database();
    for index in 0..5 {
        let mut sources = Vec::new();
        for source_index in 0..4 {
            sources.push(SourceInput {
                identity: format!("source-{index}-{source_index}"),
                ..source()
            });
        }
        database
            .create_record(
                &RecordInput {
                    scope_id: 1,
                    title: format!("Evidence {index}"),
                    payload: RecordPayload::Observation {
                        statement: format!(
                            "{} query-relevant-marker {}",
                            "leading context ".repeat(80),
                            "trailing context ".repeat(80)
                        ),
                        observed_at: Some("2026-08-31T10:00:00Z".to_owned()),
                    },
                    sources,
                    label_ids: vec![1],
                },
                &context(&format!("recall-budget-{index}")),
            )
            .unwrap();
    }
    let result = database
        .recall_context(
            "query-relevant-marker",
            None,
            "unused",
            "unused",
            2,
            1,
            false,
            &[],
            &[],
            &[],
            4_000,
        )
        .unwrap();
    assert!((3..=5).contains(&result.records.len()));
    assert_eq!(result.strategy, RecallStrategy::Bm25);
    assert!(!result.semantic_available);
    assert_eq!(
        serde_json::to_vec(&result).unwrap().len(),
        result.used_bytes
    );
    assert!(result.used_bytes <= result.budget_bytes);
    for record in &result.records {
        assert!(record.excerpt.contains("query-relevant-marker"));
        assert!(record.excerpt.len() < 1_000);
        assert_eq!(record.scope.name, "global");
        assert_eq!(record.lifecycle, Lifecycle::Active);
        assert_eq!(record.current_revision, 1);
        assert_eq!(record.provenance.actor, "agent-a");
        assert_eq!(record.source_count, 4);
        assert_eq!(record.sources.len(), 3);
    }
    let json = serde_json::to_value(result).unwrap();
    let record = &json["records"][0];
    for excluded in ["payload", "history", "labels", "relations"] {
        assert!(record.get(excluded).is_none());
    }
    assert!(record["sources"][0].get("quote").is_none());
}

#[test]
fn embedding_index_is_complete_revision_bound_private_safe_and_filterable() {
    let database = database();
    let work = database
        .create_scope("work", &context("embedding-scope"))
        .unwrap();
    let rust = database
        .create_label("topic", "rust", "Rust", &[], &context("embedding-label"))
        .unwrap();
    let global = create_note(&database, "embedding-global", "Global", "global body");
    let first = database
        .create_record(
            &note(work.id, vec![1], "First", "first body"),
            &context("embedding-first"),
        )
        .unwrap();
    let first = database
        .add_label(
            first.id,
            rust.id,
            "semantic filter",
            &context("embedding-first-label"),
        )
        .unwrap();
    let second = database
        .create_record(
            &note(work.id, vec![1], "Second", "second body"),
            &context("embedding-second"),
        )
        .unwrap();
    let model = "test/embedding";
    let revision = "revision-1";
    for (record, vector) in [
        (&global, [1.0, 0.0]),
        (&first, [0.8, 0.6]),
        (&second, [0.8, 0.6]),
    ] {
        assert!(
            database
                .store_embedding(record.id, 1, model, revision, 2, &vector)
                .unwrap()
        );
    }
    assert_eq!(
        database.embedding_status(model, revision, 2).unwrap(),
        EmbeddingStatus {
            model: model.to_owned(),
            model_revision: revision.to_owned(),
            dimensions: 2,
            eligible_records: 3,
            indexed_records: 3,
            pending_records: 0,
        }
    );
    let exact = database
        .semantic_search_records(
            &[1.0, 0.0],
            model,
            revision,
            2,
            work.id,
            false,
            &[],
            &[],
            &[],
            false,
            20,
        )
        .unwrap();
    assert_eq!(
        exact
            .records
            .iter()
            .map(|hit| hit.record.id)
            .collect::<Vec<_>>(),
        vec![second.id, first.id]
    );
    assert_eq!(exact.records[0].similarity, 0.8);
    let with_global = database
        .semantic_search_records(
            &[1.0, 0.0],
            model,
            revision,
            2,
            work.id,
            true,
            &[RecordKind::Note],
            &[Lifecycle::Active],
            &[1],
            false,
            20,
        )
        .unwrap();
    assert_eq!(with_global.records[0].record.id, global.id);
    assert_eq!(with_global.records.len(), 3);
    assert!(
        with_global.records[0]
            .match_explanation
            .contains(&"semantic:cosine".to_owned())
    );
    let labeled = database
        .semantic_search_records(
            &[1.0, 0.0],
            model,
            revision,
            2,
            work.id,
            false,
            &[],
            &[],
            &[rust.id],
            false,
            20,
        )
        .unwrap();
    assert_eq!(labeled.records.len(), 1);
    assert_eq!(labeled.records[0].record.id, first.id);
    database
        .transition_record(
            first.id,
            &Lifecycle::Retracted,
            "test lifecycle filter",
            &context("embedding-retract"),
        )
        .unwrap();
    let active = database
        .semantic_search_records(
            &[1.0, 0.0],
            model,
            revision,
            2,
            work.id,
            false,
            &[],
            &[],
            &[],
            false,
            20,
        )
        .unwrap();
    assert_eq!(active.records.len(), 1);
    assert_eq!(active.records[0].record.id, second.id);
    let retracted = database
        .semantic_search_records(
            &[1.0, 0.0],
            model,
            revision,
            2,
            work.id,
            false,
            &[],
            &[Lifecycle::Retracted],
            &[],
            false,
            20,
        )
        .unwrap();
    assert_eq!(retracted.records[0].record.id, first.id);
    {
        let connection = database.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO records(id,scope_id,kind,title,lifecycle,current_revision,readable,created_at,updated_at,actor,thread,client,idempotency_key)
                 VALUES(900,1,'note','Private','active',1,0,'now','now','user','private','test','private-record')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO record_revisions(record_id,revision,title,payload_json,reason,created_at,actor,thread,client,idempotency_key)
                 VALUES(900,1,'Private','{\"kind\":\"note\",\"body\":\"secret\"}','private','now','user','private','test','private-revision')",
                [],
            )
            .unwrap();
    }
    assert_eq!(
        database
            .embedding_status(model, revision, 2)
            .unwrap()
            .pending_records,
        0
    );
    assert!(
        database
            .pending_embedding_records(model, revision, 2)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        database.store_embedding(900, 1, model, revision, 2, &[1.0, 0.0]),
        Err(Error::MissingRecord(900))
    ));
    let revised = database
        .revise_record(
            second.id,
            1,
            "Second revised",
            &RecordPayload::Note {
                body: "revised body".to_owned(),
            },
            &[],
            "correct text",
            &context("embedding-revision"),
        )
        .unwrap();
    let status = database.embedding_status(model, revision, 2).unwrap();
    assert_eq!(status.eligible_records, 3);
    assert_eq!(status.indexed_records, 2);
    assert_eq!(status.pending_records, 1);
    assert!(
        database
            .semantic_search_records(
                &[1.0, 0.0],
                model,
                revision,
                2,
                work.id,
                false,
                &[],
                &[],
                &[],
                false,
                20,
            )
            .unwrap_err()
            .to_string()
            .contains("run sync_embeddings")
    );
    let pending = database
        .pending_embedding_records(model, revision, 2)
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, second.id);
    assert_eq!(pending[0].revision, 2);
    assert!(
        !database
            .store_embedding(second.id, 1, model, revision, 2, &[0.0, 1.0])
            .unwrap()
    );
    assert!(
        database
            .store_embedding(
                second.id,
                revised.current_revision,
                model,
                revision,
                2,
                &[0.0, 1.0]
            )
            .unwrap()
    );
    assert_eq!(
        database
            .embedding_status(model, revision, 2)
            .unwrap()
            .pending_records,
        0
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
    assert_eq!(source.import_metadata.as_ref().unwrap().legacy.revision, 3);
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
    assert_eq!(record.import_metadata.unwrap().legacy.day, "2026-08-02");
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
