mod database;
mod mcp;
mod merman;

use database::Database;

fn app_database_path() -> Result<std::path::PathBuf, String> {
    dirs::data_dir()
        .map(|path| path.join("dev.kuchmenko.archive").join("archive.sqlite3"))
        .ok_or_else(|| "application data directory is unavailable".to_owned())
}

fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
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
            eprintln!("usage: archive mcp");
            std::process::exit(2);
        }
    }
}
