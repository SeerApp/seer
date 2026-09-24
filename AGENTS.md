# Don't split what fits in one line

If something can be written in one line, do not split it across functions, structs, files, or extra types unless the user specifically requests that split.

# No thin wrappers

A type or function whose only job is to wrap another type or call is unacceptable. If a refactor leaves one behind, delete it and use the inner type or call directly.

# No comments

Do not add comments. Delete comments that are not absolutely, undeniably essential. Clap `help` attributes are allowed. `//` and `///` on code are not.

# seer `cli` module

The `seer` binary goes through `cli` because clap lives there. `cli` parses args, normalises them, dispatches to `runs` / `storage`, and formats user-facing output. It does not implement storage or run logic itself.

# historical_transaction

This table is populated only by an external seer service that is not implemented yet. seer must not read or write it. `--sig --url` pulls a transaction from RPC to create a run; that does not insert or look up a historical row. Rows are tied to a Solana network (`mainnet` / `devnet` / `testnet`), never to an RPC URL. Do not replace `network` with `url`. Each row has its own `environment`; a run may override that environment, but the historical row stays the source of truth for the indexed transaction.

