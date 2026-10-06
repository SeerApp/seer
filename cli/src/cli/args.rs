use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use solana_pubkey::Pubkey;

use super::encoding::TxEncoding;
use super::format::LsStatus;
use super::input::{PathOrValue, INPUT_HELP};
use crate::runs::run::Request;
use crate::state_accounts::Patch;

const ORIENT: &str = "\
Seer replays Solana transactions locally. You work in runs.

  seer run --sig <SIGNATURE> --url <RPC>
  seer run --sig <SIGNATURE> --url <RPC> --historical
  seer show 1
  seer run --from 1 --account <PUBKEY> --lamports 0
  seer glassbox 1 --ix 0
  seer ls
";

#[derive(Parser, Debug)]
#[command(
    name = "seer",
    about = "Replay Solana transactions locally. You work in runs.",
    before_help = ORIENT,
    arg_required_else_help = true
)]
pub(in crate::cli) struct Cli {
    #[arg(long, global = true, value_name = "DIR", help = "Storage root")]
    pub(super) storage_home: Option<std::path::PathBuf>,
    #[arg(
        short = 'v',
        action = clap::ArgAction::Count,
        global = true,
        help = "Log to stderr (-v warn, -vv info, -vvv debug)"
    )]
    pub(super) verbose: u8,
    #[arg(long, global = true, help = "Compact JSON on stdout, no next: footer")]
    pub(super) short: bool,
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    Run(RunCli),
    Show(ShowCli),
    Ls(LsCli),
    Diff(DiffCli),
    #[command(hide = true)]
    Query(QueryArgs),
    Program(ProgramCli),
    Regs(RegsCli),
    Glassbox(GlassboxCli),
    Skill(SkillCli),
}

#[derive(Parser, Debug)]
#[command(
    about = "Print the agent skill, or install it for Cursor, Claude Code, Codex, and other skill-using agents"
)]
struct SkillCli {
    #[command(subcommand)]
    cmd: Option<SkillCmd>,
}

#[derive(Subcommand, Debug)]
enum SkillCmd {
    #[command(about = "Write SKILL.md into user-level skill directories")]
    Install,
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
        help = "Solana JSON-RPC URL. With --historical this fetches only the transaction message"
    )]
    url: Option<String>,
    #[arg(
        long,
        help = "Accounts from the signature's historical state. Requires --sig and --url"
    )]
    historical: bool,
    #[arg(long, hide = true, value_name = "FILE")]
    zst: Option<std::path::PathBuf>,
    #[arg(long, hide = true, default_value = crate::captures::CAPTURES_URL)]
    server_url: String,
    #[arg(
        long,
        action = clap::ArgAction::Set,
        value_name = "BOOL",
        help = "Verify signatures (true or false)"
    )]
    sigverify: Option<bool>,
    #[arg(
        long,
        action = clap::ArgAction::Set,
        value_name = "BOOL",
        help = "Check the recent blockhash (true or false)"
    )]
    blockhash_check: Option<bool>,
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
    #[arg(long, help = "List accounts in the input state")]
    state: bool,
    #[arg(
        long,
        value_name = "PUBKEY",
        action = clap::ArgAction::Append,
        help = "Account meta keyed by pubkey"
    )]
    account: Vec<String>,
    #[arg(
        long,
        value_name = "PUBKEY",
        action = clap::ArgAction::Append,
        help = "Account data hex keyed by pubkey"
    )]
    data: Vec<String>,
    #[arg(long, help = "Print stored traces")]
    trace: bool,
    #[arg(
        long,
        value_name = "PUBKEY",
        num_args = 0..=1,
        action = clap::ArgAction::Append,
        default_missing_value = "",
        help = "Program index: elf + idl hashes, keyed by pubkey"
    )]
    program: Vec<String>,
}

#[derive(Parser, Debug)]
#[command(about = "List runs")]
struct LsCli {
    #[arg(long, help = "Show fork lineage")]
    tree: bool,
    #[arg(
        long,
        value_name = "RUN",
        help = "Children of this run (subtree with --tree)"
    )]
    from: Option<i64>,
    #[arg(long, value_enum, help = "Filter by status")]
    status: Option<LsStatus>,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 20,
        help = "First N of the top-level array (0 = all)"
    )]
    head: usize,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 0,
        help = "Drop first N of the top-level array"
    )]
    skip: usize,
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

