# Scope G retained stock-epoch closure attempt

Base: `d01245dd993bfe60c2448360733c3bb63c41aa19`, fetched from
`origin/codex/astra-open-holdout-ledger-20260912`.
Branch: `codex/astra-scope-g-stock-epochs-20260914`.
Private clone: `/tmp/percolator-scope-g.XzuiZK`.
Baseline control worktree: `/tmp/percolator-scope-g.XzuiZK-baseline`.

Neither excluded checkout was edited. The private clone has independent Git
objects, index and refs. Sources for the comparison were `scripts/loop.md`,
`INVARIANTS.md`, the invariant README, reopening ledger, replay and entitlement
dispositions, and the current INV-008/010/011/024/031/064/080/081 owner families.
No open PR diff, alternate production implementation or historical vulnerable
artifact supplied the generator or its expected outcomes.

## Coverage difference

| Existing owner family | Existing boundary | Scope G increment |
| --- | --- | --- |
| Scope A capital/insurance exchange | Two fixed exchanges, common owner and wallet, atomic/split delivery | Generated exchange/refill orders, separate signing owners, four payout destinations and receipt-based gross attribution |
| Scope Q generated insurance epochs | Two assets, insurance-only stock, quote rails and replenishment permutations | Capital and insurance share one history; signed transfers explicitly change the entitled owner and stock class |
| Scope E Live debit consumption | Successful Live insurance debit consumes the shared authority epoch | Both debit classes retain consumed consent through three further rounds of reclassification and external funding |
| Scope E retained policy/earned reserves | Retained close/policy consent, earned backing and terminal attribution | No new policy/earnings claim; these remain independent adjacent owners |
| INV-008 portfolio stock, reward, reciprocal/liquidation fee, co-owned and recreated portfolio owners | Passive stock, mixed replenishment, identity changes, rail repair and fee-shortened payouts | A single joint receipt book for capital/insurance plus explicit cross-owner SPL transfers |
| INV-008 reserve replenishment, ledger, fee and destination/native epoch owners | Reserve/role/telemetry or account-lifetime composition | No repetition of their fixed selectors; their remaining boundaries enter the route matrix |
| INV-010/011 and INV-024 owners | History relations, signed quantities and attributed owner claims | Every committed debit has one guard receipt and exact signed amount; independent final payout formulas and economic order/split comparisons |
| INV-031/064 owners | Typed stock, domain allowance, spent insurance and shared custody | Exact class/domain accounting after each joint prefix; local Live allowance only |
| INV-080/081 owners | Error propagation, exact SVM rollback, successful state and stock/encumbrance censuses | Reuse the existing full-Account rollback helper and both raw-state censuses |

The new owner is `cu/inv_008_generated_stock_reclassification.rs`, mounted under
INV-008's `withdrawal_stock_history::generated_stock_reclassification`.
The parent gains only a module mount; its existing helpers are unchanged.

## Source-complete inventory

`inv_008_stock_epoch_routes.tsv` accounts for all eight current public handlers
that directly call `transfer_tokens_signed`: capital, insurance, backing
principal, backing earnings, resolved close, resolved top-up claim, slab close
and secondary-for-primary swap. The executable gate extracts these handlers from
production source, checks each dispatcher binding, joins INV-024's outflow effect
classes, validates each evidence owner, and requires the generated subset to
equal the two debit classes in the new generator.

This is source completeness of the current direct signed SPL outflow inventory,
not execution of eight families by this generator. The other six entries are
explicitly `adjacent-only` and each names its remaining stock-history gap.
Insurance's entry separately names Resolved and shutdown gaps. Source, handler,
effect-class or indirect-outflow refactoring requires revisiting this inventory.
The existing full public replay/entitlement rosters remain the broader route
owners and pass as adjacent controls.

## Histories and oracle

Two XorShift seeds, `0x4753544f434b + 0..2`, generate three rounds of unequal
capital-to-insurance, insurance-to-capital and external-funding quantities.
The exchange amounts range from 3 through 12 and external funding from 19
through 41. Six permutations of those three words, coheld/separate owners,
repeated/alternating destinations, and atomic/split delivery produce 96 histories.

