use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use solana_pubkey::Pubkey;
use storage::{RunRow, Storage};

use crate::environment::Environment;
use crate::runs::run::{execute, Request};
use crate::state_accounts::{Patch, StateAccounts};

mod encoding;
mod format;
mod input;
#[cfg(test)]
mod test;

use encoding::TxEncoding;
use format::{diff_text, ls_text, run_card, run_json};
use input::INPUT_HELP;

pub use input::PathOrValue;

const ORIENT: &str = "\
Seer replays Solana transactions locally. You work in runs.

  seer run --sig <SIGNATURE> --url <RPC>
  seer show 1
  seer run --from 1 --account <PUBKEY> --lamports 0
  seer ls
";

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    let home = match &cli.storage_home {
        Some(path) => path.clone(),
        None => storage::default_root()?,
    };
    let storage = Storage::open_at(home)?;
    match Command::try_from(cli)? {
        Command::Status => status(&storage, json),
        Command::Run(req) => {
            let id = execute(&storage, req)?;
            emit_run(&storage, id, json)
        }
        Command::Show {
            id,
            tx,
            state,
            account,
            trace,
        } => show(&storage, id, tx, state, account, trace, json),
        Command::Ls { tree } => ls(&storage, tree, json),
        Command::Diff { a, b } => diff(&storage, a, b, json),
        Command::Query(sql) => {
            println!("{}", storage.db.query(&sql)?);
            Ok(())
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "seer",
    about = "Replay Solana transactions locally. You work in runs.",
    before_help = ORIENT
)]
struct Cli {
    #[arg(long, global = true, value_name = "DIR", help = "Storage root")]
    storage_home: Option<std::path::PathBuf>,
    #[arg(long, global = true, help = "Print JSON")]
    json: bool,
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    Run(RunCli),
    Show(ShowCli),
    Ls(LsCli),
    Diff(DiffCli),
    #[command(hide = true)]
    Query(QueryArgs),
}

#[derive(Parser, Debug)]
#[command(about = "Create a run from a signature, a transaction, or a previous run")]
struct RunCli {
    #[command(flatten)]
    encoding: TxEncoding,
    #[arg(long, value_name = "TX", help = INPUT_HELP)]
    tx: Option<PathOrValue>,
    #[arg(long, value_name = "SIGNATURE", help = "On-chain signature")]
    sig: Option<PathOrValue>,
    #[arg(long, value_name = "RUN", help = "Fork this run")]
    from: Option<i64>,
    #[arg(
        long,
        env = "SEER_RPC",
        value_name = "RPC_URL",
        help = "Solana JSON-RPC URL"
    )]
    url: Option<String>,
    #[arg(
        long = "env",
        value_name = "JSON",
        help = "SVM environment (slot, clock, compute, airdrop)"
    )]
    environment: Option<PathOrValue>,
    #[arg(long, value_name = "PUBKEY", help = "Account to patch")]
    account: Option<String>,
    #[arg(long, help = "Set account lamports")]
    lamports: Option<u64>,
    #[arg(long, value_name = "PUBKEY", help = "Set account owner")]
    owner: Option<String>,
    #[arg(long, value_name = "DATA", help = INPUT_HELP)]
    data: Option<PathOrValue>,
    #[arg(long, help = "Set account executable (true or false)")]
    executable: Option<bool>,
}

#[derive(Parser, Debug)]
#[command(about = "Inspect a run")]
struct ShowCli {
    #[arg(value_name = "RUN")]
    id: i64,
    #[arg(long, help = "Print the transaction")]
    tx: bool,
    #[arg(long, help = "Print the input state")]
    state: bool,
    #[arg(long, value_name = "PUBKEY", help = "Print one account")]
    account: Option<String>,
    #[arg(long, help = "Print traces")]
    trace: bool,
}

#[derive(Parser, Debug)]
#[command(about = "List runs")]
struct LsCli {
    #[arg(long, help = "Show fork lineage")]
    tree: bool,
}

#[derive(Parser, Debug)]
#[command(about = "Compare two runs")]
struct DiffCli {
    #[arg(value_name = "RUN")]
    a: i64,
    #[arg(value_name = "RUN")]
    b: i64,
}

#[derive(Parser, Debug)]
struct QueryArgs {
    #[arg(value_name = "SQL")]
    sql: String,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    Status,
    Run(Request),
    Show {
        id: i64,
        tx: bool,
        state: bool,
        account: Option<Pubkey>,
        trace: bool,
    },
    Ls {
        tree: bool,
    },
    Diff {
        a: i64,
        b: i64,
    },
    Query(String),
}

impl TryFrom<Cli> for Command {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        match cli.command {
            None => Ok(Self::Status),
            Some(CliCommand::Run(args)) => Ok(Self::Run(Request::try_from(args)?)),
            Some(CliCommand::Show(args)) => Ok(Self::Show {
                id: args.id,
                tx: args.tx,
                state: args.state,
                account: args.account.as_deref().map(parse_pubkey).transpose()?,
                trace: args.trace,
            }),
            Some(CliCommand::Ls(args)) => Ok(Self::Ls { tree: args.tree }),
            Some(CliCommand::Diff(args)) => Ok(Self::Diff {
                a: args.a,
                b: args.b,
            }),
            Some(CliCommand::Query(args)) => Ok(Self::Query(args.sql)),
        }
    }
}

impl TryFrom<RunCli> for Request {
    type Error = anyhow::Error;

