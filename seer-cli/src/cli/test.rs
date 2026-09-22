use std::str::FromStr;

use clap::Parser;
use solana_pubkey::Pubkey;

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
    let dir = std::env::temp_dir().join(format!("seer-cli-input-{}", std::process::id()));
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
    assert!(PathOrValue::from_str("@/definitely/missing/seer-cli-nope")
        .unwrap()
        .load_bytes()
        .is_err());

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn bare_seer_is_status() {
    assert_eq!(parse_from(["seer"]).unwrap(), Command::Status);
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
            account: None,
            trace: true,
        }
    );
    assert_eq!(
        parse_from(["seer", "ls", "--tree"]).unwrap(),
        Command::Ls { tree: true }
    );
    assert_eq!(
        parse_from(["seer", "diff", "1", "2"]).unwrap(),
        Command::Diff { a: 1, b: 2 }
    );
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
    storage::RunRow {
        id,
        transaction_blob_hash: [0; 32],
        state_blob_hash: [u8::from(id == 2); 32],
        run_at: String::new(),
        environment: "{}".into(),
        status: "finished".into(),
        error: None,
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
fn ls_show_diff_text() {
    let b = dummy_row(2, Some(1));
    let listed = super::format::ls_text(&[dummy_row(1, None), dummy_row(2, Some(1))], false);
    assert!(listed.contains("sig:abc"));
    assert!(listed.contains("lamports"));
    let tree = super::format::ls_text(&[dummy_row(1, None), dummy_row(2, Some(1))], true);
    assert!(tree.contains("1  sig:abc"));
    let card = super::format::run_card(&b);
    assert!(card.contains("run 2"));
    assert!(card.contains("next: seer show 2"));
    assert!(card.contains("next: seer run --from 2"));
    let d = super::format::diff_text(&dummy_row(1, None), &dummy_row(2, Some(1)));
    assert!(d.contains("1 vs 2"));
    assert!(d.contains("different"));
}