Public System/SPL/ATA/wrapper instructions construct all economic state.
Capital starts at 101, long/short insurance at 13/47, external funding at 211,
and an independent bystander's capital at 103. The 475-token mint supply is
fixed and its authority revoked before retention. Each stock owner has two
distinct SPL payout accounts; the donor account belongs to the insurer.
Coheld worlds share the signer while retaining distinct claim classes and
wallet accounts. Separate-owner worlds require the source owner to sign every
SPL transfer and the receiving owner to sign the subsequent stock credit.

Before any delivery, the entire transaction history is serialized and signed.
Its first capital/insurance payouts are 17/19. A late SPL error first restores
both payments, then the retained successful alternative commits. Each generated
round contains eight instructions: two payout/transfer/credit words and one
external-transfer/credit word. The external credit alternates capital,
insurance, capital. Insurance top-ups also alternate long/short/long domains.

For each round, both original consumed debits and an insufficient-funds SPL
suffix independently restore all eight successful transfers, five wrapper
calls, stock classes, consent lanes and telemetry. The same economic instruction
payloads then commit atomically or one at a time. Original-guard variants using
the other destination and omitted insurance ledger remain stale. Fresh final
capital and insurance exits also survive a late SPL failure and pay the exact
remaining entitlements, leaving only the bystander's 103 in custody.

Signature-distinct compute-budget envelopes retain identical economic payloads
and projected successor guards. Delivery verifies signatures and serialization,
uses LiteSVM directly and never invokes the harness guard-rebinding adapter.
This is retained-payload retry, not successful replay of an already cached
transaction signature, durable-nonce coverage or blockhash-expiry coverage.

The input-owned book stores each successful `(stock class, guard)` receipt,
its signed amount, exact destination and consumed capital/long/short stock.
Credited stock equals remaining stock plus receipt consumption. Wallet balances
equal external endowment plus receipt payments and signed transfers minus stock
funding. No expected amount or counter is learned from a successful SBF delta.

Each attempted transaction checks those equations, exact capital/domain budgets,
insurance spend, vault accounting, all control sequences, optional ledger fields,
portfolio identity, oracle profile, zero PnL/positions, immutable bystander state,
complete token/mint Account images, raw stock and reservation censuses, and shape.
The reused rollback helper compares all tracked/compiled Accounts including
absence, bytes, metadata and lamports. Only the exact payer signature fee changes.
Expected error indexes and successful wrapper/SPL log counts bind every failed
prefix. No snapshot is written back into the generated history.

Observation-only one-atom class and destination swaps preserve aggregate stock
or tokens but fail the same receipt predicate. A separate formula checks final
payments directly from the generated input quantities:

```text
capital owner = 101 - sum(capital -> insurance)
                   + sum(insurance -> capital) + external rounds 0 and 2
insurer      = 60 + sum(capital -> insurance)
                  - sum(insurance -> capital) + external round 1
donor wallet = 211 - all external funding
bystander    = 103 retained in the vault
```

All six orders and both delivery modes agree on per-owner economic outcomes and
consumed sequence/epoch/funding lanes. Long/short allocation and intermittent
telemetry need not commute when a debit lands before versus after funding;
each is checked exactly within its own history and excluded from the economic
equivalence projection.

## Result and remaining row state

The new exact selectors pass 2/2 in 58.32 seconds:
96 histories, 3,120 checked transactions, 1,632 exact rollbacks and 7,296
completed SPL transfers restored. Peak measured CU is **264,052**, under the
600,000 envelope. Maximum serialized packet is **1,039 bytes**, below 1,232.
Setup is reused and excluded from these new-history transaction/CU counts.
Addresses are generated by the fixture, so measured CU can vary by PDA derivation.

This is net-new bounded conformance. No production mismatch was found in the new
histories, and there is no production correction or red/green production claim.
The initial development failure was a duplicate fixture airdrop in coheld worlds;
setup now avoids a second airdrop to the already funded insurer.

