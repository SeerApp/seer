use std::str::FromStr;

use clap::{CommandFactory, Parser};
use solana_pubkey::Pubkey;

use super::args::{Cli, Command};
use super::*;
use crate::runs::run::Request;
use crate::state_accounts::Patch;

fn parse_from<I, T>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Command::try_from(Cli::try_parse_from(args)?)
}

#[test]
fn at_path_only() {
    let dir = std::env::temp_dir().join(format!("seer-input-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("blob");
    std::fs::write(&file, "from-file").unwrap();

    let path = file.to_string_lossy().into_owned();
    assert_eq!(
        PathOrValue::from_str(&path).unwrap().load_text().unwrap(),
        path
    );
    assert_eq!(
        PathOrValue::from_str(&format!("@{path}"))
            .unwrap()
            .load_text()
            .unwrap(),
        "from-file"
    );
    assert_eq!(
        PathOrValue::from_str("not-a-file-xyz")
            .unwrap()
            .load_text()
            .unwrap(),
        "not-a-file-xyz"
    );
    assert!(PathOrValue::from_str("@/definitely/missing/seer-nope")
        .unwrap()
        .load_bytes()
        .is_err());

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn bare_seer_is_help() {
    assert!(parse_from(["seer"]).is_err());
}

#[test]
fn hash_is_not_a_command() {
    assert!(parse_from(["seer", "hash", "state", "{}"]).is_err());
}

#[test]
fn parses_run_sig_tx_from_show_ls_diff() {
    let sig = solana_signature::Signature::default().to_string();
    let Command::Run(Request { signature, url, .. }) = parse_from([
        "seer",
        "run",
        "--sig",
        &sig,
        "--url",
        "https://api.devnet.solana.com",
    ])
    .unwrap() else {
        panic!("expected run --sig");
    };
    assert_eq!(signature.unwrap().to_string(), sig);
    assert_eq!(url.as_deref(), Some("https://api.devnet.solana.com"));

    let help = Cli::command().render_help().to_string();
    assert!(help.contains("--historical"), "{help}");
    assert!(!help.contains("server-url"), "{help}");
    let Command::Run(historical) = parse_from([
        "seer",
        "run",
        "--sig",
        &sig,
        "--url",
        "http://rpc.test",
        "--historical",
        "--server-url",
        "http://captures.test",
    ])
    .unwrap() else {
        panic!("expected historical run");
    };
    assert!(historical.historical);
    assert_eq!(historical.server_url, "http://captures.test");
    let Command::Run(latest) =
        parse_from(["seer", "run", "--sig", &sig, "--url", "http://rpc.test"]).unwrap()
    else {
        panic!("expected latest run");
    };
    assert!(!latest.historical);
    assert_eq!(latest.server_url, crate::captures::CAPTURES_URL);
    assert!(parse_from(["seer", "run", "--sig", &sig, "--historical"]).is_err());
    assert!(parse_from(["seer", "run", "--historical", "--from", "1"]).is_err());

    let pk = "11111111111111111111111111111111";
    let Command::Run(Request { from, patch, .. }) = parse_from([
        "seer",
        "run",
        "--from",
        "1",
        "--account",
        pk,
        "--lamports",
        "0",
    ])
    .unwrap() else {
        panic!("expected run --from");
    };
    assert_eq!(from, Some(1));
    assert_eq!(
        patch,
        Some(Patch {
            account: Pubkey::from_str(pk).unwrap(),
            lamports: Some(0),
            owner: None,
            data: None,
            executable: None,
        })
    );

    let tx = solana_transaction::Transaction::default();
    let json = serde_json::to_string(&tx).unwrap();
    let Command::Run(Request {
        tx: parsed,
        url,
        environment,
        ..
    }) = parse_from([
        "seer",
        "run",
        "--tx",
        &json,
        "--url",
        "https://api.devnet.solana.com",
        "--env",
        r#"{"slot":1}"#,
    ])
    .unwrap()
    else {
        panic!("expected run --tx");
    };
    assert_eq!(
        parsed.unwrap(),
        solana_transaction::versioned::VersionedTransaction::from(tx)
    );
    assert_eq!(url.as_deref(), Some("https://api.devnet.solana.com"));
    assert_eq!(environment.unwrap().slot, Some(1));

    assert_eq!(
        parse_from(["seer", "show", "3", "--trace"]).unwrap(),
        Command::Show {
            id: 3,
            tx: false,
            state: false,
            account: vec![],
            data: vec![],
            trace: true,
            program: None,
        }
    );
    let pk = Pubkey::from_str("11111111111111111111111111111111").unwrap();
    let Command::Show { account, data, .. } = parse_from([
        "seer",
        "show",
        "1",
        "--account",
        "11111111111111111111111111111111",
        "--account",
        "11111111111111111111111111111111",
        "--data",
        "11111111111111111111111111111111",
    ])
    .unwrap() else {
        panic!("expected show filters");
    };
    assert_eq!(account, vec![pk, pk]);
    assert_eq!(data, vec![pk]);
    assert!(parse_from(["seer", "--json", "show", "1"]).is_err());
    assert!(parse_from(["seer", "show", "1", "--head", "1"]).is_err());
    assert_eq!(
        parse_from(["seer", "ls", "--tree"]).unwrap(),
        Command::Ls {
            tree: true,
            head: 20,
            skip: 0,
            from: None,
            status: None,
        }
    );
    assert_eq!(
        parse_from(["seer", "ls", "--skip", "1", "--head", "2"]).unwrap(),
        Command::Ls {
            tree: false,
            head: 2,
            skip: 1,
            from: None,
            status: None,
        }
    );
    assert_eq!(
        parse_from(["seer", "ls", "--from", "3", "--status", "error", "--tree", "--head", "0"])
            .unwrap(),
        Command::Ls {
            tree: true,
            head: 0,
            skip: 0,
            from: Some(3),
            status: Some(super::format::LsStatus::Error),
        }
    );
    assert!(parse_from(["seer", "ls", "--status", "pending"]).is_err());
    assert_eq!(
        parse_from(["seer", "diff", "1", "2"]).unwrap(),
        Command::Diff { a: 1, b: 2 }
    );
}

#[test]
fn parses_program() {
    assert_eq!(
        parse_from(["seer", "show", "1", "--program"]).unwrap(),
        Command::Show {
            id: 1,
            tx: false,
            state: false,
            account: vec![],
            data: vec![],
            trace: false,
            program: Some(vec![]),
        }
    );
    let pk = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
    let Command::Show { program, .. } = parse_from([
        "seer",
        "show",
        "1",
        "--program",
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    ])
    .unwrap() else {
        panic!("expected show --program pubkey");
    };
    assert_eq!(program, Some(vec![pk]));
    assert!(parse_from(["seer", "show", "1", "--disasm"]).is_err());
    let Command::Program {
        disasm,
        head,
        skip,
        tail,
        ..
    } = parse_from(["seer", "program", "ab", "--disasm"]).unwrap()
    else {
        panic!("expected program --disasm");
    };
    assert!(disasm);
    assert_eq!(head, 20);
    assert_eq!(skip, 0);
    assert_eq!(tail, None);
    assert!(parse_from(["seer", "program", "ab"]).is_err());
    assert!(parse_from(["seer", "program", "ab", "--disasm", "--lifted"]).is_err());
    assert!(
        parse_from(["seer", "program", "ab", "--disasm", "--head", "1", "--tail", "1"]).is_err()
    );
    let Command::Program { head, tail, .. } =
        parse_from(["seer", "program", "ab", "--lifted", "--tail", "5"]).unwrap()
    else {
        panic!("expected program --tail");
    };
    assert_eq!(head, 0);
    assert_eq!(tail, Some(5));
    let Command::Program { pc, contains, .. } = parse_from([
        "seer",
        "program",
        "ab",
        "--disasm",
        "--pc",
        "10240",
        "--contains",
        "call",
    ])
    .unwrap() else {
        panic!("expected program --pc --contains");
    };
    assert_eq!(pc, Some(10240));
    assert_eq!(contains.as_deref(), Some("call"));
    assert!(
        parse_from(["seer", "program", "ab", "--disasm", "--pc", "1", "--start", "10"]).is_err()
    );
}

#[test]
fn parses_regs() {
    assert!(parse_from(["seer", "regs", "1"]).is_err());
    let Command::Regs {
        id,
        ix,
        head,
        skip,
        tail,
        order,
        regs,
        changed,
        delta,
        ..
    } = parse_from(["seer", "regs", "2", "--ix", "5"]).unwrap()
    else {
        panic!("expected regs");
    };
    assert_eq!(id, 2);
    assert_eq!(ix, 5);
    assert_eq!(head, 20);
    assert_eq!(skip, 0);
    assert_eq!(tail, None);
    assert_eq!(order, None);
    assert_eq!(regs, (0..11).collect::<Vec<_>>());
    assert!(!changed);
    assert!(!delta);
    assert!(parse_from(["seer", "regs", "2", "--ix", "5", "--head", "1", "--tail", "1"]).is_err());
    assert!(
        parse_from(["seer", "regs", "2", "--ix", "5", "--order", "10129", "--start", "1"]).is_err()
    );
    let Command::Regs {
        order,
        regs,
        changed,
        program,
        ..
    } = parse_from([
        "seer",
        "regs",
        "2",
        "--ix",
        "5",
        "--order",
        "10129",
        "--reg",
        "0,7",
        "--changed",
        "--program",
        "11111111111111111111111111111111",
        "--delta",
        "--head",
        "5",
    ])
    .unwrap()
    else {
        panic!("expected regs filters");
    };
    assert_eq!(order, Some(10129));
    assert_eq!(regs, vec![0, 7]);
    assert!(changed);
    assert_eq!(
        program,
        Some(Pubkey::from_str("11111111111111111111111111111111").unwrap())
    );
}

#[test]
fn parses_glassbox() {
    let Command::Glassbox {
        id,
        ix,
        force,
        head,
        skip,
        taken_only,
        ..
    } = parse_from(["seer", "glassbox", "1", "--ix", "0"]).unwrap()
    else {
        panic!("expected glassbox");
    };
    assert_eq!(id, 1);
    assert_eq!(ix, 0);
    assert!(!force);
    assert_eq!(head, 20);
    assert_eq!(skip, 0);
    assert!(!taken_only);
    assert!(parse_from(["seer", "glassbox", "1"]).is_err());
    assert!(
        parse_from(["seer", "glassbox", "1", "--ix", "0", "--head", "1", "--tail", "1"]).is_err()
    );
    let Command::Glassbox {
        force, head, tail, ..
    } = parse_from([
        "seer", "glassbox", "2", "--ix", "1", "--force", "--tail", "3",
    ])
    .unwrap()
    else {
        panic!("expected glassbox --force --tail");
    };
    assert!(force);
    assert_eq!(head, 0);
    assert_eq!(tail, Some(3));
}

#[test]
fn run_rejects_bad_combinations() {
    assert!(parse_from(["seer", "run"]).is_err());
    assert!(parse_from(["seer", "run", "--lamports", "0"]).is_err());
    assert!(parse_from([
        "seer",
        "run",
        "--from",
        "1",
        "--account",
        "11111111111111111111111111111111"
    ])
    .is_err());
    let sig = solana_signature::Signature::default().to_string();
    assert!(parse_from(["seer", "run", "--sig", &sig]).is_err());
    assert!(parse_from(["seer", "run", "--sig", &sig, "--from", "1"]).is_err());
}

fn dummy_row(id: i64, parent: Option<i64>) -> storage::RunRow {
    dummy_row_err(id, parent, None)
}

fn dummy_row_err(id: i64, parent: Option<i64>, error: Option<&str>) -> storage::RunRow {
    storage::RunRow {
        id,
        transaction_blob_hash: [0; 32],
        state_blob_hash: [u8::from(id == 2); 32],
        run_at: String::new(),
        environment: "{}".into(),
        status: "finished".into(),
        error: error.map(str::to_string),
        parent_id: parent,
        patches: if parent.is_some() {
            r#"[{"account":"11111111111111111111111111111111","lamports":0}]"#.into()
        } else {
            "[]".into()
        },
        source: match parent {
            Some(p) => format!("from:{p}"),
            None => "sig:abc".into(),
        },
    }
}

#[test]
fn ls_tree_and_state_are_objects() {
    let runs = [
        dummy_row(1, None),
        dummy_row(2, Some(1)),
        dummy_row(3, Some(1)),
        dummy_row(5, None),
        dummy_row_err(6, Some(5), Some("boom")),
    ];
    let tree = super::format::ls_tree_json(&runs);
    assert_eq!(tree[0]["id"], 5);
    assert_eq!(tree[0]["children"][0]["id"], 6);
    assert_eq!(tree[1]["id"], 1);
    assert_eq!(tree[1]["children"][0]["id"], 3);
    assert_eq!(tree[1]["children"][1]["id"], 2);

    let kids = super::format::ls_select(&runs, Some(1), false, None);
    assert_eq!(kids.iter().map(|r| r.id).collect::<Vec<_>>(), vec![2, 3]);
    let sub = super::format::ls_select(&runs, Some(1), true, None);
    assert_eq!(sub.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    let failed = super::format::ls_select(&runs, None, false, Some(super::format::LsStatus::Error));
    assert_eq!(failed.iter().map(|r| r.id).collect::<Vec<_>>(), vec![6]);

    let newest: Vec<_> = {
        let mut v: Vec<_> = runs.iter().map(super::format::run_json).collect();
        v.reverse();
        super::format::slice_json_array(v, 0, 2)
    };
    assert_eq!(newest[0]["id"], 6);
    assert_eq!(newest[1]["id"], 5);
    let sliced = super::format::slice_json_array(tree.clone(), 1, 1);
    assert_eq!(sliced[0]["id"], 1);
    let all = super::format::slice_json_array(tree.clone(), 0, 0);
    assert_eq!(all.len(), 2);

    let pk = solana_pubkey::Pubkey::default();
    let mut state = crate::state_accounts::StateAccounts::default();
    state.0.insert(
        pk,
        crate::state_accounts::StateAccount {
            lamports: 1,
            data: [0; 32],
            owner: pk,
            executable: false,
        },
    );
    let catalog = super::format::state_catalog(&state);
    assert!(catalog.is_object());
    assert_eq!(catalog[pk.to_string()]["lamports"], "1");
    assert!(catalog[pk.to_string()].get("data").is_none());
}

#[test]
fn parses_skill() {
    let Command::Skill { install } = parse_from(["seer", "skill"]).unwrap() else {
        panic!("expected skill");
    };
    assert!(!install);
    let Command::Skill { install } = parse_from(["seer", "skill", "install"]).unwrap() else {
        panic!("expected skill install");
    };
    assert!(install);
}

#[test]
fn skill_print_is_bundled_markdown() {
    assert!(!super::skill::MARKDOWN.is_empty());
    assert!(super::skill::MARKDOWN.starts_with("---\nname: seer\n"));
    assert!(super::skill::MARKDOWN.contains("seer program"));
    assert!(super::skill::MARKDOWN.contains("seer regs"));
    assert!(super::skill::MARKDOWN.contains("concolic analyzer"));
    assert!(super::skill::MARKDOWN.contains("last taken conditions"));
}

#[test]
fn skill_install_writes_four_homes() {
    let home = std::env::temp_dir().join(format!("seer-skill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let (written, errors) = super::skill::install_into(&home);
    assert!(errors.is_empty());
    assert_eq!(written.len(), 4);
    for rel in [
        ".cursor/skills/seer",
        ".claude/skills/seer",
        ".codex/skills/seer",
        ".agents/skills/seer",
    ] {
        let path = home.join(rel).join("SKILL.md");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            super::skill::MARKDOWN
        );
        std::fs::write(&path, "stale").unwrap();
    }
    let (written, errors) = super::skill::install_into(&home);
    assert!(errors.is_empty());
    assert_eq!(written.len(), 4);
    for rel in [
        ".cursor/skills/seer",
        ".claude/skills/seer",
        ".codex/skills/seer",
        ".agents/skills/seer",
    ] {
        assert_eq!(
            std::fs::read_to_string(home.join(rel).join("SKILL.md")).unwrap(),
            super::skill::MARKDOWN
        );
    }
    std::fs::remove_dir_all(home).ok();
}
