# INV-036 multi-source fee partition audit

Base: `origin/main` at `056cd6ba`. Worktree:
`/dev/shm/astra-ultra-inv024-044-cycle-20260916220015`.
The next fetched main commit, `364fb2c9`, changes INV-020 evidence only.
No open PR or issue implementation was used.

## Net-new evidence

The reviewed retained-source-fee and matcher-cap witnesses use one paying source.
The older INV-038 backing-fee split witness derives its expected split from the
observed debit. Neither exercises collection from two source slots with distinct
rates, shares, and provider beneficiaries in a single fill.

The new INV-036 child module is mounted through the existing stateful owner.
After fixture genesis, authenticated public trades create two 5,000-atom claims
for one payer. Its deposits and the signed positions independently imply a
7,498-atom IM shortfall: liens of 5,000 and 2,498 in source domains 3 and 5.
Rates 1,333/3,777 bps yield fees 667/944; shares 2,500/6,667 bps yield insurance
166/629 and provider earnings 501/315. Expected fees never use observed capital
debits, production fee helpers, or aggregate lien growth as their oracle.

The trace continues through wrong-provider rejection, each correct provider's
SPL withdrawal, closure of all positions, conversion of the 10,000-atom PnL, and
each trader's exact live withdrawal. Both CPI roles and no-CPI orientations must
converge to the same per-owner payouts. The foreign market, unused portfolio,
mint supply, stock census, and encumbrance census are checked. The separate
second provider ledger is created by a real System Program instruction.

This adds executable fee/entitlement/domain/rounding evidence, independent of the
recent INV-024 executable-witness parser, INV-028 row423 metadata guard, and
source-domain realizability composition guards. No benchmark reclassification
or whole-invariant promotion is claimed.

## Negative controls and counts

In every world, clone the post-charge observation and independently move one atom:

- From the fee payer's capital to its counterparty's capital.
- From domain 3's provider earnings to domain 5's provider earnings.
- From domain 3's insurance budget to domain 5's insurance budget.

All three preserve aggregate stock and pass `assert_market_stock_census`.
The same exact partition comparison used on the real observation rejects each
clone. No malformed bytes are installed in LiteSVM. There are **12** such controls
over **4** worlds. The arithmetic also distinguishes per-source ceiling (**1,611**)
from ceiling the combined numerator (**1,610**).

Final public command: **3 passed, 0 failed**. The new selector accounts for
**160 transactions: 120 successful and 40 exact-rollback rejections**. The latter
are 8 unauthorized provider withdrawals and 32 bounded-crank NonProgress probes.
Peak observed CU is **816,978**, within the harness's 1,400,000 limit, not a new
maximum-shape CU certificate. Existing retained-source-fee evidence runs 16 worlds
with 48 rejections; the existing cap regression runs 4 worlds with 95 public
transactions and 2 rejections.

The trace-inventory selector failed on an unmodified detached `056cd6ba` worktree:
**0 passed, 1 failed**, actual 112 versus expected 110. Since the last inventory
update (`2669bf1ba`), the two added consumers are in CU INV-058 and public-SBF
INV-045; both call public-execution validation. The only integration bookkeeping
change outside the owner is `110 -> 113`, including the new INV-036 consumer.
The final five exact host guards pass **5/5**. No zero-test run counts as evidence.

## Commands and artifacts

Host dependency artifacts were copied into the private target directory; the
wrapper and matcher SBF artifacts were built afresh from this worktree.

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TMPDIR=/dev/shm
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv024-044-cycle-20260916220015-target

CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv024-044-cycle-20260916220015-sbf-target \
  cargo build-sbf --tools-version v1.52 --offline \
  --sbf-out-dir /dev/shm/astra-ultra-inv024-044-cycle-20260916220015-target/deploy -- --locked
CARGO_TARGET_DIR=/dev/shm/astra-ultra-inv024-044-cycle-20260916220015-matcher-target \
  cargo build-sbf --tools-version v1.52 --offline \
  --manifest-path tests/fixtures/auth_matcher/Cargo.toml \
  --sbf-out-dir /dev/shm/astra-ultra-inv024-044-cycle-20260916220015/tests/fixtures/auth_matcher/target/deploy -- --locked

cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture \
  inv_036_fee_destination_and_policy_version_integrity::multi_source_fee_partition::v16_program_multi_source_fees_preserve_independent_payer_and_domain_partitions \
  inv_036_fee_destination_and_policy_version_integrity::v16_program_retained_source_fees_survive_repricing_policy_and_settlement_orders \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_backing_fee_cap::v16_program_retained_backing_fee_caps_follow_participant_and_route_consent

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture \
  inv_079_public_reachability_evidence::v16_every_public_trace_consumer_validates_reachability_evidence \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots

rustfmt --edition 2021 --check --config skip_children=true \
  tests/invariants/stateful/inv_036_multi_source_fee_partition.rs \
  tests/invariants/stateful/inv_036_fee_destination_and_policy_version_integrity.rs \
  tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs
git diff --check
git diff --exit-code 056cd6ba -- src Cargo.toml Cargo.lock
```

SHA-256: wrapper `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`;
matcher `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
Logs have prefix `/dev/shm/astra-ultra-inv024-044-cycle-20260916220015-`.

## Limits and rejected candidates

Single-source independent fee recomputation is already covered by the retained
cap witness and was rejected as duplicate work. A two-payer variant instead
returned `EngineStale` on its risk-increasing trade. Inspection suggests sequential
lien creation advances the global risk epoch after the first payer is certified;
this remains an investigative lead, not a confirmed loss-of-funds or persistent
funded-lock finding, and is not covered or fixed by this increment.

Arbitrary source-slot histories, more than two paying domains, discounted credit,
expiry, fee clipping, simultaneous fee-paying participants, policy changes during
the route, batch backing fees (currently rejected), native-token rails, and
insurance/provider principal retirement remain outside this bounded witness.
Production and dependency code is unchanged; this increment is safe for main as
test/documentation coverage, not an economic fix or closure claim.
