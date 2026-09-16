//! WARNING!
//! These tests are AI-generated TRASH and do not constitute proper invariant checks.
//! TBD.

use std::str::FromStr;

use clap::Parser;

use super::*;

fn parse_from<I, T>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Command::try_from(Cli::try_parse_from(args)?)
}

#[test]
fn resolves_file_at_path_or_inline_value() {
    let dir = std::env::temp_dir().join(format!("seer-cli-input-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("blob");
    std::fs::write(&file, "from-file").unwrap();

    let path = file.to_string_lossy().into_owned();
    assert_eq!(
        PathOrValue::from_str(&path).unwrap().load_text().unwrap(),
        "from-file"
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
fn parses_hash_commands() {
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    let Command::Hash(HashCommand::State(state)) =
        parse_from(["seer", "hash", "state", "{}"]).unwrap()
    else {
        panic!("expected hash state");
    };
    assert_eq!(state, StateAccounts::default());

    let Command::Hash(HashCommand::Simulation {
        msg_hash,
        state_hash,
    }) = parse_from(["seer", "hash", "simulation", EMPTY_SHA256, EMPTY_SHA256]).unwrap()
    else {
        panic!("expected hash simulation");
    };
    assert_eq!(msg_hash, state_hash);
    assert_eq!(hex::encode(msg_hash.0), EMPTY_SHA256);

    let tx = solana_transaction::Transaction::default();
    let json = serde_json::to_string(&tx).unwrap();
    let Command::Hash(HashCommand::Transaction(parsed)) =
        parse_from(["seer", "hash", "transaction", &json]).unwrap()
    else {
        panic!("expected hash transaction json");
    };
    assert_eq!(
        parsed,
        solana_transaction::versioned::VersionedTransaction::from(tx.clone())
    );

    let versioned = solana_transaction::versioned::VersionedTransaction::from(tx);
    let wire = bincode::serialize(&versioned).unwrap();
    let Command::Hash(HashCommand::Transaction(parsed)) = parse_from([
        "seer",
        "hash",
        "transaction",
        "--hex",
        &hex::encode(&wire),
    ])
    .unwrap()
    else {
        panic!("expected hash transaction hex");
    };
    assert_eq!(parsed, versioned);

    let dir = std::env::temp_dir().join(format!("seer-cli-tx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tx.bin");
    std::fs::write(&file, &wire).unwrap();
    let Command::Hash(HashCommand::Transaction(parsed)) = parse_from([
        "seer",
        "hash",
        "transaction",
        "--wire",
        file.to_str().unwrap(),
    ])
    .unwrap()
    else {
        panic!("expected hash transaction wire");
    };
    assert_eq!(parsed, versioned);
    std::fs::remove_dir_all(dir).ok();
}
