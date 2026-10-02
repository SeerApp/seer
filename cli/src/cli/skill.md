---
name: seer
description: >
  Replays Solana transactions locally with the seer CLI (runs, traces,
  glassbox). Use when debugging a Solana transaction, signature, on-chain
  failure, CPI, or when the user mentions seer, glassbox, or local replay.
---

# seer

This `seer` is local replay (`run`, `show`, `ls`, `diff`, `program`, `regs`, `glassbox`).

## Runs

Work in run ids.

1. `seer run --sig <SIGNATURE> --url <RPC>` — message and accounts from that RPC, as of now.
   `seer run --sig <SIGNATURE> --url <RPC> --historical` — message from that RPC only. Accounts come from the local mainnet cache, or from `https://tx.seer.run`. A missing account fails the run.
2. `seer show <ID>` then `seer show <ID> --trace` — named instructions, logs, account diffs.
3. `seer program <PUBKEY> --run <ID> --disasm` (or `--lifted`) — static listing / CFG. Prefer `--pc`, `--start`/`--end`, or `--contains`. Default `--head`. Do not dump the full ELF into chat.
4. `seer regs <ID> --ix <N>` — r0–r10 at recorded steps. Window with `--start`/`--end` (order) or `--order`. Prefer `--head`. `--changed` and `--reg` shrink the dump. Do not dump the full instruction into chat.
5. `seer glassbox <ID> --ix <N>` only if a branch is still unexplained. Prefer `--head`. Do not dump the full report into chat.

Fork: `seer run --from <ID> --account <PUBKEY> --lamports 0` (and other patches).

If the default storage home errors on schema, pass `--storage-home`.

## Glassbox

Glassbox is a concolic analyzer. Path conditions for one instruction. Usually the last taken conditions in the list are the most germane to a transaction failure. SAT `vacuous` / skipped tautologies are a hide tag, not the capture set. A timeout does not mean the jump was absent.

## Skill file

`seer skill` prints this file. `seer skill install` overwrites it in user skill dirs (Cursor, Claude Code, Codex, `.agents`). Re-run install after upgrading `seer`.