#[derive(Parser, Debug)]
#[command(about = "Show a program's disassembly or lifted CFG")]
struct ProgramCli {
    #[arg(value_name = "HASH|PUBKEY")]
    target: Option<String>,
    #[arg(long, value_name = "RUN", help = "Resolve pubkey through this run")]
    run: Option<i64>,
    #[arg(long, help = "Print disassembly")]
    disasm: bool,
    #[arg(long, help = "Print lifted CFG")]
    lifted: bool,
    #[arg(
        long,
        value_name = "N",
        help = "First N insns or blocks after the window (0 = all). Default 20"
    )]
    head: Option<usize>,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 0,
        help = "Drop first N of the window"
    )]
    skip: usize,
    #[arg(long, value_name = "N", help = "Last N of the window")]
    tail: Option<usize>,
    #[arg(long, value_name = "PC", help = "Inclusive start PC")]
    start: Option<u64>,
    #[arg(long, value_name = "PC", help = "Inclusive end PC")]
    end: Option<u64>,
    #[arg(
        long,
        value_name = "PC",
        help = "One PC (disasm) or the block that contains it (lifted)"
    )]
    pc: Option<u64>,
    #[arg(
        long,
        value_name = "SUBSTR",
        help = "Keep lines or blocks that contain this text"
    )]
    contains: Option<String>,
}

#[derive(Parser, Debug)]
#[command(about = "Show symbolic path conditions for one instruction of a run")]
struct GlassboxCli {
    #[arg(value_name = "RUN")]
    id: i64,
    #[arg(long, value_name = "N", required = true, help = "Instruction index")]
    ix: i64,
    #[arg(long, help = "Recompute and replace the stored report")]
    force: bool,
    #[arg(
        long,
        value_name = "N",
        help = "First N path conditions after filters (0 = all). Default 20"
    )]
    head: Option<usize>,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 0,
        help = "Drop first N path conditions"
    )]
    skip: usize,
    #[arg(long, value_name = "N", help = "Last N path conditions")]
    tail: Option<usize>,
    #[arg(long, value_name = "ORDER", help = "Inclusive start step order")]
    start: Option<u64>,
    #[arg(long, value_name = "ORDER", help = "Inclusive end step order")]
    end: Option<u64>,
    #[arg(long, help = "Only taken branches")]
    taken_only: bool,
    #[arg(
        long,
        help = "Every stored load def, not only those in shown conditions"
    )]
    all_load_defs: bool,
    #[arg(long, value_name = "MODE", value_parser = parse_hide_mode)]
    hide_num_account_children: Option<glassbox::HideMode>,
    #[arg(long, value_name = "SPEC", value_parser = parse_hide_data_len)]
    hide_data_len_children: Option<glassbox::HideDataLenChildren>,
    #[arg(long, value_name = "MODE", value_parser = parse_hide_mode)]
    hide_signer: Option<glassbox::HideMode>,
    #[arg(long, value_name = "MODE", value_parser = parse_hide_mode)]
    hide_writable: Option<glassbox::HideMode>,
    #[arg(long, value_name = "MODE", value_parser = parse_hide_mode)]
    hide_executable: Option<glassbox::HideMode>,
}

