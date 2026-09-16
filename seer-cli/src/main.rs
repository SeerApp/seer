mod cli;
mod runs;
mod storage;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
