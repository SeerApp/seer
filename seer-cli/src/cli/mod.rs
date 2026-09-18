// The seer binary goes through this module because clap lives here.
// Parse, normalise, dispatch to runs/storage, format user-facing output.
// Do not implement storage or run logic here.

mod encoding;
mod hash;
mod input;
#[cfg(test)]
mod test;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rusqlite::Connection;
use solana_transaction::versioned::VersionedTransaction;

use crate::overrides::Overrides;
use crate::runs::hash::{hash_simulation, hash_state, hash_transaction};
use crate::runs::run::run_simulation;
use crate::state_accounts::StateAccounts;

pub use encoding::Encoding;
pub use hash::Sha256Hash;
pub use input::PathOrValue;

pub fn run(conn: &Connection) -> Result<()> {
    match Command::try_from(Cli::parse())? {
        Command::Hash(HashCommand::State(state)) => {
            println!(
                "Account state stored at hash {}",
                hex::encode(hash_state(&state)?)
            );
            Ok(())
        }
        Command::Hash(HashCommand::Transaction(tx)) => {
            println!(
                "Transaction stored at hash {}",
                hex::encode(hash_transaction(&tx)?)
            );
            Ok(())
        }
        Command::Hash(HashCommand::Simulation {
            msg_hash,
            state_hash,
        }) => {
            hash_simulation(conn, &msg_hash.0, &state_hash.0)?;
            println!("Simulation stored");
            Ok(())
        }
        Command::Run(RunCommand::Simulation {
            tx_hash,
            state_hash,
            overrides,
        }) => {
            println!(
                "Run stored at id {}",
                run_simulation(conn, &tx_hash.0, &state_hash.0, overrides)?
            );
            Ok(())
        }
    }
}

const INPUT_HELP: &str = "File path, @path, or the value itself";

#[derive(Parser, Debug)]
#[command(name = "seer")]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    #[command(subcommand)]
    Hash(CliHash),
    #[command(subcommand)]
    Run(CliRun),
}

#[derive(Subcommand, Debug)]
enum CliHash {
    State(StateArgs),
    Transaction(TransactionArgs),
    Simulation(SimulationArgs),
}

#[derive(Parser, Debug)]
struct StateArgs {
    #[arg(value_name = "STATE", help = INPUT_HELP)]
    state: PathOrValue,
}

#[derive(Parser, Debug)]
struct TransactionArgs {
    #[arg(long, group = "encoding", help = "Input is base64-encoded")]
    b64: bool,
    #[arg(long, group = "encoding", help = "Input is base58-encoded")]
    b58: bool,
    #[arg(long, group = "encoding", help = "Input is hex-encoded")]
    hex: bool,
    #[arg(long, group = "encoding", help = "Input is Solana wire bytes")]
    wire: bool,
    #[arg(value_name = "TX", help = INPUT_HELP)]
    tx: PathOrValue,
}

#[derive(Parser, Debug)]
struct SimulationArgs {
    #[arg(value_name = "MSG_HASH", help = INPUT_HELP)]
    msg_hash: PathOrValue,
    #[arg(value_name = "STATE_HASH", help = INPUT_HELP)]
    state_hash: PathOrValue,
}

#[derive(Subcommand, Debug)]
enum CliRun {
    Simulation(RunSimulationArgs),
}

#[derive(Parser, Debug)]
struct RunSimulationArgs {
    #[arg(value_name = "TX_HASH", help = INPUT_HELP)]
    tx_hash: PathOrValue,
    #[arg(value_name = "STATE_HASH", help = INPUT_HELP)]
    state_hash: PathOrValue,
    #[arg(value_name = "OVERRIDES", help = "JSON object of LiteSVM overrides")]
    overrides: Option<PathOrValue>,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    Hash(HashCommand),
    Run(RunCommand),
}

#[derive(Clone, Debug, PartialEq)]
pub enum HashCommand {
    State(StateAccounts),
    Transaction(VersionedTransaction),
    Simulation {
        msg_hash: Sha256Hash,
        state_hash: Sha256Hash,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum RunCommand {
    Simulation {
        tx_hash: Sha256Hash,
        state_hash: Sha256Hash,
        overrides: Overrides,
    },
}

impl TryFrom<Cli> for Command {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        match cli.command {
            CliCommand::Hash(hash) => Ok(Self::Hash(HashCommand::try_from(hash)?)),
            CliCommand::Run(run) => Ok(Self::Run(RunCommand::try_from(run)?)),
        }
    }
}

impl TryFrom<CliHash> for HashCommand {
    type Error = anyhow::Error;

    fn try_from(hash: CliHash) -> Result<Self> {
        match hash {
            CliHash::State(args) => Ok(Self::State(StateAccounts::from_bytes(
                args.state.load_text()?.trim().as_bytes(),
            )?)),
            CliHash::Transaction(args) => {
                let encoding = if args.b64 {
                    Encoding::Base64
                } else if args.b58 {
                    Encoding::Base58
                } else if args.hex {
                    Encoding::Hex
                } else if args.wire {
                    Encoding::Wire
                } else {
                    Encoding::Json
                };
                Ok(Self::Transaction(encoding.decode(&args.tx.load_bytes()?)?))
            }
            CliHash::Simulation(args) => Ok(Self::Simulation {
                msg_hash: Sha256Hash::parse(&args.msg_hash)?,
                state_hash: Sha256Hash::parse(&args.state_hash)?,
            }),
        }
    }
}

impl TryFrom<CliRun> for RunCommand {
    type Error = anyhow::Error;

    fn try_from(run: CliRun) -> Result<Self> {
        match run {
            CliRun::Simulation(args) => Ok(Self::Simulation {
                tx_hash: Sha256Hash::parse(&args.tx_hash)?,
                state_hash: Sha256Hash::parse(&args.state_hash)?,
                overrides: match args.overrides {
                    Some(v) => serde_json::from_str(v.load_text()?.trim())?,
                    None => Overrides::default(),
                },
            }),
        }
    }
}
