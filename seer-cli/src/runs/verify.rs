use anyhow::{Context, Result};
use solana_transaction::versioned::VersionedTransaction;

use crate::report::Report;
use crate::storage::state_accounts::StateAccounts;

pub fn verify(tx: &VersionedTransaction, state: &StateAccounts) -> Result<()> {
    tx.sanitize().context("malformed transaction")?;
    state.verify()?;
    let mut report = Report::default();
    let mut needed: Vec<_> = tx.message.static_account_keys().to_vec();
    let lookups = tx.message.address_table_lookups().unwrap_or(&[]);
    needed.extend(lookups.iter().map(|lookup| lookup.account_key));
    match state.resolve_lookups(lookups) {
        Ok(loaded) => needed.extend(loaded),
        Err(e) => report.merge(e),
    }
    for key in needed {
        if !state.0.contains_key(&key) && !report.missing_accounts.contains(&key) {
            report.missing_accounts.push(key);
        }
    }
    if report.is_empty() {
        Ok(())
    } else {
        Err(report.into())
    }
}
