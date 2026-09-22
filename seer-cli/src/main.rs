mod cli;
mod network;
mod overrides;
mod report;
mod runs;
mod state_accounts;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
