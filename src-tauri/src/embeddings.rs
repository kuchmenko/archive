use std::{
    fmt, fs,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::Mutex,
};

use ort::{
    ep,
    session::{Session, builder::GraphOptimizationLevel},
    value::TensorRef,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;
use unicode_normalization::UnicodeNormalization;

use crate::{
    database::{self, Database, EmbeddingRecord},
    model::{EmbeddingStatus, EmbeddingSync},
};

pub const MODEL: &str = "ibm-granite/granite-embedding-311m-multilingual-r2";
pub const MODEL_REVISION: &str = "44399559930365213510b1ee2eb15ded83374f0e";
pub const DIMENSIONS: usize = 768;
const MAX_SEQUENCE_LENGTH: usize = 32_768;
const MODEL_FILE_SIZE: u64 = 1_247_170_481;
const MODEL_SHA256: &str = "75f9f258bf5013f5fe8a4dad61dd0fd16ac0cbaa7a106e3d3f41c2d04a42d541";
const TOKENIZER_FILE_SIZE: u64 = 33_384_821;
const TOKENIZER_SHA256: &str = "0087c868b33bad550a78a08d19798cfd7f713cde4f020803b8f51f405503e15f";

#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Lock,
    Io(std::io::Error),
    Json(serde_json::Error),
    Ort(String),
    Database(database::Error),
    Tokenizer(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) | Self::Ort(message) | Self::Tokenizer(message) => {
                message.fmt(formatter)
            }
            Self::Lock => write!(formatter, "embedding model lock is unavailable"),
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::Database(error) => error.fmt(formatter),
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

impl From<ort::Error> for Error {
    fn from(error: ort::Error) -> Self {
        Self::Ort(error.to_string())
    }
}

impl From<database::Error> for Error {
    fn from(error: database::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Manifest {
    model: String,
    model_revision: String,
    dimensions: usize,
    model_sha256: String,
    tokenizer_sha256: String,
}

impl Manifest {
    fn expected() -> Self {
        Self {
            model: MODEL.to_owned(),
            model_revision: MODEL_REVISION.to_owned(),
            dimensions: DIMENSIONS,
            model_sha256: MODEL_SHA256.to_owned(),
            tokenizer_sha256: TOKENIZER_SHA256.to_owned(),
        }
    }
}

pub struct Embeddings {
    directory: PathBuf,
    model: Mutex<Option<GraniteModel>>,
}

impl Embeddings {
    pub fn new(data_directory: &Path) -> Self {
        Self {
            directory: model_directory(data_directory),
            model: Mutex::new(None),
        }
    }

    pub fn install(source: &Path, data_directory: &Path) -> Result<PathBuf, Error> {
        let destination = model_directory(data_directory);
        if destination.exists() {
            verify_installation(&destination)?;
            verify_file(
                &destination.join("model.onnx"),
                MODEL_FILE_SIZE,
                MODEL_SHA256,
                "ONNX model",
            )?;
            verify_file(
                &destination.join("tokenizer.json"),
                TOKENIZER_FILE_SIZE,
                TOKENIZER_SHA256,
                "tokenizer",
            )?;
            return Ok(destination);
        }
        let source_model = fs::canonicalize(source.join("onnx/model.onnx"))?;
        let source_tokenizer = fs::canonicalize(source.join("tokenizer.json"))?;
        verify_file(&source_model, MODEL_FILE_SIZE, MODEL_SHA256, "ONNX model")?;
        verify_file(
            &source_tokenizer,
            TOKENIZER_FILE_SIZE,
            TOKENIZER_SHA256,
            "tokenizer",
        )?;
        let parent = destination
            .parent()
            .ok_or_else(|| Error::Invalid("embedding model directory has no parent".to_owned()))?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(".install-{}", std::process::id()));
        if temporary.exists() {
            fs::remove_dir_all(&temporary)?;
        }
        fs::create_dir(&temporary)?;
        link_or_copy(&source_model, &temporary.join("model.onnx"))?;
        link_or_copy(&source_tokenizer, &temporary.join("tokenizer.json"))?;
        fs::write(
            temporary.join("manifest.json"),
            serde_json::to_vec_pretty(&Manifest::expected())?,
        )?;
        fs::rename(&temporary, &destination)?;
        Ok(destination)
    }

    pub fn status(&self, database: &Database) -> Result<EmbeddingStatus, Error> {
        Ok(database.embedding_status(MODEL, MODEL_REVISION, DIMENSIONS)?)
    }

    pub fn is_installed(&self) -> bool {
        self.directory.is_dir()
    }

    pub fn sync(&self, database: &Database) -> Result<EmbeddingSync, Error> {
        let records = database.pending_embedding_records(MODEL, MODEL_REVISION, DIMENSIONS)?;
        if records.is_empty() {
            return Ok(EmbeddingSync {
                indexed_records: 0,
                status: self.status(database)?,
            });
        }
        let indexed_records = self.with_model(|model| {
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
            Ok(indexed_records)
        })?;
        Ok(EmbeddingSync {
            indexed_records,
            status: self.status(database)?,
        })
    }

    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>, Error> {
        if query.trim().is_empty() {
            return Err(Error::Invalid(
                "semantic search query cannot be empty".to_owned(),
            ));
        }
        self.with_model(|model| model.embed(&normalize_text(query)))
    }

    fn with_model<T>(
        &self,
        operation: impl FnOnce(&mut GraniteModel) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut model = self.model.lock().map_err(|_| Error::Lock)?;
        if model.is_none() {
            *model = Some(GraniteModel::load(&self.directory)?);
        }
        operation(model.as_mut().expect("embedding model was initialized"))
    }
}

struct GraniteModel {
    tokenizer: Tokenizer,
    session: Session,
}

impl GraniteModel {
    fn load(directory: &Path) -> Result<Self, Error> {
        verify_installation(directory)?;
        let tokenizer = Tokenizer::from_file(directory.join("tokenizer.json"))
            .map_err(|error| Error::Tokenizer(error.to_string()))?;
        let session = Session::builder()?
            .with_execution_providers([ep::CPU::default()
                .with_arena_allocator(false)
                .build()
                .error_on_failure()])
            .map_err(ort_error)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_error)?
            .with_inter_threads(1)
            .map_err(ort_error)?
            .with_memory_pattern(false)
            .map_err(ort_error)?
            .commit_from_file(directory.join("model.onnx"))?;
        Ok(Self { tokenizer, session })
    }

    fn embed(&mut self, text: &str) -> Result<Vec<f32>, Error> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|error| Error::Tokenizer(error.to_string()))?;
        let sequence_length = encoding.len();
        if sequence_length == 0 || sequence_length > MAX_SEQUENCE_LENGTH {
            return Err(Error::Invalid(format!(
                "embedding input has {sequence_length} tokens; expected 1..={MAX_SEQUENCE_LENGTH}"
            )));
        }
        let input_ids = encoding
            .get_ids()
            .iter()
            .map(|value| i64::from(*value))
            .collect::<Vec<_>>();
        let attention_mask = encoding
            .get_attention_mask()
            .iter()
            .map(|value| i64::from(*value))
            .collect::<Vec<_>>();
        let ids = TensorRef::from_array_view(([1_usize, sequence_length], input_ids.as_slice()))?;
        let mask =
            TensorRef::from_array_view(([1_usize, sequence_length], attention_mask.as_slice()))?;
        let outputs = self.session.run(ort::inputs! {
            "input_ids" => ids,
            "attention_mask" => mask,
        })?;
        let output = outputs.get("last_hidden_state").ok_or_else(|| {
            Error::Invalid("ONNX model has no last_hidden_state output".to_owned())
        })?;
        let (shape, hidden) = output.try_extract_tensor::<f32>()?;
        if shape.as_ref() != [1, sequence_length as i64, DIMENSIONS as i64]
            || hidden.len() != sequence_length * DIMENSIONS
        {
            return Err(Error::Invalid(format!(
                "ONNX model returned unexpected shape {shape:?}"
            )));
        }
        let mut embedding = hidden[..DIMENSIONS].to_vec();
        let norm = embedding
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        if !norm.is_finite() || norm == 0.0 {
            return Err(Error::Invalid(
                "ONNX model returned an invalid embedding".to_owned(),
            ));
        }
        for value in &mut embedding {
            *value /= norm;
        }
        Ok(embedding)
    }
}