    fn try_from(args: RunCli) -> Result<Self> {
        if args.sig.is_some() && (args.tx.is_some() || args.from.is_some()) {
            bail!("--sig cannot be combined with --tx or --from");
        }
        if args.sig.is_some() && args.url.is_none() {
            bail!("--sig requires --url");
        }
        if args.sig.is_none() && args.tx.is_none() && args.from.is_none() {
            bail!("need --sig, --tx, or --from");
        }
        if args.account.is_none()
            && (args.lamports.is_some()
                || args.owner.is_some()
                || args.data.is_some()
                || args.executable.is_some())
        {
            bail!("--account required for account patches");
        }
        if args.account.is_some()
            && args.lamports.is_none()
            && args.owner.is_none()
            && args.data.is_none()
            && args.executable.is_none()
        {
            bail!("pass --lamports, --owner, --data, or --executable");
        }
        let patch = match args.account {
            None => None,
            Some(account) => Some(Patch {
                account: parse_pubkey(&account)?,
                lamports: args.lamports,
                owner: args.owner.as_deref().map(parse_pubkey).transpose()?,
                data: args
                    .data
                    .as_ref()
                    .map(PathOrValue::load_bytes)
                    .transpose()?,
                executable: args.executable,
            }),
        };
        Ok(Self {
            tx: args
                .tx
                .as_ref()
                .map(|tx| args.encoding.decode(tx))
                .transpose()?,
            signature: args
                .sig
                .as_ref()
                .map(|s| s.load_text()?.trim().parse().context("signature"))
                .transpose()?,
            from: args.from,
            url: args.url,
            environment: args
                .environment
                .map(|v| v.load_text().and_then(|s| Environment::parse(&s)))
                .transpose()?,
            patch,
        })
    }
}

fn parse_pubkey(s: &str) -> Result<Pubkey> {
    s.parse().context("pubkey")
}

fn status(storage: &Storage, json: bool) -> Result<()> {
    let runs = storage.db.list_runs()?;
    let start = runs.len().saturating_sub(10);
    let recent = &runs[start..];
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "runs": recent.iter().map(run_json).collect::<Vec<_>>(),
            }))?
        );
        return Ok(());
    }
    print!("{ORIENT}");
    if recent.is_empty() {
        print!("\nno runs yet\nnext: seer run --sig <SIGNATURE> --url <RPC>\n");
    } else {
        print!("\n{}", ls_text(recent, false));
    }
    Ok(())
}

fn emit_run(storage: &Storage, id: i64, json: bool) -> Result<()> {
    let row = storage.db.get_run(id)?;
    emit(json, run_json(&row), run_card(&row))
}

fn ls(storage: &Storage, tree: bool, json: bool) -> Result<()> {
    let runs = storage.db.list_runs()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&runs.iter().map(run_json).collect::<Vec<_>>())?
        );
        return Ok(());
    }
    print!("{}", ls_text(&runs, tree));
    Ok(())
}

fn show(
    storage: &Storage,
    id: i64,
    tx: bool,
    state: bool,
    account: Option<Pubkey>,
    trace: bool,
    json: bool,
) -> Result<()> {
    let row = storage.db.get_run(id)?;
    if !tx && !state && account.is_none() && !trace {
        return emit(json, run_json(&row), run_card(&row));
    }
    let mut text = String::new();
    let mut value = serde_json::Map::new();
    if tx {
        let printed = crate::print::transaction(&storage.blob.read(&row.transaction_blob_hash)?)?;
        value.insert("tx".into(), serde_json::from_str(&printed)?);
        text.push_str(&printed);
        if !printed.ends_with('\n') {
            text.push('\n');
        }
    }
    if state {
        let printed = crate::print::state(&storage.blob.read(&row.state_blob_hash)?)?;
        value.insert("state".into(), serde_json::from_str(&printed)?);
        text.push_str(&printed);
        if !printed.ends_with('\n') {
            text.push('\n');
        }
    }
    if let Some(pk) = account {
        let (printed, parsed) = show_account(storage, &row, &pk)?;
        value.insert("account".into(), parsed);
        text.push_str(&printed);
    }
    if trace {
        let mut traces = Vec::new();
        for (ix, hash) in storage.db.list_run_ix(id)? {
            let Some(hash) = hash else {
                continue;
            };
            let printed = crate::print::trace(&storage.blob.read(&hash)?)?;
            traces.push(serde_json::json!({"ix": ix, "trace": printed}));
            text.push_str(&format!("ix {ix}\n{printed}"));
        }
        value.insert("trace".into(), serde_json::Value::Array(traces));
    }
    emit(json, serde_json::Value::Object(value), text)
}

fn diff(storage: &Storage, a: i64, b: i64, json: bool) -> Result<()> {
    let a = storage.db.get_run(a)?;
    let b = storage.db.get_run(b)?;
    let value = serde_json::json!({
        "a": run_json(&a),
        "b": run_json(&b),
        "tx": if a.transaction_blob_hash == b.transaction_blob_hash { "same" } else { "different" },
        "state": if a.state_blob_hash == b.state_blob_hash { "same" } else { "different" },
    });
    emit(json, value, diff_text(&a, &b))
}

fn show_account(
    storage: &Storage,
    row: &RunRow,
    pk: &Pubkey,
) -> Result<(String, serde_json::Value)> {
    let state = StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash)?)?;
    let acct = state
        .0
        .get(pk)
        .with_context(|| format!("{pk} not in run {}", row.id))?;
    let data = storage.blob.read(&acct.data)?;
    let value = serde_json::json!({
        "lamports": acct.lamports.to_string(),
        "owner": acct.owner.to_string(),
        "executable": acct.executable,
        "data_len": data.len(),
    });
    Ok((
        format!("{}\n", serde_json::to_string_pretty(&value)?),
        value,
    ))
}

fn emit(json: bool, value: serde_json::Value, text: String) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        print!("{text}");
    }
    Ok(())
}
