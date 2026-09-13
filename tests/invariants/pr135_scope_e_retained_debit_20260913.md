# Scope E retained debit conformance

Branch: `codex/pr135-scope-e-retained-debit-20260913`.
Worktree: `/home/anatoly/percolator-pr135-scope-e-20260913`.
Requested base: `origin/codex/astra-open-holdout-ledger-20260912`, captured at
`cf0f2832668a3401ab189fc2b657fb5c3eb630f9`.
Sources: invariant charter, status/coverage ledgers and this base's current code.
No open PR diff, held-out test or alternate production implementation was used.

## Scope and result

The new owner is `cu/inv_008_live_debit_consumption.rs`, mounted under
`inv_008_intent_uniqueness_and_bounded_replay::live_debit_consumption`.
Primary INV-008; bounded INV-064 insurance/custody debit conformance, with
rollback evidence relevant to INV-080. This adds actual withdrawal consumption
to the earlier authority-succession and insurance-funding sequence evidence.

Four Live-only histories cross asset 0/1 with an initially omitted/attached
insurance ledger. System, SPL, ATA and wrapper instructions create all economic
state. The mint's fixed 183-atom supply is revoked before the history. Initial
long/short budgets are `[13,47,19,61]`; each first 23-atom debit crosses its long
budget. Two public destinations have the same authorized owner. All serialized
transactions are signed before the first debit, including successor-epoch
continuations, and sent directly without the harness's guard-rebinding adapter.

An input-derived book checks each domain's remaining budget, aggregate insurance
and accounting vault, zero capital/claims/encumbrance, all control sequences,
unchanged asset IDs and authority profiles, complete SPL accounts, fixed mint
and every optional ledger field after each attempt. A successful debit must
increment only its bound authority epoch. The checker does not learn expected
debits or counters from observed post-transaction changes.

The history rolls back an initial payout after a late SPL error, then commits
the original consent. It rejects an alternate recipient/ledger envelope and an
in-transaction duplicate of fresh consent. Refill plus fresh payout rolls back
after either a retained stale debit or a late SPL error; the same two-instruction
prefix then commits. Original and alternate retained envelopes remain stale
against the independently funded 43-atom refill. A pre-signed sibling-asset
debit still succeeds; fresh target consent drains its exact residual stock.
All tracked and compiled transaction accounts compare exactly on failure,
including lamports, metadata and absence. The payer alone loses the exact
signature fee. Exact instruction errors and completed wrapper/SPL log counts
establish the executed prefixes.

On the unmodified base SBF, the new selector fails immediately after its first
successful debit: the signed epoch remains zero while the model requires one.
The earlier late-error transaction had restored the complete initial state.
This is a consumption assertion, before submitting any committed stale retry.
Base SBF SHA-256:
`8a010d8283186721866a95a71d252a3265015fbf563d6ce566cd44f263659349`.

The seven-line production adjustment advances the authority epoch already
validated by a successful Live insurance withdrawal, even without telemetry.
The existing checked increment supplies overflow rejection and transaction
atomicity also restores the counter on a later error. The selected epoch is
the local operator's asset, or the existing base-authority scope for an
authorized shutdown drain. Resolved payout code and wire/account layouts are
unchanged. This consumes a **shared** authority epoch: other pending consent
bound to that epoch must be renewed after a Live debit. Historical probes that
assume unchanged epochs through Live withdrawals need that contract accounted
for; this increment does not certify their full compatibility.

The new selector passes: **4 histories, 48 transactions, 32 exact rollbacks**,
including 24 completed SPL transfers restored by rejected transactions. Peak
CU is **75,517**, below the 300,000 transaction envelope. Adjacent portfolio
custody redeposit/retry, insurance ledger transparency and asset-local insurance
allowance selectors also pass. No additional terminal payout probe was added.

## Classification

Rows **415/428 remain OPEN**. `INV-008` remains `REFUTED_CURRENT`; `INV-064`
remains `OPEN_EVIDENCE`. This is bounded conformance plus a local Live debit
consumption correction, not a generic stock-history or global policy proof.
Backing withdrawals, terminal consumption, arbitrary policy/reclassification
histories, nonempty positions, shutdown fallback histories, other token rails,
durable nonces and previously unexecuted consent across replenishment remain
outside the new probe. Scope A-D and Scope C terminal payout work gain no claims.

## Validation commands

The default-feature SBF was built locally with platform-tools v1.52, first from
the base production source and then after the minimal adjustment. The same
new selector was used for the first-debit failure and the passing run. The
private target was built from the local Cargo cache; no other worktree's test
or SBF artifacts were copied. The final SBF is reused for all selectors below.
Final SBF SHA-256:
`71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.

```sh
cd /home/anatoly/percolator-pr135-scope-e-20260913
export CARGO_TARGET_DIR=/home/anatoly/percolator-pr135-scope-e-20260913/target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_NET_OFFLINE=true cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /home/anatoly/percolator-pr135-scope-e-20260913/target/deploy -- --locked
sha256sum target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu inv_008_intent_uniqueness_and_bounded_replay::live_debit_consumption::v16_live_insurance_debit_consumes_epoch_across_ledger_and_destination_retries -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_008_intent_uniqueness_and_bounded_replay::v16_retained_withdrawal_stays_consumed_after_redeposit_restores_custody -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_ledger_history_is_economically_transparent -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_064_insurance_withdrawal_policy_equivalence::v16_attack_live_insurance_asset_withdraw_uniform_for_asset0_and_permissionless_asset -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu inv_008_intent_uniqueness_and_bounded_replay::v16_public_replay_disposition_roster_is_source_complete -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --quiet --test-threads=1
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming -- --exact --quiet --test-threads=1
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

Only the named selectors are validated; no broad suite, alternate pin comparison
or open PR test was run. Solana-client's existing future-compatibility warning
remains, alongside the metadata target's existing dead-code warnings. Both
INV-079 metadata selectors and the formatter pass. The replay roster and final
Git whitespace checks are included in the commands above.
