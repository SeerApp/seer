// The seer binary goes through this module because clap lives here.
// Parse, normalise, dispatch to runs/storage, format user-facing output.
// Do not implement storage or run logic here.

mod encoding;
mod hash;
mod input;
#[cfg(test)]
mod test;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use storage::{Mode, Storage};

use crate::overrides::Overrides;
use crate::runs::hash::{hash_simulation, hash_state, hash_transaction, hash_transaction_accounts};
use crate::runs::run::{run as run_tx, run_signature, run_simulation};
use crate::state_accounts::StateAccounts;

pub use hash::Sha256Hash;
pub use input::PathOrValue;

use encoding::{TxEncoding, TxInput};
use input::INPUT_HELP;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let home = match &cli.storage_home {
        Some(path) => path.clone(),
        None => storage::default_root()?,
    };
    let storage = Storage::open_at(home, cli.storage_mode)?;
    match Command::try_from(cli)? {
        Command::Hash(HashCommand::State(state)) => {
            println!(
                "Account state stored at hash {}",
                hex::encode(hash_state(&storage, &state)?)
            );
            Ok(())
        }
        Command::Hash(HashCommand::Transaction(tx)) => {
            println!(
                "Transaction stored at hash {}",
                hex::encode(hash_transaction(&storage, &tx)?)
            );
            Ok(())
        }
        Command::Hash(HashCommand::TransactionAccounts { tx, url }) => {
            println!(
                "Account state stored at hash {}",
                hex::encode(hash_transaction_accounts(&storage, &tx, &url)?)
            );
            Ok(())
        }
        Command::Hash(HashCommand::Simulation {
            msg_hash,
            state_hash,
        }) => {
            hash_simulation(&storage, &msg_hash.0, &state_hash.0)?;
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
                run_simulation(&storage, &tx_hash.0, &state_hash.0, overrides)?
            );
            Ok(())
        }
        Command::Run(RunCommand::Transaction { tx, url, overrides }) => {
            println!(
                "Run stored at id {}",
                run_tx(&storage, &tx, &url, overrides)?
            );
            Ok(())
        }
        Command::Run(RunCommand::Signature {
            signature,
            url,
            overrides,
        }) => {
            println!(
                "Run stored at id {}",
                run_signature(&storage, &signature, &url, overrides)?
            );
            Ok(())
        }
        Command::PrintTransaction(hash) => {
            println!("{}", storage.blob.print_transaction(&hash.0)?);
            Ok(())
        }
        Command::PrintState(hash) => {
            println!("{}", storage.blob.print_state(&hash.0)?);
            Ok(())
        }
        Command::Query(sql) => {
            println!("{}", storage.db.query(&sql)?);
            Ok(())
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "seer")]
struct Cli {
    #[arg(long, global = true, value_name = "DIR", help = "Storage root")]
    storage_home: Option<std::path::PathBuf>,
    #[arg(
        long,
        global = true,
        default_value_t = Mode::Default,
        help = "default (write) or compare"
    )]
    storage_mode: Mode,
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    #[command(subcommand)]
    Hash(CliHash),
    Run(RunCli),
    #[command(subcommand)]
    Print(CliPrint),
    Query(QueryArgs),
}

#[derive(Subcommand, Debug)]
enum CliPrint {
    Transaction(PrintArgs),
    State(PrintArgs),
}

#[derive(Parser, Debug)]
struct PrintArgs {
    #[arg(value_name = "SHA256", help = INPUT_HELP)]
    hash: PathOrValue,
}

#[derive(Parser, Debug)]
struct QueryArgs {
    #[arg(value_name = "SQL")]
    sql: String,
}

#[derive(Subcommand, Debug)]
enum CliHash {
    State(StateArgs),
    Transaction(TransactionCli),
    Simulation(SimulationArgs),
}

#[derive(Parser, Debug)]
struct StateArgs {
    #[arg(value_name = "STATE", help = INPUT_HELP)]
    state: PathOrValue,
}

#[derive(Parser, Debug)]
#[command(subcommand_precedence_over_arg = true)]
struct TransactionCli {
    #[command(flatten)]
    encoding: TxEncoding,
    #[arg(value_name = "TX", help = INPUT_HELP)]
    tx: Option<PathOrValue>,
    #[command(subcommand)]
    command: Option<HashTransactionCommand>,
}

#[derive(Subcommand, Debug)]
enum HashTransactionCommand {
    Accounts(TransactionAccountsArgs),
}

#[derive(Parser, Debug)]
struct TransactionAccountsArgs {
    #[command(flatten)]
    input: TxInput,
    #[arg(value_name = "RPC_URL", help = "Solana JSON-RPC URL")]
    url: String,
}