fn model_directory(data_directory: &Path) -> PathBuf {
    data_directory
        .join("models")
        .join("granite-embedding-311m-multilingual-r2")
        .join(MODEL_REVISION)
}

fn verify_installation(directory: &Path) -> Result<(), Error> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)?;
    if manifest != Manifest::expected() {
        return Err(Error::Invalid(
            "installed embedding model manifest does not match the selected model".to_owned(),
        ));
    }
    verify_size(&directory.join("model.onnx"), MODEL_FILE_SIZE, "ONNX model")?;
    verify_size(
        &directory.join("tokenizer.json"),
        TOKENIZER_FILE_SIZE,
        "tokenizer",
    )
}

fn verify_file(path: &Path, size: u64, hash: &str, name: &str) -> Result<(), Error> {
    verify_size(path, size, name)?;
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    if format!("{:x}", digest.finalize()) != hash {
        return Err(Error::Invalid(format!(
            "{name} hash does not match the selected model revision"
        )));
    }
    Ok(())
}

fn verify_size(path: &Path, size: u64, name: &str) -> Result<(), Error> {
    if fs::metadata(path)?.len() != size {
        return Err(Error::Invalid(format!(
            "{name} size does not match the selected model revision"
        )));
    }
    Ok(())
}

fn link_or_copy(source: &Path, destination: &Path) -> Result<(), Error> {
    if fs::hard_link(source, destination).is_err() {
        fs::copy(source, destination)?;
    }
    Ok(())
}

fn record_text(record: &EmbeddingRecord) -> String {
    normalize_text(&format!("{}\n{}", record.title, record.payload.text()))
}

fn normalize_text(value: &str) -> String {
    value
        .nfkc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn ort_error<R>(error: ort::Error<R>) -> Error {
    Error::Ort(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::model::RecordPayload;

    use super::*;

    #[test]
    fn record_text_matches_the_benchmark_contract() {
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
