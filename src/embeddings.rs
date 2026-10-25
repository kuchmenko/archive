use std::{fmt, path::Path};

pub use granite_embed::{
    DIMENSIONS, MODEL, MODEL_IDLE_TIMEOUT, MODEL_REVISION, limit_malloc_arenas,
};
use granite_embed::{Embedder, normalize_text};

use crate::{
    database::{self, Database, EmbeddingRecord},
    model::{EmbeddingStatus, EmbeddingSync},
};

#[derive(Debug)]
pub enum Error {
    Model(granite_embed::Error),
    Database(database::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => error.fmt(formatter),
            Self::Database(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {}

impl From<granite_embed::Error> for Error {
    fn from(error: granite_embed::Error) -> Self {
        Self::Model(error)
    }
}

impl From<database::Error> for Error {
    fn from(error: database::Error) -> Self {
        Self::Database(error)
    }
}

pub struct Embeddings {
    embedder: Embedder,
}

impl Embeddings {
    pub fn new(data_directory: &Path) -> Self {
        Self {
            embedder: Embedder::new(data_directory),
        }
    }

    pub fn status(&self, database: &Database) -> Result<EmbeddingStatus, Error> {
        Ok(database.embedding_status(MODEL, MODEL_REVISION, DIMENSIONS)?)
    }

    /// Without `model.ort` the model cannot load, so record writes and recall use BM25 only
    /// instead of failing after the write has already committed.
    pub fn is_installed(&self) -> bool {
        self.embedder.is_installed()
    }

    pub fn sync(&self, database: &Database) -> Result<EmbeddingSync, Error> {
        let records = database.pending_embedding_records(MODEL, MODEL_REVISION, DIMENSIONS)?;
        if records.is_empty() {
            return Ok(EmbeddingSync {
                indexed_records: 0,
                status: self.status(database)?,
            });
        }
        let indexed_records = self.embedder.with_model(|model| {
            let mut indexed_records = 0;
            for record in records {
                let embedding = model.embed(&record_text(&record))?;
                if database.store_embedding(
                    record.id,
                    record.revision,
                    MODEL,
                    MODEL_REVISION,
                    DIMENSIONS,
                    &embedding,
                )? {
                    indexed_records += 1;
                }
            }
            Ok::<_, Error>(indexed_records)
        })?;
        Ok(EmbeddingSync {
            indexed_records,
            status: self.status(database)?,
        })
    }

    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>, Error> {
        if query.trim().is_empty() {
            return Err(granite_embed::Error::Invalid(
                "semantic search query cannot be empty".to_owned(),
            )
            .into());
        }
        Ok(self.embedder.embed(query)?)
    }

    pub fn unload_if_idle(&self, idle: std::time::Duration) {
        self.embedder.unload_if_idle(idle);
    }
}

fn record_text(record: &EmbeddingRecord) -> String {
    normalize_text(&format!("{}\n{}", record.title, record.payload.text()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::model::RecordPayload;

    use super::*;

    #[test]
    fn record_text_is_normalized_deterministically() {
        let record = EmbeddingRecord {
            id: 1,
            revision: 1,
            title: "  Résumé\nmetric ".to_owned(),
            payload: RecordPayload::Metric {
                name: "latency".to_owned(),
                value: 1.0,
                unit: "ms".to_owned(),
                observed_at: None,
                interval: None,
                dimensions: BTreeMap::from([("runtime".to_owned(), "ONNX".to_owned())]),
                method: Some(" local   test ".to_owned()),
            },
        };
        assert_eq!(
            record_text(&record),
            "Résumé metric latency 1.0 ms runtime: ONNX local test"
        );
    }
}
