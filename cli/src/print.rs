use anyhow::{Context, Result};
use solana_transaction::versioned::VersionedTransaction;

pub fn transaction(bytes: &[u8]) -> Result<serde_json::Value> {
    let tx: VersionedTransaction = bincode::deserialize(bytes).context("transaction wire")?;
    Ok(pretty_tx(&tx))
}

fn pretty_tx(tx: &VersionedTransaction) -> serde_json::Value {
    serde_json::json!({
        "signatures": tx.signatures.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "message": pretty_message(tx),
    })
}

fn pretty_message(tx: &VersionedTransaction) -> serde_json::Value {
    let message = &tx.message;
    let header = message.header();
    let mut out = serde_json::json!({
        "header": {
            "num_required_signatures": header.num_required_signatures,
            "num_readonly_signed_accounts": header.num_readonly_signed_accounts,
            "num_readonly_unsigned_accounts": header.num_readonly_unsigned_accounts,
        },
        "account_keys": message
            .static_account_keys()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "recent_blockhash": message.recent_blockhash().to_string(),
        "instructions": message
            .instructions()
            .iter()
            .map(|ix| {
                serde_json::json!({
                    "program_id_index": ix.program_id_index,
                    "accounts": ix.accounts,
                    "data": hex::encode(&ix.data),
                })
            })
            .collect::<Vec<_>>(),
    });
    if let Some(lookups) = message.address_table_lookups() {
        out["address_table_lookups"] = lookups
            .iter()
            .map(|lookup| {
                serde_json::json!({
                    "account_key": lookup.account_key.to_string(),
                    "writable_indexes": lookup.writable_indexes,
                    "readonly_indexes": lookup.readonly_indexes,
                })
            })
            .collect();
    }
    out
}

#[cfg(test)]
mod test {
    #[test]
    fn trace_is_stored_json() {
        let blob = br#"{"receiver":"11111111111111111111111111111111","children":[{"Log":{"message":"ok"}}],"loc":{"file":"lib.rs","line":12}}"#;
        let value: serde_json::Value = serde_json::from_slice(blob).unwrap();
        assert_eq!(value["loc"]["file"], "lib.rs");
        assert_eq!(value["children"][0]["Log"]["message"], "ok");
    }
}
