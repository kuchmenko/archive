mod database;
mod markdown;
mod mcp;
mod merman;

use database::{
    AttachmentSummary, DailyNeighbors, Database, Document, DocumentSummary, ReferenceSummary,
    SyncSnapshot,
};
use tauri::{Manager, State};

#[tauri::command]
async fn render_mermaid(
    source: String,
    diagram_id: String,
) -> Result<merman::MermaidResult, String> {
    tokio::task::spawn_blocking(move || merman::render(&source, &diagram_id))
        .await
        .map_err(|error| error.to_string())
}

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
fn create_project(
    database: State<'_, Database>,
    day: String,
    visibility: Option<String>,
) -> Result<Document, String> {
    database
        .create_project(&day, visibility.as_deref().unwrap_or("shared"))
        .map_err(|error| error.to_string())
}
#[tauri::command]
fn add_document_to_project(
    database: State<'_, Database>,
    project_id: i64,
    document_id: i64,
) -> Result<(), String> {
    database
        .add_document_to_project(project_id, document_id, "user")
        .map_err(|error| error.to_string())
}
#[tauri::command]
fn list_project_documents(
    database: State<'_, Database>,
    project_id: i64,
) -> Result<Vec<Document>, String> {
    database
        .list_project_documents(project_id)
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
    expected_revision: i64,
    body: String,
) -> Result<Document, String> {
    database
        .replace_document_body(id, expected_revision, &body)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn sync_document(
    database: State<'_, Database>,
    id: i64,
    known_revision: i64,
) -> Result<SyncSnapshot, String> {
    database
        .sync_document(id, known_revision)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn update_presence(
    database: State<'_, Database>,
    session_id: String,
    document_id: i64,
) -> Result<(), String> {
    database
        .set_presence(&session_id, "user", document_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_presence(database: State<'_, Database>, session_id: String) -> Result<(), String> {
    database
        .remove_presence(&session_id)
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

#[tauri::command]
fn list_daily_attachments(
    database: State<'_, Database>,
    day: String,
) -> Result<Vec<AttachmentSummary>, String> {
    database
        .list_daily_attachments(&day)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn daily_neighbors(database: State<'_, Database>, day: String) -> Result<DailyNeighbors, String> {
    database
        .daily_neighbors(&day)
        .map_err(|error| error.to_string())
}
#[tauri::command]
fn list_unreviewed_attachments(
    database: State<'_, Database>,
) -> Result<Vec<AttachmentSummary>, String> {
    database
        .list_unreviewed_attachments()
        .map_err(|error| error.to_string())
}
#[tauri::command]
fn get_attachment_by_artifact_id(
    database: State<'_, Database>,
    artifact_id: i64,
) -> Result<Option<AttachmentSummary>, String> {
    database
        .get_attachment_by_artifact_id(artifact_id)
        .map_err(|error| error.to_string())
}
#[tauri::command]
fn mark_attachment_reviewed(
    database: State<'_, Database>,
    artifact_id: i64,
) -> Result<AttachmentSummary, String> {
    database
        .mark_attachment_reviewed(artifact_id)
        .map_err(|error| error.to_string())
}
#[tauri::command]
fn render_markdown(markdown: String) -> String {
    markdown::render(&markdown)
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
            create_project,
            add_document_to_project,
            list_project_documents,
            get_document,
            update_document_body,
            sync_document,
            update_presence,
            remove_presence,
            delete_note,
            search_documents,
            resolve_references,
            list_daily_attachments,
            daily_neighbors,
            list_unreviewed_attachments,
            get_attachment_by_artifact_id,
            mark_attachment_reviewed,
            render_markdown,
            render_mermaid
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
