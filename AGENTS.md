# Don't split what fits in one line

If something can be written in one line, do not split it across functions, structs, files, or extra types unless the user specifically requests that split.

# No thin wrappers

A type or function whose only job is to wrap another type or call is unacceptable. If a refactor leaves one behind, delete it and use the inner type or call directly.

# No comments

Do not add comments. Delete comments that are not absolutely, undeniably essential. Clap `help` attributes are allowed. `//` and `///` on code are not.

# seer `cli` module

The `seer` binary goes through `cli` because clap lives there. `cli` parses args, normalises them, dispatches to `runs` / `storage`, and formats user-facing output. It does not implement storage or run logic itself.

# historical_transaction

`--historical` reads a mainnet row and, on a miss, writes one after the message and the accounts are stored. `--sig --url` without `--historical` does not read or write the table. Rows are tied to a Solana network (`mainnet` / `devnet` / `testnet`), never to an RPC URL. Do not replace `network` with `url`. seer writes only `mainnet`. The runtime sysvar accounts are named only in `cli/src/sysvars.rs` and are stored with the other accounts. `run.sigverify` and `run.blockhash_check` record the replay switches, default off. `--from` copies them. The flags on that command replace the copy.