#[derive(Parser, Debug)]
#[command(about = "Show r0–r10 at recorded steps of one instruction")]
struct RegsCli {
    #[arg(value_name = "RUN")]
    id: i64,
    #[arg(long, value_name = "N", required = true, help = "Instruction index")]
    ix: i64,
    #[arg(
        long,
        value_name = "N",
        help = "First N steps after filters (0 = all). Default 20"
    )]
    head: Option<usize>,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 0,
        help = "Drop first N steps"
    )]
    skip: usize,
    #[arg(long, value_name = "N", help = "Last N steps")]
    tail: Option<usize>,
    #[arg(long, value_name = "ORDER", help = "Inclusive start step order")]
    start: Option<u64>,
    #[arg(long, value_name = "ORDER", help = "Inclusive end step order")]
    end: Option<u64>,
    #[arg(long, value_name = "ORDER", help = "One step order")]
    order: Option<u64>,
    #[arg(long, value_name = "PC", help = "Keep steps at this PC")]
    pc: Option<u64>,
    #[arg(
        long = "reg",
        value_name = "0,7,10",
        help = "Register indexes for --changed (default all 11). JSON still emits r0–r10"
    )]
    regs: Option<String>,
    #[arg(long, help = "Keep steps where selected registers changed")]
    changed: bool,
    #[arg(long, value_name = "PUBKEY", help = "One program id")]
    program: Option<String>,
    #[arg(long, help = "Sparse stored deltas instead of reconstructed r0–r10")]
    delta: bool,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    Run(Request),
    Show {
        id: i64,
        tx: bool,
        state: bool,
        account: Vec<Pubkey>,
        data: Vec<Pubkey>,
        trace: bool,
        program: Option<Vec<Pubkey>>,
    },
    Ls {
        tree: bool,
        head: usize,
        skip: usize,
        from: Option<i64>,
        status: Option<LsStatus>,
    },
    Diff {
        a: i64,
        b: i64,
    },
    Query(String),
    Program {
        target: Option<String>,
        run: Option<i64>,
        disasm: bool,
        skip: usize,
        head: usize,
        tail: Option<usize>,
        start: Option<u64>,
        end: Option<u64>,
        pc: Option<u64>,
        contains: Option<String>,
    },
    Regs {
        id: i64,
        ix: i64,
        skip: usize,
        head: usize,
        tail: Option<usize>,
        start: Option<u64>,
        end: Option<u64>,
        order: Option<u64>,
        pc: Option<u64>,
        regs: Vec<usize>,
        changed: bool,
        program: Option<Pubkey>,
        delta: bool,
    },
    Glassbox {
        id: i64,
        ix: i64,
        force: bool,
        skip: usize,
        head: usize,
        tail: Option<usize>,
        start: Option<u64>,
        end: Option<u64>,
        taken_only: bool,
        all_load_defs: bool,
        hide_num_account_children: glassbox::HideMode,
        hide_data_len_children: glassbox::HideDataLenChildren,
        hide_signer: glassbox::HideMode,
        hide_writable: glassbox::HideMode,
        hide_executable: glassbox::HideMode,
    },
    Skill {
        install: bool,
    },
}

