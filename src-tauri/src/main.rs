mod database;
mod mcp;

use database::{Database, Document, DocumentSummary, ReferenceSummary};
use tauri::{Manager, State};

#[tauri::command]
fn get_or_create_daily(database: State<'_, Database>, day: String) -> Result<Document, String> {
    database
        .get_or_create_daily(&day)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_note(
    database: State<'_, Database>,
    day: String,
    visibility: Option<String>,
) -> Result<Document, String> {
    database
        .create_note(&day, visibility.as_deref().unwrap_or("shared"))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_document(database: State<'_, Database>, id: i64) -> Result<Document, String> {
    database.get_document(id).map_err(|error| error.to_string())
}

#[tauri::command]
fn update_document_body(
    database: State<'_, Database>,
    id: i64,
    expected_body: String,
    body: String,
) -> Result<Document, String> {
    database
        .replace_document_body(id, &expected_body, &body)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_note(database: State<'_, Database>, id: i64) -> Result<(), String> {
    database.delete_note(id).map_err(|error| error.to_string())
}

#[tauri::command]
fn search_documents(
    database: State<'_, Database>,
    active_day: String,
    query: String,
) -> Result<Vec<DocumentSummary>, String> {
    database
        .search_documents(&active_day, &query)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn resolve_references(
    database: State<'_, Database>,
    ids: Vec<i64>,
) -> Result<Vec<ReferenceSummary>, String> {
    database
        .resolve_references(&ids)
        .map_err(|error| error.to_string())
}

fn app_database_path() -> Result<std::path::PathBuf, String> {
    dirs::data_dir()
        .map(|path| path.join("dev.kuchmenko.archive").join("archive.sqlite3"))
        .ok_or_else(|| "application data directory is unavailable".to_owned())
}

fn run_gui() {
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let database_path = app_database_path().map_err(std::io::Error::other)?;
            app.manage(Database::open(database_path)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_or_create_daily,
            create_note,
            get_document,
            update_document_body,
            delete_note,
            search_documents,
            resolve_references
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Archive");
}

fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [] => run_gui(),
        [argument] if argument == "mcp" => {
            let database = app_database_path()
                .map_err(std::io::Error::other)
                .and_then(|path| Database::open(path).map_err(std::io::Error::other));
            let result = database.and_then(|database| {
                tokio::runtime::Runtime::new().and_then(|runtime| {
                    runtime
                        .block_on(mcp::run(database))
                        .map_err(|error| std::io::Error::other(error.to_string()))
                })
            });
            if let Err(error) = result {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("usage: archive [mcp]");
            std::process::exit(2);
        }
    }
}