Rows **415/428 remain OPEN**. Machine statuses and non-comment reopening records
are unchanged. The generator is one flat Live asset on one classic SPL rail,
two deterministic quantity seeds and three rounds. It does not supply arbitrary
history generation/shrinking, proof, unchanged-oracle historical rediscovery,
backing debit consumption, full insurance enable/cap/cooldown policy, previously
unexecuted insurance consent across new funding, shutdown fallback, nonempty
positions/liabilities, fees, role/identity changes, native/secondary custody,
terminal recredit or maximum-shape guarantees. Shared authority-epoch consumption
is not a separate insurance stock-sequence design. Existing independent owners
remain responsible for their other bounded cells; their union is not promoted
to a whole-invariant theorem.

## Controls and artifacts

Adjacent controls: **12 passed, 2 inherited failures** in 63.94 seconds.
Both failures reproduce on untouched base `d01245dd` with the same SBF,
0/2 in 0.76 seconds:

- `inv_008_insurance_round_trip_retry.rs:285`: actual stale error is instruction
  3, while the old test expects instruction 5. Its fixed-epoch round trip
  predates the consumed Live insurance epoch and rejects earlier.
- `inv_064_insurance_withdrawal_policy_equivalence.rs:604`: full Account
  comparison differs between coalesced and fragmented Live withdrawals.
  The current contract advances the authority epoch per debit, so differing
  debit counts cannot be assumed to have identical control bytes.

These existing tests are unchanged. Scope A/Q/E, generated portfolio stock,
fee-shortened withdrawal, underfunded rail, insurance ledger transparency,
Live/Resolved finite allowance, spent-insurance liquidation, fee/resolution
rollback and both existing source rosters pass. The largest CU printed by an
adjacent control is 300,987 for the existing INV-031 crank.

All four `v16_program_fuzz_regressions` metadata gates pass, including the final
run after the documentation and ledger comments were added (4/4, 0.01 seconds).
Full-workspace formatting and working/staged Git whitespace checks pass;
`git show --check HEAD` checks the local commit. Existing 346
metadata-target dead-code warnings and the Solana-client future-compatibility
warning remain. No broad unfiltered suite, Kani run or vulnerable pin was run.

Production is unchanged and reuses the requested artifact directly:
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`.
SHA-256: `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
No matcher is used. This base and the artifact-source checkout have identical
Git objects for `src` (`64a4febe373f17663565b835f2f630c689b664d8`),
`Cargo.toml` (`a90e5de0fd5e99a376d063b8c7e59271a4e1fdcf`) and
`Cargo.lock` (`98396fb8b7f65bad2dba1dab86cf89bb0939e62e`).
Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
Scope W records the default-feature build using platform-tools v1.52.

Host dependencies were copied, not linked, from Scope A's existing cache into
a private target. The new wrapper/tests were compiled from this checkout.
Initial commands used `/run/user/1001/percolator-scope-g-20260914-target`;
the user tmpfs was nearly full during baseline cache copying. The incomplete
task-owned copy was removed and this task's target moved to
`/run/percolator-scope-g-20260914/target`. A private bind mount enables execution
only under this task directory because the parent `/run` mount is noexec.
The first baseline compile under noexec did not run tests; after the mount
correction, the baseline recompiled and produced the two failures above.
Baseline builds use a separate `baseline-target`; no other worker's source,
target or mounts were altered.

## Exact commands

The following use the final target paths. Initial new/control/metadata runs used
the former target prefix above; their logs moved with the target. Logs are
`new.log`, `controls.log`, `metadata.log`, `metadata-final.log` and
`baseline-controls.log`.

