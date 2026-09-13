# Scope Q retained insurance stock epochs

Branch: `codex/pr135-scope-q-retained-insurance-stock-epochs-20260913`.
Worktree: `/home/anatoly/worktrees/pr135-scope-q-retained-insurance-stock-epochs-20260913`.
Base: `origin/codex/astra-open-holdout-ledger-20260912`, fetched at
`5a922618a70734e4a6db25095aadb2b94b6d9702`.
Only the requested branch was fetched into a new private bare repository. No open
PR branch, diff, test, alternate worktree source or alternate production implementation
was inspected or copied. Neither excluded checkout was modified, including its
Git metadata. Sources were this base's README, invariant charter, ledgers and
existing INV-008/064 owners, including Scope E.

## Remaining gap and increment

Rows 415 and 428 are OPEN in `coverage_reopenings.tsv`. The corresponding
`open_findings.tsv` entries remain historical `missing` inventory. Scope E has
already made a successful Live insurance debit advance its validated authority
epoch, even when no insurance ledger is supplied. Its four histories cover a
fixed SPL endowment/refill, destinations, optional telemetry and failed bundles.
Earlier underfunded-rail coverage withdraws portfolio capital, while the older
insurance round-trip owner tracks the shared top-up lane. Destination-repair and
native-recreation histories add explicit role-epoch changes. None of those is
the generated Live insurance debit/stock/quote-rail composition added here.

The new owner is `cu/inv_008_generated_insurance_stock_epochs.rs`, mounted in
the existing INV-008 CU owner as `generated_insurance_stock_epochs`.
The public System/SPL/ATA/wrapper fixture creates every economic account.
Both classic SPL mint supplies are fixed and mint authorities revoked before
the history. Both assets use the same authorized operator but four distinct
destination accounts. One secondary vault starts exactly one atom below the
first retained request. Primary custody contains separately attributed sibling
insurance, so a funded rail alone cannot establish target entitlement.

Four XorShift seeds (`0x51544f434b00 + 0..4`) generate initial long/short budgets,
the first cross-domain debit, three refills and three sibling debit quantities.
Both target assets and six permutations of refill/sibling debit/raw donation
produce 48 histories. The three rounds alternate partial, full and partial
target-stock depletion, changing the refill side and successful withdrawal rail.
Historical retries run forward or backward. Every serialized transaction,
including separately authorized successor epochs, is signed before any history
transaction is delivered. Delivery bypasses `env.send` and never rebinds guards.
The two retained quote-rail/telemetry variants assert identical payload bytes.

The initial insufficient-custody rejection restores a completed sibling debit
and lazy ledger initialization. A late SPL failure restores the initial target
debit; its retained primary envelope then commits. In each round a stale suffix
or late SPL failure restores the entire refill/sibling/donation/fresh-debit
prefix. The original prefix commits, old envelopes remain stale on both funded
rails, and an in-transaction duplicate restores a completed fresh debit.
The separately pre-signed fresh envelope then commits. The second round empties
target entitlement before independently funded stock arrives in the third.
Fresh final debits exhaust each asset's exact entitlement. An over-allowance
request rejects despite sufficient physical custody and sibling insurance;
retained secondary requests remain stale against raw surplus after both asset
budgets reach zero.

Insurance withdrawal checks physical custody before authority epoch, so the
first underfunded failure is `InvalidTokenAccount`, not `EngineStale`. Every
subsequent stale assertion runs against a funded rail and requires the exact
`EngineStale` instruction index. Route switching here means primary/secondary
mint withdrawal and optional telemetry under tag 57. Mint configuration is fixed
before consent; this does not exercise governance-driven mint replacement.

## Oracle and ownership

The input-derived book owns domain credits and remaining stock, physical custody
on each rail, sources, all control sequences and both optional insurance ledgers.
Each `(asset, authority_epoch)` receipt stores its signed amount, consumed
long/short stock and per-rail destination payment. A receipt can be inserted only
once. Its consumed stock and payments each equal its signed amount; summed
receipts plus remaining entitlement equal attributed input credits by domain.
Receipt attribution is bookkeeping over fungible domain stock, not an assertion
that the program stores deposit-lot identities. Expected states are computed
before delivery and advanced only for planned successful transactions.

After every attempt the checker reconciles the per-intent book with all domain
budgets/spend, insurance and accounting vault, source encumbrance, zero capital,
claims and open interest, asset IDs/profiles, complete control sequences and
every ledger field. Complete SPL Accounts compare against original frames with
only modeled amounts changed; mint supply and metadata stay exact. Both mint
censuses close independently. Physical raw donations are explicitly excluded
from insurance entitlement. Successful states also pass engine shape validation.
An observation-only one-atom move between asset destinations preserves aggregate
payout but must fail the same receipt-based payout predicate. Final observed
balances and epochs agree across all six landing orders for each seed/asset.

Every rejection compares complete tracked and compiled Accounts, including
bytes, lamports, metadata and absent accounts. The separate payer alone loses
the exact default fee for two signatures. Exact instruction errors and counts
of completed wrapper/SPL invocations establish the failed prefixes. No snapshot
is written back into LiteSVM. Signature-distinct envelopes prevent transaction
cache rejection from substituting for the program's epoch guard.

