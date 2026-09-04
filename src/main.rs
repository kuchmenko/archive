mod database;
mod embeddings;
mod mcp;
mod merman;
mod model;

use std::path::{Path, PathBuf};

use database::Database;
use embeddings::Embeddings;

fn app_data_directory() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|path| path.join("dev.kuchmenko.archive"))
        .ok_or_else(|| "application data directory is unavailable".to_owned())
}

fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match arguments.as_slice() {
        [argument] if argument == "mcp" => run_mcp().map_err(|error| error.to_string()),
        [command, operation, source] if command == "embeddings" && operation == "install" => {
            install_embeddings(Path::new(source)).map_err(|error| error.to_string())
        }
        [command, operation] if command == "embeddings" && operation == "status" => {
            embedding_status().map_err(|error| error.to_string())
        }
        [command, operation] if command == "embeddings" && operation == "backfill" => {
            backfill_embeddings().map_err(|error| error.to_string())
        }
        _ => {
            eprintln!(
                "usage: archive mcp | archive embeddings install <model-snapshot> | archive embeddings status | archive embeddings backfill"
            );
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run_mcp() -> Result<(), Box<dyn std::error::Error>> {
    let data_directory = app_data_directory().map_err(std::io::Error::other)?;
    let database = Database::open(data_directory.join("archive.sqlite3"))?;
    tokio::runtime::Runtime::new()?.block_on(mcp::run(database, &data_directory))
}

fn install_embeddings(source: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let data_directory = app_data_directory().map_err(std::io::Error::other)?;
    let directory = Embeddings::install(source, &data_directory)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "model": embeddings::MODEL,
            "model_revision": embeddings::MODEL_REVISION,
            "directory": directory,
        }))?
    );
    Ok(())
}

fn embedding_status() -> Result<(), Box<dyn std::error::Error>> {
    let data_directory = app_data_directory().map_err(std::io::Error::other)?;
    let database = Database::open(data_directory.join("archive.sqlite3"))?;
    let status = Embeddings::new(&data_directory).status(&database)?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

fn backfill_embeddings() -> Result<(), Box<dyn std::error::Error>> {
    let data_directory = app_data_directory().map_err(std::io::Error::other)?;
    let database = Database::open(data_directory.join("archive.sqlite3"))?;
    let sync = Embeddings::new(&data_directory).sync(&database)?;
    println!("{}", serde_json::to_string_pretty(&sync)?);
    Ok(())
}
