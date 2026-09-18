mod cli;
mod overrides;
mod report;
mod runs;
mod state_accounts;

fn main() {
    let conn = match storage::db::connect() {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("{err:#}");
            std::process::exit(1);
        }
    };
    if let Err(err) = cli::run(&conn) {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