Primary ownership: INV-008. Bounded adjacent evidence: INV-010 landing order and
route equivalence; INV-011 per-intent/aggregate signed debit bounds; INV-024
destination entitlement; INV-031 finite domain-stock consumption; INV-064 local
asset allowance across rails; INV-080 returned errors and exact SVM rollback;
INV-081 checked successful public states. The full INV-064 enable/cap/cooldown
statement is not established by this local allowance book.

## Result and limits

Conformance only; no current implementation failure was found in these histories
and no production fix or bug-reproducer commit is included. Rows **415/428 remain
OPEN** and all machine classifications are unchanged. INV-008/024 remain
`REFUTED_CURRENT`; INV-010/011/031/064/080/081 remain `OPEN_EVIDENCE`.

The new selector covers 48 histories, 2,208 transactions, 432 committed debits,
1,776 exact rollbacks and 1,392 completed SPL transfers restored. These include
48 insufficient-rail errors, 192 late SPL errors, 48 over-allowance errors and
1,488 epoch-stale errors. There are 48 observation-misattribution controls and
eight six-order outcome comparisons. Peak CU across both runs is 166,529 under
300,000; the final focused run measured 147,014. Quantity/schedule seeds are
fixed; fixture keypairs are fresh, so address/PDA-dependent CU can vary.

This is a bounded generated schedule with four deterministic quantity seeds,
three stock rounds and two assets, not a generic rediscovery/shrinking campaign.
There are no positions, liabilities, price changes, fees, role handoffs, shutdown
fallbacks, resolution, terminal recredit, backing withdrawals, partial SPL fills,
durable nonces or signature expiry transitions. Previously unexecuted consent
across funding changes without an intervening debit/epoch change remains open.
Native redemption and recipient recreation are not extended: the meaningful new
rail dimension is underfunded classic secondary custody repaired by public SPL
donation. Both roles share an owner, so this is asset/destination attribution,
not independent-owner coalition coverage. Shared authority-epoch consumption
does not establish an independent withdrawal stock sequence. No generic row or
whole-invariant closure, full test-suite compatibility or formal proof is claimed.
LiteSVM/SPL behavior, SVM rollback, default signature fees and the pinned engine
remain platform assumptions.

## Validation

Default-feature SBF was built locally from the unchanged base production source
using platform-tools v1.52 and locked/offline dependencies. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Program SHA-256:
`71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.
No other worktree's tests, target or SBF artifacts were copied. No matcher is used.
Root and `/dev/shm` capacity were nearly exhausted, so this task uses its own
target under `/run/user/1001`. When root filled during Git status, its private
21-MiB bare repository was temporarily moved to `/run/user/1001`. Once root
space became available, it was placed permanently at
`/home/anatoly/worktrees/pr135-scope-q-20260913.git` and `git worktree repair`
updated this worktree's link. The temporary symlink was removed. This affects
only this task's Git metadata and does not alter either excluded checkout.

Exact commands, from the worktree above:

```sh
export CARGO_TARGET_DIR=/run/user/1001/pr135-scope-q-20260913-target
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=$CARGO_TARGET_DIR/deploy/percolator_prog.so
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_NET_OFFLINE=true cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_008_intent_uniqueness_and_bounded_replay::generated_insurance_stock_epochs::v16_generated_live_insurance_stock_epochs_preserve_retained_rail_entitlement -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_008_intent_uniqueness_and_bounded_replay::generated_insurance_stock_epochs::v16_generated_live_insurance_stock_epochs_preserve_retained_rail_entitlement \
  inv_008_intent_uniqueness_and_bounded_replay::live_debit_consumption::v16_live_insurance_debit_consumes_epoch_across_ledger_and_destination_retries \
  inv_008_intent_uniqueness_and_bounded_replay::underfunded_rail_retry::v16_underfunded_withdrawal_retry_preserves_replenished_stock_across_quote_rails \
  inv_008_intent_uniqueness_and_bounded_replay::v16_retained_withdrawal_stays_consumed_after_redeposit_restores_custody \
  inv_064_insurance_withdrawal_policy_equivalence::v16_attack_live_insurance_asset_withdraw_uniform_for_asset0_and_permissionless_asset \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_live_and_resolved_insurance_withdrawals_share_one_finite_budget \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_ledger_history_is_economically_transparent \
  inv_008_intent_uniqueness_and_bounded_replay::v16_public_replay_disposition_roster_is_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
sha256sum "$PERCOLATOR_FUZZ_SBF"
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

The initial new-selector run passed 1/1 in 25.42s. The final focused command
passed **8/8** in 35.03s, including the new selector with its destination mutation
and six-order outcome checks, Scope E, the underfunded portfolio rail control,
retained portfolio redeposit, all three named INV-064 selectors and the replay
roster. All four metadata gates passed **4/4** in 0.01s. Formatter, working and
staged diff checks, and the committed show-check passed. No broad suite or Kani
run was performed. The existing 346 metadata-target dead-code warnings and
Solana-client future-compatibility warning remain; the new module is warning-free.

Focused and metadata output was redirected to the private target's
`scope-q-focused.log` and `scope-q-metadata.log` respectively. The commands above
are identical apart from those output redirections. The exact metadata-gate
names and selectors above, rather than an unfiltered suite, define the tested
boundary. Only the new owner, its parent module mount, the coverage README and
this audit note are committed; production, pins and TSV ledgers are unchanged.