impl TryFrom<Cli> for Command {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        match cli.command {
            CliCommand::Run(args) => Ok(Self::Run(Request::try_from(args)?)),
            CliCommand::Show(args) => Ok(Self::Show {
                id: args.id,
                tx: args.tx,
                state: args.state,
                account: args
                    .account
                    .iter()
                    .map(|s| parse_pubkey(s))
                    .collect::<Result<Vec<_>>>()?,
                data: args
                    .data
                    .iter()
                    .map(|s| parse_pubkey(s))
                    .collect::<Result<Vec<_>>>()?,
                trace: args.trace,
                program: parse_program_flag(&args.program)?,
            }),
            CliCommand::Ls(args) => Ok(Self::Ls {
                tree: args.tree,
                head: args.head,
                skip: args.skip,
                from: args.from,
                status: args.status,
            }),
            CliCommand::Diff(args) => Ok(Self::Diff {
                a: args.a,
                b: args.b,
            }),
            CliCommand::Query(args) => Ok(Self::Query(args.sql)),
            CliCommand::Program(args) => {
                if args.disasm == args.lifted {
                    bail!("exactly one of --disasm or --lifted");
                }
                if args.head.is_some() && args.tail.is_some() {
                    bail!("--head and --tail are mutually exclusive");
                }
                if let Some(pc) = args.pc {
                    if args.start.is_some_and(|s| pc < s) || args.end.is_some_and(|e| pc > e) {
                        bail!("--pc is outside --start/--end");
                    }
                }
                Ok(Self::Program {
                    target: args.target,
                    run: args.run,
                    disasm: args.disasm,
                    skip: args.skip,
                    head: args
                        .head
                        .unwrap_or(if args.tail.is_some() { 0 } else { 20 }),
                    tail: args.tail,
                    start: args.start,
                    end: args.end,
                    pc: args.pc,
                    contains: args.contains,
                })
            }
            CliCommand::Regs(args) => {
                if args.head.is_some() && args.tail.is_some() {
                    bail!("--head and --tail are mutually exclusive");
                }
                if args.order.is_some() && (args.start.is_some() || args.end.is_some()) {
                    bail!("--order cannot be combined with --start or --end");
                }
                Ok(Self::Regs {
                    id: args.id,
                    ix: args.ix,
                    skip: args.skip,
                    head: args
                        .head
                        .unwrap_or(if args.tail.is_some() { 0 } else { 20 }),
                    tail: args.tail,
                    start: args.start,
                    end: args.end,
                    order: args.order,
                    pc: args.pc,
                    regs: parse_reg_list(args.regs.as_deref())?,
                    changed: args.changed,
                    program: args.program.as_deref().map(parse_pubkey).transpose()?,
                    delta: args.delta,
                })
            }
            CliCommand::Glassbox(args) => {
                if args.head.is_some() && args.tail.is_some() {
                    bail!("--head and --tail are mutually exclusive");
                }
                Ok(Self::Glassbox {
                    id: args.id,
                    ix: args.ix,
                    force: args.force,
                    skip: args.skip,
                    head: args
                        .head
                        .unwrap_or(if args.tail.is_some() { 0 } else { 20 }),
                    tail: args.tail,
                    start: args.start,
                    end: args.end,
                    taken_only: args.taken_only,
                    all_load_defs: args.all_load_defs,
                    hide_num_account_children: args.hide_num_account_children.unwrap_or_default(),
                    hide_data_len_children: args.hide_data_len_children.unwrap_or_default(),
                    hide_signer: args.hide_signer.unwrap_or_default(),
                    hide_writable: args.hide_writable.unwrap_or_default(),
                    hide_executable: args.hide_executable.unwrap_or_default(),
                })
            }
            CliCommand::Skill(args) => Ok(Self::Skill {
                install: matches!(args.cmd, Some(SkillCmd::Install)),
            }),
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
        if args.historical && args.sig.is_none() {
            bail!("--historical requires --sig");
        }
        if args.zst.is_some()
            && (args.sig.is_some() || args.tx.is_some() || args.from.is_some() || args.historical)
        {
            bail!("--zst cannot be combined with --sig, --tx, --from, or --historical");
        }
        if args.zst.is_some() && args.url.is_none() {
            bail!("--zst requires --url");
        }
        if args.sig.is_none() && args.tx.is_none() && args.from.is_none() && args.zst.is_none() {
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
            historical: args.historical,
            zst: args.zst,
            server_url: args.server_url,
            sigverify: args.sigverify,
            blockhash_check: args.blockhash_check,
            patch,
        })
    }
}

pub(in crate::cli) fn parse_pubkey(s: &str) -> Result<Pubkey> {
    s.parse().context("pubkey")
}

fn parse_reg_list(s: Option<&str>) -> Result<Vec<usize>> {
    let Some(s) = s else {
        return Ok((0..11).collect());
    };
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let i: usize = part.parse().context("--reg")?;
        if i > 10 {
            bail!("--reg {i} is not in 0..=10");
        }
        out.push(i);
    }
    if out.is_empty() {
        bail!("--reg needs at least one index");
    }
    Ok(out)
}

fn parse_hide_mode(s: &str) -> Result<glassbox::HideMode, String> {
    glassbox::HideMode::parse(s)
        .ok_or_else(|| format!("expected words, constraints, or full (got {s:?})"))
}

fn parse_hide_data_len(s: &str) -> Result<glassbox::HideDataLenChildren, String> {
    glassbox::HideDataLenChildren::parse(s).ok_or_else(|| {
        format!("expected full or comma-separated account indices < 128 (got {s:?})")
    })
}

fn parse_program_flag(raw: &[String]) -> Result<Option<Vec<Pubkey>>> {
    if raw.is_empty() {
        return Ok(None);
    }
    if raw.iter().any(|s| s.is_empty()) {
        return Ok(Some(Vec::new()));
    }
    raw.iter()
        .map(|s| parse_pubkey(s))
        .collect::<Result<Vec<_>>>()
        .map(Some)
}
