use anyhow::{Context, Result};
use solana_account::Account;
use solana_pubkey::Pubkey;
use solana_rpc_client::api::config::{RpcTransactionConfig, UiTransactionEncoding};
use solana_rpc_client::rpc_client::RpcClient;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;

pub fn get_multiple_accounts(url: &str, keys: &[Pubkey]) -> Result<Vec<Option<Account>>> {
    let client = RpcClient::new(url.to_string());
    let mut out = Vec::with_capacity(keys.len());
    for chunk in keys.chunks(solana_rpc_client::api::request::MAX_MULTIPLE_ACCOUNTS) {
        let rpc_keys: Vec<_> = chunk.iter().map(|key| key.to_bytes().into()).collect();
        out.extend(client.get_multiple_accounts(&rpc_keys)?);
    }
    Ok(out)
}

pub fn get_transaction(url: &str, signature: &Signature) -> Result<VersionedTransaction> {
    RpcClient::new(url.to_string())
        .get_transaction_with_config(
            signature,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Base64),
                commitment: None,
                max_supported_transaction_version: Some(0),
            },
        )?
        .transaction
        .transaction
        .decode()
        .context("transaction decode")
}
