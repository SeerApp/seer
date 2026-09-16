mod encoding;
mod hash;
mod input;
mod state;
#[cfg(test)]
mod test;

use anyhow::Result;
use clap::{Parser, Subcommand};
use solana_transaction::versioned::VersionedTransaction;

pub use encoding::Encoding;
pub use hash::Sha256Hash;
pub use input::PathOrValue;
pub use state::StateObject;

pub fn run() -> Result<()> {
    Command::try_from(Cli::parse()).map(|_| ())
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

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Hash(HashCommand),
}

#[derive(Clone, Debug, PartialEq)]
pub enum HashCommand {
    State(StateObject),
    Transaction(VersionedTransaction),
    Simulation {
        msg_hash: Sha256Hash,
        state_hash: Sha256Hash,
    },
}

impl TryFrom<Cli> for Command {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        match cli.command {
            CliCommand::Hash(hash) => Ok(Self::Hash(HashCommand::try_from(hash)?)),
        }
    }
}

impl TryFrom<CliHash> for HashCommand {
    type Error = anyhow::Error;

    fn try_from(hash: CliHash) -> Result<Self> {
        match hash {
            CliHash::State(args) => Ok(Self::State(StateObject::parse(&args.state)?)),
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
