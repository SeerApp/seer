mod captures;
mod cli;
mod network;
mod print;
mod report;
mod runs;
mod state_accounts;
mod sysvars;

fn main() {
    trace::install_vm_hooks();
    if let Err(err) = cli::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
