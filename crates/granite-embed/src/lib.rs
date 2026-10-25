//! Local text embeddings with `ibm-granite/granite-embedding-311m-multilingual-r2`.
//!
//! Model files live under `<data directory>/models/`; every call takes that data directory.

use std::{
    fmt, fs,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use memmap2::{Mmap, UncheckedAdvice};

use ort::{
    ep,
    session::{
        InMemorySession, Session,
        builder::{GraphOptimizationLevel, SessionBuilder},
    },
    value::TensorRef,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;
use unicode_normalization::UnicodeNormalization;

pub const MODEL: &str = "ibm-granite/granite-embedding-311m-multilingual-r2";
pub const MODEL_REVISION: &str = "44399559930365213510b1ee2eb15ded83374f0e";
pub const DIMENSIONS: usize = 768;
const MAX_SEQUENCE_LENGTH: usize = 32_768;
const MODEL_FILE_SIZE: u64 = 1_247_170_481;
const MODEL_SHA256: &str = "75f9f258bf5013f5fe8a4dad61dd0fd16ac0cbaa7a106e3d3f41c2d04a42d541";
const TOKENIZER_FILE_SIZE: u64 = 33_384_821;
const TOKENIZER_SHA256: &str = "0087c868b33bad550a78a08d19798cfd7f713cde4f020803b8f51f405503e15f";
const DOWNLOAD_URL: &str =
    "https://huggingface.co/ibm-granite/granite-embedding-311m-multilingual-r2/resolve";

#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Lock,
    Io(std::io::Error),
    Json(serde_json::Error),
    Ort(String),
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

/// A loaded model costs about 110 MB of private memory, mostly the tokenizer.
pub const MODEL_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Loads the model on first use and keeps it until `unload_if_idle` drops it.
pub struct Embedder {
    directory: PathBuf,
    /// Mapped once and kept for the life of the process, because the session borrows it.
    model_file: OnceLock<&'static Mmap>,
    model: Mutex<Option<Model>>,
}

impl Embedder {
    pub fn new(data_directory: &Path) -> Self {
        Self {
            directory: model_directory(data_directory),
            model_file: OnceLock::new(),
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
            write_ort_model(
                &destination.join("model.onnx"),
                &destination.join("model.ort"),
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
        write_ort_model(&temporary.join("model.onnx"), &temporary.join("model.ort"))?;
        fs::rename(&temporary, &destination)?;
        Ok(destination)
    }

    /// Downloads the pinned model revision from Hugging Face and installs it. An existing
    /// installation is only verified, so nothing is downloaded twice.
    pub fn download_and_install(data_directory: &Path) -> Result<PathBuf, Error> {
        let destination = model_directory(data_directory);
        if destination.exists() {
            // install() ignores the source when the model directory already exists.
            return Self::install(&destination, data_directory);
        }
        let parent = destination
            .parent()
            .ok_or_else(|| Error::Invalid("embedding model directory has no parent".to_owned()))?;
        fs::create_dir_all(parent)?;
        // Same file system as the model directory, so install() can hard-link instead of copy.
        let temporary = parent.join(format!(".download-{}", std::process::id()));
        let result = (|| {
            fs::create_dir_all(temporary.join("onnx"))?;
            for (file, size) in [
                ("onnx/model.onnx", MODEL_FILE_SIZE),
                ("tokenizer.json", TOKENIZER_FILE_SIZE),
            ] {
                eprintln!("downloading {file} ({} MB)", size / 1_000_000);
                download_file(
                    &format!("{DOWNLOAD_URL}/{MODEL_REVISION}/{file}"),
                    &temporary.join(file),
                )?;
            }
            Self::install(&temporary, data_directory)
        })();
        let _ = fs::remove_dir_all(&temporary);
        result
    }

    /// The model loads only from `model.ort`, which `install` writes last.
    pub fn is_installed(&self) -> bool {
        self.directory.join("model.ort").is_file()
    }

    /// Returns a 768-dimensional unit vector for `text` after `normalize_text`.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, Error> {
        self.with_model(|model| model.embed(&normalize_text(text)))
    }

    /// Runs `operation` with the loaded model, loading it first if needed. Use it to embed
    /// many texts under one lock; `Model::embed` does not normalize its input.
    pub fn with_model<T, E: From<Error>>(
        &self,
        operation: impl FnOnce(&mut Model) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut model = self.model.lock().map_err(|_| Error::Lock)?;
        if model.is_none() {
            let file = match self.model_file.get() {
                Some(file) => *file,
                None => {
                    let file = map_model(&self.directory.join("model.ort"))?;
                    *self.model_file.get_or_init(|| file)
                }
            };
            *model = Some(Model::load(&self.directory, file)?);
            release_freed_memory();
        }
        let loaded = model.as_mut().expect("embedding model was initialized");
        let result = operation(loaded);
        loaded.last_used = Instant::now();
        release_freed_memory();
        result
    }

    /// Drops the tokenizer and session after `idle` without use; the next call loads them again.
    /// Skips a model that is in use right now.
    pub fn unload_if_idle(&self, idle: Duration) {
        let Ok(mut model) = self.model.try_lock() else {
            return;
        };
        if model
            .as_ref()
            .is_none_or(|model| model.last_used.elapsed() < idle)
        {
            return;
        }
        *model = None;
        if let Some(file) = self.model_file.get() {
            // SAFETY: the session that read this read-only file mapping was just dropped,
            // and later reads fault the pages back in from the file.
            let _ = unsafe { file.unchecked_advise(UncheckedAdvice::DontNeed) };
        }
        release_freed_memory();
    }
}

/// glibc keeps freed heap memory inside the process. Parsing tokenizer.json frees about
/// 150 MB and long inputs free more, so return it to the system after each model use.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn release_freed_memory() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    // SAFETY: malloc_trim only releases free heap pages and is safe to call at any time.
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn release_freed_memory() {}

/// The model loads on whichever thread serves the call, and glibc gives each thread its own
/// heap. After repeated unload and reload, freed memory spreads across those heaps and the
/// idle process grows (34 to 74 MB over 6 cycles). Two shared heaps keep it at ~41 MB.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn limit_malloc_arenas() {
    const M_ARENA_MAX: i32 = -8;
    unsafe extern "C" {
        fn mallopt(param: i32, value: i32) -> i32;
    }
    // SAFETY: called once at startup before the model or other worker threads allocate.
    unsafe {
        mallopt(M_ARENA_MAX, 2);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn limit_malloc_arenas() {}

/// A loaded tokenizer and ONNX session.
pub struct Model {
    tokenizer: Tokenizer,
    session: InMemorySession<'static>,
    last_used: Instant,
}

impl Model {
    fn load(directory: &Path, file: &'static Mmap) -> Result<Self, Error> {
        verify_installation(directory)?;
        let tokenizer = Tokenizer::from_file(directory.join("tokenizer.json"))
            .map_err(|error| Error::Tokenizer(error.to_string()))?;
        let session = load_ort_session(file)?;
        Ok(Self {
            tokenizer,
            session,
            last_used: Instant::now(),
        })
    }

    /// Returns the normalized first-token vector of `text` as given.
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>, Error> {
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

/// Settings shared by conversion and loading. Prepacking is off because it copies
/// weights into each process, which defeats sharing the mapped model file.
fn session_builder(level: GraphOptimizationLevel) -> Result<SessionBuilder, Error> {
    Session::builder()?
        .with_execution_providers([ep::CPU::default()
            .with_arena_allocator(false)
            .build()
            .error_on_failure()])
        .map_err(ort_error)?
        .with_optimization_level(level)
        .map_err(ort_error)?
        .with_inter_threads(1)
        .map_err(ort_error)?
        .with_memory_pattern(false)
        .map_err(ort_error)?
        .with_prepacking(false)
        .map_err(ort_error)
}

/// Writes an optimized ORT-format copy of `onnx` to `ort` unless it already exists.
/// The file appears only through `rename`, so processes that map it never see it change.
fn write_ort_model(onnx: &Path, ort: &Path) -> Result<(), Error> {
    if ort.exists() {
        return Ok(());
    }
    let temporary = ort.with_extension(format!("ort.{}.tmp", std::process::id()));
    let result = session_builder(GraphOptimizationLevel::Level3)
        .and_then(|builder| {
            builder
                .with_config_entry("session.save_model_format", "ORT")
                .map_err(ort_error)?
                .with_optimized_model_path(&temporary)
                .map_err(ort_error)?
                .commit_from_file(onnx)
                .map_err(Error::from)
        })
        .and_then(|session| {
            drop(session);
            Ok(fs::rename(&temporary, ort)?)
        });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Streams `url` into `destination`. The file is created only after a successful response.
fn download_file(url: &str, destination: &Path) -> Result<(), Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .build(),
        )
        .build()
        .into();
    let response = agent
        .get(url)
        .call()
        .map_err(|error| Error::Invalid(format!("download of {url} failed: {error}")))?;
    let mut file = File::create(destination)?;
    std::io::copy(&mut response.into_body().into_reader(), &mut file)?;
    Ok(())
}

fn map_model(path: &Path) -> Result<&'static Mmap, Error> {
    if !path.exists() {
        return Err(Error::Invalid(
            "embedding model.ort is missing; run archive embeddings install".to_owned(),
        ));
    }
    let file = File::open(path)?;
    // SAFETY: model.ort is only ever replaced by rename, never written in place,
    // so the mapped bytes do not change while the process uses them.
    let map = unsafe { Mmap::map(&file)? };
    // Sessions borrow the mapping, so it is never freed; unloading only drops its pages.
    Ok(Box::leak(Box::new(map)))
}

fn load_ort_session(file: &'static Mmap) -> Result<InMemorySession<'static>, Error> {
    Ok(session_builder(GraphOptimizationLevel::Disable)?.commit_from_memory_directly(file)?)
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

/// NFKC with runs of whitespace collapsed to one space.
pub fn normalize_text(value: &str) -> String {
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
    use super::*;

    // Y = X * W with a 3x2 initializer W; copied from ONNX Runtime's test datasets (MIT).
    const MUL_MODEL: &[u8] = include_bytes!("../tests/fixtures/mul_1.onnx");

    #[test]
    fn ort_model_is_written_once_and_loads_from_memory() {
        let directory = tempfile::tempdir().unwrap();
        let onnx = directory.path().join("model.onnx");
        let ort = directory.path().join("model.ort");
        fs::write(&onnx, MUL_MODEL).unwrap();

        write_ort_model(&onnx, &ort).unwrap();
        let bytes = fs::read(&ort).unwrap();
        let modified = fs::metadata(&ort).unwrap().modified().unwrap();
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);

        write_ort_model(&onnx, &ort).unwrap();
        assert_eq!(fs::read(&ort).unwrap(), bytes);
        assert_eq!(fs::metadata(&ort).unwrap().modified().unwrap(), modified);

        let mut session = load_ort_session(map_model(&ort).unwrap()).unwrap();
        let x = [1.0_f32; 6];
        let outputs = session
            .run(ort::inputs! {
                "X" => TensorRef::from_array_view(([3_usize, 2], x.as_slice())).unwrap(),
            })
            .unwrap();
        let (shape, y) = outputs["Y"].try_extract_tensor::<f32>().unwrap();
        assert_eq!(shape.as_ref(), [3, 2]);
        assert_eq!(y, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn installation_without_ort_model_is_not_installed() {
        let data_directory = tempfile::tempdir().unwrap();
        let directory = model_directory(data_directory.path());
        fs::create_dir_all(&directory).unwrap();
        assert!(!Embedder::new(data_directory.path()).is_installed());

        fs::write(directory.join("model.ort"), b"").unwrap();
        assert!(Embedder::new(data_directory.path()).is_installed());
    }

    /// Serves one HTTP response on a local port and returns its URL.
    fn serve_once(response: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request);
            std::io::Write::write_all(&mut stream, response.as_bytes()).unwrap();
        });
        format!("http://{address}/file")
    }

    #[test]
    fn download_writes_response_body() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("tokenizer.json");
        let url =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");

        download_file(&url, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"hello");
    }

    #[test]
    fn failed_download_writes_no_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("tokenizer.json");
        let url =
            serve_once("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");

        let error = download_file(&url, &destination).err().unwrap();
        assert!(error.to_string().contains(&url));
        assert!(!destination.exists());
    }

    #[test]
    fn missing_ort_model_reports_install_command() {
        let directory = tempfile::tempdir().unwrap();
        let error = map_model(&directory.path().join("model.ort"))
            .err()
            .unwrap();
        assert!(matches!(error, Error::Invalid(_)));
        assert!(error.to_string().contains("archive embeddings install"));
    }

    #[test]
    fn failed_conversion_leaves_no_files() {
        let directory = tempfile::tempdir().unwrap();
        let onnx = directory.path().join("model.onnx");
        fs::write(&onnx, b"not an onnx model").unwrap();

        assert!(write_ort_model(&onnx, &directory.path().join("model.ort")).is_err());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