```sh
cd /tmp/percolator-scope-g.XzuiZK
export CARGO_TARGET_DIR=/run/percolator-scope-g-20260914/target
export TMPDIR=$CARGO_TARGET_DIR/tmp
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so

cargo test --locked --offline --test v16_cu --no-run
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_008_intent_uniqueness_and_bounded_replay::withdrawal_stock_history::generated_stock_reclassification::v16_generated_retained_stock_reclassification_preserves_each_owners_budget \
  inv_008_intent_uniqueness_and_bounded_replay::withdrawal_stock_history::generated_stock_reclassification::v16_retained_stock_epoch_route_matrix_accounts_for_every_signed_outflow
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_008_intent_uniqueness_and_bounded_replay::capital_insurance_exchange::v16_retained_capital_insurance_exchange_preserves_typed_stock_and_atomic_retry \
  inv_008_intent_uniqueness_and_bounded_replay::generated_insurance_stock_epochs::v16_generated_live_insurance_stock_epochs_preserve_retained_rail_entitlement \
  inv_008_intent_uniqueness_and_bounded_replay::live_debit_consumption::v16_live_insurance_debit_consumes_epoch_across_ledger_and_destination_retries \
  inv_008_intent_uniqueness_and_bounded_replay::withdrawal_stock_history::v16_program_generated_withdrawal_stock_histories_preserve_first_execution_budget \
  inv_008_intent_uniqueness_and_bounded_replay::withdrawal_stock_history::v16_program_fee_shortened_withdrawal_consumes_intent_before_passive_replenishment \
  inv_008_intent_uniqueness_and_bounded_replay::underfunded_rail_retry::v16_underfunded_withdrawal_retry_preserves_replenished_stock_across_quote_rails \
  inv_008_intent_uniqueness_and_bounded_replay::insurance_round_trip_retry::v16_insurance_round_trip_consumption_survives_cross_route_retry \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_schedules_preserve_asset_allowance_and_exact_retry \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_ledger_history_is_economically_transparent \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_live_and_resolved_insurance_withdrawals_share_one_finite_budget \
  inv_031_no_double_use_of_claim_backing_or_insurance_atoms::v16_program_liquidation_spent_insurance_cannot_be_withdrawn_again \
  inv_081_success_state_validity_over_complete_public_routes::fee_resolution_atomicity::v16_program_retained_withdrawal_rolls_back_fee_resolution_and_paid_prefix \
  inv_008_intent_uniqueness_and_bounded_replay::v16_public_replay_disposition_roster_is_source_complete \
  inv_024_attributed_quote_value_conservation::v16_program_entitlement_effect_roster_is_source_complete
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant

git worktree add --detach /tmp/percolator-scope-g.XzuiZK-baseline d01245dd993bfe60c2448360733c3bb63c41aa19
cd /tmp/percolator-scope-g.XzuiZK-baseline
CARGO_TARGET_DIR=/run/percolator-scope-g-20260914/baseline-target \
TMPDIR=/run/percolator-scope-g-20260914/baseline-target/tmp \
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_008_intent_uniqueness_and_bounded_replay::insurance_round_trip_retry::v16_insurance_round_trip_consumption_survives_cross_route_retry \
  inv_064_insurance_withdrawal_policy_equivalence::v16_program_insurance_withdrawal_schedules_preserve_asset_allowance_and_exact_retry

cd /tmp/percolator-scope-g.XzuiZK
sha256sum "$PERCOLATOR_FUZZ_SBF"
git ls-tree HEAD src Cargo.toml Cargo.lock
git -C /run/percolator-pr135-scope-w-20260913 ls-tree HEAD src Cargo.toml Cargo.lock
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git diff --exit-code d01245dd -- src Cargo.toml Cargo.lock tests/invariants/invariant_status.tsv tests/invariants/open_findings.tsv
git show --check HEAD
```

Changed paths:

- `tests/invariants/cu/inv_008_generated_stock_reclassification.rs`
- `tests/invariants/cu/inv_008_withdrawal_stock_history.rs`
- `tests/invariants/inv_008_stock_epoch_routes.tsv`
- `tests/invariants/README.md`
- `tests/invariants/coverage_reopenings.tsv` (comments only)
- `tests/invariants/astra_scope_g_retained_stock_epochs_20260914.md`

One local coverage/documentation commit; no production or status promotion.
