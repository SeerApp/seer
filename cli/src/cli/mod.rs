use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use storage::Storage;

use crate::runs::run::execute;

mod args;
mod encoding;
mod format;
mod glassbox_cmd;
mod input;
mod present;
mod program;
mod regs;
mod skill;
#[cfg(test)]
mod test;

use args::{Cli, Command};
#[cfg(test)]
pub use input::PathOrValue;
use present::{diff, emit_run, ls, show};

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    trace::init_seer_logger(trace::SeerLogger::from_verbosity(cli.verbose));
    let short = cli.short;
    let storage_home = cli.storage_home.clone();
    let cmd = Command::try_from(cli)?;
    if let Command::Skill { install } = cmd {
        return skill::cmd(install, short);
    }
    let home = match storage_home {
        Some(path) => path,
        None => storage::default_root()?,
    };
    let storage = Arc::new(Storage::open_at(home)?);
    match cmd {
        Command::Skill { .. } => unreachable!(),
        Command::Run(req) => {
            let id = execute(Arc::clone(&storage), req)?;
            emit_run(&storage, id, short)
        }
        Command::Show {
            id,
            tx,
            state,
            account,
            data,
            trace,
            program,
        } => show(
            &storage,
            id,
            tx,
            state,
            &account,
            &data,
            trace,
            program.as_deref(),
            short,
        ),
        Command::Ls {
            tree,
            head,
            skip,
            from,
            status,
        } => ls(&storage, tree, head, skip, from, status, short),
        Command::Diff { a, b } => diff(&storage, a, b, short),
        Command::Query(sql) => {
            println!("{}", storage.db.query(&sql)?);
            Ok(())
        }
        cmd @ Command::Program { .. } => program::cmd(&storage, cmd, short),
        cmd @ Command::Regs { .. } => regs::cmd(&storage, cmd, short),
        cmd @ Command::Glassbox { .. } => glassbox_cmd::cmd(&storage, cmd, short),
    }
}
