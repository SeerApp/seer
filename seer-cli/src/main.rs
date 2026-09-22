mod cli;
mod environment;
mod network;
mod print;
mod report;
mod runs;
mod state_accounts;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
