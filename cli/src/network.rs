use anyhow::{Context, Result};
use solana_account::Account;
use solana_message::v0::LoadedAddresses;
use solana_pubkey::Pubkey;
use solana_rpc_client::api::config::{RpcTransactionConfig, UiTransactionEncoding};
use solana_rpc_client::rpc_client::RpcClient;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_status_client_types::UiLoadedAddresses;

pub fn get_multiple_accounts(url: &str, keys: &[Pubkey]) -> Result<Vec<Option<Account>>> {
    let client = RpcClient::new(url.to_string());
    let mut out = Vec::new();
    for chunk in keys.chunks(solana_rpc_client::api::request::MAX_MULTIPLE_ACCOUNTS) {
        let rpc_keys: Vec<_> = chunk.iter().map(|key| key.to_bytes().into()).collect();
        out.extend(client.get_multiple_accounts(&rpc_keys)?);
    }
    Ok(out)
}

pub fn get_transaction(
    url: &str,
    signature: &Signature,
) -> Result<(VersionedTransaction, Option<LoadedAddresses>)> {
    let got = RpcClient::new(url.to_string()).get_transaction_with_config(
        signature,
        RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Base64),
            commitment: None,
            max_supported_transaction_version: Some(1),
        },
    )?;
    let version = got.transaction.version;
    let tx = got
        .transaction
        .transaction
        .decode()
        .with_context(|| format!("transaction decode (version {version:?})"))?;
    let loaded = loaded_addresses(got.transaction.meta.as_ref())?;
    if tx.message.address_table_lookups().unwrap_or(&[]).is_empty() {
        return Ok((tx, None));
    }
    let Some(loaded) = loaded.filter(|loaded| !loaded.is_empty()) else {
        anyhow::bail!("v0 transaction is missing loadedAddresses in RPC meta");
    };
    Ok((tx, Some(loaded)))
}

fn loaded_addresses(
    meta: Option<&solana_transaction_status_client_types::UiTransactionStatusMeta>,
) -> Result<Option<LoadedAddresses>> {
    let Some(meta) = meta else {
        return Ok(None);
    };
    let Some(ui) = Option::<UiLoadedAddresses>::from(meta.loaded_addresses.clone()) else {
        return Ok(None);
    };
    Ok(Some(LoadedAddresses {
        writable: parse_keys(&ui.writable)?,
        readonly: parse_keys(&ui.readonly)?,
    }))
}

fn parse_keys(keys: &[String]) -> Result<Vec<solana_address::Address>> {
    keys.iter()
        .map(|key| key.parse().with_context(|| format!("loaded address {key}")))
        .collect()
}