#[derive(Parser, Debug)]
struct SimulationArgs {
    #[arg(value_name = "MSG_HASH", help = INPUT_HELP)]
    msg_hash: PathOrValue,
    #[arg(value_name = "STATE_HASH", help = INPUT_HELP)]
    state_hash: PathOrValue,
}

#[derive(Parser, Debug)]
#[command(subcommand_precedence_over_arg = true)]
struct RunCli {
    #[command(flatten)]
    encoding: TxEncoding,
    #[arg(value_name = "TX", help = INPUT_HELP)]
    tx: Option<PathOrValue>,
    #[arg(value_name = "RPC_URL", help = "Solana JSON-RPC URL")]
    url: Option<String>,
    #[arg(value_name = "OVERRIDES", help = "JSON object of LiteSVM overrides")]
    overrides: Option<PathOrValue>,
    #[command(subcommand)]
    command: Option<CliRunCommand>,
}

#[derive(Subcommand, Debug)]
enum CliRunCommand {
    Simulation(RunSimulationArgs),
    Signature(RunSignatureArgs),
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

#[derive(Parser, Debug)]
struct RunSignatureArgs {
    #[arg(value_name = "SIGNATURE", help = INPUT_HELP)]
    signature: PathOrValue,
    #[arg(value_name = "RPC_URL", help = "Solana JSON-RPC URL")]
    url: String,
    #[arg(value_name = "OVERRIDES", help = "JSON object of LiteSVM overrides")]
    overrides: Option<PathOrValue>,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    Hash(HashCommand),
    Run(RunCommand),
    PrintTransaction(Sha256Hash),
    PrintState(Sha256Hash),
    Query(String),
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum HashCommand {
    State(StateAccounts),
    Transaction(VersionedTransaction),
    TransactionAccounts {
        tx: VersionedTransaction,
        url: String,
    },
    Simulation {
        msg_hash: Sha256Hash,
        state_hash: Sha256Hash,
    },
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum RunCommand {
    Simulation {
        tx_hash: Sha256Hash,
        state_hash: Sha256Hash,
        overrides: Overrides,
    },
    Transaction {
        tx: VersionedTransaction,
        url: String,
        overrides: Overrides,
    },
    Signature {
        signature: Signature,
        url: String,
        overrides: Overrides,
    },
}

impl TryFrom<Cli> for Command {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        match cli.command {
            CliCommand::Hash(hash) => Ok(Self::Hash(HashCommand::try_from(hash)?)),
            CliCommand::Run(run) => Ok(Self::Run(RunCommand::try_from(run)?)),
            CliCommand::Print(CliPrint::Transaction(args)) => {
                Ok(Self::PrintTransaction(Sha256Hash::parse(&args.hash)?))
            }
            CliCommand::Print(CliPrint::State(args)) => {
                Ok(Self::PrintState(Sha256Hash::parse(&args.hash)?))
            }
            CliCommand::Query(args) => Ok(Self::Query(args.sql)),
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
            CliHash::Transaction(args) => match args.command {
                None => {
                    let tx = args.tx.context("missing transaction")?;
                    Ok(Self::Transaction(args.encoding.decode(&tx)?))
                }
                Some(HashTransactionCommand::Accounts(accounts)) => Ok(Self::TransactionAccounts {
                    tx: accounts.input.decode()?,
                    url: accounts.url,
                }),
            },
            CliHash::Simulation(args) => Ok(Self::Simulation {
                msg_hash: Sha256Hash::parse(&args.msg_hash)?,
                state_hash: Sha256Hash::parse(&args.state_hash)?,
            }),
        }
    }
}

impl TryFrom<RunCli> for RunCommand {
    type Error = anyhow::Error;

    fn try_from(run: RunCli) -> Result<Self> {
        match run.command {
            Some(CliRunCommand::Simulation(args)) => Ok(Self::Simulation {
                tx_hash: Sha256Hash::parse(&args.tx_hash)?,
                state_hash: Sha256Hash::parse(&args.state_hash)?,
                overrides: Overrides::parse(args.overrides)?,
            }),
            Some(CliRunCommand::Signature(args)) => Ok(Self::Signature {
                signature: args
                    .signature
                    .load_text()?
                    .trim()
                    .parse()
                    .context("signature")?,
                url: args.url,
                overrides: Overrides::parse(args.overrides)?,
            }),
            None => {
                let tx = run.tx.context("missing transaction")?;
                let url = run.url.context("missing RPC URL")?;
                Ok(Self::Transaction {
                    tx: run.encoding.decode(&tx)?,
                    url,
                    overrides: Overrides::parse(run.overrides)?,
                })
            }
        }
    }
}
